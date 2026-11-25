import type { DismissReason, ItemAction, SayResult, SseEvent } from '../types.ts';
import type { Backend } from './backend.ts';
import { Observable } from './observable.ts';
import { initialState, quietReplies, reduce, type Action, type AppState, type ConnStatus } from '../state/reducer.ts';
import { BubbleQueue, bubbleFromDelivery, bubbleFromReply, type Bubble } from '../lib/bubble.ts';
import { describeError } from '../lib/api.ts';
import { playChime } from '../lib/audio.ts';
import { speak } from '../lib/voice.ts';
import { loadPrefs, savePrefs, type Prefs } from '../lib/prefs.ts';

export interface Toast {
  id: number;
  text: string;
}

/** 보낸 뒤 이만큼 답이 없으면 channel 플래그를 확인하라고 안내한다(§10.1). */
export const REPLY_WAIT_MS = 30_000;

const ACTION_LABEL: Record<ItemAction, string> = {
  open: '열어 봄',
  done: '처리함',
  dismiss: '넘기기',
  snooze: '내일 다시',
};

export interface ControllerOptions {
  /** 소리·음성을 내는 함수(시험에서 바꿔 끼운다) */
  chime?: () => unknown;
  speak?: (text: string, prefs: Prefs) => unknown;
  prefs?: Prefs;
  persistPrefs?: boolean;
}

/**
 * 얼굴의 두뇌. 상태(reducer)·말풍선 차례·낙관적 동작·소리를 한곳에서 엮는다.
 * 화면 구성 요소는 이것만 본다.
 */
export class Controller {
  readonly state: Observable<AppState>;
  readonly bubble = new Observable<Bubble | null>(null);
  readonly toasts = new Observable<Toast[]>([]);
  readonly prefs: Observable<Prefs>;
  readonly queue: BubbleQueue;
  private readonly backend: Backend;
  private readonly opts: ControllerOptions;
  private toastSeq = 0;
  private listeningOn = false;
  private listeningTimer: ReturnType<typeof setTimeout> | null = null;
  private msgSeq = 0;

  constructor(backend: Backend, opts: ControllerOptions = {}, initialConn: ConnStatus = 'connecting') {
    this.backend = backend;
    this.opts = opts;
    this.state = new Observable(initialState(initialConn));
    this.prefs = new Observable(opts.prefs ?? loadPrefs());
    this.queue = new BubbleQueue((b) => this.bubble.set(b));
  }

  start(): void {
    this.backend.start({
      event: (ev) => this.onEvent(ev),
      status: (conn) => this.dispatch({ type: 'conn', conn }),
    });
  }

  stop(): void {
    this.backend.stop();
    this.queue.dispose();
  }

  retryNow(): void {
    this.backend.retryNow();
  }

  dispatch(action: Action): void {
    this.state.set(reduce(this.state.get(), action));
  }

  /** SSE 이벤트 하나 — 상태에 넣고, 말풍선·소리 같은 곁가지 일을 한다. */
  onEvent(ev: SseEvent): void {
    const at = Date.now();
    this.dispatch({ type: 'sse', ev, at });
    const s = this.state.get();
    switch (ev.event) {
      case 'deliver': {
        const b = bubbleFromDelivery(ev.data, s.items[ev.data.item] ?? null);
        if (b && this.queue.push(b) && b.kind === 'interrupt') this.ring(b);
        break;
      }
      case 'reply': {
        // 집중 모드·sleep 이면 말풍선 없이 대화 기록과 배지로만(§12.4). 배지는 리듀서가 센다.
        if (quietReplies(s, at)) break;
        const b = bubbleFromReply(ev.data);
        if (b) this.queue.push(b);
        break;
      }
      case 'item_removed':
        if (ev.data && typeof ev.data.id === 'string') this.queue.removeItem(ev.data.id);
        break;
      case 'item':
        if (ev.data && ev.data.state && ev.data.state !== 'open') this.queue.removeItem(ev.data.id);
        break;
      case 'snapshot': {
        // 끊긴 동안 닫힌 항목의 말풍선은 거둔다.
        const cur = this.queue.current;
        if (cur?.item && !s.items[cur.item]) this.queue.removeItem(cur.item);
        for (const q of [...this.queue.queue]) if (q.item && !s.items[q.item]) this.queue.removeItem(q.item);
        break;
      }
      default:
        break;
    }
  }

  private ring(b: Bubble): void {
    const p = this.prefs.get();
    if (p.sound) (this.opts.chime ?? (() => void playChime()))();
    if (p.voice && b.speak) {
      const say = this.opts.speak ?? ((text: string, pr: Prefs) => speak(text, { voiceURI: pr.voiceURI, rate: pr.rate }));
      // 차임이 먼저 울리고 말한다.
      setTimeout(() => say(b.line, p), p.sound ? 450 : 0);
    }
  }

  // ── 말풍선 ──

  ackBubble(): void {
    this.queue.ack();
  }

  holdBubble(source: string, on: boolean): void {
    this.queue.hold(source, on);
  }

  /** 사람이 대화 탭을 봤다 — 안 읽은 답 배지를 지운다. */
  markChatSeen(): void {
    if (this.state.get().unreadReplies > 0) this.dispatch({ type: 'chatSeen' });
  }

  // ── 당신 차례 동작(낙관적, 실패하면 되돌림) ──

  async itemAction(
    id: string,
    action: ItemAction,
    opts: { reason?: DismissReason; minutes?: number } = {},
  ): Promise<boolean> {
    const hides = action !== 'open';
    if (hides) this.dispatch({ type: 'hideItem', id });
    else this.dispatch({ type: 'markSeen', id });
    try {
      await this.backend.itemAction(id, action, opts);
      if (hides) this.queue.removeItem(id);
      return true;
    } catch (e) {
      if (hides) this.dispatch({ type: 'unhideItem', id });
      this.toast(`‘${ACTION_LABEL[action]}’을 하지 못했어요 — ${describeError(e)}`);
      return false;
    }
  }

  /** 결정 고르기. 전달되면 카드를 숨긴다. */
  async decide(itemId: string, decisionId: string, choice: string): Promise<{ ok: boolean; message?: string }> {
    try {
      const r = await this.backend.decide(decisionId, choice);
      if (r.delivered_to) {
        this.dispatch({ type: 'hideItem', id: itemId });
        this.queue.removeItem(itemId);
        return { ok: true };
      }
      return { ok: false, message: '세션에 전달하지 못했어요. channel이 끊겼을 수 있어요' };
    } catch (e) {
      return { ok: false, message: describeError(e) };
    }
  }

  // ── 집중 모드 ──

  async setFocus(minutes: number | null): Promise<void> {
    const prev = this.state.get().presence.focus_mode_until_ms;
    const on = minutes !== null && minutes > 0;
    this.dispatch({ type: 'focusUntil', until_ms: on ? Date.now() + this.state.get().clockOffsetMs + minutes * 60_000 : null });
    try {
      const r = on ? await this.backend.focus(true, minutes) : await this.backend.focus(false);
      this.dispatch({ type: 'focusUntil', until_ms: typeof r.until_ms === 'number' ? r.until_ms : null });
    } catch (e) {
      this.dispatch({ type: 'focusUntil', until_ms: prev });
      this.toast(`집중 모드를 바꾸지 못했어요 — ${describeError(e)}`);
    }
  }

  // ── 듣는 중(입력창에 초점) ──

  setListening(on: boolean): void {
    // 초점이 잠깐 옮겨 다니는 것(단추 누르기 등)은 무시한다.
    if (this.listeningTimer) clearTimeout(this.listeningTimer);
    this.listeningTimer = setTimeout(
      () => {
        this.listeningTimer = null;
        if (on === this.listeningOn) return;
        this.listeningOn = on;
        void this.backend.listening(on).catch(() => {});
      },
      on ? 0 : 200,
    );
  }

  // ── 대화 ──

  async say(text: string, session: string | null, project: string | null): Promise<void> {
    const body = text.trim();
    if (!body) return;
    const id = `m:${Date.now()}:${++this.msgSeq}`;
    this.dispatch({
      type: 'chatAdd',
      msg: {
        id,
        role: 'me',
        text: body,
        at_ms: Date.now() + this.state.get().clockOffsetMs,
        session,
        project,
        status: 'sending',
      },
    });
    await this.deliver(id, body, session);
  }

  async resend(id: string): Promise<void> {
    const m = this.state.get().chat.find((x) => x.id === id);
    if (!m || m.role !== 'me') return;
    this.dispatch({ type: 'chatUpdate', id, patch: { status: 'sending', reason: undefined } });
    await this.deliver(id, m.text, m.session);
  }

  private async deliver(id: string, text: string, session: string | null): Promise<void> {
    let r: SayResult;
    try {
      r = await this.backend.say(text, session);
    } catch (e) {
      this.dispatch({ type: 'chatUpdate', id, patch: { status: 'error', reason: describeError(e) } });
      return;
    }
    if (r.delivered_to) {
      const target = this.state.get().sessions.find((s) => s.id === r.delivered_to);
      this.dispatch({
        type: 'chatUpdate',
        id,
        patch: {
          status: 'sent',
          deliveredTo: r.delivered_to,
          session: r.delivered_to,
          project: target?.project ?? null,
          reason: r.reason,
          answered: false,
          noReply: false,
        },
      });
      this.watchReply(id);
    } else {
      this.dispatch({
        type: 'chatUpdate',
        id,
        patch: { status: 'undelivered', deliveredTo: null, reason: r.reason || '전달할 세션을 찾지 못했어요' },
      });
    }
  }

  /**
   * delivered_to 는 "channel 에 넘김"일 뿐 Claude 가 읽었다는 뜻이 아니다(§10.1).
   * 30초 안에 그 세션의 답이 없으면 안내를 한 번 보인다. 같은 세션에 이미 떠 있으면 또 띄우지 않는다.
   */
  private watchReply(id: string): void {
    setTimeout(() => {
      const chat = this.state.get().chat;
      const m = chat.find((x) => x.id === id);
      if (!m || m.status !== 'sent' || m.answered) return;
      const shown = chat.some((x) => x.id !== id && x.noReply && !x.answered && x.deliveredTo === m.deliveredTo);
      if (shown) return;
      this.dispatch({ type: 'chatUpdate', id, patch: { noReply: true } });
    }, REPLY_WAIT_MS);
  }

  // ── 제안 ──

  async answerProposal(id: string, accept: boolean): Promise<void> {
    this.dispatch({ type: 'hideProposal', id });
    try {
      await this.backend.proposal(id, accept);
      this.dispatch({ type: 'removeProposal', id });
    } catch (e) {
      this.dispatch({ type: 'unhideProposal', id });
      this.toast(`제안에 답하지 못했어요 — ${describeError(e)}`);
    }
  }

  // ── 이 기기의 설정 ──

  setPrefs(patch: Partial<Prefs>): void {
    const next = { ...this.prefs.get(), ...patch };
    this.prefs.set(next);
    if (this.opts.persistPrefs !== false) savePrefs(next);
  }

  testChime(): void {
    (this.opts.chime ?? (() => void playChime()))();
  }

  testVoice(text = '곁이에요. 이렇게 읽어 드릴게요.'): void {
    const p = this.prefs.get();
    const say = this.opts.speak ?? ((t: string, pr: Prefs) => speak(t, { voiceURI: pr.voiceURI, rate: pr.rate }));
    say(text, p);
  }

  // ── 알림 띠 ──

  toast(text: string): void {
    const id = ++this.toastSeq;
    this.toasts.update((ts) => [...ts.slice(-2), { id, text }]);
    setTimeout(() => this.dropToast(id), 6_000);
  }

  dropToast(id: number): void {
    this.toasts.update((ts) => ts.filter((t) => t.id !== id));
  }
}
