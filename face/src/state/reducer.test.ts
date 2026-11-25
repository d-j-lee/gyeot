import { describe, expect, it } from 'vitest';
import type { Item, Snapshot, SseEvent } from '../types.ts';
import {
  applyEvent,
  chatTargets,
  CHAT_LIMIT,
  initialState,
  quietReplies,
  reduce,
  visibleItems,
  visibleProposals,
  type AppState,
} from './reducer.ts';
import { demoReply, demoSnapshot } from '../demo/data.ts';

const NOW = Date.UTC(2026, 10, 26, 5, 0, 0);

function item(over: Partial<Item> = {}): Item {
  return {
    id: 'it_a',
    kind: 'approval',
    session: 's1',
    agent: 'claude-code',
    project: 'proj',
    cwd: 'C:\\work\\proj',
    title: '권한 승인 대기 · Bash',
    body: 'npm i',
    blocking: true,
    score: 95,
    reasons: ['권한 승인 대기: 기본 95'],
    created_ms: NOW - 60_000,
    updated_ms: NOW - 60_000,
    signature: 'approval:1',
    state: 'open',
    snooze_until_ms: null,
    options: null,
    decision_id: null,
    ...over,
  };
}

function snap(over: Partial<Snapshot> = {}): Snapshot {
  return {
    version: '0.1.0',
    now_ms: NOW,
    presence: { mood: 'waiting', level: 'nudge', focus_item: 'it_a', line: '권한 승인 1건', since_ms: NOW, focus_mode_until_ms: null },
    items: [item()],
    sessions: [{ id: 's1', agent: 'claude-code', project: 'proj', cwd: 'C:\\work\\proj', state: 'waiting', last_ms: NOW, channel: 'ch_1' }],
    proposals: [],
    budget: { nudge: 4, interrupt: 1 },
    settings: { nudge_min: 80, interrupt_min: 95, stale_minutes: 30, quiet_hours: [], quiet_kinds: [] },
    ...over,
  };
}

const ev = (e: SseEvent) => e;
const synced = (): AppState => applyEvent(initialState(), ev({ event: 'snapshot', data: snap() }), NOW - 250);

describe('applyEvent — snapshot', () => {
  it('상태를 통째로 바꾸고 서버 시계 차이를 잰다', () => {
    const s = synced();
    expect(s.synced).toBe(true);
    expect(s.version).toBe('0.1.0');
    expect(Object.keys(s.items)).toEqual(['it_a']);
    expect(s.presence.mood).toBe('waiting');
    expect(s.sessions).toHaveLength(1);
    expect(s.settings?.nudge_min).toBe(80);
    expect(s.clockOffsetMs).toBe(250);
  });

  it('열린 항목만 받고 모양이 깨진 항목은 버린다', () => {
    const bad = { id: '', kind: 'x' } as unknown as Item;
    const s = applyEvent(
      initialState(),
      ev({ event: 'snapshot', data: snap({ items: [item(), item({ id: 'it_closed', state: 'done' }), bad] }) }),
    );
    expect(Object.keys(s.items)).toEqual(['it_a']);
  });

  it('대화 기록은 재동기화해도 남는다', () => {
    let s = synced();
    s = applyEvent(s, ev({ event: 'reply', data: { session: 's1', project: 'proj', chat_id: 'c1', text: '안녕', at_ms: NOW } }));
    s = applyEvent(s, ev({ event: 'snapshot', data: snap({ items: [] }) }));
    expect(s.chat).toHaveLength(1);
    expect(s.items).toEqual({});
  });

  it('모르는 mood 는 idle 로 본다', () => {
    const s = applyEvent(
      initialState(),
      ev({ event: 'snapshot', data: snap({ presence: { ...snap().presence, mood: 'dancing' as never } }) }),
    );
    expect(s.presence.mood).toBe('idle');
  });
});

describe('applyEvent — item / item_removed', () => {
  it('새 항목을 더하고 같은 id 는 갱신한다', () => {
    let s = synced();
    s = applyEvent(s, ev({ event: 'item', data: item({ id: 'it_b', kind: 'failed', score: 80, blocking: false }) }));
    expect(Object.keys(s.items).sort()).toEqual(['it_a', 'it_b']);
    s = applyEvent(s, ev({ event: 'item', data: item({ title: '바뀐 제목' }) }));
    expect(s.items.it_a!.title).toBe('바뀐 제목');
  });

  it('item_removed 는 항목과 숨김·봤음 표시를 함께 지운다', () => {
    let s = synced();
    s = reduce(s, { type: 'hideItem', id: 'it_a' });
    s = reduce(s, { type: 'markSeen', id: 'it_a' });
    s = applyEvent(s, ev({ event: 'item_removed', data: { id: 'it_a' } }));
    expect(s.items).toEqual({});
    expect(s.hidden).toEqual({});
    expect(s.seen).toEqual({});
  });

  it('state 가 open 이 아닌 item 은 닫힘으로 본다', () => {
    let s = synced();
    s = applyEvent(s, ev({ event: 'item', data: item({ state: 'snoozed' }) }));
    expect(s.items).toEqual({});
  });

  it('없는 id 를 지워도 같은 상태를 돌려준다', () => {
    const s = synced();
    expect(applyEvent(s, ev({ event: 'item_removed', data: { id: 'nope' } }))).toBe(s);
  });
});

describe('낙관적 숨김', () => {
  it('숨긴 항목은 보이지 않고 되돌리면 다시 보인다', () => {
    let s = synced();
    s = reduce(s, { type: 'hideItem', id: 'it_a' });
    expect(visibleItems(s, NOW)).toHaveLength(0);
    s = reduce(s, { type: 'unhideItem', id: 'it_a' });
    expect(visibleItems(s, NOW)).toHaveLength(1);
  });

  it('숨긴 뒤 새 신호(서명 바뀜)가 오면 다시 보인다', () => {
    let s = synced();
    s = reduce(s, { type: 'hideItem', id: 'it_a' });
    s = applyEvent(s, ev({ event: 'item', data: item({ score: 95.4 }) }));
    expect(visibleItems(s, NOW)).toHaveLength(0); // 같은 서명이면 계속 숨김
    s = applyEvent(s, ev({ event: 'item', data: item({ signature: 'approval:2' }) }));
    expect(visibleItems(s, NOW).map((i) => i.id)).toEqual(['it_a']);
  });

  it('스냅샷이 와도 같은 서명이면 숨김을 지킨다', () => {
    let s = synced();
    s = reduce(s, { type: 'hideItem', id: 'it_a' });
    s = applyEvent(s, ev({ event: 'snapshot', data: snap() }));
    expect(s.hidden).toEqual({ it_a: 'approval:1' });
    s = applyEvent(s, ev({ event: 'snapshot', data: snap({ items: [item({ signature: 'approval:9' })] }) }));
    expect(s.hidden).toEqual({});
  });

  it('미뤄 둔(snooze) 항목은 시각이 지나기 전까지 숨는다', () => {
    let s = synced();
    s = applyEvent(s, ev({ event: 'item', data: item({ snooze_until_ms: NOW + 60_000 }) }));
    expect(visibleItems(s, NOW)).toHaveLength(0);
    expect(visibleItems(s, NOW + 61_000)).toHaveLength(1);
  });
});

describe('presence / deliver / reply / proposal / sessions', () => {
  it('presence 를 바꾼다', () => {
    const s = applyEvent(synced(), ev({ event: 'presence', data: { ...snap().presence, mood: 'alert', line: '실패' } }));
    expect(s.presence.mood).toBe('alert');
    expect(s.presence.line).toBe('실패');
  });

  it('deliver 는 마지막 전달과 순번을 남긴다(말풍선은 컨트롤러가 띄운다)', () => {
    const d = { item: 'it_a', level: 'interrupt' as const, line: '4분째 기다려요', speak: true, reasons: ['r'], at_ms: NOW };
    const s = applyEvent(synced(), ev({ event: 'deliver', data: d }));
    expect(s.lastDelivery).toEqual(d);
    expect(s.deliverySeq).toBe(1);
  });

  it('reply 는 대화에 붙고 같은 답은 한 번만 붙는다', () => {
    const r = { session: 's1', project: 'proj', chat_id: 'c1', text: '네', at_ms: NOW };
    let s = applyEvent(synced(), ev({ event: 'reply', data: r }));
    s = applyEvent(s, ev({ event: 'reply', data: r }));
    expect(s.chat).toHaveLength(1);
    expect(s.chat[0]).toMatchObject({ role: 'claude', text: '네', project: 'proj' });
  });

  it('chat_id 가 §10.1 꼴이 아니면 버린다', () => {
    const base = { session: 's1', project: 'proj', text: '네', at_ms: NOW };
    let s = synced();
    for (const bad of ['', 'a b', 'x'.repeat(65), '<img>', 7 as unknown as string]) {
      s = applyEvent(s, ev({ event: 'reply', data: { ...base, chat_id: bad } }));
    }
    expect(s.chat).toHaveLength(0);
    s = applyEvent(s, ev({ event: 'reply', data: { ...base, chat_id: 'm17' } }));
    s = applyEvent(s, ev({ event: 'reply', data: { ...base, chat_id: 'A_z-9'.padEnd(64, 'x'), at_ms: NOW + 1 } }));
    expect(s.chat).toHaveLength(2);
  });

  it('그 세션의 답이 오면 보낸 글을 "답 받음"으로 바꾸고 안내를 거둔다', () => {
    let s = synced();
    s = reduce(s, { type: 'chatAdd', msg: { id: 'm1', role: 'me', text: '안녕', at_ms: NOW, session: 's1', project: 'proj', status: 'sent', deliveredTo: 's1', noReply: true } });
    s = reduce(s, { type: 'chatAdd', msg: { id: 'm2', role: 'me', text: '다른 곳', at_ms: NOW, session: 's9', project: 'p9', status: 'sent', deliveredTo: 's9' } });
    s = applyEvent(s, ev({ event: 'reply', data: { session: 's1', project: 'proj', chat_id: 'm17', text: '네', at_ms: NOW + 5 } }));
    expect(s.chat.find((m) => m.id === 'm1')).toMatchObject({ answered: true, noReply: false });
    expect(s.chat.find((m) => m.id === 'm2')!.answered).toBeUndefined();
  });

  it(`대화는 ${CHAT_LIMIT}개까지만 둔다`, () => {
    let s = synced();
    for (let i = 0; i < CHAT_LIMIT + 5; i++) {
      s = applyEvent(s, ev({ event: 'reply', data: { session: null, project: null, chat_id: `c${i}`, text: `${i}`, at_ms: NOW + i } }));
    }
    expect(s.chat).toHaveLength(CHAT_LIMIT);
    expect(s.chat[0]!.text).toBe('5');
  });

  it('proposal 은 같은 id 면 갱신하고 아니면 더한다', () => {
    const p = { id: 'pr_unread_quiet', kind: 'unread' as const, change: 'quiet', line: 'a', evidence: { dismissed: 5, total: 6, window_days: 14 } };
    let s = applyEvent(synced(), ev({ event: 'proposal', data: p }));
    s = applyEvent(s, ev({ event: 'proposal', data: { ...p, line: 'b' } }));
    expect(s.proposals).toEqual([{ ...p, line: 'b' }]);
  });

  it('제안을 낙관적으로 숨기고, 성공하면 지우고, 실패하면 되돌린다', () => {
    const p = { id: 'pr_1', kind: 'unread' as const, change: 'quiet', line: 'a', evidence: { dismissed: 5, total: 6, window_days: 14 } };
    let s = applyEvent(synced(), ev({ event: 'proposal', data: p }));
    s = reduce(s, { type: 'hideProposal', id: 'pr_1' });
    expect(visibleProposals(s)).toHaveLength(0);
    s = reduce(s, { type: 'unhideProposal', id: 'pr_1' });
    expect(visibleProposals(s)).toHaveLength(1);
    s = reduce(s, { type: 'removeProposal', id: 'pr_1' });
    expect(s.proposals).toHaveLength(0);
  });

  it('sessions 는 통째로 바꾸고 빈 channel 은 null 로 본다', () => {
    const s = applyEvent(
      synced(),
      ev({
        event: 'sessions',
        data: [{ id: 's2', agent: 'codex', project: 'p2', cwd: '', state: 'idle', last_ms: 1, channel: '' as unknown as null }],
      }),
    );
    expect(s.sessions).toEqual([{ id: 's2', agent: 'codex', project: 'p2', cwd: '', state: 'idle', last_ms: 1, channel: null }]);
  });

  it('모르는 이벤트는 무시한다', () => {
    const s = synced();
    expect(applyEvent(s, { event: 'mystery', data: {} } as unknown as SseEvent)).toBe(s);
  });
});

describe('고르개', () => {
  it('visibleItems 는 점수순', () => {
    const s = applyEvent(initialState(), ev({ event: 'snapshot', data: demoSnapshot(NOW) }));
    const scores = visibleItems(s, NOW).map((i) => i.score);
    expect(scores).toEqual([...scores].sort((a, b) => b - a));
    expect(visibleItems(s, NOW)[0]!.kind).toBe('approval');
  });

  it('chatTargets 는 channel 있는 세션 먼저, 끝난 세션은 뺀다', () => {
    const t = chatTargets([
      { id: 'a', agent: 'codex', project: 'a', cwd: '', state: 'idle', last_ms: 30, channel: null },
      { id: 'b', agent: 'claude-code', project: 'b', cwd: '', state: 'idle', last_ms: 10, channel: 'ch' },
      { id: 'c', agent: 'claude-code', project: 'c', cwd: '', state: 'ended', last_ms: 50, channel: null },
      { id: 'd', agent: 'claude-code', project: 'd', cwd: '', state: 'working', last_ms: 20, channel: 'ch2' },
    ]);
    expect(t.map((s) => s.id)).toEqual(['d', 'b', 'a']);
  });

  it('focusUntil 은 집중 모드 시각만 바꾼다', () => {
    const s = reduce(synced(), { type: 'focusUntil', until_ms: NOW + 1000 });
    expect(s.presence.focus_mode_until_ms).toBe(NOW + 1000);
    expect(s.presence.mood).toBe('waiting');
  });
});

describe('집중·잠잘 때의 답(§12.4)', () => {
  const synced = (presence: Partial<AppState['presence']>) =>
    applyEvent(initialState('open'), { event: 'snapshot', data: { ...demoSnapshot(NOW), presence: { ...demoSnapshot(NOW).presence, ...presence } } }, NOW);

  it('집중 모드가 앞날이거나 기분이 sleep 이면 조용히, 아니면 아니다', () => {
    expect(quietReplies(synced({ mood: 'waiting', focus_mode_until_ms: NOW + 60_000 }), NOW)).toBe(true);
    expect(quietReplies(synced({ mood: 'sleep', focus_mode_until_ms: null }), NOW)).toBe(true);
    expect(quietReplies(synced({ mood: 'waiting', focus_mode_until_ms: NOW - 1 }), NOW)).toBe(false);
    expect(quietReplies(synced({ mood: 'idle', focus_mode_until_ms: null }), NOW)).toBe(false);
  });

  it('집중 모드 끝은 서버 시계로 잰다(현지 시계와 차이 반영)', () => {
    // 서버가 현지보다 5분 앞섬: 서버 기준 집중은 2분 남았다
    const s = applyEvent(
      initialState('open'),
      { event: 'snapshot', data: { ...demoSnapshot(NOW + 300_000), presence: { ...demoSnapshot(NOW).presence, mood: 'waiting', focus_mode_until_ms: NOW + 420_000 } } },
      NOW,
    );
    expect(quietReplies(s, NOW + 60_000)).toBe(true);
    expect(quietReplies(s, NOW + 121_000)).toBe(false);
  });

  it('조용할 때 온 답은 안 읽은 답으로 센다(같은 답은 한 번), 대화를 보면 지운다', () => {
    let s = synced({ mood: 'sleep' });
    expect(s.unreadReplies).toBe(0);
    const r: SseEvent = { event: 'reply', data: demoReply(NOW) };
    s = applyEvent(s, r, NOW);
    s = applyEvent(s, r, NOW);
    expect(s.chat).toHaveLength(1);
    expect(s.unreadReplies).toBe(1);
    s = applyEvent(s, { event: 'reply', data: { ...demoReply(NOW + 1), chat_id: 'm2' } }, NOW);
    expect(s.unreadReplies).toBe(2);
    s = reduce(s, { type: 'chatSeen' });
    expect(s.unreadReplies).toBe(0);
  });

  it('조용하지 않을 때 온 답은 세지 않는다(말풍선으로 보인다)', () => {
    const s = applyEvent(synced({ mood: 'waiting', focus_mode_until_ms: null }), { event: 'reply', data: demoReply(NOW) }, NOW);
    expect(s.chat).toHaveLength(1);
    expect(s.unreadReplies).toBe(0);
  });
});
