import type {
  DecisionResult,
  DismissReason,
  FocusResult,
  ItemAction,
  SayResult,
  Snapshot,
} from '../types.ts';

const DEV_TOKEN_KEY = 'gyeot-dev-token';
export const DEFAULT_DAEMON = 'http://127.0.0.1:47321';

/**
 * 토큰 찾기: ① 데몬이 채운 <meta name="gyeot-token"> ② 개발용 ?t= (읽은 뒤 주소창에서 지운다)
 * ③ 같은 탭에서 앞서 받은 개발용 토큰(sessionStorage).
 */
export function resolveToken(doc: Document = document, loc: Location = location, store?: Storage | null): string {
  const storage = store === undefined ? safeSession() : store;
  const meta = doc.querySelector('meta[name="gyeot-token"]')?.getAttribute('content')?.trim() ?? '';
  if (meta) return meta;
  const url = new URL(loc.href);
  const t = url.searchParams.get('t')?.trim() ?? '';
  if (t) {
    try {
      storage?.setItem(DEV_TOKEN_KEY, t);
      url.searchParams.delete('t');
      history.replaceState(history.state, '', url.pathname + url.search + url.hash);
    } catch {
      /* 저장이 막혀도 이번 화면에서는 쓴다 */
    }
    return t;
  }
  return storage?.getItem(DEV_TOKEN_KEY)?.trim() ?? '';
}

function safeSession(): Storage | null {
  try {
    return typeof sessionStorage === 'undefined' ? null : sessionStorage;
  } catch {
    return null;
  }
}

/**
 * API 주소: ?api= 가 있으면 그것, Tauri 묶음(tauri://, tauri.localhost)에서 열렸으면 데몬 기본 주소,
 * 그 밖에는 같은 출처(데몬이 얼굴을 직접 내보낸다).
 */
export function resolveApiBase(loc: Location = location): string {
  const url = new URL(loc.href);
  const api = url.searchParams.get('api');
  if (api) return api.replace(/\/+$/, '');
  if (url.protocol === 'tauri:' || url.hostname === 'tauri.localhost') return DEFAULT_DAEMON;
  return '';
}

export class ApiError extends Error {
  readonly status: number;
  constructor(status: number, message: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

/** 사람에게 보일 오류 문구. */
export function describeError(e: unknown): string {
  if (e instanceof ApiError) {
    if (e.status === 401) return '토큰이 맞지 않아요. 곁을 다시 열어 주세요';
    if (e.status === 403 || e.status === 421) return '이 창의 출처를 데몬이 막았어요';
    if (e.status === 404) return '데몬이 이 요청을 모른대요(버전이 다를 수 있어요)';
    if (e.status >= 500) return '데몬 안에서 오류가 났어요';
    return `요청이 거절됐어요(${e.status})`;
  }
  if (e instanceof DOMException && e.name === 'AbortError') return '응답이 너무 늦어요';
  return '곁 데몬에 닿지 않아요';
}

export interface Api {
  readonly base: string;
  readonly token: string;
  state(): Promise<Snapshot>;
  itemAction(id: string, action: ItemAction, opts?: { reason?: DismissReason; minutes?: number }): Promise<void>;
  focus(on: boolean, minutes?: number): Promise<FocusResult>;
  listening(on: boolean): Promise<void>;
  say(text: string, session?: string | null): Promise<SayResult>;
  decide(decisionId: string, choice: string): Promise<DecisionResult>;
  proposal(id: string, accept: boolean): Promise<void>;
  streamUrl(): string;
}

export function createApi(base: string, token: string, fetchImpl: typeof fetch = (...a) => fetch(...a)): Api {
  async function call<T>(method: 'GET' | 'POST', path: string, body?: unknown): Promise<T> {
    const ctrl = new AbortController();
    const timer = setTimeout(() => ctrl.abort(), 8_000);
    try {
      const res = await fetchImpl(base + path, {
        method,
        headers: {
          Authorization: `Bearer ${token}`,
          ...(body !== undefined ? { 'Content-Type': 'application/json' } : {}),
        },
        body: body !== undefined ? JSON.stringify(body) : undefined,
        signal: ctrl.signal,
        cache: 'no-store',
      });
      if (!res.ok) throw new ApiError(res.status, `${method} ${path} → ${res.status}`);
      const text = await res.text();
      return (text ? JSON.parse(text) : {}) as T;
    } finally {
      clearTimeout(timer);
    }
  }
  const enc = encodeURIComponent;
  return {
    base,
    token,
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
    // EventSource 는 머리글을 못 붙여서 계약대로 ?t= 로 넘긴다.
    streamUrl: () => `${base}/v1/stream?t=${enc(token)}`,
  };
}
