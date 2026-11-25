// 개발용 가짜 곁 데몬. docs/contracts.md §7·§8 의 엔드포인트와 SSE 를 흉내 낸다.
//   npm run mock                 → http://127.0.0.1:47321/ (dist 가 있으면 토큰을 채워 내보낸다)
//   GYEOT_PORT=47322 npm run mock
//   GYEOT_TOKEN=xxx npm run mock (기본 토큰: dev)
//   npm run mock -- --quiet      → 저절로 일어나는 일(대본) 없이
// 손으로 일으키기: GET /mock/trigger?what=interrupt|nudge|reply|alert|done|working|item|reset&t=<토큰>
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
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

const here = dirname(fileURLToPath(import.meta.url));
const dist = join(here, '..', 'dist');
const PORT = Number(process.env.GYEOT_PORT ?? 47321);
const TOKEN = process.env.GYEOT_TOKEN ?? 'dev';
const QUIET = process.argv.includes('--quiet');
const MAX_BODY = 1024 * 1024;

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

function authed(req: IncomingMessage, url: URL): boolean {
  const h = req.headers.authorization ?? '';
  return h === `Bearer ${TOKEN}` || url.searchParams.get('t') === TOKEN;
}

const ALLOWED_ORIGINS = new Set([
  `http://127.0.0.1:${PORT}`,
  `http://localhost:${PORT}`,
  'tauri://localhost',
  'http://tauri.localhost',
  'https://tauri.localhost',
  // 개발 서버(vite)에서 프록시로 들어오는 요청
  'http://127.0.0.1:5173',
  'http://localhost:5173',
]);

const MIME: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
};

async function serveStatic(res: ServerResponse, pathname: string) {
  if (!existsSync(join(dist, 'index.html'))) {
    res.writeHead(200, { 'Content-Type': 'text/plain; charset=utf-8' });
    res.end(`dist 가 없어요. face/ 에서 npm run build 를 하거나, npm run dev 로 띄운 뒤 http://localhost:5173/?t=${TOKEN} 를 여세요.\n`);
    return;
  }
  let p = normalize(decodeURIComponent(pathname));
  if (p === '/' || !extname(p)) p = '/index.html';
  const file = join(dist, p);
  if (!file.startsWith(dist)) return json(res, 404, { error: 'not found' });
  try {
    let body: Buffer | string = await readFile(file);
    if (p === '/index.html') body = body.toString('utf8').replace('<meta name="gyeot-token" content="">', `<meta name="gyeot-token" content="${TOKEN}">`);
    res.writeHead(200, { 'Content-Type': MIME[extname(file)] ?? 'application/octet-stream' });
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
  const url = new URL(req.url ?? '/', `http://${req.headers.host ?? '127.0.0.1'}`);
  const method = req.method ?? 'GET';
  const origin = req.headers.origin;
  if (origin && !ALLOWED_ORIGINS.has(origin)) return json(res, 403, { error: 'origin' });

  try {
    if (method === 'GET' && url.pathname === '/health') return json(res, 200, { ok: true, version: '0.1.0-mock' });
    if (method === 'GET' && !url.pathname.startsWith('/v1/') && !url.pathname.startsWith('/mock/')) return serveStatic(res, url.pathname);
    if (!authed(req, url)) return json(res, 401, { error: 'unauthorized' });

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

server.listen(PORT, '127.0.0.1', () => {
  console.log(`가짜 곁 데몬: http://127.0.0.1:${PORT}/  (토큰 ${TOKEN})`);
  console.log(`  오버레이: http://127.0.0.1:${PORT}/?mode=overlay`);
  console.log(`  개발 서버와 함께: npm run dev → http://localhost:5173/?t=${TOKEN}`);
  console.log(`  손으로: curl "http://127.0.0.1:${PORT}/mock/trigger?what=interrupt&t=${TOKEN}"`);
  console.log(`  dj-assis 세션은 channel 이 있지만 답하지 않아요(§10.1 안내 확인용).`);
  if (!QUIET) {
    for (const [ms, what] of SCRIPT) setTimeout(() => trigger(what), ms).unref();
    setInterval(() => {
      for (const [ms, what] of SCRIPT) setTimeout(() => trigger(what), ms).unref();
    }, 90_000).unref();
  }
});
