import type { DecisionResult, DismissReason, FocusResult, ItemAction, SayResult, SseEvent } from '../types.ts';
import type { ConnState } from '../state/reducer.ts';
import { createApi, isUnauthorized, NotConnected, type Api } from '../lib/api.ts';
import { FaceAuth } from '../lib/session.ts';
import { EventStream } from '../lib/stream.ts';
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
  /** 주소에서 뗀 부트 코드(§12.1-3). 없으면 저장된 열쇠를 쓴다. */
  bootCode?: string | null;
  fetchImpl?: typeof fetch;
  auth?: FaceAuth;
  api?: Api;
  /** 401 을 받았을 때 셸에 새 열쇠를 청한다(시험에서 바꿔 끼운다). */
  renew?: () => unknown;
}

/**
 * 실제 데몬. 인증은 세션 열쇠(§12.1) — 토큰을 모르고 쿠키를 쓰지 않는다.
 * 스트림은 열 때마다 hello 로 상대가 내 세션을 아는지 확인한 뒤에만 열쇠를 싣는다(§14.1).
 * REST 요청은 그렇게 확인된 스트림이 열려 있는 동안에만 보낸다 — 끊긴 동안은 그 포트를 누가 잡았는지 모른다.
 * 열쇠가 없거나(부트 코드도 없음) 확인에서 모르는 서버이거나 스트림·요청이 401 이면 스트림을 멈추고(다시 두드리지 않는다)
 * 셸 안이면 `renew_session` 을 청한다. 브라우저에서는 화면이 열쇠 안내를 보인다(conn.status = 'auth').
 */
export class HttpBackend implements Backend {
  readonly api: Api;
  readonly auth: FaceAuth;
  private stream: EventStream | null = null;
  /** 스트림이 hello 로 확인되고 열려 있다(그때만 REST 를 보낸다). */
  private connected = false;
  private readonly deps: HttpBackendDeps;
  private wake = () => this.retryNow();
  private onVisible = () => {
    if (document.visibilityState === 'visible') this.retryNow();
  };

  constructor(base: string, deps: HttpBackendDeps = {}) {
    this.deps = deps;
    this.auth = deps.auth ?? new FaceAuth({ base, code: deps.bootCode ?? null, fetchImpl: deps.fetchImpl });
    this.api = deps.api ?? createApi(base, { auth: this.auth, fetchImpl: deps.fetchImpl });
  }

  start(sink: BackendSink): void {
    const api = this.api;
    this.stream = new EventStream({
      open: (signal) => api.openStream(signal),
      onEvent: (ev) => sink.event(ev),
      onStatus: (c) => {
        // 'open' 은 hello 확인과 스트림 머리를 모두 지난 뒤에만 온다.
        this.connected = c.status === 'open';
        sink.status(c);
      },
      onAuthLost: () => this.renew(),
    });
    // 열쇠도 부트 코드도 없으면 요청을 하나도 보내지 않고 처음부터 안내한다(§12.1-5).
    // (스트림은 열쇠가 없으면 보내지 않고 401 로 끝나므로, 여기서는 연결을 시작만 한다.)
    this.stream.start();
    window.addEventListener('online', this.wake);
    document.addEventListener('visibilitychange', this.onVisible);
  }

  stop(): void {
    this.connected = false;
    this.stream?.stop();
    this.stream = null;
    window.removeEventListener('online', this.wake);
    document.removeEventListener('visibilitychange', this.onVisible);
  }

  /**
   * 지금 다시. 열쇠가 없으면 다시 잇지 않는다 — 요청 없이 안내를 그대로 두고(깜빡이지 않게) 셸에만 다시 청한다(30초 제한).
   */
  retryNow(): void {
    if (!this.stream) return;
    if (!this.auth.available()) {
      this.lost();
      return;
    }
    this.stream.retryNow();
  }

  private lost(): void {
    this.stream?.authLost();
    this.renew();
  }

  private renew(): void {
    (this.deps.renew ?? (() => renewSession()))();
  }

  /**
   * 확인된 스트림이 열려 있을 때만 보낸다(아니면 요청 없이 NotConnected).
   * 요청이 401 이면 열쇠가 없다고 보고 스트림을 멈춘다(열쇠는 api 가 이미 지웠다). 오류는 그대로 돌려준다(화면이 알림을 띄운다).
   */
  private async guard<T>(send: () => Promise<T>): Promise<T> {
    if (!this.connected) throw new NotConnected();
    try {
      return await send();
    } catch (e) {
      if (isUnauthorized(e) && this.stream) this.lost();
      throw e;
    }
  }

  itemAction(id: string, action: ItemAction, opts?: { reason?: DismissReason; minutes?: number }) {
    return this.guard(() => this.api.itemAction(id, action, opts));
  }
  focus(on: boolean, minutes?: number) {
    return this.guard(() => this.api.focus(on, minutes));
  }
  listening(on: boolean) {
    return this.guard(() => this.api.listening(on));
  }
  say(text: string, session?: string | null) {
    return this.guard(() => this.api.say(text, session));
  }
  decide(decisionId: string, choice: string) {
    return this.guard(() => this.api.decide(decisionId, choice));
  }
  proposal(id: string, accept: boolean) {
    return this.guard(() => this.api.proposal(id, accept));
  }
}
