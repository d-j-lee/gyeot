import type { DecisionResult, DismissReason, FocusResult, ItemAction, SayResult, SseEvent } from '../types.ts';
import type { ConnState } from '../state/reducer.ts';
import { ApiError, createApi, type Api } from '../lib/api.ts';
import { EventStream, type ProbeResult } from '../lib/stream.ts';

export interface BackendSink {
  event(ev: SseEvent): void;
  status(conn: ConnState): void;
}

/** 얼굴이 데몬에 바라는 것. 실제 데몬(HTTP)과 데모가 같은 모양을 지킨다. */
export interface Backend {
  start(sink: BackendSink): void;
  stop(): void;
  retryNow(): void;
  itemAction(id: string, action: ItemAction, opts?: { reason?: DismissReason; minutes?: number }): Promise<void>;
  focus(on: boolean, minutes?: number): Promise<FocusResult>;
  listening(on: boolean): Promise<void>;
  say(text: string, session?: string | null): Promise<SayResult>;
  decide(decisionId: string, choice: string): Promise<DecisionResult>;
  proposal(id: string, accept: boolean): Promise<void>;
}

export class HttpBackend implements Backend {
  readonly api: Api;
  private stream: EventStream | null = null;
  private wake = () => this.stream?.retryNow();
  private onVisible = () => {
    if (document.visibilityState === 'visible') this.stream?.retryNow();
  };

  constructor(base: string, token: string, api?: Api) {
    this.api = api ?? createApi(base, token);
  }

  start(sink: BackendSink): void {
    const api = this.api;
    this.stream = new EventStream({
      url: () => api.streamUrl(),
      onEvent: (ev) => sink.event(ev),
      onStatus: (c) => sink.status(c),
      probe: async (): Promise<ProbeResult> => {
        if (!api.token) return 'auth';
        try {
          await api.state();
          return 'ok';
        } catch (e) {
          return e instanceof ApiError && e.status === 401 ? 'auth' : 'down';
        }
      },
    });
    this.stream.start();
    window.addEventListener('online', this.wake);
    document.addEventListener('visibilitychange', this.onVisible);
  }

  stop(): void {
    this.stream?.stop();
    this.stream = null;
    window.removeEventListener('online', this.wake);
    document.removeEventListener('visibilitychange', this.onVisible);
  }

  retryNow(): void {
    this.stream?.retryNow();
  }

  itemAction(id: string, action: ItemAction, opts?: { reason?: DismissReason; minutes?: number }) {
    return this.api.itemAction(id, action, opts);
  }
  focus(on: boolean, minutes?: number) {
    return this.api.focus(on, minutes);
  }
  listening(on: boolean) {
    return this.api.listening(on);
  }
  say(text: string, session?: string | null) {
    return this.api.say(text, session);
  }
  decide(decisionId: string, choice: string) {
    return this.api.decide(decisionId, choice);
  }
  proposal(id: string, accept: boolean) {
    return this.api.proposal(id, accept);
  }
}
