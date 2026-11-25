import { afterEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { ApiError, createApi, describeError, resolveApiBase } from './api.ts';

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

describe('요청(§7)', () => {
  function api(status = 200, body: unknown = { ok: true }) {
    const fetchImpl = vi.fn(async (_url: string, _init?: RequestInit) => new Response(JSON.stringify(body), { status }));
    return { a: createApi('http://d', fetchImpl as unknown as typeof fetch), fetchImpl };
  }

  it('토큰 머리글 없이 같은 출처 쿠키로 인증한다, 본문은 JSON', async () => {
    const { a, fetchImpl } = api();
    await a.itemAction('it_1', 'dismiss', { reason: '과민' });
    await a.state();
    const [url, init] = fetchImpl.mock.calls[0]!;
    expect(url).toBe('http://d/v1/items/it_1/action');
    expect(init!.method).toBe('POST');
    expect(init!.credentials).toBe('same-origin');
    expect(init!.headers).toEqual({ 'Content-Type': 'application/json' });
    expect(JSON.parse(init!.body as string)).toEqual({ action: 'dismiss', reason: '과민' });
    const [, getInit] = fetchImpl.mock.calls[1]!;
    expect(getInit!.credentials).toBe('same-origin');
    expect(getInit!.headers).toEqual({});
    for (const [, i] of fetchImpl.mock.calls) {
      expect(JSON.stringify(i!.headers)).not.toMatch(/authorization/i);
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

  it('EventSource 주소에 토큰(?t=)을 싣지 않는다 — 같은 출처라 쿠키가 실린다', () => {
    expect(createApi('').streamUrl()).toBe('/v1/stream');
    expect(createApi('http://127.0.0.1:47399').streamUrl()).toBe('http://127.0.0.1:47399/v1/stream');
  });

  it('실패 상태는 ApiError, 문구는 한국어', async () => {
    const { a } = api(401);
    await expect(a.state()).rejects.toBeInstanceOf(ApiError);
    expect(describeError(new ApiError(401, ''))).toBe('열쇠가 만료됐어요');
    expect(describeError(new ApiError(421, ''))).toContain('출처');
    expect(describeError(new TypeError('x'))).toBe('곁 데몬에 닿지 않아요');
  });
});
