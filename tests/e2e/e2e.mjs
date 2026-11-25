#!/usr/bin/env node
// 곁 끝-끝 실측: 가짜 없이 실제 gyeotd · gyeot-hook · gyeot-channel · 얼굴(Chrome)을 잇는다.
//
// 준비: cargo build --workspace, (cd face && npm ci && npm run build), (cd channel && npm ci && npm run build)
// 실행: node tests/e2e/e2e.mjs [--screenshots <폴더>] [--no-chrome]
// 결과: 점검마다 한 줄, 마지막에 요약. 하나라도 실패하면 종료 코드 1.

import { spawn, spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const EXE = process.platform === 'win32' ? '.exe' : '';
const BIN = path.join(process.env.CARGO_TARGET_DIR ? path.resolve(process.env.CARGO_TARGET_DIR) : path.join(ROOT, 'target'), 'debug');
const GYEOTD = path.join(BIN, `gyeotd${EXE}`);
const HOOK = path.join(BIN, `gyeot-hook${EXE}`);
const CHANNEL = path.join(ROOT, 'channel', 'dist', 'gyeot-channel.mjs');
const FACE = path.join(ROOT, 'face', 'dist');
const args = process.argv.slice(2);
const SHOT_DIR = args.includes('--screenshots') ? path.resolve(args[args.indexOf('--screenshots') + 1]) : null;
const NO_CHROME = args.includes('--no-chrome');

const requireFromChannel = createRequire(path.join(ROOT, 'channel', 'package.json'));
const { Client } = requireFromChannel('@modelcontextprotocol/sdk/client/index.js');
const { StdioClientTransport } = requireFromChannel('@modelcontextprotocol/sdk/client/stdio.js');

const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok: Boolean(ok), detail });
  console.log(`${ok ? '통과' : '실패'}  ${name}${detail ? ` — ${detail}` : ''}`);
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function waitFor(fn, ms = 5000, step = 50) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() > end) return null;
    await sleep(step);
  }
}

for (const p of [GYEOTD, HOOK, CHANNEL, path.join(FACE, 'index.html')]) {
  if (!fs.existsSync(p)) {
    console.error(`준비되지 않음: ${p}`);
    process.exit(2);
  }
}

async function freePort() {
  return new Promise((resolve) => {
    const s = net.createServer().listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

const HOME = fs.mkdtempSync(path.join(os.tmpdir(), 'gyeot-e2e-'));
const PORT = await freePort();
const BASE = `http://127.0.0.1:${PORT}`;
const ENV = { ...process.env, GYEOT_HOME: HOME, GYEOT_PORT: String(PORT), GYEOT_URL: BASE, GYEOT_FACE_DIR: FACE };
let daemon = null;
const children = [];

function startDaemon() {
  const d = spawn(GYEOTD, [], { env: ENV, stdio: ['ignore', 'ignore', 'pipe'] });
  d.stderr.on('data', () => {});
  children.push(d);
  return d;
}
async function stopDaemon(d) {
  if (!d || d.exitCode !== null) return;
  d.kill('SIGINT');
  await waitFor(() => d.exitCode !== null, 5000);
  if (d.exitCode === null) d.kill('SIGKILL');
}

function raw(method, p, { headers = {}, body } = {}) {
  return new Promise((resolve, reject) => {
    const req = http.request({ host: '127.0.0.1', port: PORT, method, path: p, headers }, (res) => {
      let data = '';
      res.on('data', (c) => (data += c));
      res.on('end', () => resolve({ status: res.statusCode, body: data, headers: res.headers }));
    });
    req.on('error', reject);
    if (body !== undefined) req.write(typeof body === 'string' ? body : JSON.stringify(body));
    req.end();
  });
}
let TOKEN = '';
let COOKIE = '';
const auth = () => ({ Authorization: `Bearer ${TOKEN}`, 'Content-Type': 'application/json' });
async function api(method, p, body) {
  const r = await raw(method, p, { headers: auth(), body });
  let json = null;
  try {
    json = JSON.parse(r.body);
  } catch {}
  return { status: r.status, json };
}

// 얼굴이 받는 SSE를 그대로 받아 둔다.
const sse = [];
function openStream() {
  const req = http.get({ host: '127.0.0.1', port: PORT, path: '/v1/stream', headers: { Authorization: `Bearer ${TOKEN}` } }, (res) => {
    let buf = '';
    res.setEncoding('utf8');
    res.on('data', (chunk) => {
      buf += chunk.replace(/\r\n/g, '\n');
      let i;
      while ((i = buf.indexOf('\n\n')) >= 0) {
        const block = buf.slice(0, i);
        buf = buf.slice(i + 2);
        let event = 'message';
        const data = [];
        for (const line of block.split('\n')) {
          if (line.startsWith('event:')) event = line.slice(6).trim();
          else if (line.startsWith('data:')) data.push(line.slice(5).replace(/^ /, ''));
        }
        if (data.length) {
          try {
            sse.push({ event, data: JSON.parse(data.join('\n')) });
          } catch {}
        }
      }
    });
  });
  req.on('error', () => {});
  return req;
}
const seen = (event, pred) => waitFor(() => sse.find((e) => e.event === event && pred(e.data)), 5000);

function hook(source, payload) {
  const r = spawnSync(HOOK, [source], { input: JSON.stringify(payload), env: ENV, timeout: 5000 });
  return { code: r.status, stdout: r.stdout.toString() };
}

const S1 = 'e2e-s1';
const S2 = 'e2e-s2';
const CWD1 = path.join(HOME, 'work', 'agent-ops-kit');
const CWD2 = path.join(HOME, 'work', 'filesystemMap');
const common = (sid, cwd, name) => ({
  session_id: sid,
  transcript_path: path.join(HOME, 'transcript.jsonl'),
  cwd,
  permission_mode: 'default',
  hook_event_name: name,
});

let client = null;
let chromeProc = null;
try {
  // 1. 데몬
  daemon = startDaemon();
  const healthy = await waitFor(async () => (await raw('GET', '/health').catch(() => null))?.status === 200, 15000, 100);
  check('데몬이 뜨고 /health가 200', healthy);
  TOKEN = fs.readFileSync(path.join(HOME, 'token'), 'utf8').trim();
  check('토큰 파일이 64자 hex', /^[0-9a-f]{64}$/.test(TOKEN));

  // 2. 보안 경계
  check('토큰 없이 /v1/state → 401', (await raw('GET', '/v1/state')).status === 401);
  check('다른 Host → 421', (await raw('GET', '/v1/state', { headers: { ...auth(), Host: `evil.example:${PORT}` } })).status === 421);
  check('다른 Origin → 403', (await raw('GET', '/v1/state', { headers: { ...auth(), Origin: 'https://evil.example' } })).status === 403);
  check('쿼리 토큰(?t=)은 받지 않음 → 401', (await raw('GET', `/v1/state?t=${TOKEN}`)).status === 401);
  check('Tauri 출처도 거절 → 403', (await raw('GET', '/v1/state', { headers: { ...auth(), Origin: 'http://tauri.localhost' } })).status === 403);
  const bare = await raw('GET', '/');
  check('쿠키 없는 GET / → 401 안내 쪽, 토큰 없음', bare.status === 401 && !bare.body.includes(TOKEN) && /gyeotd --open/.test(bare.body));
  check('얼굴 응답에 CSP frame-ancestors none·X-Frame-Options DENY', /frame-ancestors 'none'/.test(bare.headers['content-security-policy'] ?? '') && bare.headers['x-frame-options'] === 'DENY');
  const boot = await api('POST', '/v1/boot');
  check('POST /v1/boot(토큰) → 일회용 코드', boot.status === 200 && /^[0-9a-f]{64}$/.test(boot.json?.code ?? ''), boot.json?.url);
  const enter = await raw('GET', `/?mode=overlay&boot=${boot.json?.code}`);
  const setCookie = String(enter.headers['set-cookie'] ?? '');
  COOKIE = (setCookie.match(/gyeot_session=([0-9a-f]+)/) ?? [])[1] ?? '';
  check('부트 코드 → 303 + HttpOnly·SameSite=Strict 쿠키, 주소에서 코드 제거', enter.status === 303 && /HttpOnly/i.test(setCookie) && /SameSite=Strict/i.test(setCookie) && enter.headers.location === '/?mode=overlay' && COOKIE.length === 64, enter.headers.location);
  check('같은 코드 재사용 → 401', (await raw('GET', `/?boot=${boot.json?.code}`)).status === 401);
  const face = await raw('GET', '/', { headers: { Cookie: `gyeot_session=${COOKIE}` } });
  check('쿠키로 GET / → 얼굴, 페이지에 토큰 없음', face.status === 200 && /<div id="app"|<script/.test(face.body) && !face.body.includes(TOKEN));
  check('쿠키로 /v1/state → 200', (await raw('GET', '/v1/state', { headers: { Cookie: `gyeot_session=${COOKIE}` } })).status === 200);
  check('쿠키로는 부트 코드를 못 만듦 → 401', (await raw('POST', '/v1/boot', { headers: { Cookie: `gyeot_session=${COOKIE}` } })).status === 401);

  openStream();
  check('SSE 첫 snapshot', await seen('snapshot', (d) => Array.isArray(d.items)));

  // 3. channel(실제 번들)을 MCP 클라이언트로 띄운다. 부모는 이 프로세스 = 훅의 부모와 같다(Claude Code 역할).
  const notes = [];
  const transport = new StdioClientTransport({ command: process.execPath, args: [CHANNEL], env: ENV, stderr: 'pipe' });
  client = new Client({ name: 'gyeot-e2e', version: '0.0.0' }, { capabilities: {} });
  client.fallbackNotificationHandler = async (n) => {
    notes.push(n);
  };
  await client.connect(transport);
  const caps = client.getServerCapabilities() ?? {};
  check('channel capability 선언', Boolean(caps.experimental?.['claude/channel']) && Boolean(caps.tools));
  check('권한 중계 capability 미선언(G-005)', !('claude/channel/permission' in (caps.experimental ?? {})));

  // 4. 세션 1: 사람이 방금 입력한 세션의 승인 대기 → 표정만(ambient)
  const h0 = hook('claude-code', { ...common(S1, CWD1, 'SessionStart'), source: 'startup' });
  check('훅 심: stdout 비어 있고 exit 0', h0.code === 0 && h0.stdout === '');
  hook('claude-code', { ...common(S1, CWD1, 'UserPromptSubmit'), prompt: '테스트 하네스를 붙여 줘' });
  hook('claude-code', {
    ...common(S1, CWD1, 'PermissionRequest'),
    tool_name: 'Bash',
    tool_input: { command: 'npm install -D vitest', description: 'vitest 설치' },
  });
  hook('claude-code', {
    ...common(S1, CWD1, 'Notification'),
    message: 'Claude needs your permission to use Bash',
    title: 'Permission needed',
    notification_type: 'permission_prompt',
  });
  const appr = await seen('item', (d) => d.session === S1 && d.kind === 'approval');
  check('승인 대기 → approval 항목(95)', appr && appr.data.score >= 95 && appr.data.blocking === true, appr?.data.title);
  check('승인 본문에 명령 미리보기', appr && /npm install -D vitest/.test(appr.data.body));
  const d1 = await seen('deliver', (d) => d.item === appr?.data.id);
  check('방금 입력한 세션 → ambient(사람이 이미 거기 있음)', d1 && d1.data.level === 'ambient', d1?.data.reasons?.join(' / '));
  check('존재감 waiting', await seen('presence', (d) => d.mood === 'waiting'));

  // 5. channel ↔ 세션 연결(pid_hint == channel ppid)
  const linked = await waitFor(async () => {
    const s = (await api('GET', '/v1/state')).json;
    return s?.sessions?.find((x) => x.id === S1 && x.channel);
  });
  check('channel이 세션에 연결됨(pid)', linked, linked?.channel);

  // 6. 세션 2: 사람이 없는 세션의 승인 대기 → 말풍선(nudge)
  hook('claude-code', { ...common(S2, CWD2, 'SessionStart'), source: 'startup' });
  hook('claude-code', {
    ...common(S2, CWD2, 'Notification'),
    message: 'Claude needs your permission to use Bash',
    notification_type: 'permission_prompt',
  });
  const appr2 = await seen('item', (d) => d.session === S2 && d.kind === 'approval');
  const d2 = appr2 && (await seen('deliver', (d) => d.item === appr2.data.id));
  check('다른 세션 승인 대기 → nudge 말풍선', d2 && d2.data.level === 'nudge', d2?.data.line);
  check('전달 판단에 한국어 이유', d2 && d2.data.reasons?.length > 0 && /[가-힣]/.test(d2.data.reasons.join('')));

  // 7. 승인된 뒤 같은 도구 작업 신호 → approval 닫힘
  // 설치기가 기본으로 넣는 PostToolBatch(도구 이름 없음)가 승인 대기를 닫아야 한다(R1 H2).
  hook('claude-code', { ...common(S1, CWD1, 'PostToolBatch') });
  check('PostToolBatch(설치 기본 훅) → 승인 항목 닫힘', await seen('item_removed', (d) => d.id === appr?.data.id));

  // 8. 질문으로 끝난 턴 → decision
  hook('claude-code', {
    ...common(S1, CWD1, 'Stop'),
    stop_hook_active: false,
    last_assistant_message: '하네스를 붙였습니다. 시험 12개가 통과합니다.\nCI에도 같은 게이트를 걸까요?',
  });
  check('질문으로 끝난 턴 → decision 항목', await seen('item', (d) => d.session === S1 && d.kind === 'decision'));

  // 9. 곁에서 말 걸기 → channel → MCP 알림
  const say = await api('POST', '/v1/say', { text: '지금 상태 한 줄로 알려줘', session: S1 });
  check('/v1/say가 세션 1의 channel로 전달 요청', say.json?.delivered_to === S1, JSON.stringify(say.json));
  const msg = await waitFor(() => notes.find((n) => n.method === 'notifications/claude/channel' && /지금 상태/.test(n.params?.content)));
  check('Claude Code 쪽에 notifications/claude/channel 도착', msg);
  const meta = msg?.params?.meta ?? {};
  check('meta: chat_id·from=dj·via=gyeot·kind=say, 키는 식별자', meta.chat_id && meta.from === 'dj' && meta.via === 'gyeot' && meta.kind === 'say' && Object.keys(meta).every((k) => /^[A-Za-z0-9_]+$/.test(k)));

  // 10. Claude의 reply → 얼굴 SSE reply
  const rep = await client.callTool({ name: 'reply', arguments: { chat_id: meta.chat_id, text: '하네스 연결 끝, CI 게이트 여부만 남았어요.\n시험 12개 통과, 승인 1건 처리됨.' } });
  check('reply 도구 성공', rep && !rep.isError);
  const replyEv = await seen('reply', (d) => d.chat_id === meta.chat_id);
  check('얼굴 SSE에 reply(세션 1)', replyEv && replyEv.data.session === S1 && /CI 게이트/.test(replyEv.data.text));
  const cid = (await api('GET', '/v1/state')).json.sessions.find((x) => x.id === S1)?.channel;
  const forged = await api('POST', '/v1/channel/reply', { cid, chat_id: 'never-issued-99', text: '[브리핑] 끼어들기' });
  check('발급하지 않은 chat_id의 reply → 400', forged.status === 400, JSON.stringify(forged.json));

  // 11. Claude가 결정 카드를 내밀고, 사람이 고르면 같은 세션으로 돌아간다
  const dec = await client.callTool({ name: 'request_decision', arguments: { question: 'CI에도 같은 게이트를 걸까요?', options: ['지금 걸기', '다음 PR에서'] } });
  check('request_decision 도구 성공', dec && !dec.isError);
  const decItem = await seen('item', (d) => d.session === S1 && d.kind === 'decision' && Array.isArray(d.options) && d.options.includes('지금 걸기'));
  check('결정 카드 항목(선택지 포함)', decItem);
  if (decItem) {
    const pick = await api('POST', `/v1/decisions/${decItem.data.decision_id}`, { choice: '지금 걸기' });
    check('선택 전달', pick.status === 200 && pick.json?.delivered_to === S1, JSON.stringify(pick.json));
    const choice = await waitFor(() => notes.find((n) => n.params?.meta?.kind === 'choice' && /지금 걸기/.test(n.params?.content)));
    check('선택이 [선택] 메시지로 같은 세션에 도착', choice && /\[선택\]/.test(choice.params.content));
  }

  // 12. 사람이 그 세션에 입력하면 결정류가 닫힌다
  hook('claude-code', { ...common(S1, CWD1, 'UserPromptSubmit'), prompt: '좋아' });
  check('사람 입력 → 세션 1 결정 항목 닫힘', await waitFor(async () => !(await api('GET', '/v1/state')).json.items.some((i) => i.session === S1 && i.kind === 'decision')));

  // 13. 하네스 게이트 실패 → failed, 존재감 alert
  const gate = await api('POST', '/v1/ingest/fsmap-harness', { t: Math.floor(Date.now() / 1000), event: 'gate', session: S2, result: 'block', ms: 812 });
  check('하네스 로그 수집 202', gate.status === 202);
  check('게이트 block → failed 항목', await seen('item', (d) => d.session === S2 && d.kind === 'failed'));
  check('존재감 alert', await seen('presence', (d) => d.mood === 'alert'));

  // 14. 넘기기(이유 과민) → 항목 사라짐
  const st = (await api('GET', '/v1/state')).json;
  const failed = st.items.find((i) => i.session === S2 && i.kind === 'failed');
  const dis = await api('POST', `/v1/items/${failed.id}/action`, { action: 'dismiss', reason: '과민' });
  check('넘기기 동작', dis.status === 200 && (await seen('item_removed', (d) => d.id === failed.id)));

  // 15. 스크린샷(실제 데몬 데이터)
  if (SHOT_DIR && !NO_CHROME) {
    fs.mkdirSync(SHOT_DIR, { recursive: true });
    chromeProc = await shootFace(SHOT_DIR);
  }

  // 16. 재시작해도 상태가 이벤트 로그 재생으로 돌아온다
  const before = (await api('GET', '/v1/state')).json.items.map((i) => `${i.session}:${i.kind}:${i.state}`).sort();
  await stopDaemon(daemon);
  daemon = startDaemon();
  await waitFor(async () => (await raw('GET', '/health').catch(() => null))?.status === 200, 15000, 100);
  const after = (await api('GET', '/v1/state')).json.items.map((i) => `${i.session}:${i.kind}:${i.state}`).sort();
  check('재시작 뒤 같은 항목(재생)', JSON.stringify(before) === JSON.stringify(after), after.join(', '));
  check('재시작 뒤에도 세션 쿠키 유효(sessions.json)', (await raw('GET', '/v1/state', { headers: { Cookie: `gyeot_session=${COOKIE}` } })).status === 200);
  const sess = fs.readFileSync(path.join(HOME, 'sessions.json'), 'utf8');
  check('sessions.json에 세션 값 원문이 없음(해시만)', !sess.includes(COOKIE));
  const logs = fs.readdirSync(path.join(HOME, 'events'));
  const logText = logs.map((f) => fs.readFileSync(path.join(HOME, 'events', f), 'utf8')).join('');
  check('프롬프트 본문은 저장하지 않음', !logText.includes('테스트 하네스를 붙여 줘'));

  // 17. 데몬이 꺼져도 훅은 기다리지 않고 스풀에 쓴다
  await stopDaemon(daemon);
  const t0 = Date.now();
  const h = hook('claude-code', { ...common(S2, CWD2, 'Stop'), last_assistant_message: '끝났습니다.' });
  const took = Date.now() - t0;
  const spooled = fs.existsSync(path.join(HOME, 'spool')) && fs.readdirSync(path.join(HOME, 'spool')).length > 0;
  check('데몬 없음: 훅 exit 0, stdout 비어 있음, 스풀에 남음', h.code === 0 && h.stdout === '' && spooled, `${took}ms`);
  const SECRET = 'sk-ant-api03-' + 'A'.repeat(40);
  hook('claude-code', { ...common(S2, CWD2, 'UserPromptSubmit'), prompt: `내 비밀번호는 hunter2 그리고 ${SECRET}` });
  hook('claude-code', { ...common(S2, CWD2, 'PermissionRequest'), tool_name: 'Write', tool_input: { file_path: path.join(CWD2, '.env'), content: `DB_PASSWORD=supersecret\nKEY=${SECRET}` } });
  const spoolDir = path.join(HOME, 'spool');
  const spoolText = fs.readdirSync(spoolDir).map((f) => fs.readFileSync(path.join(spoolDir, f), 'utf8')).join('\n');
  check('스풀에 프롬프트 본문·파일 내용·비밀이 없음(R1 M3)', !/hunter2|supersecret/.test(spoolText) && !spoolText.includes(SECRET));
  if (process.platform !== 'win32') {
    const modes = fs.readdirSync(spoolDir).map((f) => fs.statSync(path.join(spoolDir, f)).mode & 0o777);
    check('스풀 파일 권한 0600', modes.length > 0 && modes.every((m) => m === 0o600), modes.map((m) => m.toString(8)).join(','));
  }
  daemon = startDaemon();
  await waitFor(async () => (await raw('GET', '/health').catch(() => null))?.status === 200, 15000, 100);
  check('데몬이 다시 뜨면 스풀을 비움', await waitFor(() => fs.readdirSync(path.join(HOME, 'spool')).length === 0, 8000, 200));
} catch (e) {
  check('예외 없이 끝남', false, String(e?.stack ?? e));
} finally {
  try {
    await client?.close();
  } catch {}
  if (chromeProc) chromeProc.kill('SIGKILL');
  await stopDaemon(daemon);
  for (const c of children) if (c.exitCode === null) c.kill('SIGKILL');
  try {
    fs.rmSync(HOME, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  } catch {}
}

const failed = results.filter((r) => !r.ok);
console.log(`\n요약: ${results.length - failed.length}/${results.length} 통과`);
process.exit(failed.length ? 1 : 0);

// ---- Chrome DevTools Protocol로 얼굴을 찍는다 ------------------------------------
async function shootFace(dir) {
  const chrome = ['/usr/bin/google-chrome', '/usr/bin/chromium', 'C:/Program Files/Google/Chrome/Application/chrome.exe'].find((p) => fs.existsSync(p));
  if (!chrome) {
    console.log('건너뜀  Chrome 없음 — 스크린샷 생략');
    return null;
  }
  const cdpPort = await freePort();
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'gyeot-e2e-chrome-'));
  const proc = spawn(chrome, ['--headless=new', '--no-sandbox', '--disable-gpu', `--remote-debugging-port=${cdpPort}`, `--user-data-dir=${profile}`, 'about:blank'], { stdio: 'ignore' });
  const ver = await waitFor(async () => {
    try {
      return await (await fetch(`http://127.0.0.1:${cdpPort}/json/version`)).json();
    } catch {
      return null;
    }
  }, 15000, 200);
  if (!ver) {
    check('Chrome 원격 디버깅 연결', false);
    return proc;
  }
  const target = await (await fetch(`http://127.0.0.1:${cdpPort}/json/new?about:blank`, { method: 'PUT' })).json();
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener('open', r, { once: true }));
  let id = 0;
  const pending = new Map();
  ws.addEventListener('message', (ev) => {
    const m = JSON.parse(ev.data);
    if (m.id && pending.has(m.id)) {
      pending.get(m.id)(m);
      pending.delete(m.id);
    }
  });
  const send = (method, params = {}) =>
    new Promise((resolve) => {
      const i = ++id;
      pending.set(i, resolve);
      ws.send(JSON.stringify({ id: i, method, params }));
    });
  const shot = async (name) => {
    const r = await send('Page.captureScreenshot', { format: 'png' });
    fs.writeFileSync(path.join(dir, name), Buffer.from(r.result.data, 'base64'));
    return name;
  };
  await send('Page.enable');
  await send('Emulation.setDefaultBackgroundColorOverride', { color: { r: 226, g: 231, b: 238, a: 1 } });

  await send('Emulation.setDeviceMetricsOverride', { width: 420, height: 700, deviceScaleFactor: 2, mobile: false });
  const bootShot = await api('POST', '/v1/boot');
  await send('Page.navigate', { url: `${BASE}/?mode=overlay&boot=${bootShot.json?.code}` });
  await sleep(2500);
  const ovOk = await send('Runtime.evaluate', { expression: '!!document.querySelector(".ov-avatar")', returnByValue: true });
  check('실제 데몬에 붙은 얼굴(오버레이)이 그려짐', ovOk.result?.result?.value === true);
  await shot('live-overlay.png');
  await send('Runtime.evaluate', { expression: 'document.querySelector(".ov-avatar")?.click()' });
  await sleep(900);
  await shot('live-overlay-panel.png');

  await send('Emulation.setDeviceMetricsOverride', { width: 1180, height: 760, deviceScaleFactor: 2, mobile: false });
  await send('Page.navigate', { url: `${BASE}/` });
  await sleep(2500);
  await shot('live-default.png');
  const items = await send('Runtime.evaluate', { expression: 'document.body.innerText.includes("승인은 터미널에서 직접 하세요")', returnByValue: true });
  check('승인 카드에 "승인은 터미널에서" 안내(승인 단추 없음)', items.result?.result?.value === true);
  ws.close();
  return proc;
}
