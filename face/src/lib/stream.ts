import type { SseEvent, SseEventName } from '../types.ts';
import type { ConnState } from '../state/reducer.ts';
import { isUnauthorized } from './api.ts';
import { SseParser } from './sse.ts';

export const SSE_EVENTS: readonly SseEventName[] = [
  'snapshot',
  'item',
  'item_removed',
  'presence',
  'deliver',
  'reply',
  'proposal',
  'sessions',
];
const KNOWN = new Set<string>(SSE_EVENTS);

/** 다시 잇기 간격: 1→2→4→8→16→30초(최대 30초). attempt 는 1부터. */
export function backoffDelay(attempt: number): number {
  const n = Math.max(1, Math.floor(attempt));
  return Math.min(30_000, 1_000 * 2 ** (n - 1));
}

/** 이만큼 아무것도(ping 도) 오지 않으면 끊긴 것으로 본다. 데몬은 15초마다 `: ping` 을 보낸다(§8). */
export const STREAM_IDLE_MS = 45_000;

export interface StreamOptions {
  /** GET /v1/stream 을 연다. 머리가 오면 응답, 아니면 던진다(401 이면 ApiError(401)). */
  open: (signal: AbortSignal) => Promise<Response>;
  onEvent: (ev: SseEvent) => void;
  onStatus: (conn: ConnState) => void;
  /** 열쇠 문제(401)로 멈췄을 때 한 번. 스트림은 이미 멈춘 뒤다. */
  onAuthLost?: () => void;
}

/**
 * /v1/stream 연결. `EventSource` 는 Authorization 머리를 실을 수 없으므로 `fetch` 스트림을 직접 읽는다(§12.1-3).
 * 끊기면 간격을 늘려 가며 다시 잇는다. 다시 이으면 데몬이 snapshot 을 먼저 보내므로 상태가 저절로 맞춰진다.
 * 열쇠 문제(401)면 저절로 다시 잇지 않는다 — 서버를 두드려 봐야 소용없다. `retryNow()`(사람·창 복귀)로만 다시 본다.
 * 연결마다 세대 번호를 늘려, 늦게 끝난 옛 연결(머리·이벤트·끝남·401)이 새 연결을 건드리지 못하게 한다.
 */
export class EventStream {
  private ctrl: AbortController | null = null;
  private isOpen = false;
  private attempt = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private idle: ReturnType<typeof setTimeout> | null = null;
  private stopped = true;
  private gen = 0;
  private readonly opts: StreamOptions;

  constructor(opts: StreamOptions) {
    this.opts = opts;
  }

  start(): void {
    this.stopped = false;
    this.connect();
  }

  stop(): void {
    this.stopped = true;
    this.gen += 1;
    this.clearTimer();
    this.close();
  }

  /** "지금 다시" 단추·온라인 복귀·창이 다시 보일 때. 이미 열려 있으면 그대로 둔다. */
  retryNow(): void {
    if (this.stopped || this.isOpen) return;
    this.clearTimer();
    this.close();
    this.connect();
  }

  /** 열쇠가 없다(401). 열린 연결도 끊고 다시 잇지 않는다. `retryNow()`로만 다시 시도한다. */
  authLost(): void {
    if (this.stopped) return;
    this.gen += 1;
    this.clearTimer();
    this.close();
    this.opts.onStatus({ status: 'auth', attempt: this.attempt, retryAt: null });
  }

  private connect(): void {
    const gen = ++this.gen;
    const ctrl = new AbortController();
    this.ctrl = ctrl;
    this.isOpen = false;
    this.opts.onStatus({ status: this.attempt === 0 ? 'connecting' : 'retrying', attempt: this.attempt, retryAt: null });
    void this.run(gen, ctrl);
  }

  private async run(gen: number, ctrl: AbortController): Promise<void> {
    let res: Response;
    try {
      res = await this.opts.open(ctrl.signal);
    } catch (e) {
      if (gen === this.gen) this.fail(gen, isUnauthorized(e));
      return;
    }
    if (gen !== this.gen || !res.body) {
      void res.body?.cancel().catch(() => {});
      if (gen === this.gen) this.fail(gen, false);
      return;
    }
    this.attempt = 0;
    this.isOpen = true;
    this.opts.onStatus({ status: 'open', attempt: 0, retryAt: null });
    this.armIdle(gen, ctrl);

    const parser = new SseParser((m) => {
      if (gen !== this.gen || !KNOWN.has(m.event)) return;
      let data: unknown;
      try {
        data = JSON.parse(m.data);
      } catch {
        return; // 깨진 줄은 버린다
      }
      this.opts.onEvent({ event: m.event, data } as SseEvent);
    });
    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (gen !== this.gen) break;
        if (done) break;
        this.armIdle(gen, ctrl);
        parser.push(decoder.decode(value, { stream: true }));
      }
    } catch {
      /* 끊김·끊음(abort)·너무 큰 사건 — 아래에서 다시 잇는다 */
    } finally {
      void reader.cancel().catch(() => {});
    }
    if (gen === this.gen) this.fail(gen, false);
  }

  /** 받을 때마다 다시 건다. 시간이 다 되면 그 연결을 끊는다(읽기가 끝나며 다시 잇는다). */
  private armIdle(gen: number, ctrl: AbortController): void {
    if (this.idle) clearTimeout(this.idle);
    this.idle = setTimeout(() => {
      this.idle = null;
      if (gen === this.gen) ctrl.abort();
    }, STREAM_IDLE_MS);
  }

  private fail(gen: number, auth: boolean): void {
    if (this.stopped || gen !== this.gen) return;
    this.close();
    this.attempt += 1;
    if (auth) {
      this.authLost();
      this.opts.onAuthLost?.();
      return;
    }
    const delay = backoffDelay(this.attempt);
    const retryAt = Date.now() + delay;
    this.opts.onStatus({ status: 'retrying', attempt: this.attempt, retryAt });
    this.clearTimer();
    this.timer = setTimeout(() => {
      this.timer = null;
      if (!this.stopped) this.connect();
    }, delay);
  }

  private close(): void {
    this.isOpen = false;
    if (this.idle) clearTimeout(this.idle);
    this.idle = null;
    if (this.ctrl) {
      this.ctrl.abort();
      this.ctrl = null;
    }
  }

  private clearTimer(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
  }
}
