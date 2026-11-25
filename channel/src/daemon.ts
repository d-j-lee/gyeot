// 곁 데몬(contracts §7)에 말하는 작은 HTTP 클라이언트. 토큰은 오류 문구·기록에 절대 싣지 않는다.

import { readFile } from 'node:fs/promises';

export type DaemonErrorKind =
  /** 데몬에 닿지 못함: 연결 거부·시간 초과·토큰 파일 없음·주소 설정 오류 */
  | 'unreachable'
  /** 401: 토큰이 맞지 않음 */
  | 'unauthorized'
  /** 404·410: 데몬이 이 cid를 모름(데몬 재시작 등) */
  | 'unknown_cid'
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

  constructor(private readonly opts: DaemonClientOptions) {}

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
      throw this.normalize(err, timer, token);
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
    const url = new URL('/v1/channel/stream', origin);
    url.searchParams.set('cid', cid);
    url.searchParams.set('t', token);
    const timer = new Deadline(connectTimeoutMs, signal);
    try {
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
      throw this.normalize(err, timer, token);
    }
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

function scrub(err: DaemonError, token: string): DaemonError {
  if (!err.message.includes(token)) return err;
  return new DaemonError(err.kind, err.message.split(token).join('[가림]'), err.status);
}
