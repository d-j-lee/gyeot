// @vitest-environment node
// 가짜 데몬(npm run mock)을 실제로 띄우고 HTTP 로만 본다 — 얼굴 개발이 배포와 같은 열쇠 흐름(§12.1)을 타는지.
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { spawn, type ChildProcess } from 'node:child_process';
import { createServer, request } from 'node:http';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { AddressInfo } from 'node:net';

const TOKEN = 'ab'.repeat(32);
const INDEX = '<!doctype html><html lang="ko"><head><title>곁</title></head><body><script type="module" src="/assets/app.js"></script></body></html>';
const CSP =
  "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; media-src 'self' data: blob:; connect-src 'self' ipc: http://ipc.localhost; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";
const HEX64 = /^[0-9a-f]{64}$/;

let child: ChildProcess;
let port = 0;
let out = '';
let dist = '';
const base = () => `http://127.0.0.1:${port}`;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function freePort(): Promise<number> {
  const s = createServer();
  await new Promise<void>((r) => s.listen(0, '127.0.0.1', r));
  const p = (s.address() as AddressInfo).port;
  await new Promise((r) => s.close(r));
  return p;
}

beforeAll(async () => {
  dist = mkdtempSync(join(tmpdir(), 'gyeot-mock-'));
  writeFileSync(join(dist, 'index.html'), INDEX);
  mkdirSync(join(dist, 'assets'));
  writeFileSync(join(dist, 'assets', 'app.js'), 'console.log("곁")');
  port = await freePort();
  child = spawn(process.execPath, ['mock/server.ts', '--quiet'], {
    cwd: process.cwd(), // vitest 의 root = face/
    env: { ...process.env, GYEOT_PORT: String(port), GYEOT_TOKEN: TOKEN, GYEOT_MOCK_DIST: dist },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stdout!.setEncoding('utf8').on('data', (d: string) => (out += d));
  child.stderr!.setEncoding('utf8').on('data', (d: string) => (out += d));
  for (let i = 0; i < 200; i++) {
    try {
      if ((await fetch(`${base()}/health`)).ok && out.includes('가짜 곁 데몬')) break;
    } catch {
      /* 아직 안 떴다 */
    }
    await sleep(50);
  }
}, 30_000);

afterAll(() => {
  child?.kill();
  if (dist) rmSync(dist, { recursive: true, force: true });
});

async function boot(headers: Record<string, string> = { Authorization: `Bearer ${TOKEN}` }) {
  return fetch(`${base()}/v1/boot`, { method: 'POST', headers });
}

async function newCode(): Promise<string> {
  const r = await boot();
  expect(r.status).toBe(200);
  return ((await r.json()) as { code: string }).code;
}

async function redeem(code: string, pathAndQuery = '/') {
  const sep = pathAndQuery.includes('?') ? '&' : '?';
  const r = await fetch(`${base()}${pathAndQuery}${sep}boot=${code}`, { redirect: 'manual' });
  const cookie = r.headers.getSetCookie()[0] ?? '';
  return { r, location: r.headers.get('location'), setCookie: cookie, cookie: cookie.split(';')[0]! };
}

async function session(): Promise<string> {
  const { cookie } = await redeem(await newCode());
  expect(cookie).toMatch(/^gyeot_session=[0-9a-f]{64}$/);
  return cookie;
}

/** 요청줄·Host 를 그대로 보낸다(fetch 는 Host 를 바꿀 수 없고, 경로의 . 조각을 먼저 접는다). */
function raw(path: string, headers: Record<string, string> = {}): Promise<{ status: number; location: string | undefined }> {
  return new Promise((resolve, reject) => {
    const req = request({ host: '127.0.0.1', port, path, method: 'GET', headers }, (res) => {
      res.resume();
      resolve({ status: res.statusCode ?? 0, location: res.headers.location });
    });
    req.on('error', reject);
    req.end();
  });
}
const rawGet = async (path: string, headers: Record<string, string>) => (await raw(path, headers)).status;

describe('부트 코드(POST /v1/boot, §12.1-1)', () => {
  it('토큰(Bearer)으로만 받는다 — 없음·틀림·?t=·쿠키는 401', async () => {
    expect((await boot({})).status).toBe(401);
    expect((await boot({ Authorization: `Bearer ${'cd'.repeat(32)}` })).status).toBe(401);
    expect((await fetch(`${base()}/v1/boot?t=${TOKEN}`, { method: 'POST' })).status).toBe(401);
    expect((await boot({ Cookie: await session() })).status).toBe(401);
  });

  it('코드는 32바이트 hex, 주소와 120초 만료를 돌려준다', async () => {
    const before = Date.now();
    const r = await boot();
    expect(r.status).toBe(200);
    const body = (await r.json()) as { code: string; url: string; expires_ms: number };
    expect(body.code).toMatch(HEX64);
    expect(body.url).toBe(`${base()}/?boot=${body.code}`);
    expect(body.expires_ms).toBeGreaterThanOrEqual(before + 119_000);
    expect(body.expires_ms).toBeLessThanOrEqual(Date.now() + 121_000);
  });
});

describe('GET /?boot= → 쿠키 + 303(§12.1-2)', () => {
  it('쿠키를 달고 같은 경로·쿼리에서 boot 만 뺀 주소로 보낸다', async () => {
    const { r, location, setCookie } = await redeem(await newCode(), '/?mode=overlay&tab=chat');
    expect(r.status).toBe(303);
    expect(location).toBe('/?mode=overlay&tab=chat');
    expect(setCookie).toMatch(/^gyeot_session=[0-9a-f]{64}; HttpOnly; SameSite=Strict; Path=\/; Max-Age=2592000$/);
    expect(r.headers.get('cache-control')).toBe('no-store');
  });

  it('boot 만 있으면 / 로', async () => {
    const { r, location } = await redeem(await newCode());
    expect(r.status).toBe(303);
    expect(location).toBe('/');
  });

  it('코드는 한 번만 — 두 번째는 안내 쪽(401)', async () => {
    const code = await newCode();
    expect((await redeem(code)).r.status).toBe(303);
    const again = await redeem(code);
    expect(again.r.status).toBe(401);
    expect(again.setCookie).toBe('');
    expect(again.r.headers.get('content-type')).toMatch(/text\/html/);
    expect(await again.r.text()).toContain('gyeotd --open');
  });

  it('모르는 코드도 안내 쪽(401), 쿠키 없음', async () => {
    const r = await redeem('ef'.repeat(32));
    expect(r.r.status).toBe(401);
    expect(r.setCookie).toBe('');
  });

  it('경로가 // 로 접혀도 다른 호스트로 보내지 않는다(열린 넘겨주기 없음)', async () => {
    // '/.//x' 와 '/a/..//x' 는 URL 해석 뒤 경로가 '//x' 가 된다. 그대로 Location 에 실으면 //x 는 다른 호스트다.
    for (const path of ['/.//evil.example/', '/a/..//evil.example/', '//evil.example/']) {
      const r = await raw(`${path}?mode=overlay&boot=${await newCode()}`, { Host: `127.0.0.1:${port}` });
      expect(r.status, path).toBe(303);
      expect(r.location, path).toMatch(/^\/(?!\/)/);
      expect(r.location, path).not.toContain('//');
    }
  });
});

describe('얼굴과 정적 파일(§7)', () => {
  it('쿠키가 없으면 / 는 한국어 안내 쪽(401)', async () => {
    const r = await fetch(`${base()}/`);
    expect(r.status).toBe(401);
    const html = await r.text();
    expect(html).toContain('곁 열기');
    expect(html).toContain('gyeotd --open');
    expect(r.headers.get('content-security-policy')).toBe(CSP);
  });

  it('쿠키가 있으면 index.html 을 그대로(토큰을 싣지 않는다), 보안 머리글과 함께', async () => {
    const r = await fetch(`${base()}/?mode=overlay`, { headers: { Cookie: await session() } });
    expect(r.status).toBe(200);
    expect(await r.text()).toBe(INDEX);
    expect(r.headers.get('content-security-policy')).toBe(CSP);
    expect(r.headers.get('x-frame-options')).toBe('DENY');
    expect(r.headers.get('referrer-policy')).toBe('no-referrer');
    expect(r.headers.get('x-content-type-options')).toBe('nosniff');
    expect(r.headers.get('cache-control')).toBe('no-store');
  });

  it('/assets/* 는 열쇠 없이', async () => {
    const r = await fetch(`${base()}/assets/app.js`);
    expect(r.status).toBe(200);
    expect(await r.text()).toContain('곁');
  });
});

describe('API·SSE 인증(§7) — 쿠키 또는 Bearer, ?t= 는 받지 않는다', () => {
  it('GET /v1/state', async () => {
    expect((await fetch(`${base()}/v1/state`)).status).toBe(401);
    expect((await fetch(`${base()}/v1/state?t=${TOKEN}`)).status).toBe(401);
    expect((await fetch(`${base()}/v1/state`, { headers: { Authorization: `Bearer ${TOKEN}` } })).status).toBe(200);
    const r = await fetch(`${base()}/v1/state`, { headers: { Cookie: await session() } });
    expect(r.status).toBe(200);
    expect(((await r.json()) as { version: string }).version).toMatch(/mock/);
  });

  it('GET /v1/stream: ?t= 는 401, 쿠키면 snapshot 부터', async () => {
    expect((await fetch(`${base()}/v1/stream?t=${TOKEN}`)).status).toBe(401);
    const ctrl = new AbortController();
    const r = await fetch(`${base()}/v1/stream`, { headers: { Cookie: await session() }, signal: ctrl.signal });
    expect(r.status).toBe(200);
    expect(r.headers.get('content-type')).toMatch(/text\/event-stream/);
    const reader = r.body!.getReader();
    let text = '';
    while (!text.includes('event: snapshot')) {
      const { value, done } = await reader.read();
      if (done) break;
      text += new TextDecoder().decode(value);
    }
    expect(text).toContain('event: snapshot');
    ctrl.abort();
  });

  it('얼굴의 동작(POST)도 쿠키로, 같은 출처 Origin 이면 받는다', async () => {
    const r = await fetch(`${base()}/v1/focus`, {
      method: 'POST',
      headers: { Cookie: await session(), Origin: base(), 'Content-Type': 'application/json' },
      body: JSON.stringify({ on: false }),
    });
    expect(r.status).toBe(200);
  });

  it('/mock/trigger 도 ?t= 로는 안 되고 Bearer 로', async () => {
    expect((await fetch(`${base()}/mock/trigger?what=nudge&t=${TOKEN}`)).status).toBe(401);
    expect((await fetch(`${base()}/mock/trigger?what=nudge`, { headers: { Authorization: `Bearer ${TOKEN}` } })).status).toBe(200);
  });
});

describe('출처·Host(§7·§12.2)', () => {
  it('Tauri 출처와 남의 출처는 403', async () => {
    for (const origin of ['tauri://localhost', 'http://tauri.localhost', 'https://tauri.localhost', 'null', 'http://evil.example']) {
      const r = await fetch(`${base()}/v1/state`, { headers: { Authorization: `Bearer ${TOKEN}`, Origin: origin } });
      expect(r.status, origin).toBe(403);
    }
  });

  it('Host 가 127.0.0.1·localhost:<포트> 가 아니면 421', async () => {
    expect(await rawGet('/health', { Host: `127.0.0.1:${port}` })).toBe(200);
    expect(await rawGet('/health', { Host: `localhost:${port}` })).toBe(200);
    expect(await rawGet('/health', { Host: 'evil.example' })).toBe(421);
    expect(await rawGet('/health', { Host: `127.0.0.1:${port + 1}` })).toBe(421);
  });
});

describe('시작 안내', () => {
  it('개발용 토큰과 바로 열 수 있는 부트 주소(데몬·개발 서버)를 보인다', async () => {
    expect(out).toContain(TOKEN);
    const direct = out.match(new RegExp(`http://127\\.0\\.0\\.1:${port}/\\?boot=([0-9a-f]{64})`));
    const dev = out.match(/http:\/\/127\.0\.0\.1:5173\/\?boot=([0-9a-f]{64})/);
    expect(direct?.[1]).toMatch(HEX64);
    expect(dev?.[1]).toMatch(HEX64);
    expect(direct![1]).not.toBe(dev![1]);
    expect((await redeem(direct![1]!)).r.status).toBe(303);
    expect(out).not.toMatch(/[?&]t=/);
  });
});
