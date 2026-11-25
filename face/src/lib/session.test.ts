import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FaceAuth, readStoredSession, SESSION_STORAGE_KEY, takeBootCode, writeStoredSession } from './session.ts';

const KEY = 'ab'.repeat(32);
const KEY2 = 'cd'.repeat(32);
const CODE = 'ef'.repeat(32);
const NOW = 1_800_000_000_000;

function fakeHistory(href: string) {
  const loc = new URL(href) as unknown as Location;
  const replaceState = vi.fn((_state: unknown, _title: string, url?: string | URL | null) => {
    if (url) (loc as unknown as URL).href = new URL(String(url), (loc as unknown as URL).href).href;
  });
  return { loc, hist: { state: { keep: 1 }, replaceState } as unknown as History, replaceState };
}

beforeEach(() => sessionStorage.clear());
afterEach(() => sessionStorage.clear());

describe('주소에서 부트 코드 떼기(§12.1-3)', () => {
  it('boot 만 지우고 다른 쿼리·경로·조각은 글자 그대로 남긴다', () => {
    const { loc, hist, replaceState } = fakeHistory(`http://127.0.0.1:47321/?mode=overlay&boot=${CODE}&tab=chat#x`);
    expect(takeBootCode(loc, hist)).toBe(CODE);
    expect(replaceState).toHaveBeenCalledTimes(1);
    expect(replaceState.mock.calls[0]![0]).toEqual({ keep: 1 }); // history.state 는 그대로
    expect(replaceState.mock.calls[0]![2]).toBe('/?mode=overlay&tab=chat#x');
    expect(loc.search).toBe('?mode=overlay&tab=chat');
  });

  it('boot 가 맨 앞이거나 혼자여도', () => {
    const a = fakeHistory(`http://127.0.0.1:47321/?boot=${CODE}&mode=overlay`);
    expect(takeBootCode(a.loc, a.hist)).toBe(CODE);
    expect(a.replaceState.mock.calls[0]![2]).toBe('/?mode=overlay');
    const b = fakeHistory(`http://127.0.0.1:47321/?boot=${CODE}`);
    expect(takeBootCode(b.loc, b.hist)).toBe(CODE);
    expect(b.replaceState.mock.calls[0]![2]).toBe('/');
  });

  it('다른 쿼리를 다시 인코딩하지 않는다(api= 주소 등)', () => {
    const { loc, hist, replaceState } = fakeHistory(`http://127.0.0.1:5173/?api=http://127.0.0.1:47399/&boot=${CODE}&x=a%20b`);
    takeBootCode(loc, hist);
    expect(replaceState.mock.calls[0]![2]).toBe('/?api=http://127.0.0.1:47399/&x=a%20b');
  });

  it('boot 가 여러 개면 모두 지우고 첫 값을 쓴다, 인코딩된 이름(b%6Fot)도 boot 다', () => {
    const { loc, hist, replaceState } = fakeHistory(`http://127.0.0.1:47321/?boot=${CODE}&mode=overlay&b%6Fot=${KEY}&boot`);
    expect(takeBootCode(loc, hist)).toBe(CODE);
    expect(replaceState.mock.calls[0]![2]).toBe('/?mode=overlay');
  });

  it('빈 boot 는 지우되 코드로 쓰지 않는다', () => {
    const { loc, hist, replaceState } = fakeHistory('http://127.0.0.1:47321/?mode=overlay&boot=');
    expect(takeBootCode(loc, hist)).toBeNull();
    expect(replaceState.mock.calls[0]![2]).toBe('/?mode=overlay');
  });

  it('boot 가 없으면 주소를 건드리지 않는다', () => {
    const { loc, hist, replaceState } = fakeHistory('http://127.0.0.1:47321/?mode=overlay&booty=1&reboot=2');
    expect(takeBootCode(loc, hist)).toBeNull();
    expect(replaceState).not.toHaveBeenCalled();
  });
});

describe('sessionStorage 의 열쇠(§12.1-3)', () => {
  it(`이름은 ${'gyeot.session'}, 값은 {"session","expires_ms"} JSON`, () => {
    expect(SESSION_STORAGE_KEY).toBe('gyeot.session');
    writeStoredSession(sessionStorage, { session: KEY, expires_ms: NOW + 1000 });
    expect(JSON.parse(sessionStorage.getItem('gyeot.session')!)).toEqual({ session: KEY, expires_ms: NOW + 1000 });
    expect(readStoredSession(sessionStorage, NOW)).toEqual({ session: KEY, expires_ms: NOW + 1000 });
  });

  it('만료가 지났거나 꼴이 이상하면 버린다(지운다)', () => {
    for (const raw of [
      JSON.stringify({ session: KEY, expires_ms: NOW }),
      JSON.stringify({ session: KEY, expires_ms: NOW - 1 }),
      JSON.stringify({ session: 'nope', expires_ms: NOW + 1000 }),
      JSON.stringify({ session: KEY }),
      JSON.stringify([KEY]),
      '{oops',
    ]) {
      sessionStorage.setItem('gyeot.session', raw);
      expect(readStoredSession(sessionStorage, NOW), raw).toBeNull();
      expect(sessionStorage.getItem('gyeot.session'), raw).toBeNull();
    }
  });

  it('쿠키를 쓰지 않는다', () => {
    writeStoredSession(sessionStorage, { session: KEY, expires_ms: NOW + 1000 });
    expect(document.cookie).toBe('');
  });
});

describe('열쇠 얻기(FaceAuth)', () => {
  function sessionFetch(...responses: (Response | Error)[]) {
    return vi.fn(async (_url: string, _init?: RequestInit) => {
      const r = responses.shift();
      if (!r) throw new Error('예상하지 못한 요청');
      if (r instanceof Error) throw r;
      return r;
    });
  }
  const ok = (session = KEY2, expires_ms = NOW + 7 * 86_400_000) => new Response(JSON.stringify({ session, expires_ms }), { status: 200 });

  it('부트 코드가 있으면 POST /v1/session {code} 로 바꿔 sessionStorage 에 둔다(머리에 열쇠·쿠키 없음)', async () => {
    const fetchImpl = sessionFetch(ok());
    const auth = new FaceAuth({ base: 'http://d', code: CODE, fetchImpl: fetchImpl as unknown as typeof fetch, now: () => NOW });
    expect(auth.available()).toBe(true);
    expect(await auth.key()).toBe(KEY2);
    expect(fetchImpl).toHaveBeenCalledTimes(1);
    const [url, init] = fetchImpl.mock.calls[0]!;
    expect(url).toBe('http://d/v1/session');
    expect(init!.method).toBe('POST');
    expect(JSON.parse(init!.body as string)).toEqual({ code: CODE });
    expect(init!.credentials).toBe('omit');
    expect(JSON.stringify(init!.headers)).not.toMatch(/authorization|cookie/i);
    expect(JSON.parse(sessionStorage.getItem('gyeot.session')!)).toEqual({ session: KEY2, expires_ms: NOW + 7 * 86_400_000 });
    // 두 번째부터는 저장한 열쇠
    expect(await auth.key()).toBe(KEY2);
    expect(fetchImpl).toHaveBeenCalledTimes(1);
  });

  it('부트 코드는 한 번만 보낸다 — 동시에 여러 요청이 열쇠를 찾아도', async () => {
    let release: (r: Response) => void = () => {};
    const fetchImpl = vi.fn(() => new Promise<Response>((r) => (release = r)));
    const auth = new FaceAuth({ base: '', code: CODE, fetchImpl: fetchImpl as unknown as typeof fetch, now: () => NOW });
    const all = Promise.all([auth.key(), auth.key(), auth.key()]);
    release(ok());
    expect(await all).toEqual([KEY2, KEY2, KEY2]);
    expect(fetchImpl).toHaveBeenCalledTimes(1);
  });

  it('부트 코드가 새로 왔으면 저장된 열쇠보다 먼저 바꾼다', async () => {
    writeStoredSession(sessionStorage, { session: KEY, expires_ms: NOW + 1000 });
    const fetchImpl = sessionFetch(ok());
    const auth = new FaceAuth({ base: '', code: CODE, fetchImpl: fetchImpl as unknown as typeof fetch, now: () => NOW });
    expect(await auth.key()).toBe(KEY2);
  });

  it('/v1/session 이 401 이면 저장된 열쇠도 지우고 null — 다시 보내지 않는다', async () => {
    writeStoredSession(sessionStorage, { session: KEY, expires_ms: NOW + 1000 });
    const fetchImpl = sessionFetch(new Response('{"ok":false}', { status: 401 }));
    const auth = new FaceAuth({ base: '', code: CODE, fetchImpl: fetchImpl as unknown as typeof fetch, now: () => NOW });
    expect(await auth.key()).toBeNull();
    expect(sessionStorage.getItem('gyeot.session')).toBeNull();
    expect(auth.available()).toBe(false);
    expect(await auth.key()).toBeNull();
    expect(fetchImpl).toHaveBeenCalledTimes(1);
  });

  it('데몬에 닿지 않으면(네트워크·5xx) 던지고 코드를 쥐고 있다가 다음에 다시 바꾼다', async () => {
    const fetchImpl = sessionFetch(new TypeError('fetch failed'), new Response('x', { status: 503 }), ok());
    const auth = new FaceAuth({ base: '', code: CODE, fetchImpl: fetchImpl as unknown as typeof fetch, now: () => NOW });
    await expect(auth.key()).rejects.toBeInstanceOf(TypeError);
    expect(auth.available()).toBe(true);
    await expect(auth.key()).rejects.toMatchObject({ status: 503 });
    expect(await auth.key()).toBe(KEY2);
    expect(fetchImpl).toHaveBeenCalledTimes(3);
  });

  it('답의 꼴이 이상하면 열쇠로 쓰지 않는다(401 과 같이)', async () => {
    const fetchImpl = sessionFetch(new Response(JSON.stringify({ session: 'short', expires_ms: NOW + 1 }), { status: 200 }));
    const auth = new FaceAuth({ base: '', code: CODE, fetchImpl: fetchImpl as unknown as typeof fetch, now: () => NOW });
    expect(await auth.key()).toBeNull();
    expect(sessionStorage.getItem('gyeot.session')).toBeNull();
  });

  it('부트 코드가 없으면 저장된 열쇠를 쓴다(요청 없음), 만료면 버린다', async () => {
    const fetchImpl = vi.fn();
    writeStoredSession(sessionStorage, { session: KEY, expires_ms: NOW + 1000 });
    let now = NOW;
    const auth = new FaceAuth({ base: '', code: null, fetchImpl: fetchImpl as unknown as typeof fetch, now: () => now });
    expect(auth.available()).toBe(true);
    expect(await auth.key()).toBe(KEY);
    now = NOW + 1000;
    expect(auth.available()).toBe(false);
    expect(await auth.key()).toBeNull();
    expect(sessionStorage.getItem('gyeot.session')).toBeNull();
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it('열쇠도 부트 코드도 없으면 요청 없이 null', async () => {
    const fetchImpl = vi.fn();
    const auth = new FaceAuth({ base: '', code: null, fetchImpl: fetchImpl as unknown as typeof fetch, now: () => NOW });
    expect(auth.available()).toBe(false);
    expect(await auth.key()).toBeNull();
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it('invalidate(쓴 열쇠)는 그 열쇠일 때만 지운다(늦게 온 401 이 새 열쇠를 지우지 않게)', async () => {
    writeStoredSession(sessionStorage, { session: KEY, expires_ms: NOW + 1000 });
    const auth = new FaceAuth({ base: '', code: null, fetchImpl: vi.fn() as unknown as typeof fetch, now: () => NOW });
    expect(await auth.key()).toBe(KEY);
    auth.invalidate(KEY2);
    expect(await auth.key()).toBe(KEY);
    auth.invalidate(KEY);
    expect(await auth.key()).toBeNull();
    expect(sessionStorage.getItem('gyeot.session')).toBeNull();
  });

  it('sessionStorage 가 막혀도(던져도) 이 창의 메모리로 쓴다', async () => {
    const broken = {
      getItem: () => {
        throw new Error('denied');
      },
      setItem: () => {
        throw new Error('denied');
      },
      removeItem: () => {
        throw new Error('denied');
      },
    } as unknown as Storage;
    const fetchImpl = sessionFetch(ok());
    const auth = new FaceAuth({ base: '', code: CODE, fetchImpl: fetchImpl as unknown as typeof fetch, now: () => NOW, storage: broken });
    expect(await auth.key()).toBe(KEY2);
    expect(await auth.key()).toBe(KEY2);
    auth.invalidate(KEY2);
    expect(await auth.key()).toBeNull();
  });
});
