import type { DecisionResult, DismissReason, FocusResult, ItemAction, SayResult, SseEvent } from '../types.ts';
import type { ConnState } from '../state/reducer.ts';
import { ApiError, createApi, type Api } from '../lib/api.ts';
import { EventStream, type ProbeResult } from '../lib/stream.ts';
import { renewSession } from '../lib/tauri.ts';

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

export interface HttpBackendDeps {
  api?: Api;
  EventSourceImpl?: typeof EventSource;
  /** 401 을 받았을 때 셸에 새 열쇠를 청한다(시험에서 바꿔 끼운다). */
  renew?: () => unknown;
}

/**
 * 실제 데몬. 인증은 같은 출처 세션 쿠키다(§12.1) — 토큰을 모른다.
 * 스트림이든 요청이든 401 이 나면 스트림을 멈추고(다시 두드리지 않는다) 셸 안이면 `renew_session` 을 청한다.
 * 브라우저에서는 화면이 열쇠 만료 안내를 보인다(conn.status = 'auth').
 */
export class HttpBackend implements Backend {
  readonly api: Api;
  private stream: EventStream | null = null;
  private readonly deps: HttpBackendDeps;
  private wake = () => this.stream?.retryNow();
  private onVisible = () => {
    if (document.visibilityState === 'visible') this.stream?.retryNow();
  };

  constructor(base: string, deps: HttpBackendDeps = {}) {
    this.deps = deps;
    this.api = deps.api ?? createApi(base);
  }

  start(sink: BackendSink): void {
    const api = this.api;
    this.stream = new EventStream({
      url: () => api.streamUrl(),
      onEvent: (ev) => sink.event(ev),
      onStatus: (c) => sink.status(c),
      probe: async (): Promise<ProbeResult> => {
        try {
          await api.state();
          return 'ok';
        } catch (e) {
          return isUnauthorized(e) ? 'auth' : 'down';
        }
      },
      onAuthLost: () => this.renew(),
      EventSourceImpl: this.deps.EventSourceImpl,
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

  private renew(): void {
    (this.deps.renew ?? (() => renewSession()))();
  }

  /** 요청이 401 이면 열쇠가 없다고 보고 스트림을 멈춘다. 오류는 그대로 돌려준다(화면이 알림을 띄운다). */
  private async guard<T>(p: Promise<T>): Promise<T> {
    try {
      return await p;
    } catch (e) {
      if (isUnauthorized(e) && this.stream) {
        this.stream.authLost();
        this.renew();
      }
      throw e;
    }
  }

  itemAction(id: string, action: ItemAction, opts?: { reason?: DismissReason; minutes?: number }) {
    return this.guard(this.api.itemAction(id, action, opts));
  }
  focus(on: boolean, minutes?: number) {
    return this.guard(this.api.focus(on, minutes));
  }
  listening(on: boolean) {
    return this.guard(this.api.listening(on));
  }
  say(text: string, session?: string | null) {
    return this.guard(this.api.say(text, session));
  }
  decide(decisionId: string, choice: string) {
    return this.guard(this.api.decide(decisionId, choice));
  }
  proposal(id: string, accept: boolean) {
    return this.guard(this.api.proposal(id, accept));
  }
}

function isUnauthorized(e: unknown): boolean {
  return e instanceof ApiError && e.status === 401;
}
