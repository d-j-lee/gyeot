import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { backoffDelay, EventStream } from './stream.ts';
import type { ConnState } from '../state/reducer.ts';
import type { SseEvent } from '../types.ts';

class FakeES {
  static all: FakeES[] = [];
  url: string;
  readyState = 0;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  closed = false;
  private listeners = new Map<string, ((e: MessageEvent) => void)[]>();
  constructor(url: string) {
    this.url = url;
    FakeES.all.push(this);
  }
  addEventListener(name: string, fn: (e: MessageEvent) => void) {
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), fn]);
  }
  close() {
    this.closed = true;
    this.readyState = 2;
  }
  open() {
    this.readyState = 1;
    this.onopen?.();
  }
  emit(name: string, data: string) {
    for (const fn of this.listeners.get(name) ?? []) fn({ data } as MessageEvent);
  }
  fail() {
    this.onerror?.();
  }
}

beforeEach(() => {
  FakeES.all = [];
  vi.useFakeTimers();
});
afterEach(() => vi.useRealTimers());

function start(probe?: () => Promise<'ok' | 'auth' | 'down'>) {
  const events: SseEvent[] = [];
  const statuses: ConnState[] = [];
  const s = new EventStream({
    url: () => '/v1/stream?t=x',
    onEvent: (e) => events.push(e),
    onStatus: (c) => statuses.push(c),
    probe,
    EventSourceImpl: FakeES as unknown as typeof EventSource,
  });
  s.start();
  return { s, events, statuses };
}

describe('다시 잇기 간격', () => {
  it('1→2→4→8→16→30초, 최대 30초', () => {
    expect([1, 2, 3, 4, 5, 6, 7, 20].map(backoffDelay)).toEqual([1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000]);
  });
});

describe('EventStream', () => {
  it('이름 붙은 이벤트를 JSON 으로 풀어 넘기고, 깨진 줄은 버린다', () => {
    const { events, statuses } = start();
    const es = FakeES.all[0]!;
    es.open();
    es.emit('presence', JSON.stringify({ mood: 'waiting' }));
    es.emit('item_removed', '{oops');
    expect(events).toEqual([{ event: 'presence', data: { mood: 'waiting' } }]);
    expect(statuses.map((x) => x.status)).toEqual(['connecting', 'open']);
  });

  it('끊기면 간격을 늘려 가며 다시 잇고, 이어지면 처음으로 돌아간다', async () => {
    const { statuses } = start(async () => 'down');
    FakeES.all[0]!.fail();
    await vi.advanceTimersByTimeAsync(0);
    expect(statuses.at(-1)).toMatchObject({ status: 'retrying', attempt: 1 });
    await vi.advanceTimersByTimeAsync(999);
    expect(FakeES.all).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(FakeES.all).toHaveLength(2);
    FakeES.all[1]!.fail();
    await vi.advanceTimersByTimeAsync(1999);
    expect(FakeES.all).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(FakeES.all).toHaveLength(3);
    FakeES.all[2]!.open();
    expect(statuses.at(-1)).toMatchObject({ status: 'open', attempt: 0 });
    expect(FakeES.all[0]!.closed).toBe(true);
  });

  it('토큰 문제면 auth 로 알리고 30초 뒤에 다시 본다', async () => {
    const { statuses } = start(async () => 'auth');
    FakeES.all[0]!.fail();
    await vi.advanceTimersByTimeAsync(0);
    expect(statuses.at(-1)?.status).toBe('auth');
    await vi.advanceTimersByTimeAsync(29_999);
    expect(FakeES.all).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(FakeES.all).toHaveLength(2);
  });

  it('지금 다시는 기다리지 않고 잇는다, stop 뒤에는 잇지 않는다', async () => {
    const { s } = start(async () => 'down');
    FakeES.all[0]!.fail();
    await vi.advanceTimersByTimeAsync(0);
    s.retryNow();
    expect(FakeES.all).toHaveLength(2);
    s.stop();
    expect(FakeES.all[1]!.closed).toBe(true);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(FakeES.all).toHaveLength(2);
  });
});
