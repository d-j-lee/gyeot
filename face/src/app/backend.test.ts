// @vitest-environment-options {"url": "http://127.0.0.1:47321/"}
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createHash, createHmac } from 'node:crypto';
import { HttpBackend } from './backend.ts';
import { NotConnected } from '../lib/api.ts';
import { resetTauriMemo } from '../lib/tauri.ts';
import type { ConnState } from '../state/reducer.ts';

const KEY = 'ab'.repeat(32);
const KEY2 = 'cd'.repeat(32);
const CODE = 'ef'.repeat(32);

type TauriWin = Window & { __TAURI__?: { core: { invoke: ReturnType<typeof vi.fn> } } };
type Route = (init: RequestInit, url: string) => Response | Promise<Response>;

/** 시험 쪽에서 따로 계산한 얼굴 확인 값(§14.1). 포트는 이 파일의 location(47321). */
const proofOf = (key: string, n: string, port = '47321') =>
  createHmac('sha256', createHash('sha256').update(key, 'utf8').digest()).update(`gyeot-face-hello/v1|${port}|${n}`).digest('hex');

/**
 * 길마다 답을 정하는 가짜 데몬. 요청은 모두 적어 둔다(hello 는 calls 에서 빼고 hellos 에).
 * `/v1/hello` 는 따로 정하지 않으면 knows 의 열쇠를 아는 데몬처럼 답한다.
 */
function fakeDaemon(routes: Record<string, Route | Route[]>, knows: string[] = [KEY, KEY2]) {
  const calls: { path: string; init: RequestInit }[] = [];
  const hellos: string[] = [];
  const streams: ReadableStreamDefaultController<Uint8Array>[] = [];
  const state = { knows };
  const fetchImpl = vi.fn(async (url: string, init: RequestInit = {}) => {
    const path = url.replace(/^https?:\/\/[^/]+/, '');
    if (path.startsWith('/v1/hello?') && !routes['/v1/hello']) {
      const n = new URL(url, 'http://x').searchParams.get('n')!;
      hellos.push(JSON.stringify(init.headers ?? {}));
      return new Response(JSON.stringify({ v: 1, proof: '0'.repeat(64), face: state.knows.map((k) => proofOf(k, n)) }));
    }
    calls.push({ path: path.replace(/\?.*$/, ''), init });
    const r = routes[path.replace(/\?.*$/, '')];
    const route = Array.isArray(r) ? (r.length > 1 ? r.shift()! : r[0]!) : r;
    if (!route) return new Response('{}', { status: 404 });
    return route(init, url);
  });
  const sse = (init: RequestInit): Response => {
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
  /** 어떤 요청이든 Authorization 을 실은 수 */
  const authorized = () => calls.filter((c) => JSON.stringify(c.init.headers ?? {}).includes('Authorization')).length;
  return { fetchImpl: fetchImpl as unknown as typeof fetch, calls, hellos, streams, sse, state, authorized };
}

const status = (code: number) => () => new Response('{"ok":false}', { status: code });
const down = () => Promise.reject(new TypeError('fetch failed'));
const sessionOk = () => new Response(JSON.stringify({ session: KEY2, expires_ms: Date.now() + 7 * 86_400_000 }), { status: 200 });
// WebCrypto(얼굴 확인)는 Node 의 스레드 풀에서 끝나므로 가짜 시계만으로는 흘러가지 않는다 — 진짜 시간도 조금 준다.
const realTimeout = globalThis.setTimeout;
const flush = async () => {
  for (let i = 0; i < 20; i++) {
    await vi.advanceTimersByTimeAsync(0);
    await new Promise((r) => realTimeout(r, 2));
  }
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
    await flush();
    expect(d.calls).toHaveLength(2);
  });
});

/** /v1/hello 를 직접 정할 때: knows 열쇠를 아는 답 */
const helloKnowing =
  (knows: string[]): Route =>
  (_init, url) => {
    const n = new URL(url, 'http://x').searchParams.get('n')!;
    return new Response(JSON.stringify({ v: 1, proof: '0'.repeat(64), face: knows.map((k) => proofOf(k, n)) }));
  };

describe('열쇠를 보내기 전에 상대를 확인한다(§14.1, G-036)', () => {
  it('포트를 잡은 다른 서버(hello 가 내 열쇠를 모름)에는 열쇠를 보내지 않는다 — 지우고 401 처럼 안내, 되풀이 없음', async () => {
    storeKey();
    const d = fakeDaemon({ '/v1/stream': (i) => d.sse(i) }, []);
    const { b, statuses } = setup(d);
    await flush();
    expect(d.authorized()).toBe(0);
    expect(d.calls).toEqual([]);
    expect(d.hellos).toEqual(['{}']); // hello 에도 인증 머리 없음
    expect(statuses.at(-1)).toMatchObject({ status: 'auth', retryAt: null });
    expect(sessionStorage.getItem('gyeot.session')).toBeNull();
    b.retryNow();
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(d.hellos).toHaveLength(1);
    expect(d.calls).toEqual([]);
  });

  it('셸 안이면 renew_session 을 한 번 청한다', async () => {
    const invoke = inShell();
    storeKey();
    const d = fakeDaemon({ '/v1/stream': (i) => d.sse(i) }, []);
    setup(d);
    await flush();
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('renew_session');
    expect(d.authorized()).toBe(0);
  });

  it('다시 이을 때마다 다시 확인한다 — 끊긴 사이 포트를 잡은 서버에는 열쇠 0건(R5 H1 재현)', async () => {
    storeKey();
    const d = fakeDaemon({ '/v1/stream': (i) => d.sse(i) }, [KEY]);
    const { statuses } = setup(d);
    await flush();
    expect(statuses.at(-1)?.status).toBe('open');
    expect(d.authorized()).toBe(1);
    // 데몬이 멈추고 다른 사용자의 서버가 같은 포트를 잡는다
    d.state.knows = [];
    d.streams[0]!.close();
    await flush();
    expect(statuses.at(-1)).toMatchObject({ status: 'retrying', attempt: 1 });
    await vi.advanceTimersByTimeAsync(1_000);
    await flush();
    expect(d.hellos).toHaveLength(2);
    expect(d.authorized()).toBe(1); // 처음 스트림 하나뿐
    expect(d.calls.map((c) => c.path)).toEqual(['/v1/stream']);
    expect(statuses.at(-1)?.status).toBe('auth');
  });

  it('hello 가 실패하면(닿지 않음·5xx·꼴 이상) 끊긴 것으로 보고 다시 시도 간격을 따른다 — 열쇠는 보내지도 지우지도 않는다', async () => {
    storeKey();
    const d = fakeDaemon({
      '/v1/hello': [down, status(503), () => new Response(JSON.stringify({ v: 1, proof: 'x' })), helloKnowing([KEY])],
      '/v1/stream': (i) => d.sse(i),
    });
    const { statuses } = setup(d);
    await flush();
    expect(statuses.at(-1)).toMatchObject({ status: 'retrying', attempt: 1 });
    await vi.advanceTimersByTimeAsync(1_000);
    await flush();
    expect(statuses.at(-1)).toMatchObject({ status: 'retrying', attempt: 2 });
    await vi.advanceTimersByTimeAsync(2_000);
    await flush();
    expect(statuses.at(-1)).toMatchObject({ status: 'retrying', attempt: 3 });
    expect(d.authorized()).toBe(0);
    expect(sessionStorage.getItem('gyeot.session')).not.toBeNull();
    await vi.advanceTimersByTimeAsync(4_000);
    await flush();
    expect(statuses.at(-1)?.status).toBe('open');
    expect(d.calls.map((c) => c.path)).toEqual(['/v1/hello', '/v1/hello', '/v1/hello', '/v1/hello', '/v1/stream']);
    expect(d.authorized()).toBe(1);
  });

  it('데몬이 다시 뜨면(세션은 메모리에만) 열쇠가 무효 — 확인에서 걸러 401 처럼: 셸은 renew 한 번, 열쇠 0건, 되풀이 없음', async () => {
    const invoke = inShell();
    storeKey();
    const d = fakeDaemon({ '/v1/stream': (i) => d.sse(i) }, [KEY]);
    const { statuses } = setup(d);
    await flush();
    expect(d.authorized()).toBe(1);
    // 데몬이 죽었다가 다시 뜬다: 스트림이 끊기고, 다시 뜬 데몬은 이전 세션을 모른다
    d.state.knows = [];
    d.streams[0]!.error(new TypeError('network error'));
    await flush();
    expect(statuses.at(-1)?.status).toBe('retrying');
    await vi.advanceTimersByTimeAsync(1_000);
    await flush();
    expect(statuses.at(-1)).toMatchObject({ status: 'auth', retryAt: null });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(sessionStorage.getItem('gyeot.session')).toBeNull();
    expect(d.authorized()).toBe(1);
    const n = d.hellos.length;
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(d.hellos).toHaveLength(n);
    expect(d.authorized()).toBe(1);
    // 상태가 오락가락하지 않는다: auth 뒤로는 auth 만
    const firstAuth = statuses.findIndex((c) => c.status === 'auth');
    expect(statuses.slice(firstAuth).every((c) => c.status === 'auth')).toBe(true);
  });

  it('다시 뜬 데몬이 확인은 통과시켰는데(경합) 스트림이 401 이면 같은 흐름 — 브라우저는 안내, 되풀이 없음', async () => {
    storeKey();
    const d = fakeDaemon({ '/v1/stream': [(i) => d.sse(i), status(401)] }, [KEY]);
    const { statuses } = setup(d);
    await flush();
    d.streams[0]!.close();
    await flush();
    await vi.advanceTimersByTimeAsync(1_000);
    await flush();
    expect(statuses.at(-1)?.status).toBe('auth');
    expect(sessionStorage.getItem('gyeot.session')).toBeNull();
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(d.calls).toHaveLength(2);
  });

  it('REST 요청은 확인된 스트림이 열려 있을 때만 보낸다 — 확인 전·끊긴 동안은 요청 없이 NotConnected', async () => {
    storeKey();
    const d = fakeDaemon({ '/v1/stream': (i) => d.sse(i), '/v1/focus': () => new Response('{"ok":true,"until_ms":null}') }, [KEY]);
    const { b } = setup(d);
    await expect(b.focus(true, 30)).rejects.toBeInstanceOf(NotConnected); // 아직 확인 전
    await flush();
    await expect(b.focus(true, 30)).resolves.toMatchObject({ ok: true });
    expect(d.calls.find((c) => c.path === '/v1/focus')!.init.headers).toMatchObject({ Authorization: `Bearer ${KEY}` });
    // 끊긴 동안(그 포트를 누가 잡았는지 모른다)
    d.state.knows = [];
    d.streams[0]!.close();
    await flush();
    for (const p of [b.focus(false), b.itemAction('it_1', 'done'), b.say('안녕'), b.decide('dc_1', 'A'), b.proposal('pr_1', true), b.listening(true)]) {
      await expect(p).rejects.toBeInstanceOf(NotConnected);
    }
    expect(d.calls.filter((c) => c.path !== '/v1/stream').map((c) => c.path)).toEqual(['/v1/focus']);
    expect(d.authorized()).toBe(2);
  });
});
