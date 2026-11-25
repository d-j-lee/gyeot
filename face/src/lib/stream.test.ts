import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { backoffDelay, EventStream, STREAM_IDLE_MS } from './stream.ts';
import { ApiError } from './api.ts';
import type { ConnState } from '../state/reducer.ts';
import type { SseEvent } from '../types.ts';

/** 손으로 조각을 밀어 넣는 SSE 응답 하나 */
class FakeConn {
  static all: FakeConn[] = [];
  readonly signal: AbortSignal;
  private ctl!: ReadableStreamDefaultController<Uint8Array>;
  private settle!: { ok: (r: Response) => void; no: (e: unknown) => void };
  readonly result: Promise<Response>;
  cancelled = false;
  constructor(signal: AbortSignal) {
    this.signal = signal;
    FakeConn.all.push(this);
    this.result = new Promise<Response>((ok, no) => (this.settle = { ok, no }));
  }
  /** 머리가 왔다(200 text/event-stream) */
  accept(): void {
    const body = new ReadableStream<Uint8Array>({
      start: (c) => {
        this.ctl = c;
      },
      cancel: () => {
        this.cancelled = true;
      },
    });
    this.signal.addEventListener('abort', () => {
      try {
        this.ctl.error(new DOMException('aborted', 'AbortError'));
      } catch {
        /* 이미 닫힘 */
      }
    });
    this.settle.ok(new Response(body, { status: 200, headers: { 'Content-Type': 'text/event-stream' } }));
  }
  reject(e: unknown): void {
    this.settle.no(e);
  }
  send(text: string | Uint8Array): void {
    this.ctl.enqueue(typeof text === 'string' ? new TextEncoder().encode(text) : text);
  }
  end(): void {
    this.ctl.close();
  }
  breakDown(): void {
    this.ctl.error(new TypeError('network error'));
  }
}

const flush = async () => {
  for (let i = 0; i < 5; i++) await vi.advanceTimersByTimeAsync(0);
};

beforeEach(() => {
  FakeConn.all = [];
  vi.useFakeTimers();
});
afterEach(() => vi.useRealTimers());

function start() {
  const events: SseEvent[] = [];
  const statuses: ConnState[] = [];
  const onAuthLost = vi.fn();
  const open = vi.fn((signal: AbortSignal) => new FakeConn(signal).result);
  const s = new EventStream({
    open,
    onEvent: (e) => events.push(e),
    onStatus: (c) => statuses.push(c),
    onAuthLost,
  });
  s.start();
  return { s, events, statuses, onAuthLost, open };
}

describe('다시 잇기 간격', () => {
  it('1→2→4→8→16→30초, 최대 30초', () => {
    expect([1, 2, 3, 4, 5, 6, 7, 20].map(backoffDelay)).toEqual([1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000]);
  });
});

describe('EventStream(fetch 스트림, §8·§12.1-3)', () => {
  it('이름 붙은 이벤트를 JSON 으로 풀어 넘긴다 — 조각난 청크·여러 줄 data·ping·깨진 줄·모르는 이름', async () => {
    const { events, statuses } = start();
    const c = FakeConn.all[0]!;
    c.accept();
    await flush();
    expect(statuses.map((x) => x.status)).toEqual(['connecting', 'open']);
    c.send(': hello\n\nevent: pres');
    c.send('ence\ndata: {"mood":\ndata: "waiting"}\n');
    await flush();
    expect(events).toEqual([]); // 빈 줄 전에는 내보내지 않는다
    c.send('\n: ping\n\nevent: item_removed\ndata: {oops\n\nevent: nope\ndata: {}\n\n');
    // '가'(EA B0 80)를 바이트 사이에서 자른다
    const bytes = new TextEncoder().encode('event: item_removed\ndata: {"id":"가"}\n\n');
    const cut = bytes.indexOf(0xb0);
    c.send(bytes.slice(0, cut));
    c.send(bytes.slice(cut));
    await flush();
    expect(events).toEqual([
      { event: 'presence', data: { mood: 'waiting' } },
      { event: 'item_removed', data: { id: '가' } },
    ]);
  });

  it('끊기면(연결 실패·스트림 끝·오류) 간격을 늘려 가며 다시 잇고, 이어지면 처음으로 돌아간다', async () => {
    const { statuses, open } = start();
    FakeConn.all[0]!.reject(new TypeError('fetch failed'));
    await flush();
    expect(statuses.at(-1)).toMatchObject({ status: 'retrying', attempt: 1 });
    await vi.advanceTimersByTimeAsync(999);
    expect(open).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(open).toHaveBeenCalledTimes(2);
    FakeConn.all[1]!.reject(new ApiError(503, 'x'));
    await flush();
    await vi.advanceTimersByTimeAsync(1999);
    expect(open).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(open).toHaveBeenCalledTimes(3);
    FakeConn.all[2]!.accept();
    await flush();
    expect(statuses.at(-1)).toMatchObject({ status: 'open', attempt: 0 });
    FakeConn.all[2]!.end(); // 데몬이 내림
    await flush();
    expect(statuses.at(-1)).toMatchObject({ status: 'retrying', attempt: 1 });
    await vi.advanceTimersByTimeAsync(1000);
    expect(open).toHaveBeenCalledTimes(4);
    FakeConn.all[3]!.accept();
    await flush();
    FakeConn.all[3]!.breakDown();
    await flush();
    expect(statuses.at(-1)).toMatchObject({ status: 'retrying', attempt: 1 });
  });

  it('열쇠 문제(401)면 auth 로 알리고 저절로 다시 잇지 않는다 — 지금 다시(retryNow)로만', async () => {
    const { s, statuses, onAuthLost, open } = start();
    FakeConn.all[0]!.reject(new ApiError(401, 'GET /v1/stream → 401'));
    await flush();
    expect(statuses.at(-1)).toEqual({ status: 'auth', attempt: 1, retryAt: null });
    expect(onAuthLost).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(open).toHaveBeenCalledTimes(1);
    s.retryNow();
    expect(open).toHaveBeenCalledTimes(2);
    FakeConn.all[1]!.accept();
    await flush();
    expect(statuses.at(-1)?.status).toBe('open');
  });

  it('authLost() 는 열린 연결도 끊고(abort) 다시 잇지 않는다(API 가 401 을 받았을 때)', async () => {
    const { s, statuses, open } = start();
    FakeConn.all[0]!.accept();
    await flush();
    s.authLost();
    expect(FakeConn.all[0]!.signal.aborted).toBe(true);
    expect(statuses.at(-1)).toMatchObject({ status: 'auth', retryAt: null });
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(open).toHaveBeenCalledTimes(1);
  });

  it('세대: 다시 이은 뒤 늦게 끝난 옛 연결의 결과(401·이벤트·끝남)는 버린다', async () => {
    const { s, statuses, events, onAuthLost, open } = start();
    const old = FakeConn.all[0]!;
    s.retryNow(); // 아직 머리가 안 온 연결을 버리고 새로
    expect(old.signal.aborted).toBe(true);
    expect(open).toHaveBeenCalledTimes(2);
    old.reject(new ApiError(401, 'late'));
    await flush();
    expect(statuses.some((c) => c.status === 'auth')).toBe(false);
    expect(onAuthLost).not.toHaveBeenCalled();

    const fresh = FakeConn.all[1]!;
    fresh.accept();
    await flush();
    s.retryNow(); // 열려 있으면 아무것도 하지 않는다
    expect(open).toHaveBeenCalledTimes(2);
    expect(statuses.at(-1)?.status).toBe('open');

    // 옛 연결이 늦게 머리를 받아도 새 연결을 건드리지 않는다
    const s2 = start();
    const a = FakeConn.all[2]!;
    s2.s.retryNow();
    const b = FakeConn.all[3]!;
    b.accept();
    await flush();
    a.accept();
    await flush();
    expect(a.cancelled || a.signal.aborted).toBe(true);
    b.send('event: presence\ndata: {"mood":"done"}\n\n');
    await flush();
    expect(s2.events).toEqual([{ event: 'presence', data: { mood: 'done' } }]);
    expect(s2.statuses.filter((c) => c.status === 'open')).toHaveLength(1);
    expect(events).toEqual([]);
  });

  it(`${STREAM_IDLE_MS / 1000}초 동안 아무것도(ping 도) 오지 않으면 끊고 다시 잇는다, ping 이 오면 그대로`, async () => {
    expect(STREAM_IDLE_MS).toBe(45_000);
    const { statuses, open } = start();
    const c = FakeConn.all[0]!;
    c.accept();
    await flush();
    for (let i = 0; i < 6; i++) {
      await vi.advanceTimersByTimeAsync(15_000);
      c.send(': ping\n\n');
      await flush();
    }
    expect(open).toHaveBeenCalledTimes(1);
    expect(statuses.at(-1)?.status).toBe('open');
    await vi.advanceTimersByTimeAsync(STREAM_IDLE_MS);
    await flush();
    expect(c.signal.aborted).toBe(true);
    expect(statuses.at(-1)).toMatchObject({ status: 'retrying', attempt: 1 });
    await vi.advanceTimersByTimeAsync(1_000);
    expect(open).toHaveBeenCalledTimes(2);
  });

  it('지금 다시는 기다리지 않고 잇는다, stop 뒤에는 잇지 않는다', async () => {
    const { s, open } = start();
    FakeConn.all[0]!.reject(new TypeError('x'));
    await flush();
    s.retryNow();
    expect(open).toHaveBeenCalledTimes(2);
    s.stop();
    expect(FakeConn.all[1]!.signal.aborted).toBe(true);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(open).toHaveBeenCalledTimes(2);
  });
});
