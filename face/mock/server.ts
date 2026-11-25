// 개발용 가짜 곁 데몬. docs/contracts.md §7·§8·§12.1 의 엔드포인트·SSE·열쇠 흐름을 흉내 낸다.
//   npm run mock                 → http://127.0.0.1:47321/ (dist 가 있으면 / 에서 얼굴을 내보낸다)
//   GYEOT_PORT=47322 npm run mock
//   GYEOT_TOKEN=<hex> npm run mock (기본: 띄울 때마다 새 난수. 터미널에 보인다)
//   GYEOT_MOCK_DIST=<폴더>        → 얼굴 폴더(기본 face/dist)
//   npm run mock -- --quiet      → 저절로 일어나는 일(대본) 없이
// 열쇠(배포와 같다): 토큰(Bearer)으로 POST /v1/boot → 부트 주소 /?boot=<코드>(120초, 한 번) → 쿠키 + 303.
//   그 뒤 얼굴의 API·SSE 는 같은 출처 쿠키로만. ?t= 는 받지 않는다. 띄울 때 부트 주소를 보이고, Enter 마다 새로 보인다.
// 손으로 일으키기: curl -H "Authorization: Bearer <토큰>" "http://127.0.0.1:<포트>/mock/trigger?what=interrupt|nudge|reply|alert|done|working|item|reset"
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { randomBytes } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { extname, join, normalize, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import type { Delivery, Item, Mood, Presence, Proposal, Reply, Session, Snapshot } from '../src/types.ts';
import {
  CHANNEL_MISSING_REASON,
  DEMO_PROPOSALS,
  DEMO_SETTINGS,
  demoInterrupt,
  demoItems,
  demoNudge,
  demoSessions,
} from '../src/demo/data.ts';
import { AuthStore } from './auth.ts';

const here = dirname(fileURLToPath(import.meta.url));
const dist = process.env.GYEOT_MOCK_DIST ? normalize(process.env.GYEOT_MOCK_DIST) : join(here, '..', 'dist');
const PORT = Number(process.env.GYEOT_PORT ?? 47321);
const TOKEN = process.env.GYEOT_TOKEN || randomBytes(32).toString('hex');
const QUIET = process.argv.includes('--quiet');
const MAX_BODY = 1024 * 1024;
/** vite 개발 서버(vite.config.ts 의 server.port). 그 프록시가 Origin 을 그대로 넘기므로 가짜 데몬만 받아 준다. */
const DEV_PORT = 5173;
const auth = new AuthStore(TOKEN);

// channel 은 있지만 개발 채널 플래그 없이 시작된 세션(§10.1): 말을 넘겨도 답이 오지 않는다.
const SILENT_SESSIONS = new Set(['c3d95e11-dja']);

// ── 상태 ────────────────────────────────────────────────

const t0 = Date.now();
let items = new Map<string, Item>(demoItems(t0).map((i) => [i.id, i]));
let sessions: Session[] = demoSessions(t0, [...items.values()]);
let proposals: Proposal[] = DEMO_PROPOSALS.map((p) => ({ ...p }));
let focusUntil: number | null = null;
let listening = false;
let workingUntil = 0;
let doneUntil = 0;
let alertSince: number | null = t0 - 12 * 60_000;
let chatSeq = 0;

function presence(now = Date.now()): Presence {
  const open = [...items.values()];
  const blocking = open.filter((i) => i.blocking);
  const failedRecent = open.some((i) => i.kind === 'failed') && alertSince !== null && now - alertSince < 10 * 60_000;
  const focusOn = focusUntil !== null && focusUntil > now;
  let mood: Mood = 'idle';
  if (failedRecent) mood = 'alert';
  else if (blocking.length) mood = 'waiting';
  else if (listening) mood = 'listening';
  else if (workingUntil > now) mood = 'working';
  else if (doneUntil > now) mood = 'done';
  else if (focusOn) mood = 'sleep';
  const top = [...open].sort((a, b) => b.score - a.score)[0] ?? null;
  const line =
    mood === 'waiting'
      ? `막힌 일 ${blocking.length}건이 기다려요`
      : mood === 'alert'
        ? '실패한 일이 있어요'
        : mood === 'sleep'
          ? '집중 모드예요'
          : '';
  return {
    mood,
    level: mood === 'waiting' || mood === 'alert' ? 'nudge' : 'ambient',
    focus_item: mood === 'waiting' || mood === 'alert' ? (top?.id ?? null) : null,
    line,
    since_ms: now,
    focus_mode_until_ms: focusOn ? focusUntil : null,
  };
}

function snapshot(): Snapshot {
  const now = Date.now();
  return {
    version: '0.1.0-mock',
    now_ms: now,
    presence: presence(now),
    items: [...items.values()],
    sessions,
    proposals,
    budget: { nudge: 4, interrupt: 1 },
    settings: { ...DEMO_SETTINGS },
  };
}

// ── SSE ────────────────────────────────────────────────

const clients = new Set<ServerResponse>();

function send(res: ServerResponse, event: string, data: unknown) {
  res.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
}

function broadcast(event: string, data: unknown) {
  for (const c of clients) send(c, event, data);
  console.log(`  → ${event}${event === 'deliver' ? ` (${(data as Delivery).level})` : ''}`);
}

let lastMood: Mood | null = null;
function pushPresence() {
  const p = presence();
  if (p.mood !== lastMood) {
    lastMood = p.mood;
    broadcast('presence', p);
  }
}

setInterval(() => {
  for (const c of clients) c.write(': ping\n\n');
}, 15_000).unref();
setInterval(pushPresence, 1_000).unref();

// ── HTTP 도우미 ─────────────────────────────────────────

function json(res: ServerResponse, status: number, body: unknown) {
  res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': 'no-store' });
  res.end(JSON.stringify(body));
}

async function readJson(req: IncomingMessage): Promise<Record<string, unknown>> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const c of req) {
    size += (c as Buffer).length;
    if (size > MAX_BODY) throw Object.assign(new Error('too large'), { status: 413 });
    chunks.push(c as Buffer);
  }
  const text = Buffer.concat(chunks).toString('utf8');
  if (!text) return {};
  try {
    const v = JSON.parse(text);
    return typeof v === 'object' && v ? v : {};
  } catch {
    throw Object.assign(new Error('bad json'), { status: 400 });
  }
}

/** §7: 토큰(Bearer) 또는 세션 쿠키. 쿼리 토큰(?t=)은 받지 않는다. */
function authed(req: IncomingMessage): boolean {
  return auth.bearerOk(req.headers.authorization) || auth.sessionOk(req.headers.cookie);
}

let listenPort = PORT;
const allowedHosts = () => new Set([`127.0.0.1:${listenPort}`, `localhost:${listenPort}`]);
// §7·§12.2: 같은 출처만. Tauri 출처는 받지 않는다. 개발 서버(vite) 출처는 가짜 데몬만의 예외다.
const allowedOrigins = () =>
  new Set([
    `http://127.0.0.1:${listenPort}`,
    `http://localhost:${listenPort}`,
    `http://127.0.0.1:${DEV_PORT}`,
    `http://localhost:${DEV_PORT}`,
  ]);

/** §7: 얼굴 응답(index·안내 쪽)의 보안 머리글 */
const FACE_HEADERS: Record<string, string> = {
  'Content-Security-Policy':
    "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; media-src 'self' data: blob:; connect-src 'self' ipc: http://ipc.localhost; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
  'X-Frame-Options': 'DENY',
  'Referrer-Policy': 'no-referrer',
  'X-Content-Type-Options': 'nosniff',
  'Cache-Control': 'no-store',
};

const GUIDE_PAGE = `<!doctype html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>곁 — 열쇠가 필요해요</title>
<style>body{font-family:system-ui,"Malgun Gothic",sans-serif;max-width:40rem;margin:3rem auto;padding:0 1.25rem;line-height:1.6}code{background:#ebe5db;border-radius:6px;padding:.1rem .35rem}</style>
</head>
<body>
<h1>곁을 열려면 열쇠가 필요해요</h1>
<p>트레이의 ‘곁 열기’를 누르거나 터미널에서 <code>gyeotd --open</code>을 실행하세요.</p>
<p>가짜 데몬(<code>npm run mock</code>)이면 그 터미널에서 Enter 를 누르면 새 주소가 나와요. 주소는 120초 안에 한 번만 쓸 수 있어요.</p>
</body>
</html>
`;

function guide(res: ServerResponse) {
  res.writeHead(401, { 'Content-Type': 'text/html; charset=utf-8', ...FACE_HEADERS });
  res.end(GUIDE_PAGE);
}

/** §12.1-2: 코드를 쿠키로 바꾸고 같은 경로·같은 쿼리에서 boot 만 뺀 주소로 303. */
function redeemBoot(res: ServerResponse, url: URL) {
  const value = auth.redeem(url.searchParams.get('boot') ?? '');
  if (!value) return guide(res);
  const q = new URLSearchParams(url.search);
  q.delete('boot');
  const rest = q.toString();
  // '//host' 꼴은 다른 호스트로 읽히므로 앞의 / 를 하나로 접는다.
  const path = `/${url.pathname.replace(/^\/+/, '')}`;
  res.writeHead(303, {
    Location: rest ? `${path}?${rest}` : path,
    'Set-Cookie': auth.cookieFor(value),
    'Cache-Control': 'no-store',
    'Referrer-Policy': 'no-referrer',
  });
  res.end();
  console.log('  ⚿ 부트 코드 → 세션 쿠키');
}

function issueBoot(port = listenPort) {
  const c = auth.issueCode();
  return { ...c, url: `http://127.0.0.1:${port}/?boot=${c.code}` };
}

const MIME: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
};

/** §7: index(`/`·확장자 없는 경로)는 쿠키가 있어야, 그 밖의 정적 파일(`/assets/*` 등)은 열쇠 없이. 토큰을 싣지 않는다. */
async function serveStatic(req: IncomingMessage, res: ServerResponse, pathname: string) {
  let p: string;
  try {
    p = normalize(decodeURIComponent(pathname));
  } catch {
    return json(res, 404, { error: 'not found' });
  }
  const isIndex = p === '/' || p === '/index.html' || !extname(p);
  if (isIndex && !auth.sessionOk(req.headers.cookie)) return guide(res);
  if (isIndex && !existsSync(join(dist, 'index.html'))) {
    res.writeHead(200, { 'Content-Type': 'text/plain; charset=utf-8', ...FACE_HEADERS });
    res.end('dist 가 없어요. face/ 에서 npm run build 를 하거나, npm run dev 로 띄운 뒤 터미널에 보인 개발 서버 부트 주소를 여세요.\n');
    return;
  }
  if (isIndex) p = '/index.html';
  const file = join(dist, p);
  if (!file.startsWith(dist)) return json(res, 404, { error: 'not found' });
  try {
    const body = await readFile(file);
    res.writeHead(200, {
      'Content-Type': MIME[extname(file)] ?? 'application/octet-stream',
      ...(isIndex ? FACE_HEADERS : { 'X-Content-Type-Options': 'nosniff' }),
    });
    res.end(body);
  } catch {
    json(res, 404, { error: 'not found' });
  }
}

// ── 일 ─────────────────────────────────────────────────

function removeItem(id: string) {
  if (items.delete(id)) {
    broadcast('item_removed', { id });
    refreshSessions();
    pushPresence();
  }
}

function refreshSessions() {
  const blockingSessions = new Set([...items.values()].filter((i) => i.blocking).map((i) => i.session));
  sessions = sessions.map((s) => ({
    ...s,
    state: blockingSessions.has(s.id) ? 'waiting' : s.state === 'waiting' ? 'idle' : s.state,
  }));
  broadcast('sessions', sessions);
}

function reply(session: Session, text: string, delay = 1_500) {
  setTimeout(() => {
    const r: Reply = { session: session.id, project: session.project, chat_id: `m${++chatSeq}`, text, at_ms: Date.now() };
    broadcast('reply', r);
  }, delay).unref();
}

function deliver(d: Delivery) {
  broadcast('deliver', { ...d, at_ms: Date.now() });
}

function reset() {
  const now = Date.now();
  items = new Map(demoItems(now).map((i) => [i.id, i]));
  sessions = demoSessions(now, [...items.values()]);
  proposals = DEMO_PROPOSALS.map((p) => ({ ...p }));
  focusUntil = null;
  alertSince = now - 12 * 60_000;
  broadcast('snapshot', snapshot());
}

function trigger(what: string): boolean {
  const now = Date.now();
  switch (what) {
    case 'interrupt':
      deliver(demoInterrupt(now));
      return true;
    case 'nudge':
      deliver(demoNudge(now));
      return true;
    case 'alert':
      alertSince = now;
      pushPresence();
      return true;
    case 'working':
      workingUntil = now + 60_000;
      pushPresence();
      return true;
    case 'done':
      doneUntil = now + 5_000;
      workingUntil = 0;
      pushPresence();
      return true;
    case 'reply': {
      const s = sessions.find((x) => x.channel && !SILENT_SESSIONS.has(x.id));
      if (s) reply(s, '테스트를 다 돌렸어요.\n실패 2건: src/lib/time.test.ts 의 자정 경계, api.test.ts 의 토큰 저장.', 0);
      return true;
    }
    case 'item': {
      const s = sessions.find((x) => x.project === 'infra-scripts')!;
      const it: Item = {
        ...demoItems(now).find((i) => i.kind === 'unread')!,
        id: `it_mock${now.toString(16)}`,
        session: s.id,
        project: s.project,
        cwd: s.cwd,
        agent: s.agent,
        title: '안 읽은 결과 · Claude Code',
        body: '배포 스크립트 정리를 마쳤어요. deploy.ps1 의 중복 단계 3개를 지웠어요.',
        created_ms: now,
        updated_ms: now,
        signature: `unread:${now}`,
        score: 55,
        reasons: ['결과를 아직 읽지 않음: 기본 55'],
      };
      items.set(it.id, it);
      broadcast('item', it);
      return true;
    }
    case 'reset':
      reset();
      return true;
    default:
      return false;
  }
}

// ── 경로 ───────────────────────────────────────────────

const server = createServer(async (req, res) => {
  // §7: DNS 리바인딩 방어(Host), 같은 출처만(Origin). vite 프록시는 changeOrigin 으로 Host 를 데몬 주소로 바꾼다.
  const host = req.headers.host ?? '';
  if (!allowedHosts().has(host)) return json(res, 421, { error: 'host' });
  const origin = req.headers.origin;
  if (origin !== undefined && !allowedOrigins().has(origin)) return json(res, 403, { error: 'origin' });
  const url = new URL(req.url ?? '/', `http://${host}`);
  const method = req.method ?? 'GET';

  try {
    if (method === 'GET' && url.pathname === '/health') return json(res, 200, { ok: true, version: '0.1.0-mock' });
    // §7: 부트 코드는 토큰으로만(쿠키로는 부를 수 없다)
    if (url.pathname === '/v1/boot') {
      if (method !== 'POST') return json(res, 405, { error: 'method' });
      if (!auth.bearerOk(req.headers.authorization)) return json(res, 401, { error: 'unauthorized' });
      return json(res, 200, issueBoot());
    }
    if (method === 'GET' && !url.pathname.startsWith('/v1/') && !url.pathname.startsWith('/mock/')) {
      if (url.searchParams.has('boot')) return redeemBoot(res, url);
      return serveStatic(req, res, url.pathname);
    }
    if (!authed(req)) return json(res, 401, { error: 'unauthorized' });

    if (method === 'GET' && url.pathname === '/mock/trigger') {
      const what = url.searchParams.get('what') ?? '';
      return json(res, trigger(what) ? 200 : 400, { ok: true, what });
    }
    if (method === 'GET' && url.pathname === '/v1/state') return json(res, 200, snapshot());
    if (method === 'GET' && url.pathname === '/v1/stream') {
      res.writeHead(200, { 'Content-Type': 'text/event-stream; charset=utf-8', 'Cache-Control': 'no-store', Connection: 'keep-alive' });
      res.write(': hello\n\n');
      clients.add(res);
      send(res, 'snapshot', snapshot());
      console.log(`  ⇄ 스트림 연결(${clients.size}개)`);
      req.on('close', () => {
        clients.delete(res);
        console.log(`  ⇄ 스트림 끊김(${clients.size}개)`);
      });
      return;
    }
    if (method !== 'POST') return json(res, 405, { error: 'method' });

    const body = await readJson(req);
    let m: RegExpMatchArray | null;

    if ((m = url.pathname.match(/^\/v1\/items\/([^/]+)\/action$/))) {
      const id = decodeURIComponent(m[1]!);
      const action = body.action;
      if (!['open', 'done', 'dismiss', 'snooze'].includes(String(action))) return json(res, 400, { error: 'action' });
      if (action === 'dismiss' && !['과민', '이미 처리', '무관'].includes(String(body.reason))) return json(res, 400, { error: 'reason' });
      if (!items.has(id)) return json(res, 404, { error: 'item' });
      console.log(`  ← ${action} ${id}${body.reason ? ` (${body.reason})` : ''}`);
      if (action !== 'open') removeItem(id);
      return json(res, 200, { ok: true });
    }
    if (url.pathname === '/v1/focus') {
      const on = body.on === true;
      const minutes = typeof body.minutes === 'number' ? body.minutes : 60;
      focusUntil = on ? Date.now() + minutes * 60_000 : null;
      broadcast('presence', presence());
      return json(res, 200, { ok: true, until_ms: focusUntil });
    }
    if (url.pathname === '/v1/presence/listening') {
      listening = body.on === true;
      pushPresence();
      return json(res, 200, { ok: true });
    }
    if (url.pathname === '/v1/say') {
      const text = typeof body.text === 'string' ? body.text : '';
      if (!text.trim()) return json(res, 400, { error: 'text' });
      const target =
        typeof body.session === 'string'
          ? sessions.find((s) => s.id === body.session)
          : [...sessions].filter((s) => s.channel).sort((a, b) => b.last_ms - a.last_ms)[0];
      if (!target?.channel) return json(res, 200, { delivered_to: null, reason: CHANNEL_MISSING_REASON });
      console.log(`  ← say → ${target.project}: ${text.slice(0, 40)}`);
      if (!SILENT_SESSIONS.has(target.id)) reply(target, `‘${text.slice(0, 30)}’ 받았어요. 하던 일을 마치고 바로 볼게요.`);
      return json(res, 200, { delivered_to: target.id, reason: '' });
    }
    if ((m = url.pathname.match(/^\/v1\/decisions\/([^/]+)$/))) {
      const did = decodeURIComponent(m[1]!);
      const it = [...items.values()].find((i) => i.decision_id === did);
      if (!it) return json(res, 200, { delivered_to: null });
      const choice = String(body.choice ?? '');
      if (!it.options?.includes(choice)) return json(res, 400, { error: 'choice' });
      removeItem(it.id);
      const s = sessions.find((x) => x.id === it.session);
      if (s) reply(s, `[선택] ${choice} — 알겠어요, 그렇게 할게요.`, 1_000);
      return json(res, 200, { delivered_to: it.session });
    }
    if ((m = url.pathname.match(/^\/v1\/proposals\/([^/]+)$/))) {
      const id = decodeURIComponent(m[1]!);
      proposals = proposals.filter((p) => p.id !== id);
      console.log(`  ← proposal ${id} ${body.accept ? '받아들임' : '거절'}`);
      return json(res, 200, { ok: true });
    }
    return json(res, 404, { error: 'not found' });
  } catch (e) {
    const status = (e as { status?: number }).status ?? 500;
    return json(res, status, { error: String((e as Error).message) });
  }
});

// ── 대본: 저절로 일어나는 일 ─────────────────────────────

const SCRIPT: [number, string][] = [
  [8_000, 'working'],
  [20_000, 'nudge'],
  [34_000, 'done'],
  [48_000, 'reply'],
  [62_000, 'interrupt'],
];

server.on('error', (e: NodeJS.ErrnoException) => {
  if (e.code === 'EADDRINUSE') {
    console.error(`포트 ${PORT}를 이미 누가 쓰고 있어요(곁 데몬일 수 있어요). GYEOT_PORT=47322 npm run mock 처럼 바꿔 띄우세요.`);
    process.exit(1);
  }
  throw e;
});

/** 부트 주소 두 개(데몬이 내보내는 얼굴 / vite 개발 서버). 코드마다 120초 안에 한 번만 쓴다. */
function printBootUrls() {
  const direct = issueBoot();
  const dev = issueBoot(DEV_PORT);
  console.log(`  얼굴 열기(dist, 120초 안에 한 번):  ${direct.url}`);
  console.log(`  개발 서버(npm run dev, 120초 안에 한 번): ${dev.url}`);
  console.log(`    오버레이는 주소 끝에 &mode=overlay — 새 주소는 이 터미널에서 Enter`);
}

server.listen(PORT, '127.0.0.1', () => {
  const addr = server.address();
  if (addr && typeof addr === 'object') listenPort = addr.port;
  const bearer = `-H "Authorization: Bearer ${TOKEN}"`;
  console.log(`가짜 곁 데몬: http://127.0.0.1:${listenPort}/`);
  console.log(`  개발용 토큰: ${TOKEN}  (GYEOT_TOKEN 으로 정할 수 있어요)`);
  printBootUrls();
  console.log(`  부트 코드 직접: curl -s -X POST ${bearer} http://127.0.0.1:${listenPort}/v1/boot`);
  console.log(`  손으로: curl ${bearer} "http://127.0.0.1:${listenPort}/mock/trigger?what=interrupt"`);
  console.log(`  dj-assis 세션은 channel 이 있지만 답하지 않아요(§10.1 안내 확인용).`);
  if (process.stdin.isTTY) {
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', () => printBootUrls());
  }
  if (!QUIET) {
    for (const [ms, what] of SCRIPT) setTimeout(() => trigger(what), ms).unref();
    setInterval(() => {
      for (const [ms, what] of SCRIPT) setTimeout(() => trigger(what), ms).unref();
    }, 90_000).unref();
  }
});
