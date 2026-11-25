// 곁 데몬과의 연결: 확인(§13.1) → 등록(cid) → SSE 스트림 → 끊기면 1→2→4…최대 30단위로 다시(contracts §10).
// 확인에 실패하면 연결하지 않고 stderr에 이유를 적은 뒤 같은 간격으로 다시 시도한다.

import { DaemonClient, DaemonError } from './daemon.js';
import { describeError, log } from './log.js';
import { SseParser, type SseEvent } from './sse.js';
import { checkMessage, type ChannelMessage } from './validate.js';

/** 재연결 대기: unit × 2^attempt, 최대 unit × 30. */
export function backoffDelay(attempt: number, unitMs: number): number {
  const factor = Math.min(30, 2 ** Math.min(attempt, 5));
  return factor * unitMs;
}

export interface Registration {
  readonly pid: number;
  readonly ppid: number;
  readonly cwd: string;
}

export interface LinkOptions {
  readonly client: DaemonClient;
  readonly registration: Registration;
  readonly onMessage: (message: ChannelMessage) => Promise<void>;
  readonly backoffUnitMs: number;
}

const REGISTER_TIMEOUT_MS = 3000;
const CONNECT_TIMEOUT_MS = 5000;
/** 종료 때 unregister를 기다리는 한도. Windows의 Claude Code는 stdin을 닫고 300ms 뒤 프로세스를 끝낸다. */
export const UNREGISTER_TIMEOUT_MS = 250;
/** 이만큼 버틴 연결은 건강했다고 보고 대기를 처음(1단위)부터 다시 센다. */
const STABLE_MS = 5000;

type StreamOutcome = { kind: 'ended' | 'unknown_cid'; healthy: boolean };

export class Link {
  private cid: string | null = null;
  private registering: Promise<string> | null = null;
  private lastEventId: string | undefined;
  private started = false;
  private stopped = false;
  private streamAbort: AbortController | null = null;
  private wake: (() => void) | null = null;
  private lastProblem = '';

  constructor(private readonly opts: LinkOptions) {}

  get currentCid(): string | null {
    return this.cid;
  }

  start(): void {
    if (this.started || this.stopped) return;
    this.started = true;
    void this.run();
  }

  /** 등록된 cid를 돌려준다. 없으면 지금 등록한다(동시에 불러도 한 번만). */
  ensureCid(): Promise<string> {
    if (this.stopped) return Promise.reject(new DaemonError('unreachable', 'channel이 끝나는 중입니다'));
    if (this.cid !== null) return Promise.resolve(this.cid);
    if (this.registering === null) {
      this.registering = this.register().finally(() => {
        this.registering = null;
      });
    }
    return this.registering;
  }

  /** 데몬이 cid를 모른다고 했다. 다음 ensureCid가 다시 등록한다. */
  invalidate(cid: string): void {
    if (this.cid !== cid) return;
    this.cid = null;
    this.lastEventId = undefined;
    this.streamAbort?.abort();
  }

  /** 재연결을 멈추고 데몬에 unregister를 보낸다(짧게 기다린다). */
  async stop(): Promise<void> {
    if (this.stopped) return;
    this.stopped = true;
    this.wake?.();
    const cid = this.cid;
    this.cid = null;
    if (cid !== null) await this.unregister(cid);
    this.streamAbort?.abort();
  }

  private async register(): Promise<string> {
    const res = await this.opts.client.post('/v1/channel/register', this.opts.registration, {
      timeoutMs: REGISTER_TIMEOUT_MS,
      cidBound: false,
    });
    const cid = (res as { cid?: unknown }).cid;
    const session = (res as { session?: unknown }).session;
    if (typeof cid !== 'string' || cid === '') {
      throw new DaemonError('rejected', '데몬 등록 응답에 cid가 없습니다');
    }
    if (this.stopped) {
      await this.unregister(cid);
      throw new DaemonError('unreachable', 'channel이 끝나는 중입니다');
    }
    this.cid = cid;
    this.lastEventId = undefined;
    log('info', `곁에 등록됨: cid=${cid}${typeof session === 'string' ? ` session=${session}` : ''}`);
    this.wake?.();
    return cid;
  }

  private async unregister(cid: string): Promise<void> {
    try {
      await this.opts.client.post('/v1/channel/unregister', { cid }, { timeoutMs: UNREGISTER_TIMEOUT_MS, cidBound: true });
      log('info', `곁에서 등록 해제: cid=${cid}`);
    } catch (err) {
      log('warn', `등록 해제 실패(무시): ${describeError(err)}`);
    }
  }

  private async run(): Promise<void> {
    let attempt = 0;
    let previous: StreamOutcome['kind'] | 'failed' = 'ended';
    while (!this.stopped) {
      let outcome: StreamOutcome['kind'] | 'failed' = 'failed';
      let usedCid: string | null = null;
      try {
        usedCid = await this.ensureCid();
        const r = await this.streamOnce(usedCid);
        outcome = r.kind;
        if (r.healthy) attempt = 0;
      } catch (err) {
        if (this.stopped) break;
        if (err instanceof DaemonError && err.kind === 'unknown_cid') {
          outcome = 'unknown_cid';
        } else {
          this.report(`곁 연결 실패(다시 시도함): ${describeError(err)}`);
        }
      }
      if (this.stopped) break;
      if (outcome === 'unknown_cid') {
        log('info', '데몬이 cid를 모름 — 다시 등록합니다');
        if (usedCid !== null) this.invalidate(usedCid);
        // 데몬 재시작 직후: 한 번은 기다리지 않고 바로 다시 등록한다.
        if (previous !== 'unknown_cid') {
          previous = outcome;
          continue;
        }
      }
      previous = outcome;
      const delay = backoffDelay(attempt, this.opts.backoffUnitMs);
      attempt += 1;
      await this.sleep(delay);
    }
  }

  private async streamOnce(cid: string): Promise<StreamOutcome> {
    const abort = new AbortController();
    this.streamAbort = abort;
    let healthy = false;
    const pending: SseEvent[] = [];
    const parser = new SseParser(
      (ev) => {
        healthy = true;
        pending.push(ev);
      },
      () => {
        healthy = true;
      },
    );
    try {
      const body = await this.opts.client.openStream(cid, this.lastEventId, abort.signal, CONNECT_TIMEOUT_MS);
      const openedAt = Date.now();
      this.report('');
      log('info', `곁 스트림 연결됨: cid=${cid}`);
      const reader = body.getReader();
      const decoder = new TextDecoder('utf-8');
      try {
        for (;;) {
          const { value, done } = await reader.read();
          if (done) break;
          parser.push(decoder.decode(value, { stream: true }));
          // 순서를 지키려고 한 사건씩 기다리며 보낸다.
          while (pending.length > 0) await this.handle(pending.shift() as SseEvent);
        }
        log('info', '곁 스트림이 끝남 — 다시 연결합니다');
      } catch (err) {
        if (this.stopped || abort.signal.aborted) return { kind: 'ended', healthy };
        log('warn', `곁 스트림 끊김: ${describeError(err)}`);
      } finally {
        parser.reset();
        abort.abort();
      }
      return { kind: 'ended', healthy: healthy || Date.now() - openedAt >= STABLE_MS };
    } finally {
      if (this.streamAbort === abort) this.streamAbort = null;
      // 다시 연결할 때마다 다시 확인한다(§13.1): 끊긴 사이에 그 포트의 주인이 바뀌었을 수 있다.
      this.opts.client.forgetVerification();
    }
  }

  private async handle(ev: SseEvent): Promise<void> {
    if (ev.id !== undefined) this.lastEventId = ev.id;
    if (ev.event !== 'message') return;
    let data: unknown;
    try {
      data = JSON.parse(ev.data);
    } catch {
      log('warn', 'message 사건의 data가 JSON이 아니어서 버림');
      return;
    }
    const checked = checkMessage(data);
    if (!checked.ok) {
      log('warn', `계약과 다른 message를 버림: ${checked.error}`);
      return;
    }
    try {
      await this.opts.onMessage(checked.value);
    } catch (err) {
      log('error', `Claude Code로 보내지 못함: ${describeError(err)}`);
    }
  }

  private sleep(ms: number): Promise<void> {
    return new Promise((resolve) => {
      const done = (): void => {
        clearTimeout(timer);
        if (this.wake === done) this.wake = null;
        resolve();
      };
      const timer = setTimeout(done, ms);
      this.wake = done;
    });
  }

  /** 같은 문제를 거듭 기록하지 않는다. */
  private report(problem: string): void {
    if (problem === this.lastProblem) return;
    this.lastProblem = problem;
    if (problem) log('warn', problem);
  }
}
