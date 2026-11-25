import type { SseEvent, SseEventName } from '../types.ts';
import type { ConnState } from '../state/reducer.ts';

export const SSE_EVENTS: readonly SseEventName[] = [
  'snapshot',
  'item',
  'item_removed',
  'presence',
  'deliver',
  'reply',
  'proposal',
  'sessions',
];

/** 다시 잇기 간격: 1→2→4→8→16→30초(최대 30초). attempt 는 1부터. */
export function backoffDelay(attempt: number): number {
  const n = Math.max(1, Math.floor(attempt));
  return Math.min(30_000, 1_000 * 2 ** (n - 1));
}

export type ProbeResult = 'ok' | 'auth' | 'down';

export interface StreamOptions {
  url: () => string;
  onEvent: (ev: SseEvent) => void;
  onStatus: (conn: ConnState) => void;
  /** 끊겼을 때 원인을 가린다(세션 쿠키가 없거나 만료됐으면 auth). */
  probe?: () => Promise<ProbeResult>;
  /** 원인이 auth 로 판정됐을 때 한 번. 스트림은 이미 멈춘 뒤다. */
  onAuthLost?: () => void;
  EventSourceImpl?: typeof EventSource;
}

/**
 * /v1/stream 연결. 브라우저의 자동 재연결 대신 직접 간격을 늘려 가며 다시 잇는다.
 * 다시 이으면 데몬이 snapshot 을 먼저 보내므로 상태가 저절로 맞춰진다.
 * 열쇠 문제(401)면 저절로 다시 잇지 않는다 — 서버를 두드려 봐야 소용없다. `retryNow()`(사람·창 복귀)로만 다시 본다.
 */
export class EventStream {
  private es: EventSource | null = null;
  private attempt = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private stopped = true;
  /** 연결마다 늘린다. 늦게 끝난 원인 판정이 새 연결을 건드리지 못하게 한다. */
  private gen = 0;
  private readonly opts: StreamOptions;

  constructor(opts: StreamOptions) {
    this.opts = opts;
  }

  start(): void {
    this.stopped = false;
    this.connect();
  }

  stop(): void {
    this.stopped = true;
    this.clearTimer();
    this.close();
  }

  /** "지금 다시" 단추·온라인 복귀·창이 다시 보일 때 */
  retryNow(): void {
    if (this.stopped || this.es?.readyState === 1) return;
    this.clearTimer();
    this.close();
    this.connect();
  }

  /** 열쇠가 없다(401). 열린 연결도 닫고 다시 잇지 않는다. `retryNow()`로만 다시 시도한다. */
  authLost(): void {
    if (this.stopped) return;
    this.gen += 1;
    this.clearTimer();
    this.close();
    this.opts.onStatus({ status: 'auth', attempt: this.attempt, retryAt: null });
  }

  private connect(): void {
    const Impl = this.opts.EventSourceImpl ?? EventSource;
    const gen = ++this.gen;
    this.opts.onStatus({ status: this.attempt === 0 ? 'connecting' : 'retrying', attempt: this.attempt, retryAt: null });
    let es: EventSource;
    try {
      es = new Impl(this.opts.url());
    } catch {
      void this.fail(gen);
      return;
    }
    this.es = es;
    es.onopen = () => {
      if (this.es !== es) return;
      this.attempt = 0;
      this.opts.onStatus({ status: 'open', attempt: 0, retryAt: null });
    };
    es.onerror = () => {
      if (this.es !== es) return;
      this.close();
      void this.fail(gen);
    };
    for (const name of SSE_EVENTS) {
      es.addEventListener(name, (msg: Event) => {
        if (this.es !== es) return;
        const raw = (msg as MessageEvent).data;
        let data: unknown;
        try {
          data = typeof raw === 'string' ? JSON.parse(raw) : raw;
        } catch {
          return; // 깨진 줄은 버린다
        }
        this.opts.onEvent({ event: name, data } as SseEvent);
      });
    }
  }

  private async fail(gen: number): Promise<void> {
    if (this.stopped) return;
    this.attempt += 1;
    let kind: ProbeResult = 'down';
    if (this.opts.probe) {
      try {
        kind = await this.opts.probe();
      } catch {
        kind = 'down';
      }
    }
    // 기다리는 동안 멈췄거나 이미 다시 이었으면(retryNow) 이 판정은 낡았다.
    if (this.stopped || gen !== this.gen) return;
    if (kind === 'auth') {
      this.authLost();
      this.opts.onAuthLost?.();
      return;
    }
    const delay = backoffDelay(this.attempt);
    const retryAt = Date.now() + delay;
    this.opts.onStatus({ status: 'retrying', attempt: this.attempt, retryAt });
    this.clearTimer();
    this.timer = setTimeout(() => {
      this.timer = null;
      if (!this.stopped) this.connect();
    }, delay);
  }

  private close(): void {
    if (this.es) {
      this.es.onopen = null;
      this.es.onerror = null;
      this.es.close();
      this.es = null;
    }
  }

  private clearTimer(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
  }
}
