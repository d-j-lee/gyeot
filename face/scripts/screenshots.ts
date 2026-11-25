// dist 를 작은 정적 서버로 띄우고 headless Chrome 으로 화면을 찍는다.
//   npm run build && npm run screenshots
//   CHROME=/path/to/chrome 로 브라우저를 바꿀 수 있다. 결과는 screenshots/*.png
import { createServer } from 'node:http';
import { readFile, mkdir } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { extname, join, normalize, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import type { AddressInfo } from 'node:net';

const run = promisify(execFile);
const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..');
const dist = join(root, 'dist');
const outDir = join(root, 'screenshots');
const chrome = process.env.CHROME ?? '/usr/bin/google-chrome';

const MIME: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.json': 'application/json',
};

interface Shot {
  name: string;
  query: string;
  w: number;
  h: number;
  transparent?: boolean;
  path?: string;
}

const MOODS = ['idle', 'working', 'waiting', 'alert', 'done', 'sleep', 'listening'] as const;

const shots: Shot[] = [
  ...MOODS.map((m) => ({ name: `overlay-mood-${m}`, query: `mode=overlay&demo=${m}&theme=light`, w: 240, h: 200, transparent: true })),
  { name: 'overlay-bubble-interrupt', query: 'mode=overlay&demo=bubble&theme=light', w: 380, h: 300, transparent: true },
  { name: 'overlay-bubble-nudge', query: 'mode=overlay&demo=nudge&theme=light', w: 380, h: 300, transparent: true },
  { name: 'overlay-bubble-reply', query: 'mode=overlay&demo=reply&theme=light', w: 380, h: 300, transparent: true },
  { name: 'overlay-panel-light', query: 'mode=overlay&demo=panel&theme=light', w: 440, h: 780, transparent: true },
  { name: 'overlay-panel-dark', query: 'mode=overlay&demo=panel&theme=dark', w: 440, h: 780, transparent: true },
  { name: 'overlay-chat-light', query: 'mode=overlay&demo=panel&tab=chat&theme=light', w: 440, h: 780, transparent: true },
  { name: 'default-panel-light', query: 'demo=panel&theme=light', w: 1200, h: 820 },
  { name: 'default-panel-dark', query: 'demo=panel&theme=dark', w: 1200, h: 820 },
  { name: 'default-chat-light', query: 'demo=panel&tab=chat&theme=light', w: 1200, h: 820 },
  { name: 'default-settings-light', query: 'demo=panel&tab=settings&theme=light', w: 1200, h: 1220 },
  { name: 'default-settings-dark', query: 'demo=panel&tab=settings&theme=dark', w: 1200, h: 1220 },
  { name: 'default-narrow-light', query: 'demo=bubble&theme=light', w: 560, h: 820 },
  { name: 'mood-sheet-light', query: 'demo=sheet&theme=light', w: 1040, h: 300 },
  { name: 'mood-sheet-dark', query: 'demo=sheet&theme=dark', w: 1040, h: 300 },
  { name: 'app-icon-1024', query: '', path: '/app-icon.svg', w: 1024, h: 1024, transparent: true },
];

async function serve() {
  const server = createServer(async (req, res) => {
    try {
      const url = new URL(req.url ?? '/', 'http://x');
      let p = normalize(decodeURIComponent(url.pathname)).replace(/^(\.\.[/\\])+/, '');
      if (p === '/' || !extname(p)) p = '/index.html';
      const file = join(dist, p);
      if (!file.startsWith(dist)) throw new Error('밖');
      const body = await readFile(file);
      res.writeHead(200, { 'Content-Type': MIME[extname(file)] ?? 'application/octet-stream' });
      res.end(body);
    } catch {
      res.writeHead(404).end();
    }
  });
  await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
  return server;
}

async function main() {
  if (!existsSync(join(dist, 'index.html'))) {
    console.error('dist 가 없어요. 먼저 npm run build 를 하세요.');
    process.exit(1);
  }
  await mkdir(outDir, { recursive: true });
  const server = await serve();
  const port = (server.address() as AddressInfo).port;
  const only = process.argv[2] ? new RegExp(process.argv[2]) : null;
  try {
    for (const s of shots) {
      if (only && !only.test(s.name)) continue;
      const url = `http://127.0.0.1:${port}${s.path ?? '/'}${s.query ? `?${s.query}` : ''}`;
      const out = join(outDir, `${s.name}.png`);
      const args = [
        '--headless=new',
        '--no-sandbox',
        '--disable-gpu',
        '--hide-scrollbars',
        '--mute-audio',
        '--no-first-run',
        '--lang=ko-KR',
        '--force-device-scale-factor=2',
        `--window-size=${s.w},${s.h}`,
        '--virtual-time-budget=1800',
        `--screenshot=${out}`,
      ];
      if (s.transparent) args.push('--default-background-color=00000000');
      args.push(url);
      await run(chrome, args, { timeout: 60_000 });
      console.log(`찍음 ${s.name}.png`);
    }
  } finally {
    server.close();
  }
}

await main();
