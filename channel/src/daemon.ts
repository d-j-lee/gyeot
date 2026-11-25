// 곁 데몬(contracts §7)에 말하는 작은 HTTP 클라이언트. 토큰은 오류 문구·기록에 절대 싣지 않는다.
//
// 토큰을 보내기 전에 늘 §13.1로 확인한다(G-032): `GET /v1/hello?n=<32바이트 난수 hex>`의 `proof`가
// hex(HMAC-SHA256(열쇠 = 토큰 UTF-8 바이트, 글 = "gyeot-hello/v1|<연결한 포트>|<n>"))와 같아야 한다(상수 시간 비교).
// 확인은 연결(등록·스트림)마다 한다. 스트림이 끊기거나 요청이 실패하면 다시 확인한다.

import { createHmac, randomBytes, timingSafeEqual } from 'node:crypto';
import { readFile } from 'node:fs/promises';

export type DaemonErrorKind =
  /** 데몬에 닿지 못함: 연결 거부·시간 초과·토큰 파일 없음·주소 설정 오류 */
  | 'unreachable'
  /** 401: 토큰이 맞지 않음 */
  | 'unauthorized'
  /** 404·410: 데몬이 이 cid를 모름(데몬 재시작 등) */
  | 'unknown_cid'
  /** §13.1 확인 실패: 이 포트의 서버가 내 토큰을 모름(다른 사용자·다른 프로그램). 토큰을 보내지 않았다 */
  | 'not_daemon'
  /** 그 밖의 거절·계약과 다른 응답 */
  | 'rejected';

export class DaemonError extends Error {
  constructor(
    readonly kind: DaemonErrorKind,
    message: string,
    readonly status?: number,
  ) {
    super(message);
    this.name = 'DaemonError';
  }
}

const TOKEN_RE = /^[0-9a-f]{64}$/;
const PROOF_RE = /^[0-9a-fA-F]{64}$/;

/** 확인에 실패했을 때의 이유 머리(셸·`gyeotd --open`과 같은 말). */
export const NOT_MINE = '이 포트의 서버는 내 곁 데몬이 아니에요(다른 사용자나 프로그램일 수 있어요)';

/** §13.1 증명(소문자 hex). */
export function helloProof(token: string, port: number, nonce: string): string {
  return helloMac(token, port, nonce).toString('hex');
}

function helloMac(token: string, port: number, nonce: string): Buffer {
  return createHmac('sha256', Buffer.from(token, 'utf8')).update(`gyeot-hello/v1|${port}|${nonce}`, 'utf8').digest();
}

/** `/v1/hello` 응답 본문이 이 토큰·포트·난수의 증명이면 undefined, 아니면 이유. 비교는 상수 시간이다. */
export function helloProblem(body: unknown, token: string, port: number, nonce: string): string | undefined {
  if (typeof body !== 'object' || body === null || Array.isArray(body)) return '확인 응답이 JSON 객체가 아닙니다';
  const { v, proof } = body as { v?: unknown; proof?: unknown };
  if (v !== 1) return '확인 응답의 판(v)이 1이 아닙니다';
  if (typeof proof !== 'string' || !PROOF_RE.test(proof)) return '확인 응답에 64자 hex 증명이 없습니다';
  const expected = helloMac(token, port, nonce);
  const got = Buffer.from(proof, 'hex');
  if (got.length !== expected.length || !timingSafeEqual(got, expected)) {
    return '확인 값이 맞지 않습니다(이 서버는 내 토큰을 모릅니다)';
  }
  return undefined;
}

export interface DaemonClientOptions {
  /** 검사를 마친 데몬 출처(`http://127.0.0.1:47321`). 없으면 configError를 쓴다. */
  readonly origin: string | undefined;
  readonly configError?: string | undefined;
  readonly tokenFile: string;
}

export interface PostOptions {
  readonly timeoutMs: number;
  /** cid를 싣는 요청이면 404·410을 unknown_cid로 본다. */
  readonly cidBound: boolean;
}

export class DaemonClient {
  private token: string | undefined;
  /** 지금 이어 쓰는 데몬을 §13.1로 확인했다. 연결이 끊기거나 요청이 실패하면 지운다. */
  private verified = false;

  constructor(private readonly opts: DaemonClientOptions) {}

  /** 다음 요청 전에 다시 확인하게 한다(스트림이 끊겼을 때 = 다시 연결할 때). */
  forgetVerification(): void {
    this.verified = false;
  }

  get tokenFile(): string {
    return this.opts.tokenFile;
  }

  /** 다음 요청에서 토큰 파일을 다시 읽게 한다(401을 받았을 때). */
  invalidateToken(): void {
    this.token = undefined;
  }

  async post(path: string, body: unknown, opts: PostOptions): Promise<unknown> {
    const origin = this.requireOrigin();
    const token = await this.readToken();
    const timer = new Deadline(opts.timeoutMs);
    try {
      await this.verify(origin, token, timer);
      const res = await fetch(new URL(path, origin), {
        method: 'POST',
        headers: {
          Authorization: `Bearer ${token}`,
          'Content-Type': 'application/json',
          Accept: 'application/json',
        },
        body: JSON.stringify(body),
        redirect: 'error',
        signal: timer.signal,
      });
      await this.checkStatus(res, opts.cidBound);
      const text = await res.text();
      try {
        return text === '' ? {} : (JSON.parse(text) as unknown);
      } catch {
        throw new DaemonError('rejected', '데몬 응답이 JSON이 아닙니다', res.status);
      }
    } catch (err) {
      throw this.fail(err, timer, token);
    } finally {
      timer.dispose();
    }
  }

  /**
   * `GET /v1/channel/stream?cid=…`를 연다. 응답 머리가 connectTimeoutMs 안에 와야 한다.
   * 본문은 signal이 끊을 때까지 열려 있다.
   */
  async openStream(
    cid: string,
    lastEventId: string | undefined,
    signal: AbortSignal,
    connectTimeoutMs: number,
  ): Promise<ReadableStream<Uint8Array>> {
    const origin = this.requireOrigin();
    const token = await this.readToken();
    // 토큰은 Authorization 머리로만 보낸다(§7: 쿼리 ?t= 는 받지 않는다, URL에 비밀을 싣지 않는다).
    const url = new URL('/v1/channel/stream', origin);
    url.searchParams.set('cid', cid);
    const timer = new Deadline(connectTimeoutMs, signal);
    try {
      await this.verify(origin, token, timer);
      const headers: Record<string, string> = {
        Authorization: `Bearer ${token}`,
        Accept: 'text/event-stream',
        'Cache-Control': 'no-cache',
      };
      if (lastEventId !== undefined) headers['Last-Event-ID'] = lastEventId;
      const res = await fetch(url, { headers, redirect: 'error', signal: timer.signal });
      await this.checkStatus(res, true);
      const type = res.headers.get('content-type') ?? '';
      if (!type.toLowerCase().startsWith('text/event-stream')) {
        await res.body?.cancel().catch(() => {});
        throw new DaemonError('rejected', `스트림 응답 형식이 text/event-stream이 아닙니다(${type || '없음'})`, res.status);
      }
      if (!res.body) throw new DaemonError('rejected', '스트림 응답에 본문이 없습니다', res.status);
      timer.clearTimeout(); // 머리를 받았다. 본문은 조용해도 된다.
      return res.body;
    } catch (err) {
      timer.dispose();
      throw this.fail(err, timer, token);
    }
  }

  /**
   * §13.1 확인. 이미 확인했으면 그대로 둔다. 확인 요청에는 토큰을 싣지 않는다.
   * 실패하면 'not_daemon'(토큰도 페이로드도 보내지 않는다).
   */
  private async verify(origin: string, token: string, timer: Deadline): Promise<void> {
    if (this.verified) return;
    const port = portOf(origin);
    const nonce = randomBytes(32).toString('hex');
    const url = new URL('/v1/hello', origin);
    url.searchParams.set('n', nonce);
    const res = await fetch(url, { headers: { Accept: 'application/json' }, redirect: 'error', signal: timer.signal });
    if (res.status !== 200) {
      await res.body?.cancel().catch(() => {});
      throw this.notMine(port, `/v1/hello가 HTTP ${res.status}를 돌려줬습니다(곁 데몬이 아니거나 확인을 모르는 옛 판)`);
    }
    let body: unknown;
    try {
      body = JSON.parse(await res.text());
    } catch {
      throw this.notMine(port, '확인 응답이 JSON이 아닙니다');
    }
    const problem = helloProblem(body, token, port, nonce);
    if (problem !== undefined) throw this.notMine(port, problem);
    this.verified = true;
  }

  private notMine(port: number, why: string): DaemonError {
    // 토큰 파일이 바뀌었을 수 있다(다시 설치 등). 다음 시도에서 다시 읽는다.
    this.invalidateToken();
    return new DaemonError(
      'not_daemon',
      `${NOT_MINE} — 포트 ${port}: ${why}. 내 토큰 파일(${this.opts.tokenFile})이 그 포트의 곁 데몬 것과 같은지도 확인하세요`,
    );
  }

  /** 오류를 정리하고, 거절('rejected')이 아니면 다음 요청 전에 다시 확인하게 한다. */
  private fail(err: unknown, timer: Deadline, token: string): unknown {
    const e = this.normalize(err, timer, token);
    if (!(e instanceof DaemonError && e.kind === 'rejected')) this.verified = false;
    return e;
  }

  private requireOrigin(): string {
    if (this.opts.origin === undefined) {
      throw new DaemonError('unreachable', this.opts.configError ?? 'GYEOT_URL을 해석하지 못했습니다');
    }
    return this.opts.origin;
  }

  private async readToken(): Promise<string> {
    if (this.token !== undefined) return this.token;
    let raw: string;
    try {
      raw = await readFile(this.opts.tokenFile, 'utf8');
    } catch (err) {
      const code = (err as { code?: unknown }).code;
      if (code === 'ENOENT') {
        throw new DaemonError('unreachable', `토큰 파일이 없습니다(${this.opts.tokenFile}) — 곁을 먼저 실행하세요`);
      }
      throw new DaemonError('unreachable', `토큰 파일을 읽지 못했습니다(${this.opts.tokenFile}, ${String(code ?? err)})`);
    }
    const token = raw.trim();
    if (!TOKEN_RE.test(token)) {
      throw new DaemonError('unreachable', `토큰 파일 형식이 올바르지 않습니다(${this.opts.tokenFile})`);
    }
    this.token = token;
    return token;
  }

  private async checkStatus(res: Response, cidBound: boolean): Promise<void> {
    if (res.ok) return;
    const snippet = await readSnippet(res);
    if (res.status === 401) {
      this.invalidateToken();
      throw new DaemonError('unauthorized', `토큰이 맞지 않습니다(HTTP 401) — ${this.opts.tokenFile}`, 401);
    }
    if (cidBound && (res.status === 404 || res.status === 410)) {
      throw new DaemonError('unknown_cid', `데몬이 이 channel을 모릅니다(HTTP ${res.status})`, res.status);
    }
    throw new DaemonError('rejected', `HTTP ${res.status}${snippet ? ` ${snippet}` : ''}`, res.status);
  }

  private normalize(err: unknown, timer: Deadline, token: string): unknown {
    if (err instanceof DaemonError) return scrub(err, token);
    if (timer.timedOut) return new DaemonError('unreachable', `응답 시간 초과(${timer.ms}ms)`);
    if (timer.outerAborted) return err; // 호출한 쪽이 끊었다(종료 중).
    return scrub(new DaemonError('unreachable', describeFetchError(err)), token);
  }
}

/** 시간 제한과 바깥 신호를 하나로 묶는다(AbortSignal.any 없이: Node 20.0 호환). */
class Deadline {
  private readonly controller = new AbortController();
  private timer: NodeJS.Timeout | undefined;
  private readonly onOuterAbort: () => void;
  timedOut = false;
  outerAborted = false;

  constructor(
    readonly ms: number,
    private readonly outer?: AbortSignal,
  ) {
    this.timer = setTimeout(() => {
      this.timedOut = true;
      this.controller.abort(new Error('timeout'));
    }, ms);
    this.onOuterAbort = () => {
      this.outerAborted = true;
      this.controller.abort(outer?.reason);
    };
    if (outer?.aborted) this.onOuterAbort();
    else outer?.addEventListener('abort', this.onOuterAbort, { once: true });
  }

  get signal(): AbortSignal {
    return this.controller.signal;
  }

  clearTimeout(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = undefined;
  }

  dispose(): void {
    this.clearTimeout();
    this.outer?.removeEventListener('abort', this.onOuterAbort);
  }
}

async function readSnippet(res: Response): Promise<string> {
  try {
    const text = (await res.text()).replace(/\s+/g, ' ').trim();
    return text.length > 200 ? `${text.slice(0, 200)}…` : text;
  } catch {
    return '';
  }
}

function describeFetchError(err: unknown): string {
  const cause = (err as { cause?: { code?: unknown; message?: unknown } } | undefined)?.cause;
  const code = typeof cause?.code === 'string' ? cause.code : undefined;
  switch (code) {
    case 'ECONNREFUSED':
      return '연결이 거부됨 — 곁이 켜져 있는지 확인하세요';
    case 'ECONNRESET':
      return '연결이 끊김';
    case 'ENOTFOUND':
    case 'EAI_AGAIN':
      return '주소를 찾지 못함';
    default: {
      const msg = err instanceof Error ? err.message : String(err);
      const causeMsg = typeof cause?.message === 'string' ? cause.message : undefined;
      return causeMsg && causeMsg !== msg ? `${msg}: ${causeMsg}` : msg;
    }
  }
}

/** 연결할 포트(증명에 넣는다). 기본 포트면 80. */
function portOf(origin: string): number {
  const url = new URL(origin);
  return url.port === '' ? 80 : Number(url.port);
}

function scrub(err: DaemonError, token: string): DaemonError {
  if (!err.message.includes(token)) return err;
  return new DaemonError(err.kind, err.message.split(token).join('[가림]'), err.status);
}
