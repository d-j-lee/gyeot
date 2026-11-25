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
  /** 끊겼을 때 원인을 가린다(토큰 문제면 auth). */
  probe?: () => Promise<ProbeResult>;
  EventSourceImpl?: typeof EventSource;
}

/**
 * /v1/stream 연결. 브라우저의 자동 재연결 대신 직접 간격을 늘려 가며 다시 잇는다.
 * 다시 이으면 데몬이 snapshot 을 먼저 보내므로 상태가 저절로 맞춰진다.
 */
export class EventStream {
  private es: EventSource | null = null;
  private attempt = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private stopped = true;
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

  private connect(): void {
    const Impl = this.opts.EventSourceImpl ?? EventSource;
    this.opts.onStatus({ status: this.attempt === 0 ? 'connecting' : 'retrying', attempt: this.attempt, retryAt: null });
    let es: EventSource;
    try {
      es = new Impl(this.opts.url());
    } catch {
      this.fail();
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
      this.fail();
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

  private async fail(): Promise<void> {
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
    if (this.stopped) return;
    const delay = kind === 'auth' ? 30_000 : backoffDelay(this.attempt);
    const retryAt = Date.now() + delay;
    this.opts.onStatus({ status: kind === 'auth' ? 'auth' : 'retrying', attempt: this.attempt, retryAt });
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
