import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { HttpBackend } from './backend.ts';
import { resetTauriMemo } from '../lib/tauri.ts';
import type { ConnState } from '../state/reducer.ts';

const KEY = 'ab'.repeat(32);
const KEY2 = 'cd'.repeat(32);
const CODE = 'ef'.repeat(32);

type TauriWin = Window & { __TAURI__?: { core: { invoke: ReturnType<typeof vi.fn> } } };
type Route = (init: RequestInit) => Response | Promise<Response>;

/** 길마다 답을 정하는 가짜 데몬. 요청은 모두 적어 둔다. */
function fakeDaemon(routes: Record<string, Route | Route[]>) {
  const calls: { path: string; init: RequestInit }[] = [];
  const streams: ReadableStreamDefaultController<Uint8Array>[] = [];
  const fetchImpl = vi.fn(async (url: string, init: RequestInit = {}) => {
    const path = url.replace(/^https?:\/\/[^/]+/, '');
    calls.push({ path, init });
    const r = routes[path];
    const route = Array.isArray(r) ? (r.length > 1 ? r.shift()! : r[0]!) : r;
    if (!route) return new Response('{}', { status: 404 });
    return route(init);
  });
  const sse: Route = (init) => {
    const body = new ReadableStream<Uint8Array>({ start: (c) => void streams.push(c) });
    init.signal?.addEventListener('abort', () => {
      try {
        streams.at(-1)?.error(new DOMException('aborted', 'AbortError'));
      } catch {
        /* 닫힘 */
      }
    });
    return new Response(body, { status: 200, headers: { 'Content-Type': 'text/event-stream' } });
  };
  return { fetchImpl: fetchImpl as unknown as typeof fetch, calls, streams, sse };
}

const status = (code: number) => () => new Response('{"ok":false}', { status: code });
const down = () => Promise.reject(new TypeError('fetch failed'));
const sessionOk = () => new Response(JSON.stringify({ session: KEY2, expires_ms: Date.now() + 7 * 86_400_000 }), { status: 200 });
const flush = async () => {
  for (let i = 0; i < 5; i++) await vi.advanceTimersByTimeAsync(0);
};
const storeKey = (session = KEY) => sessionStorage.setItem('gyeot.session', JSON.stringify({ session, expires_ms: Date.now() + 86_400_000 }));

let backend: HttpBackend | null = null;

function setup(d: ReturnType<typeof fakeDaemon>, bootCode: string | null = null) {
  const statuses: ConnState[] = [];
  backend = new HttpBackend('', { fetchImpl: d.fetchImpl, bootCode });
  backend.start({ event: () => {}, status: (c) => statuses.push(c) });
  return { b: backend, statuses };
}

function inShell() {
  const invoke = vi.fn(async (_cmd: string) => undefined);
  (window as TauriWin).__TAURI__ = { core: { invoke } };
  return invoke;
}

beforeEach(() => {
  resetTauriMemo();
  sessionStorage.clear();
  vi.useFakeTimers();
});
afterEach(() => {
  backend?.stop();
  backend = null;
  delete (window as TauriWin).__TAURI__;
  sessionStorage.clear();
  vi.useRealTimers();
});

describe('부트 코드 → 세션 열쇠(§12.1-3)', () => {
  it('부트 코드를 /v1/session 으로 바꾼 뒤, 그 열쇠를 Authorization 에 실어 스트림을 연다', async () => {
    const d = fakeDaemon({ '/v1/session': sessionOk, '/v1/stream': (i) => d.sse(i) });
    const { statuses } = setup(d, CODE);
    await flush();
    expect(d.calls.map((c) => c.path)).toEqual(['/v1/session', '/v1/stream']);
    expect(d.calls[0]!.init.headers).toEqual({ 'Content-Type': 'application/json' });
    expect(d.calls[1]!.init.headers).toEqual({ Authorization: `Bearer ${KEY2}`, Accept: 'text/event-stream' });
    expect(JSON.parse(sessionStorage.getItem('gyeot.session')!).session).toBe(KEY2);
    expect(statuses.at(-1)?.status).toBe('open');
  });

  it('/v1/session 이 401 이면 401 과 같이 다룬다 — 저장된 열쇠도 지우고, 셸이면 renew, 스트림 요청 없음', async () => {
    const invoke = inShell();
    storeKey();
    const d = fakeDaemon({ '/v1/session': status(401), '/v1/stream': (i) => d.sse(i) });
    const { statuses } = setup(d, CODE);
    await flush();
    expect(d.calls.map((c) => c.path)).toEqual(['/v1/session']);
    expect(sessionStorage.getItem('gyeot.session')).toBeNull();
    expect(statuses.at(-1)?.status).toBe('auth');
    expect(invoke).toHaveBeenCalledWith('renew_session');
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(d.calls).toHaveLength(1);
  });

  it('/v1/session 에 닿지 않으면 코드를 쥐고 다시 잇기 때 다시 바꾼다', async () => {
    const d = fakeDaemon({ '/v1/session': [down, sessionOk], '/v1/stream': (i) => d.sse(i) });
    const { statuses } = setup(d, CODE);
    await flush();
    expect(statuses.at(-1)).toMatchObject({ status: 'retrying', attempt: 1 });
    await vi.advanceTimersByTimeAsync(1_000);
    await flush();
    expect(d.calls.map((c) => c.path)).toEqual(['/v1/session', '/v1/session', '/v1/stream']);
    expect(statuses.at(-1)?.status).toBe('open');
  });
});

describe('열쇠가 없을 때(401, §12.1-5)', () => {
  it('열쇠도 boot 도 없이 열면 요청을 하나도 보내지 않고 처음부터 안내(auth)', async () => {
    const d = fakeDaemon({});
    const { b, statuses } = setup(d);
    await flush();
    expect(statuses.at(-1)).toMatchObject({ status: 'auth', retryAt: null });
    b.retryNow();
    window.dispatchEvent(new Event('online'));
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(d.calls).toEqual([]);
  });

  it('셸 안에서 열쇠 없이 열리면 renew_session 을 한 번 청한다', async () => {
    const invoke = inShell();
    const d = fakeDaemon({});
    setup(d);
    await flush();
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('renew_session');
    expect(d.calls).toEqual([]);
  });

  it('셸 안에서 스트림이 401 이면 열쇠를 지우고 renew_session 을 청하고, 서버를 다시 두드리지 않는다', async () => {
    const invoke = inShell();
    storeKey();
    const d = fakeDaemon({ '/v1/stream': status(401) });
    const { statuses } = setup(d);
    await flush();
    expect(d.calls[0]!.init.headers).toMatchObject({ Authorization: `Bearer ${KEY}` });
    expect(sessionStorage.getItem('gyeot.session')).toBeNull();
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('renew_session');
    expect(statuses.at(-1)).toMatchObject({ status: 'auth', retryAt: null });
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(d.calls).toHaveLength(1);
  });

  it('30초 안에 또 눌러도 renew 는 한 번만, 30초가 지나면 다시 청한다(요청은 더 보내지 않는다)', async () => {
    const invoke = inShell();
    storeKey();
    const d = fakeDaemon({ '/v1/stream': status(401) });
    const { b } = setup(d);
    await flush();
    b.retryNow();
    await flush();
    expect(invoke).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(30_000);
    b.retryNow();
    await flush();
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(d.calls).toHaveLength(1);
  });

  it('브라우저(셸 밖)에서는 renew 없이 auth 로 멈추고, 다시 확인을 눌러도 안내가 깜빡이지 않는다', async () => {
    storeKey();
    const d = fakeDaemon({ '/v1/stream': status(401) });
    const { b, statuses } = setup(d);
    await flush();
    expect(statuses.at(-1)?.status).toBe('auth');
    const n = statuses.length;
    b.retryNow();
    document.dispatchEvent(new Event('visibilitychange'));
    await flush();
    expect(statuses.slice(n).every((c) => c.status === 'auth')).toBe(true);
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(d.calls).toHaveLength(1);
  });

  it('API 요청이 401 이면 열린 스트림도 멈추고 열쇠를 지우고 셸에 새 열쇠를 청한다(오류는 그대로 던진다)', async () => {
    const invoke = inShell();
    storeKey();
    const d = fakeDaemon({ '/v1/stream': (i) => d.sse(i), '/v1/items/it_1/action': status(401) });
    const { b, statuses } = setup(d);
    await flush();
    expect(statuses.at(-1)?.status).toBe('open');
    await expect(b.itemAction('it_1', 'done')).rejects.toMatchObject({ status: 401 });
    expect((d.calls[0]!.init.signal as AbortSignal).aborted).toBe(true);
    expect(statuses.at(-1)).toMatchObject({ status: 'auth', retryAt: null });
    expect(sessionStorage.getItem('gyeot.session')).toBeNull();
    expect(invoke).toHaveBeenCalledWith('renew_session');
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(d.calls).toHaveLength(2);
  });

  it('401 이 아닌 실패는 열쇠 문제로 보지 않는다', async () => {
    const invoke = inShell();
    storeKey();
    const d = fakeDaemon({ '/v1/stream': (i) => d.sse(i), '/v1/focus': status(500) });
    const { b, statuses } = setup(d);
    await flush();
    await expect(b.focus(true, 30)).rejects.toMatchObject({ status: 500 });
    expect((d.calls[0]!.init.signal as AbortSignal).aborted).toBe(false);
    expect(statuses.at(-1)?.status).toBe('open');
    expect(sessionStorage.getItem('gyeot.session')).not.toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });

  it('데몬이 꺼진 것(401 아님)은 전처럼 간격을 늘려 다시 잇는다', async () => {
    storeKey();
    const d = fakeDaemon({ '/v1/stream': down });
    const { statuses } = setup(d);
    await flush();
    expect(statuses.at(-1)).toMatchObject({ status: 'retrying', attempt: 1 });
    await vi.advanceTimersByTimeAsync(1_000);
    expect(d.calls).toHaveLength(2);
  });
});
