import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Delivery, Reply } from '../types.ts';
import { BUBBLE_MS, BubbleQueue, bubbleFromDelivery, bubbleFromReply, firstLine, type Bubble } from './bubble.ts';

let seq = 0;
function b(kind: Bubble['kind'], over: Partial<Bubble> = {}): Bubble {
  seq += 1;
  return {
    key: `k${seq}`,
    kind,
    line: `${kind} ${seq}`,
    reasons: [],
    item: null,
    project: null,
    session: null,
    at_ms: seq,
    speak: false,
    ...over,
  };
}

function setup() {
  const changes: (string | null)[] = [];
  const q = new BubbleQueue((cur) => changes.push(cur?.line ?? null));
  return { q, changes };
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe('전달 → 말풍선', () => {
  const d: Delivery = { item: 'it_a', level: 'nudge', line: '  기다려요 ', speak: false, reasons: ['r1'], at_ms: 10 };

  it('ambient 는 말풍선을 만들지 않는다(표정만)', () => {
    expect(bubbleFromDelivery({ ...d, level: 'ambient' })).toBeNull();
    expect(bubbleFromDelivery({ ...d, line: '   ' })).toBeNull();
  });

  it('nudge·interrupt 는 줄을 다듬고 이유·항목을 싣는다', () => {
    const out = bubbleFromDelivery({ ...d, level: 'interrupt', speak: true });
    expect(out).toMatchObject({ kind: 'interrupt', line: '기다려요', reasons: ['r1'], item: 'it_a', speak: true });
  });

  it('reply 는 첫 줄만, 길면 자른다', () => {
    const r: Reply = { session: 's', project: 'proj', chat_id: 'c', text: '\n\n  첫 줄이에요  \n둘째 줄', at_ms: 1 };
    const out = bubbleFromReply(r)!;
    expect(out.line).toBe('첫 줄이에요');
    expect(out.kind).toBe('reply');
    expect(out.reasons[0]).toContain('proj');
    expect(firstLine('가'.repeat(200), 10)).toBe(`${'가'.repeat(9)}…`);
    expect(bubbleFromReply({ ...r, text: '  \n ' })).toBeNull();
    expect(bubbleFromReply({ ...r, chat_id: 'bad id' })).toBeNull(); // §10.1
  });
});

describe('사라지는 시간', () => {
  it('요구 사항의 값: nudge 8초, reply 12초', () => {
    expect(BUBBLE_MS).toEqual({ nudge: 8_000, reply: 12_000 });
  });

  it('nudge 는 7.9초엔 남아 있고 8초에 사라진다(상수와 무관하게)', () => {
    const { q } = setup();
    q.push(b('nudge'));
    vi.advanceTimersByTime(7_900);
    expect(q.current).not.toBeNull();
    vi.advanceTimersByTime(100);
    expect(q.current).toBeNull();
  });

  it(`nudge 는 ${BUBBLE_MS.nudge / 1000}초 뒤 사라진다`, () => {
    const { q } = setup();
    q.push(b('nudge'));
    vi.advanceTimersByTime(BUBBLE_MS.nudge - 1);
    expect(q.current).not.toBeNull();
    vi.advanceTimersByTime(1);
    expect(q.current).toBeNull();
  });

  it(`reply 는 ${BUBBLE_MS.reply / 1000}초 뒤 사라진다`, () => {
    const { q } = setup();
    q.push(b('reply'));
    vi.advanceTimersByTime(BUBBLE_MS.nudge + 1);
    expect(q.current?.kind).toBe('reply');
    vi.advanceTimersByTime(BUBBLE_MS.reply - BUBBLE_MS.nudge);
    expect(q.current).toBeNull();
  });

  it('interrupt 는 넘길 때까지 남는다', () => {
    const { q } = setup();
    q.push(b('interrupt'));
    vi.advanceTimersByTime(60 * 60_000);
    expect(q.current?.kind).toBe('interrupt');
    q.ack();
    expect(q.current).toBeNull();
  });
});

describe('차례', () => {
  it('떠 있는 nudge 뒤에 줄 서고 차례로 보인다', () => {
    const { q, changes } = setup();
    q.push(b('nudge', { line: 'A' }));
    q.push(b('nudge', { line: 'B' }));
    expect(q.current?.line).toBe('A');
    vi.advanceTimersByTime(BUBBLE_MS.nudge);
    expect(q.current?.line).toBe('B');
    vi.advanceTimersByTime(BUBBLE_MS.nudge);
    expect(changes).toEqual(['A', 'B', null]);
  });

  it('interrupt 는 지나가는 말을 밀어낸다', () => {
    const { q } = setup();
    q.push(b('nudge', { line: 'A' }));
    q.push(b('interrupt', { line: 'URGENT' }));
    expect(q.current?.line).toBe('URGENT');
    vi.advanceTimersByTime(BUBBLE_MS.nudge * 3);
    expect(q.current?.line).toBe('URGENT');
  });

  it('interrupt 가 떠 있으면 새 interrupt 는 잔잔한 것보다 앞에 선다', () => {
    const { q } = setup();
    q.push(b('interrupt', { line: 'I1' }));
    q.push(b('nudge', { line: 'N1' }));
    q.push(b('interrupt', { line: 'I2' }));
    expect(q.queue.map((x) => x.line)).toEqual(['I2', 'N1']);
    q.ack();
    expect(q.current?.line).toBe('I2');
  });

  it('줄은 최대 3개, 넘치면 오래된 잔잔한 것부터 버린다', () => {
    const { q } = setup();
    q.push(b('interrupt', { line: 'I' }));
    for (const l of ['n1', 'n2', 'n3', 'n4']) q.push(b('nudge', { line: l }));
    expect(q.queue.map((x) => x.line)).toEqual(['n2', 'n3', 'n4']);
  });

  it('같은 열쇠는 한 번만(재연결 뒤 중복 막기)', () => {
    const { q } = setup();
    expect(q.push(b('nudge', { key: 'same' }))).toBe(true);
    expect(q.push(b('nudge', { key: 'same' }))).toBe(false);
    expect(q.queue).toHaveLength(0);
  });

  it('같은 항목의 새 전달은 그 자리에서 바꾸고, interrupt 를 nudge 로 내리지 않는다', () => {
    const { q } = setup();
    q.push(b('interrupt', { item: 'it_a', line: '2분째' }));
    q.push(b('nudge', { item: 'it_a', line: '4분째' }));
    expect(q.current).toMatchObject({ kind: 'interrupt', line: '4분째' });
    expect(q.queue).toHaveLength(0);
  });

  it('항목이 닫히면 그 말풍선을 거둔다(줄에 선 것도)', () => {
    const { q } = setup();
    q.push(b('interrupt', { item: 'it_a', line: 'A' }));
    q.push(b('nudge', { item: 'it_b', line: 'B' }));
    q.push(b('nudge', { item: 'it_c', line: 'C' }));
    q.removeItem('it_b');
    expect(q.queue.map((x) => x.line)).toEqual(['C']);
    q.removeItem('it_a');
    expect(q.current?.line).toBe('C');
  });
});

describe('멈춤(hover·focus·왜?)', () => {
  it('잡고 있는 동안 시계가 멈추고, 놓으면 남은 시간만큼(최소 2초) 더 보인다', () => {
    const { q } = setup();
    q.push(b('nudge'));
    vi.advanceTimersByTime(7_000);
    q.hold('hover', true);
    vi.advanceTimersByTime(60_000);
    expect(q.current).not.toBeNull();
    q.hold('hover', false);
    vi.advanceTimersByTime(1_999);
    expect(q.current).not.toBeNull();
    vi.advanceTimersByTime(1);
    expect(q.current).toBeNull();
  });

  it('사유가 여럿이면 모두 놓아야 다시 간다', () => {
    const { q } = setup();
    q.push(b('nudge'));
    q.hold('hover', true);
    q.hold('why', true);
    q.hold('hover', false);
    vi.advanceTimersByTime(30_000);
    expect(q.current).not.toBeNull();
    expect(q.isHeld).toBe(true);
    q.hold('why', false);
    vi.advanceTimersByTime(BUBBLE_MS.nudge);
    expect(q.current).toBeNull();
  });

  it('잡은 채로 새 말풍선이 떠도 시계는 놓을 때부터 간다', () => {
    const { q } = setup();
    q.hold('focus', true);
    q.push(b('nudge'));
    vi.advanceTimersByTime(BUBBLE_MS.nudge * 2);
    expect(q.current).not.toBeNull();
    q.hold('focus', false);
    vi.advanceTimersByTime(BUBBLE_MS.nudge);
    expect(q.current).toBeNull();
  });
});
