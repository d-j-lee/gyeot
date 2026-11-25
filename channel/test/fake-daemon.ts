// 시험용 가짜 곁 데몬: contracts §7의 channel 끝점만 흉내 낸다(토큰·Host 검사 포함).

import http from 'node:http';
import type { AddressInfo } from 'node:net';

export interface Recorded {
  readonly method: string;
  readonly path: string;
  readonly query: URLSearchParams;
  readonly headers: http.IncomingHttpHeaders;
  readonly body: unknown;
}

export class FakeDaemon {
  readonly requests: Recorded[] = [];
  private readonly server: http.Server;
  private readonly cids = new Set<string>();
  private readonly streams = new Map<string, Set<http.ServerResponse>>();
  private nextCid = 1;
  private nextDecision = 1;
  port = 0;

  constructor(readonly token: string) {
    this.server = http.createServer((req, res) => void this.handle(req, res));
  }

  async start(port = 0): Promise<void> {
    await new Promise<void>((resolve, reject) => {
      this.server.once('error', reject);
      this.server.listen(port, '127.0.0.1', () => resolve());
    });
    this.port = (this.server.address() as AddressInfo).port;
  }

  get url(): string {
    return `http://127.0.0.1:${this.port}`;
  }

  async stop(): Promise<void> {
    for (const set of this.streams.values()) for (const res of set) res.destroy();
    this.server.closeAllConnections();
    await new Promise<void>((resolve) => this.server.close(() => resolve()));
  }

  of(path: string): Recorded[] {
    return this.requests.filter((r) => r.path === path);
  }

  streamCount(cid: string): number {
    return this.streams.get(cid)?.size ?? 0;
  }

  /** SSE 원문을 그대로 쓴다(조각 나눔·여러 줄 data 시험용). */
  writeRaw(cid: string, raw: string): void {
    for (const res of this.streams.get(cid) ?? []) res.write(raw);
  }

  sendMessage(cid: string, msg: unknown): void {
    this.writeRaw(cid, `event: message\ndata: ${JSON.stringify(msg)}\n\n`);
  }

  /** 열린 스트림을 모두 끊는다(연결 끊김 흉내). graceful이면 응답을 정상으로 끝낸다. */
  dropStreams(graceful = false): void {
    for (const set of this.streams.values()) for (const res of set) (graceful ? res.end() : res.destroy());
    this.streams.clear();
  }

  /** 데몬 재시작 흉내: 아는 cid를 모두 잊고 스트림을 끊는다. */
  forgetAll(): void {
    this.cids.clear();
    this.dropStreams();
  }

  /** 스트림은 둔 채 cid만 잊는다(도구 호출의 재등록 시험용). */
  forgetCids(): void {
    this.cids.clear();
  }

  private authorized(req: http.IncomingMessage, query: URLSearchParams): boolean {
    const auth = req.headers.authorization;
    return auth === `Bearer ${this.token}` || query.get('t') === this.token;
  }

  private async handle(req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
    const url = new URL(req.url ?? '/', 'http://x');
    const raw = await readBody(req);
    let body: unknown = undefined;
    if (raw) {
      try {
        body = JSON.parse(raw);
      } catch {
        body = raw;
      }
    }
    this.requests.push({ method: req.method ?? '', path: url.pathname, query: url.searchParams, headers: req.headers, body });

    const host = req.headers.host;
    if (host !== `127.0.0.1:${this.port}` && host !== `localhost:${this.port}`) return json(res, 421, { error: 'host' });
    if (!this.authorized(req, url.searchParams)) return json(res, 401, { error: 'unauthorized' });

    const b = (body ?? {}) as Record<string, unknown>;
    const route = `${req.method} ${url.pathname}`;
    switch (route) {
      case 'POST /v1/channel/register': {
        const cid = `ch_${this.nextCid++}`;
        this.cids.add(cid);
        return json(res, 200, { cid, session: null });
      }
      case 'GET /v1/channel/stream': {
        const cid = url.searchParams.get('cid') ?? '';
        if (!this.cids.has(cid)) return json(res, 404, { error: 'unknown cid' });
        res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' });
        res.write(': connected\n\n');
        let set = this.streams.get(cid);
        if (!set) this.streams.set(cid, (set = new Set()));
        set.add(res);
        res.on('close', () => set?.delete(res));
        return;
      }
      case 'POST /v1/channel/reply':
        if (!this.cids.has(String(b['cid']))) return json(res, 404, { error: 'unknown cid' });
        return json(res, 200, { ok: true });
      case 'POST /v1/channel/decision':
        if (!this.cids.has(String(b['cid']))) return json(res, 404, { error: 'unknown cid' });
        return json(res, 200, { decision_id: `d_${this.nextDecision++}` });
      case 'POST /v1/channel/unregister':
        this.cids.delete(String(b['cid']));
        return json(res, 200, { ok: true });
      default:
        return json(res, 404, { error: 'not found' });
    }
  }
}

function readBody(req: http.IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    req.on('data', (c: Buffer) => chunks.push(c));
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
    req.on('error', reject);
  });
}

function json(res: http.ServerResponse, status: number, body: unknown): void {
  res.writeHead(status, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify(body));
}

/** 조건이 참이 될 때까지 기다린다. */
export async function waitFor(cond: () => boolean, timeoutMs = 5000, label = '조건'): Promise<void> {
  const start = Date.now();
  while (!cond()) {
    if (Date.now() - start > timeoutMs) throw new Error(`시간 초과: ${label}`);
    await new Promise((r) => setTimeout(r, 10));
  }
}
