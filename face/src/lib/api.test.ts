import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { ApiError, createApi, describeError, resolveApiBase } from './api.ts';
import { FaceAuth } from './session.ts';

const loc = (href: string) => new URL(href) as unknown as Location;

describe('열쇠(§12.1) — 얼굴은 토큰을 받지 않는다', () => {
  it('index.html 에 토큰 meta 가 없다', () => {
    const html = readFileSync(resolve(process.cwd(), 'index.html'), 'utf8'); // vitest 의 root = face/
    expect(html).not.toMatch(/gyeot-token/i);
  });

  it('토큰을 읽는 함수를 내보내지 않는다', async () => {
    const mod: Record<string, unknown> = await import('./api.ts');
    expect(mod.resolveToken).toBeUndefined();
  });

  it('얼굴 코드 어디에도 쿠키·EventSource 를 쓰지 않는다(G-030) — 열쇠는 sessionStorage 와 Authorization 머리뿐', () => {
    const root = resolve(process.cwd(), 'src');
    const files = (readdirSync(root, { recursive: true }) as string[])
      .filter((f) => /\.(ts|tsx)$/.test(f) && !/\.test\.(ts|tsx)$/.test(f))
      .map((f) => [f, readFileSync(resolve(root, f), 'utf8')] as const);
    expect(files.length).toBeGreaterThan(20);
    const bad: string[] = [];
    for (const [f, text] of files) {
      // 주석은 빼고 본다(설명에는 쿠키라는 말이 나올 수 있다).
      const code = text.replace(/\/\*[\s\S]*?\*\//g, '').replace(/(^|[^:])\/\/.*$/gm, '$1');
      for (const re of [/document\.cookie/, /credentials:\s*['"](same-origin|include)['"]/, /\bEventSource\b/, /withCredentials/]) {
        if (re.test(code)) bad.push(`${f}: ${re}`);
      }
    }
    expect(bad).toEqual([]);
  });
});

describe('API 주소(§12.1-5)', () => {
  afterEach(() => vi.unstubAllEnvs());

  it('배포 빌드는 ?api= 를 무시하고 늘 같은 출처', () => {
    for (const href of [
      'http://127.0.0.1:47321/',
      'http://127.0.0.1:47321/?api=http://127.0.0.1:47399/',
      'http://127.0.0.1:47321/?api=http://evil.example/',
      'http://127.0.0.1:47321/?mode=overlay&api=http://localhost:1/',
    ]) {
      expect(resolveApiBase(loc(href), false)).toBe('');
    }
  });

  it('기본값은 import.meta.env.DEV 를 따른다(배포 빌드 흉내)', () => {
    vi.stubEnv('DEV', false);
    expect(resolveApiBase(loc('http://127.0.0.1:47321/?api=http://127.0.0.1:47399/'))).toBe('');
    vi.stubEnv('DEV', true);
    expect(resolveApiBase(loc('http://127.0.0.1:5173/?api=http://127.0.0.1:47399/'))).toBe('http://127.0.0.1:47399');
  });

  it('배포 빌드(DEV=false)에서는 dev=true 를 넘겨도 무시한다 — 분기가 통째로 빠진다', () => {
    vi.stubEnv('DEV', false);
    expect(resolveApiBase(loc('http://127.0.0.1:47321/?api=http://127.0.0.1:47399/'), true)).toBe('');
  });

  it('개발 빌드는 루프백 http 주소일 때만 받고, 출처만 남긴다', () => {
    const dev = (api: string) => resolveApiBase(loc(`http://127.0.0.1:5173/?api=${encodeURIComponent(api)}`), true);
    expect(dev('http://127.0.0.1:47399/')).toBe('http://127.0.0.1:47399');
    expect(dev('http://localhost:47399/some/path?x=1')).toBe('http://localhost:47399');
    expect(dev('http://[::1]:47399')).toBe('http://[::1]:47399');
    expect(dev('http://127.0.0.1')).toBe('http://127.0.0.1');
  });

  it('개발 빌드라도 루프백 http 가 아니면 같은 출처', () => {
    const dev = (api: string) => resolveApiBase(loc(`http://127.0.0.1:5173/?api=${encodeURIComponent(api)}`), true);
    for (const bad of [
      'http://evil.example:47321',
      'http://127.0.0.1.evil.example',
      'http://localhost.evil.example',
      'http://127.0.0.1@evil.example/',
      'http://127.0.0.2:47321',
      'http://0.0.0.0:47321',
      'https://127.0.0.1:47321',
      'ws://127.0.0.1:47321',
      'javascript:alert(1)',
      '//127.0.0.1:47321',
      '/v1',
      'not a url',
    ]) {
      expect(dev(bad), bad).toBe('');
    }
  });

  it('Tauri 출처에서 열려도 데몬 주소로 바꾸지 않는다(§12.2, 셸은 데몬 주소를 직접 연다)', () => {
    expect(resolveApiBase(loc('http://tauri.localhost/?mode=overlay'), false)).toBe('');
    expect(resolveApiBase(loc('tauri://localhost/?mode=overlay'), true)).toBe('');
  });
});

describe('요청(§7·§12.1-3) — 세션 열쇠를 Authorization 머리로만', () => {
  const KEY = 'ab'.repeat(32);
  const NOW = 1_800_000_000_000;

  beforeEach(() => {
    sessionStorage.clear();
    sessionStorage.setItem('gyeot.session', JSON.stringify({ session: KEY, expires_ms: NOW + 60_000 }));
  });
  afterEach(() => sessionStorage.clear());

  function api(status = 200, body: unknown = { ok: true }) {
    const fetchImpl = vi.fn(async (_url: string, _init?: RequestInit) => new Response(JSON.stringify(body), { status }));
    const auth = new FaceAuth({ base: 'http://d', code: null, fetchImpl: fetchImpl as unknown as typeof fetch, now: () => NOW });
    return { a: createApi('http://d', { fetchImpl: fetchImpl as unknown as typeof fetch, auth }), fetchImpl, auth };
  }

  it('모든 요청에 Authorization: Bearer <세션 열쇠>, 쿠키는 싣지도 받지도 않는다(credentials omit)', async () => {
    const { a, fetchImpl } = api();
    await a.itemAction('it_1', 'dismiss', { reason: '과민' });
    await a.state();
    const [url, init] = fetchImpl.mock.calls[0]!;
    expect(url).toBe('http://d/v1/items/it_1/action');
    expect(init!.method).toBe('POST');
    expect(init!.credentials).toBe('omit');
    expect(init!.headers).toEqual({ Authorization: `Bearer ${KEY}`, 'Content-Type': 'application/json' });
    expect(JSON.parse(init!.body as string)).toEqual({ action: 'dismiss', reason: '과민' });
    const [getUrl, getInit] = fetchImpl.mock.calls[1]!;
    expect(getUrl).toBe('http://d/v1/state');
    expect(getInit!.credentials).toBe('omit');
    expect(getInit!.headers).toEqual({ Authorization: `Bearer ${KEY}` });
    for (const [u] of fetchImpl.mock.calls) expect(u).not.toMatch(/[?&](t|session|key)=/);
  });

  it('열쇠가 없으면 요청을 보내지 않고 401', async () => {
    sessionStorage.clear();
    const { a, fetchImpl } = api();
    await expect(a.state()).rejects.toMatchObject({ name: 'ApiError', status: 401 });
    await expect(a.focus(true, 30)).rejects.toMatchObject({ status: 401 });
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it('401 을 받으면 그 열쇠를 지운다 — 다음 요청은 보내지 않는다', async () => {
    const { a, fetchImpl } = api(401);
    await expect(a.state()).rejects.toMatchObject({ status: 401 });
    expect(sessionStorage.getItem('gyeot.session')).toBeNull();
    await expect(a.state()).rejects.toMatchObject({ status: 401 });
    expect(fetchImpl).toHaveBeenCalledTimes(1);
  });

  it('401 이 아닌 실패(403·500)는 열쇠를 지우지 않는다', async () => {
    for (const status of [403, 500]) {
      const { a } = api(status);
      await expect(a.state()).rejects.toMatchObject({ status });
      expect(sessionStorage.getItem('gyeot.session')).not.toBeNull();
    }
  });

  it('경로와 본문이 계약과 같다', async () => {
    const { a, fetchImpl } = api(200, { delivered_to: null, reason: 'r', ok: true, until_ms: null });
    await a.itemAction('it_1', 'snooze', { minutes: 1440 });
    await a.focus(true, 30);
    await a.focus(false);
    await a.listening(true);
    await a.say('안녕', 's1');
    await a.say('안녕');
    await a.decide('dc 1', '지금 적용');
    await a.proposal('pr_x', false);
    await a.state();
    const calls = fetchImpl.mock.calls.map(([u, i]) => [u.replace('http://d', ''), i!.method, i!.body ? JSON.parse(i!.body as string) : undefined]);
    expect(calls).toEqual([
      ['/v1/items/it_1/action', 'POST', { action: 'snooze', minutes: 1440 }],
      ['/v1/focus', 'POST', { on: true, minutes: 30 }],
      ['/v1/focus', 'POST', { on: false }],
      ['/v1/presence/listening', 'POST', { on: true }],
      ['/v1/say', 'POST', { text: '안녕', session: 's1' }],
      ['/v1/say', 'POST', { text: '안녕' }],
      ['/v1/decisions/dc%201', 'POST', { choice: '지금 적용' }],
      ['/v1/proposals/pr_x', 'POST', { accept: false }],
      ['/v1/state', 'GET', undefined],
    ]);
  });

  it('스트림(GET /v1/stream)도 fetch 로, 같은 머리를 싣고 주소에 비밀이 없다', async () => {
    const fetchImpl = vi.fn(
      async (_url: string, _init?: RequestInit) =>
        new Response('', { status: 200, headers: { 'Content-Type': 'text/event-stream; charset=utf-8' } }),
    );
    const auth = new FaceAuth({ base: '', code: null, fetchImpl: fetchImpl as unknown as typeof fetch, now: () => NOW });
    const a = createApi('', { fetchImpl: fetchImpl as unknown as typeof fetch, auth });
    const ctrl = new AbortController();
    const res = await a.openStream(ctrl.signal);
    expect(res.status).toBe(200);
    const [url, init] = fetchImpl.mock.calls[0]!;
    expect(url).toBe('/v1/stream');
    expect(init!.method).toBe('GET');
    expect(init!.credentials).toBe('omit');
    expect(init!.cache).toBe('no-store');
    expect(init!.headers).toEqual({ Authorization: `Bearer ${KEY}`, Accept: 'text/event-stream' });
    expect((init!.signal as AbortSignal).aborted).toBe(false);
    ctrl.abort();
    expect((init!.signal as AbortSignal).aborted).toBe(true);
  });

  it('스트림이 401 이면 열쇠를 지우고 ApiError(401)', async () => {
    const fetchImpl = vi.fn(async () => new Response('', { status: 401 }));
    const auth = new FaceAuth({ base: '', code: null, fetchImpl: fetchImpl as unknown as typeof fetch, now: () => NOW });
    const a = createApi('', { fetchImpl: fetchImpl as unknown as typeof fetch, auth });
    await expect(a.openStream(new AbortController().signal)).rejects.toMatchObject({ status: 401 });
    expect(sessionStorage.getItem('gyeot.session')).toBeNull();
  });

  it('스트림이 event-stream 이 아니면(잘못 온 index.html 등) 끊긴 것으로 본다', async () => {
    const fetchImpl = vi.fn(async () => new Response('<!doctype html>', { status: 200, headers: { 'Content-Type': 'text/html' } }));
    const auth = new FaceAuth({ base: '', code: null, fetchImpl: fetchImpl as unknown as typeof fetch, now: () => NOW });
    const a = createApi('', { fetchImpl: fetchImpl as unknown as typeof fetch, auth });
    await expect(a.openStream(new AbortController().signal)).rejects.not.toMatchObject({ status: 401 });
    expect(sessionStorage.getItem('gyeot.session')).not.toBeNull();
  });

  it('실패 상태는 ApiError, 문구는 한국어', async () => {
    const { a } = api(500);
    await expect(a.state()).rejects.toBeInstanceOf(ApiError);
    expect(describeError(new ApiError(401, ''))).toBe('열쇠가 없거나 만료됐어요');
    expect(describeError(new ApiError(421, ''))).toContain('출처');
    expect(describeError(new TypeError('x'))).toBe('곁 데몬에 닿지 않아요');
  });
});
