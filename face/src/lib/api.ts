import type {
  DecisionResult,
  DismissReason,
  FocusResult,
  ItemAction,
  SayResult,
  Snapshot,
} from '../types.ts';
import type { FaceAuth } from './session.ts';

const LOOPBACK_HOSTS = new Set(['127.0.0.1', 'localhost', '[::1]']);

/**
 * API 주소(§12.1-6). 배포 빌드는 늘 같은 출처('')다 — 데몬이 얼굴을 직접 내보내고, 셸도 데몬 주소를 연다(§12.2).
 * 개발 빌드(`import.meta.env.DEV`)에서만 `?api=` 를 받되, 루프백 http 주소일 때만 그 출처를 쓴다.
 * 데몬은 CORS 를 내지 않으므로 다른 출처를 가리키면 요청이 막힌다. 개발 서버에서는 프록시(같은 출처)를 쓴다.
 */
export function resolveApiBase(loc: Pick<Location, 'href'> = location, dev: boolean = import.meta.env.DEV): string {
  // 배포 빌드에서는 import.meta.env.DEV 가 false 로 박혀 아래가 통째로 빠진다(dev 를 넘겨도 못 켠다).
  if (!import.meta.env.DEV || !dev) return '';
  const raw = new URL(loc.href).searchParams.get('api');
  if (!raw) return '';
  let api: URL;
  try {
    api = new URL(raw);
  } catch {
    return '';
  }
  if (api.protocol !== 'http:' || api.username || api.password || !LOOPBACK_HOSTS.has(api.hostname)) return '';
  return api.origin;
}

export class ApiError extends Error {
  readonly status: number;
  constructor(status: number, message: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

export function isUnauthorized(e: unknown): boolean {
  return e instanceof ApiError && e.status === 401;
}

/** 사람에게 보일 오류 문구. */
export function describeError(e: unknown): string {
  if (e instanceof ApiError) {
    if (e.status === 401) return '열쇠가 없거나 만료됐어요';
    if (e.status === 403 || e.status === 421) return '이 창의 출처를 데몬이 막았어요';
    if (e.status === 404) return '데몬이 이 요청을 모른대요(버전이 다를 수 있어요)';
    if (e.status >= 500) return '데몬 안에서 오류가 났어요';
    return `요청이 거절됐어요(${e.status})`;
  }
  if (e instanceof DOMException && e.name === 'AbortError') return '응답이 너무 늦어요';
  return '곁 데몬에 닿지 않아요';
}

/**
 * 데몬 API(§7). 인증은 세션 열쇠 하나를 `Authorization: Bearer` 머리로 싣는 것뿐이다(§12.1-3).
 * 얼굴은 토큰을 모르고, 주소·쿠키에 비밀을 두지 않는다(쿠키는 싣지도 받지도 않는다: credentials 'omit').
 * 열쇠가 없으면 요청을 보내지 않고 401 로 다룬다. 401 을 받으면 그 열쇠를 지운다(§12.1-5).
 */
export interface Api {
  readonly base: string;
  state(): Promise<Snapshot>;
  itemAction(id: string, action: ItemAction, opts?: { reason?: DismissReason; minutes?: number }): Promise<void>;
  focus(on: boolean, minutes?: number): Promise<FocusResult>;
  listening(on: boolean): Promise<void>;
  say(text: string, session?: string | null): Promise<SayResult>;
  decide(decisionId: string, choice: string): Promise<DecisionResult>;
  proposal(id: string, accept: boolean): Promise<void>;
  /** GET /v1/stream(§8). 머리가 오면(200 text/event-stream) 응답을, 아니면 ApiError 를 던진다. 몸은 부른 쪽이 읽는다. */
  openStream(signal: AbortSignal): Promise<Response>;
}

export interface ApiDeps {
  auth: FaceAuth;
  fetchImpl?: typeof fetch;
}

/** 요청 머리가 늦을 때 끊는 시간. 스트림은 머리가 온 뒤로는 끊지 않는다(ping 으로 살아 있음을 본다). */
export const REQUEST_TIMEOUT_MS = 8_000;

export function createApi(base: string, deps: ApiDeps): Api {
  const { auth } = deps;
  const fetchImpl = deps.fetchImpl ?? ((...a) => fetch(...a));

  /** 열쇠를 실어 보낸다. 열쇠가 없으면 보내지 않고 401. 401 이면 그 열쇠를 지운다. */
  async function send(method: 'GET' | 'POST', path: string, init: { body?: unknown; accept?: string; signal?: AbortSignal }): Promise<Response> {
    const key = await auth.key();
    if (!key) throw new ApiError(401, `${method} ${path} → 열쇠 없음`);
    const headers: Record<string, string> = { Authorization: `Bearer ${key}` };
    if (init.body !== undefined) headers['Content-Type'] = 'application/json';
    if (init.accept) headers.Accept = init.accept;
    const res = await fetchImpl(base + path, {
      method,
      headers,
      body: init.body !== undefined ? JSON.stringify(init.body) : undefined,
      credentials: 'omit',
      cache: 'no-store',
      signal: init.signal,
    });
    if (res.status === 401) {
      auth.invalidate(key);
      void res.body?.cancel().catch(() => {});
    }
    if (!res.ok) throw new ApiError(res.status, `${method} ${path} → ${res.status}`);
    return res;
  }

  async function call<T>(method: 'GET' | 'POST', path: string, body?: unknown): Promise<T> {
    const ctrl = new AbortController();
    const timer = setTimeout(() => ctrl.abort(), REQUEST_TIMEOUT_MS);
    try {
      const res = await send(method, path, { body, signal: ctrl.signal });
      const text = await res.text();
      return (text ? JSON.parse(text) : {}) as T;
    } finally {
      clearTimeout(timer);
    }
  }

  const enc = encodeURIComponent;
  return {
    base,
    state: () => call<Snapshot>('GET', '/v1/state'),
    async itemAction(id, action, opts = {}) {
      const body: Record<string, unknown> = { action };
      if (opts.reason !== undefined) body.reason = opts.reason;
      if (opts.minutes !== undefined) body.minutes = opts.minutes;
      await call('POST', `/v1/items/${enc(id)}/action`, body);
    },
    focus: (on, minutes) => call<FocusResult>('POST', '/v1/focus', minutes === undefined ? { on } : { on, minutes }),
    async listening(on) {
      await call('POST', '/v1/presence/listening', { on });
    },
    say: (text, session) => call<SayResult>('POST', '/v1/say', session ? { text, session } : { text }),
    decide: (decisionId, choice) => call<DecisionResult>('POST', `/v1/decisions/${enc(decisionId)}`, { choice }),
    async proposal(id, accept) {
      await call('POST', `/v1/proposals/${enc(id)}`, { accept });
    },
    async openStream(signal) {
      // 머리가 오기까지만 시간 제한을 둔다. 부른 쪽이 끊으면(signal) 같이 끊는다.
      const ctrl = new AbortController();
      const onAbort = () => ctrl.abort();
      if (signal.aborted) ctrl.abort();
      else signal.addEventListener('abort', onAbort, { once: true });
      const timer = setTimeout(() => ctrl.abort(), REQUEST_TIMEOUT_MS);
      try {
        const res = await send('GET', '/v1/stream', { accept: 'text/event-stream', signal: ctrl.signal });
        const type = res.headers.get('content-type') ?? '';
        if (!/^text\/event-stream\b/i.test(type)) {
          void res.body?.cancel().catch(() => {});
          throw new ApiError(502, `GET /v1/stream → ${type || '형식 없음'}`);
        }
        return res;
      } finally {
        clearTimeout(timer);
      }
    },
  };
}
