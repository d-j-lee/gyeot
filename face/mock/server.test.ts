// @vitest-environment node
// 가짜 데몬(npm run mock)을 실제로 띄우고 HTTP 로만 본다 — 얼굴 개발이 배포와 같은 열쇠 흐름(§7·§12.1·§13.1)을 타는지.
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { spawn, type ChildProcess } from 'node:child_process';
import { createServer, request } from 'node:http';
import { createHash, createHmac } from 'node:crypto';
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

async function exchange(code: unknown, headers: Record<string, string> = {}) {
  return fetch(`${base()}/v1/session`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...headers },
    body: JSON.stringify({ code }),
  });
}

/** 부트 코드 → 세션 열쇠(얼굴이 하는 일) */
async function session(): Promise<string> {
  const r = await exchange(await newCode());
  expect(r.status).toBe(200);
  const { session: key } = (await r.json()) as { session: string };
  expect(key).toMatch(HEX64);
  return key;
}
const asKey = (key: string) => ({ Authorization: `Bearer ${key}` });

/** 요청줄·Host 를 그대로 보낸다(fetch 는 Host 를 바꿀 수 없다). */
function raw(path: string, headers: Record<string, string> = {}): Promise<{ status: number }> {
  return new Promise((resolve, reject) => {
    const req = request({ host: '127.0.0.1', port, path, method: 'GET', headers }, (res) => {
      res.resume();
      resolve({ status: res.statusCode ?? 0 });
    });
    req.on('error', reject);
    req.end();
  });
}
const rawGet = async (path: string, headers: Record<string, string>) => (await raw(path, headers)).status;

describe('부트 코드(POST /v1/boot, §12.1-1)', () => {
  it('토큰(Bearer)으로만 받는다 — 없음·틀림·?t=·쿠키는 401, 세션 열쇠는 403', async () => {
    expect((await boot({})).status).toBe(401);
    expect((await boot({ Authorization: `Bearer ${'cd'.repeat(32)}` })).status).toBe(401);
    expect((await fetch(`${base()}/v1/boot?t=${TOKEN}`, { method: 'POST' })).status).toBe(401);
    const key = await session();
    expect((await boot({ Cookie: `gyeot_session=${key}` })).status).toBe(401);
    const r = await boot(asKey(key));
    expect(r.status).toBe(403);
    expect(await r.json()).toEqual({ ok: false, error: '이 열쇠로는 쓸 수 없어요' });
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

describe('POST /v1/session — 부트 코드를 세션 열쇠로(§12.1-3)', () => {
  it('인증 없이 코드만으로 {session, expires_ms(7일)}, 쿠키 없음', async () => {
    const before = Date.now();
    const r = await exchange(await newCode());
    expect(r.status).toBe(200);
    expect(r.headers.get('set-cookie')).toBeNull();
    expect(r.headers.get('cache-control')).toBe('no-store');
    const body = (await r.json()) as { session: string; expires_ms: number };
    expect(Object.keys(body).sort()).toEqual(['expires_ms', 'session']);
    expect(body.session).toMatch(HEX64);
    const week = 7 * 86_400_000;
    expect(body.expires_ms).toBeGreaterThanOrEqual(before + week - 1_000);
    expect(body.expires_ms).toBeLessThanOrEqual(Date.now() + week + 1_000);
  });

  it('코드는 한 번만 — 두 번째·모르는 코드·빈 본문은 401', async () => {
    const code = await newCode();
    expect((await exchange(code)).status).toBe(200);
    expect((await exchange(code)).status).toBe(401);
    expect((await exchange('ef'.repeat(32))).status).toBe(401);
    expect((await exchange(undefined)).status).toBe(401);
    expect((await exchange(12)).status).toBe(401);
    const bad = await fetch(`${base()}/v1/session`, { method: 'POST', body: '{oops' });
    expect(bad.status).toBe(400);
    expect((await fetch(`${base()}/v1/session`)).status).toBe(405);
  });
});

describe('얼굴과 정적 파일(§7)', () => {
  it('GET / 는 인증 없이 index.html 을 그대로(비밀 없음), 보안 머리글과 함께', async () => {
    const r = await fetch(`${base()}/?mode=overlay`);
    expect(r.status).toBe(200);
    expect(await r.text()).toBe(INDEX);
    expect(r.headers.get('content-security-policy')).toBe(CSP);
    expect(r.headers.get('x-frame-options')).toBe('DENY');
    expect(r.headers.get('referrer-policy')).toBe('no-referrer');
    expect(r.headers.get('x-content-type-options')).toBe('nosniff');
    expect(r.headers.get('cache-control')).toBe('no-store');
    expect(r.headers.get('set-cookie')).toBeNull();
  });

  it('서버는 ?boot= 를 읽지 않는다 — 넘겨주기·쿠키 없이 index, 코드는 그대로 남아 얼굴이 쓴다', async () => {
    const code = await newCode();
    const r = await fetch(`${base()}/?mode=overlay&boot=${code}`, { redirect: 'manual' });
    expect(r.status).toBe(200);
    expect(r.headers.get('location')).toBeNull();
    expect(r.headers.get('set-cookie')).toBeNull();
    expect(await r.text()).toBe(INDEX);
    expect((await exchange(code)).status).toBe(200);
  });

  it('/assets/* 는 열쇠 없이', async () => {
    const r = await fetch(`${base()}/assets/app.js`);
    expect(r.status).toBe(200);
    expect(await r.text()).toContain('곁');
  });
});

describe('API·SSE 인증(§7) — Authorization: Bearer <토큰|세션 열쇠> 하나', () => {
  it('GET /v1/state: 없음·?t=·쿠키는 401, 토큰·세션 열쇠는 200', async () => {
    const key = await session();
    expect((await fetch(`${base()}/v1/state`)).status).toBe(401);
    expect((await fetch(`${base()}/v1/state?t=${TOKEN}`)).status).toBe(401);
    expect((await fetch(`${base()}/v1/state`, { headers: { Cookie: `gyeot_session=${key}` } })).status).toBe(401);
    expect((await fetch(`${base()}/v1/state`, { headers: { Authorization: `Bearer ${TOKEN}` } })).status).toBe(200);
    const r = await fetch(`${base()}/v1/state`, { headers: asKey(key) });
    expect(r.status).toBe(200);
    const snap = (await r.json()) as { version: string; settings: { approval_interrupt: boolean } };
    expect(snap.version).toMatch(/mock/);
    expect(snap.settings.approval_interrupt).toBe(false);
  });

  it('GET /v1/stream: ?t= 는 401, 세션 열쇠면 snapshot 부터', async () => {
    expect((await fetch(`${base()}/v1/stream?t=${TOKEN}`)).status).toBe(401);
    const ctrl = new AbortController();
    const r = await fetch(`${base()}/v1/stream`, { headers: asKey(await session()), signal: ctrl.signal });
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

  it('얼굴의 동작(POST)도 세션 열쇠로, 같은 출처 Origin 이면 받는다', async () => {
    const r = await fetch(`${base()}/v1/focus`, {
      method: 'POST',
      headers: { ...asKey(await session()), Origin: base(), 'Content-Type': 'application/json' },
      body: JSON.stringify({ on: false }),
    });
    expect(r.status).toBe(200);
  });

  it('세션 열쇠의 범위(G-031): ingest·channel·boot·mock 조작은 403, 토큰은 403 이 아니다', async () => {
    const key = await session();
    const paths: [string, string][] = [
      ['POST', '/v1/ingest/claude-code'],
      ['POST', '/v1/channel/register'],
      ['GET', '/v1/channel/stream?cid=ch_1'],
      ['POST', '/v1/channel/reply'],
      ['POST', '/v1/boot'],
      ['GET', '/mock/trigger?what=nudge'],
    ];
    for (const [method, p] of paths) {
      const init = (h: Record<string, string>) => ({ method, headers: { ...h, 'Content-Type': 'application/json' }, ...(method === 'POST' ? { body: '{}' } : {}) });
      const asSession = await fetch(`${base()}${p}`, init(asKey(key)));
      expect(asSession.status, `${method} ${p}`).toBe(403);
      expect(await asSession.json()).toEqual({ ok: false, error: '이 열쇠로는 쓸 수 없어요' });
      expect((await fetch(`${base()}${p}`, init({}))).status, `${method} ${p} 열쇠 없음`).toBe(401);
      expect((await fetch(`${base()}${p}`, init({ Authorization: `Bearer ${TOKEN}` }))).status, `${method} ${p} 토큰`).not.toBe(403);
    }
  });

  it('/mock/trigger 는 ?t= 로는 안 되고 토큰으로', async () => {
    expect((await fetch(`${base()}/mock/trigger?what=nudge&t=${TOKEN}`)).status).toBe(401);
    expect((await fetch(`${base()}/mock/trigger?what=nudge`, { headers: { Authorization: `Bearer ${TOKEN}` } })).status).toBe(200);
  });
});

describe('데몬 확인(GET /v1/hello, §13.1)', () => {
  it('인증 없이 토큰·포트·n 의 HMAC 을 준다', async () => {
    const n = 'a1'.repeat(32);
    const r = await fetch(`${base()}/v1/hello?n=${n}`);
    expect(r.status).toBe(200);
    const want = createHmac('sha256', TOKEN).update(`gyeot-hello/v1|${port}|${n}`).digest('hex');
    const body = (await r.json()) as { v: number; proof: string; face: string[] };
    expect(body.v).toBe(1);
    expect(body.proof).toBe(want);
  });

  it('face: 살아 있는 세션 열쇠마다 hex(HMAC-SHA256(SHA-256(열쇠), "gyeot-face-hello/v1|<포트>|<n>"))(§14.1)', async () => {
    const key = await session();
    const n = 'c3'.repeat(32);
    const proofOf = (k: string, p: number) =>
      createHmac('sha256', createHash('sha256').update(k, 'utf8').digest()).update(`gyeot-face-hello/v1|${p}|${n}`).digest('hex');
    const body = (await (await fetch(`${base()}/v1/hello?n=${n}`)).json()) as { v: number; proof: string; face: string[] };
    expect(Array.isArray(body.face)).toBe(true);
    expect(body.face).toContain(proofOf(key, port));
    expect(body.face).not.toContain(proofOf(key, port + 1));
    expect(body.face).not.toContain(proofOf('ef'.repeat(32), port));
    expect(body.face.length).toBeLessThanOrEqual(20);
    for (const f of body.face) expect(f).toMatch(HEX64);
  });

  it('개발 서버 프록시(x-forwarded-port: 5173)를 거친 hello 는 5173 으로 계산한다 — 가짜 데몬만의 예외, 다른 값은 무시', async () => {
    const key = await session();
    const n = 'd4'.repeat(32);
    const proofOf = (p: number) =>
      createHmac('sha256', createHash('sha256').update(key, 'utf8').digest()).update(`gyeot-face-hello/v1|${p}|${n}`).digest('hex');
    const via = async (h: Record<string, string>) => ((await (await fetch(`${base()}/v1/hello?n=${n}`, { headers: h })).json()) as { face: string[] }).face;
    expect(await via({ 'X-Forwarded-Port': '5173' })).toContain(proofOf(5173));
    const other = await via({ 'X-Forwarded-Port': '8080' });
    expect(other).toContain(proofOf(port));
    expect(other).not.toContain(proofOf(8080));
  });

  it('n 이 16–128자 hex 가 아니면 400', async () => {
    for (const q of ['', '?n=', '?n=zz', `?n=${'a'.repeat(15)}`, `?n=${'a'.repeat(129)}`]) {
      expect((await fetch(`${base()}/v1/hello${q}`)).status, q).toBe(400);
    }
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
    expect((await exchange(direct![1]!)).status).toBe(200);
    expect(out).not.toMatch(/[?&]t=/);
    expect(out).not.toMatch(/쿠키/);
  });
});
