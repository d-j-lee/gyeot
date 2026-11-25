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
  const onAuthLost = vi.fn();
  const s = new EventStream({
    url: () => '/v1/stream',
    onEvent: (e) => events.push(e),
    onStatus: (c) => statuses.push(c),
    probe,
    onAuthLost,
    EventSourceImpl: FakeES as unknown as typeof EventSource,
  });
  s.start();
  return { s, events, statuses, onAuthLost };
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

  it('열쇠 문제(401)면 auth 로 알리고 저절로 다시 잇지 않는다 — 지금 다시(retryNow)로만', async () => {
    let probes = 0;
    const { s, statuses, onAuthLost } = start(async () => {
      probes += 1;
      return 'auth';
    });
    FakeES.all[0]!.fail();
    await vi.advanceTimersByTimeAsync(0);
    expect(statuses.at(-1)).toEqual({ status: 'auth', attempt: 1, retryAt: null });
    expect(onAuthLost).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(FakeES.all).toHaveLength(1);
    expect(probes).toBe(1);
    s.retryNow();
    expect(FakeES.all).toHaveLength(2);
    FakeES.all[1]!.open();
    expect(statuses.at(-1)?.status).toBe('open');
  });

  it('authLost() 는 열린 연결도 닫고 다시 잇지 않는다(API 가 401 을 받았을 때)', async () => {
    const { s, statuses } = start(async () => 'down');
    FakeES.all[0]!.open();
    s.authLost();
    expect(FakeES.all[0]!.closed).toBe(true);
    expect(statuses.at(-1)).toMatchObject({ status: 'auth', retryAt: null });
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(FakeES.all).toHaveLength(1);
  });

  it('원인을 가리는 동안 다시 이었으면, 늦게 온 판정은 버린다(겹친 연결 없음)', async () => {
    let resolve: (r: 'down' | 'auth') => void = () => {};
    const { s, statuses } = start(() => new Promise((r) => (resolve = r)));
    FakeES.all[0]!.fail();
    s.retryNow();
    expect(FakeES.all).toHaveLength(2);
    resolve('auth');
    await vi.advanceTimersByTimeAsync(60_000);
    expect(FakeES.all).toHaveLength(2);
    expect(FakeES.all[1]!.closed).toBe(false);
    expect(statuses.some((c) => c.status === 'auth')).toBe(false);
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
