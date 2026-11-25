import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Backend, BackendSink } from './backend.ts';
import { Controller, REPLY_WAIT_MS } from './controller.ts';
import { visibleItems, visibleProposals } from '../state/reducer.ts';
import { ApiError } from '../lib/api.ts';
import { CHANNEL_MISSING_REASON, demoInterrupt, demoNudge, demoReply, demoSnapshot } from '../demo/data.ts';
import { DEFAULT_PREFS } from '../lib/prefs.ts';

const NOW = Date.now();

function fakeBackend(over: Partial<Backend> = {}) {
  let sink: BackendSink | null = null;
  const b: Backend & { sink: () => BackendSink } = {
    start: (s) => {
      sink = s;
    },
    stop: vi.fn(),
    retryNow: vi.fn(),
    itemAction: vi.fn(async () => {}),
    focus: vi.fn(async (on: boolean, minutes?: number) => ({ ok: true, until_ms: on ? NOW + (minutes ?? 0) * 60_000 : null })),
    listening: vi.fn(async () => {}),
    say: vi.fn(async () => ({ delivered_to: 'a1f0c2d9-aok', reason: '' })),
    decide: vi.fn(async () => ({ delivered_to: 'c3d95e11-dja' })),
    proposal: vi.fn(async () => {}),
    ...over,
    sink: () => sink!,
  };
  return b;
}

function setup(over: Partial<Backend> = {}, prefs = { ...DEFAULT_PREFS }) {
  const backend = fakeBackend(over);
  const chime = vi.fn();
  const speak = vi.fn();
  const ctl = new Controller(backend, { chime, speak, prefs, persistPrefs: false });
  ctl.start();
  backend.sink().event({ event: 'snapshot', data: demoSnapshot(NOW) });
  return { ctl, backend, chime, speak };
}

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
});
afterEach(() => {
  vi.useRealTimers();
});

describe('전달과 말풍선·소리', () => {
  it('interrupt 는 말풍선을 띄우고 차임을 울린다(음성은 켰을 때만)', () => {
    const { ctl, backend, chime, speak } = setup();
    backend.sink().event({ event: 'deliver', data: demoInterrupt(NOW) });
    expect(ctl.bubble.get()?.kind).toBe('interrupt');
    expect(ctl.bubble.get()?.project).toBe('agent-ops-kit');
    expect(chime).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(1_000);
    expect(speak).not.toHaveBeenCalled();
  });

  it('음성 읽기를 켜면 차임 뒤에 line 을 읽는다', () => {
    const { ctl, backend, speak } = setup({}, { ...DEFAULT_PREFS, voice: true, rate: 1.2 });
    backend.sink().event({ event: 'deliver', data: demoInterrupt(NOW) });
    vi.advanceTimersByTime(500);
    expect(speak).toHaveBeenCalledWith('agent-ops-kit에서 권한 승인을 4분째 기다려요', expect.objectContaining({ rate: 1.2 }));
    expect(ctl.bubble.get()).not.toBeNull();
  });

  it('소리를 끄면 차임이 울리지 않는다', () => {
    const { backend, chime } = setup({}, { ...DEFAULT_PREFS, sound: false });
    backend.sink().event({ event: 'deliver', data: demoInterrupt(NOW) });
    expect(chime).not.toHaveBeenCalled();
  });

  it('nudge 는 소리 없이 말풍선만', () => {
    const { ctl, backend, chime } = setup();
    backend.sink().event({ event: 'deliver', data: demoNudge(NOW) });
    expect(ctl.bubble.get()?.kind).toBe('nudge');
    expect(chime).not.toHaveBeenCalled();
  });

  it('ambient 와 presence.line 은 말풍선을 띄우지 않는다(먼저 브리핑하지 않음)', () => {
    const { ctl, backend } = setup();
    backend.sink().event({ event: 'deliver', data: { ...demoNudge(NOW), level: 'ambient' } });
    backend.sink().event({ event: 'presence', data: { ...demoSnapshot(NOW).presence, line: '요약: 오늘 할 일 3개' } });
    expect(ctl.bubble.get()).toBeNull();
  });

  it('reply 는 첫 줄로 말풍선을 띄우고 대화에 남긴다', () => {
    const { ctl, backend } = setup();
    backend.sink().event({ event: 'reply', data: demoReply(NOW) });
    expect(ctl.bubble.get()?.line).toBe('vitest 설치 승인이 나면 바로 시험을 돌릴게요.');
    expect(ctl.state.get().chat).toHaveLength(1);
  });

  it('항목이 닫히면(item_removed) interrupt 말풍선도 거둔다', () => {
    const { ctl, backend } = setup();
    const d = demoInterrupt(NOW);
    backend.sink().event({ event: 'deliver', data: d });
    backend.sink().event({ event: 'item_removed', data: { id: d.item } });
    expect(ctl.bubble.get()).toBeNull();
  });

  it('재연결 스냅샷에 없는 항목의 말풍선은 거둔다', () => {
    const { ctl, backend } = setup();
    const d = demoInterrupt(NOW);
    backend.sink().event({ event: 'deliver', data: d });
    const snap = demoSnapshot(NOW);
    backend.sink().event({ event: 'snapshot', data: { ...snap, items: snap.items.filter((i) => i.id !== d.item) } });
    expect(ctl.bubble.get()).toBeNull();
  });
});

describe('집중·잠잘 때의 답(§12.4)', () => {
  const presence = (over: object) => ({ ...demoSnapshot(NOW).presence, ...over });

  it('집중 모드면 reply 를 말풍선으로 띄우지 않고 대화 기록과 배지로만', () => {
    const { ctl, backend } = setup();
    backend.sink().event({ event: 'presence', data: presence({ mood: 'waiting', focus_mode_until_ms: Date.now() + 30 * 60_000 }) });
    backend.sink().event({ event: 'reply', data: demoReply(NOW) });
    expect(ctl.bubble.get()).toBeNull();
    expect(ctl.state.get().chat).toHaveLength(1);
    expect(ctl.state.get().unreadReplies).toBe(1);
  });

  it('기분이 sleep 이면 같다', () => {
    const { ctl, backend } = setup();
    backend.sink().event({ event: 'presence', data: presence({ mood: 'sleep', focus_mode_until_ms: null }) });
    backend.sink().event({ event: 'reply', data: demoReply(NOW) });
    expect(ctl.bubble.get()).toBeNull();
    expect(ctl.state.get().unreadReplies).toBe(1);
  });

  it('집중 모드가 지났으면 전처럼 말풍선을 띄우고 배지는 없다', () => {
    const { ctl, backend } = setup();
    backend.sink().event({ event: 'presence', data: presence({ mood: 'idle', focus_mode_until_ms: Date.now() - 1_000 }) });
    backend.sink().event({ event: 'reply', data: demoReply(NOW) });
    expect(ctl.bubble.get()?.kind).toBe('reply');
    expect(ctl.state.get().unreadReplies).toBe(0);
  });

  it('데몬이 정한 전달(nudge)은 이 규칙과 상관없이 그대로', () => {
    const { ctl, backend } = setup();
    backend.sink().event({ event: 'presence', data: presence({ mood: 'waiting', focus_mode_until_ms: Date.now() + 30 * 60_000 }) });
    backend.sink().event({ event: 'deliver', data: demoNudge(NOW) });
    expect(ctl.bubble.get()?.kind).toBe('nudge');
  });

  it('대화를 보면(markChatSeen) 배지를 지운다', () => {
    const { ctl, backend } = setup();
    backend.sink().event({ event: 'presence', data: presence({ mood: 'sleep' }) });
    backend.sink().event({ event: 'reply', data: demoReply(NOW) });
    ctl.markChatSeen();
    expect(ctl.state.get().unreadReplies).toBe(0);
  });
});

describe('낙관적 동작', () => {
  it('처리함은 바로 숨기고 성공하면 그대로 둔다', async () => {
    const { ctl, backend } = setup();
    const id = visibleItems(ctl.state.get(), NOW)[0]!.id;
    const p = ctl.itemAction(id, 'done');
    expect(visibleItems(ctl.state.get(), NOW).some((i) => i.id === id)).toBe(false);
    expect(await p).toBe(true);
    expect(backend.itemAction).toHaveBeenCalledWith(id, 'done', {});
    expect(visibleItems(ctl.state.get(), NOW).some((i) => i.id === id)).toBe(false);
  });

  it('실패하면 되돌리고 한국어 알림을 띄운다', async () => {
    const { ctl } = setup({ itemAction: vi.fn(async () => Promise.reject(new ApiError(500, 'x'))) });
    const id = visibleItems(ctl.state.get(), NOW)[0]!.id;
    expect(await ctl.itemAction(id, 'dismiss', { reason: '과민' })).toBe(false);
    expect(visibleItems(ctl.state.get(), NOW)[0]!.id).toBe(id);
    expect(ctl.toasts.get()[0]!.text).toContain('‘넘기기’을 하지 못했어요');
    expect(ctl.toasts.get()[0]!.text).toContain('데몬 안에서 오류');
  });

  it('열어 봄은 숨기지 않고 봤음 표시만', async () => {
    const { ctl } = setup();
    const it = visibleItems(ctl.state.get(), NOW)[0]!;
    await ctl.itemAction(it.id, 'open');
    expect(ctl.state.get().seen[it.id]).toBe(it.signature);
    expect(visibleItems(ctl.state.get(), NOW)[0]!.id).toBe(it.id);
  });

  it('결정이 전달되면 카드를 숨기고, 안 되면 이유를 돌려준다', async () => {
    const { ctl } = setup();
    const it = visibleItems(ctl.state.get(), NOW).find((i) => i.kind === 'decision')!;
    expect(await ctl.decide(it.id, it.decision_id!, '지금 적용')).toEqual({ ok: true });
    expect(ctl.state.get().hidden[it.id]).toBe(it.signature);

    const { ctl: c2 } = setup({ decide: vi.fn(async () => ({ delivered_to: null })) });
    const r = await c2.decide(it.id, it.decision_id!, '취소');
    expect(r.ok).toBe(false);
    expect(c2.state.get().hidden[it.id]).toBeUndefined();
  });

  it('제안: 받아들이면 지우고, 실패하면 되돌린다', async () => {
    const { ctl, backend } = setup();
    await ctl.answerProposal('pr_unread_quiet', true);
    expect(backend.proposal).toHaveBeenCalledWith('pr_unread_quiet', true);
    expect(visibleProposals(ctl.state.get()).map((p) => p.id)).toEqual(['pr_context_quiet']);

    const { ctl: c2 } = setup({ proposal: vi.fn(async () => Promise.reject(new TypeError('fetch failed'))) });
    await c2.answerProposal('pr_unread_quiet', false);
    expect(visibleProposals(c2.state.get())).toHaveLength(2);
    expect(c2.toasts.get()[0]!.text).toContain('곁 데몬에 닿지 않아요');
  });

  it('집중 모드: 응답의 until_ms 를 쓰고, 실패하면 되돌린다', async () => {
    const { ctl, backend } = setup();
    await ctl.setFocus(30);
    expect(backend.focus).toHaveBeenCalledWith(true, 30);
    expect(ctl.state.get().presence.focus_mode_until_ms).toBe(NOW + 30 * 60_000);
    await ctl.setFocus(null);
    expect(backend.focus).toHaveBeenLastCalledWith(false);
    expect(ctl.state.get().presence.focus_mode_until_ms).toBeNull();

    const { ctl: c2 } = setup({ focus: vi.fn(async () => Promise.reject(new ApiError(401, 'x'))) });
    await c2.setFocus(60);
    expect(c2.state.get().presence.focus_mode_until_ms).toBeNull();
    expect(c2.toasts.get()[0]!.text).toContain('열쇠가 없거나 만료됐어요');
  });
});

describe('대화', () => {
  it('전달되면 어느 세션에 갔는지 남긴다', async () => {
    const { ctl, backend } = setup();
    await ctl.say('  결과 알려 줘 ', null, null);
    expect(backend.say).toHaveBeenCalledWith('결과 알려 줘', null);
    expect(ctl.state.get().chat[0]).toMatchObject({ role: 'me', status: 'sent', deliveredTo: 'a1f0c2d9-aok', project: 'agent-ops-kit' });
  });

  it('30초 안에 그 세션의 답이 없으면 안내를 한 번 켠다(§10.1)', async () => {
    const { ctl } = setup();
    await ctl.say('하나', null, null);
    await ctl.say('둘', null, null);
    vi.advanceTimersByTime(REPLY_WAIT_MS - 100);
    expect(ctl.state.get().chat.some((m) => m.noReply)).toBe(false);
    vi.advanceTimersByTime(200);
    // 같은 세션에는 한 번만
    expect(ctl.state.get().chat.filter((m) => m.noReply).map((m) => m.text)).toEqual(['하나']);
  });

  it('30초 안에 답이 오면 안내를 켜지 않는다', async () => {
    const { ctl, backend } = setup();
    await ctl.say('하나', null, null);
    vi.advanceTimersByTime(5_000);
    backend.sink().event({ event: 'reply', data: { ...demoReply(Date.now()), session: 'a1f0c2d9-aok', chat_id: 'm17' } });
    vi.advanceTimersByTime(REPLY_WAIT_MS);
    const mine = ctl.state.get().chat.find((m) => m.role === 'me')!;
    expect(mine.answered).toBe(true);
    expect(mine.noReply).toBeFalsy();
  });

  it('delivered_to 가 null 이면 서버의 이유를 그대로 보인다', async () => {
    const { ctl } = setup({ say: vi.fn(async () => ({ delivered_to: null, reason: CHANNEL_MISSING_REASON })) });
    await ctl.say('안녕', 'e2a6c930-wat', 'work-atlas');
    expect(ctl.state.get().chat[0]).toMatchObject({ status: 'undelivered', reason: CHANNEL_MISSING_REASON });
  });

  it('네트워크 오류면 error 로 두고 다시 보낼 수 있다', async () => {
    let fail = true;
    const { ctl } = setup({
      say: vi.fn(async () => {
        if (fail) throw new TypeError('down');
        return { delivered_to: 'a1f0c2d9-aok', reason: '' };
      }),
    });
    await ctl.say('안녕', null, null);
    const id = ctl.state.get().chat[0]!.id;
    expect(ctl.state.get().chat[0]!.status).toBe('error');
    fail = false;
    await ctl.resend(id);
    expect(ctl.state.get().chat[0]!.status).toBe('sent');
    expect(ctl.state.get().chat).toHaveLength(1);
  });

  it('듣는 중: 초점이 잠깐 옮겨 다니는 것은 무시하고 바뀔 때만 보낸다', () => {
    const { ctl, backend } = setup();
    ctl.setListening(true);
    vi.advanceTimersByTime(1);
    ctl.setListening(false);
    vi.advanceTimersByTime(50);
    ctl.setListening(true);
    vi.advanceTimersByTime(300);
    ctl.setListening(false);
    vi.advanceTimersByTime(300);
    expect((backend.listening as ReturnType<typeof vi.fn>).mock.calls).toEqual([[true], [false]]);
  });
});
