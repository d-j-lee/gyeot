// 얼굴의 세션 열쇠(§12.1-3·5, G-030).
// 주소의 일회용 부트 코드(?boot=)를 곧바로 지우고 POST /v1/session 으로 세션 열쇠로 바꾼다.
// 열쇠는 이 출처(스킴·호스트·포트)의 sessionStorage 에만 두고(탭을 닫으면 사라진다), Authorization 머리로만 보낸다.
// 쿠키는 쓰지 않는다 — 쿠키는 포트를 가리지 않아 같은 호스트의 다른 포트 페이지로 샌다(R3 H1).
import { ApiError } from './api.ts';

export const SESSION_STORAGE_KEY = 'gyeot.session';

export interface StoredSession {
  session: string;
  expires_ms: number;
}

const HEX64 = /^[0-9a-f]{64}$/;

function isStoredSession(v: unknown): v is StoredSession {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return false;
  const o = v as Record<string, unknown>;
  return typeof o.session === 'string' && HEX64.test(o.session) && typeof o.expires_ms === 'number' && Number.isFinite(o.expires_ms);
}

function decodeName(raw: string): string {
  try {
    return decodeURIComponent(raw.replace(/\+/g, ' '));
  } catch {
    return raw;
  }
}

/**
 * 주소에서 boot 만 떼어 낸다(§12.1-3). 다른 쿼리(`mode=overlay` 등)·경로·조각은 글자 그대로 둔다.
 * boot 가 있으면 `history.replaceState` 로 지운 주소로 바꾸고 첫 값을 돌려준다(빈 값이면 null). 없으면 주소를 건드리지 않는다.
 */
export function takeBootCode(loc: Pick<Location, 'pathname' | 'search' | 'hash'> = location, hist: Pick<History, 'state' | 'replaceState'> = history): string | null {
  const search = loc.search.startsWith('?') ? loc.search.slice(1) : loc.search;
  if (!search) return null;
  const parts = search.split('&');
  let code: string | null = null;
  let found = false;
  const keep: string[] = [];
  for (const part of parts) {
    const eq = part.indexOf('=');
    const name = decodeName(eq === -1 ? part : part.slice(0, eq));
    if (name !== 'boot') {
      keep.push(part);
      continue;
    }
    if (!found) code = decodeName(eq === -1 ? '' : part.slice(eq + 1)) || null;
    found = true;
  }
  if (!found) return null;
  const rest = keep.filter((p) => p !== '').join('&');
  hist.replaceState(hist.state, '', `${loc.pathname}${rest ? `?${rest}` : ''}${loc.hash}`);
  return code;
}

function safeStorage(): Storage | null {
  try {
    return typeof sessionStorage === 'undefined' ? null : sessionStorage;
  } catch {
    return null;
  }
}

/** 저장된 열쇠. 만료가 지났거나 꼴이 이상하면 지우고 null. */
export function readStoredSession(storage: Storage | null = safeStorage(), now: number = Date.now()): StoredSession | null {
  if (!storage) return null;
  let raw: string | null;
  try {
    raw = storage.getItem(SESSION_STORAGE_KEY);
  } catch {
    return null;
  }
  if (raw === null) return null;
  let v: unknown;
  try {
    v = JSON.parse(raw);
  } catch {
    v = null;
  }
  if (isStoredSession(v) && v.expires_ms > now) return { session: v.session, expires_ms: v.expires_ms };
  clearStoredSession(storage);
  return null;
}

export function writeStoredSession(storage: Storage | null, s: StoredSession): void {
  try {
    storage?.setItem(SESSION_STORAGE_KEY, JSON.stringify({ session: s.session, expires_ms: s.expires_ms }));
  } catch {
    /* 저장이 막혀도 이 창의 메모리로는 쓴다 */
  }
}

export function clearStoredSession(storage: Storage | null): void {
  try {
    storage?.removeItem(SESSION_STORAGE_KEY);
  } catch {
    /* 막혀 있으면 지울 것도 없다 */
  }
}

export interface FaceAuthOptions {
  /** API 출처(같은 출처면 '') */
  base: string;
  /** 주소에서 뗀 부트 코드(없으면 null) */
  code: string | null;
  fetchImpl?: typeof fetch;
  storage?: Storage | null;
  now?: () => number;
}

/**
 * 열쇠 하나를 맡는다. 부트 코드가 있으면 처음 필요할 때 한 번만 바꾼다(동시에 여러 요청이 기다려도 한 번).
 * - `/v1/session` 401: 코드를 버리고 저장된 열쇠도 지운다(§12.1-5, 401 과 같이). 다시 보내지 않는다.
 * - 닿지 않음(네트워크·5xx 등): 코드를 쥔 채 던진다. 다음에 열쇠를 찾을 때(다시 잇기) 다시 바꾼다.
 */
export class FaceAuth {
  private code: string | null;
  private pending: Promise<string | null> | null = null;
  private mem: StoredSession | null = null;
  private readonly base: string;
  private readonly fetchImpl: typeof fetch;
  private readonly storage: Storage | null;
  private readonly now: () => number;

  constructor(opts: FaceAuthOptions) {
    this.base = opts.base;
    this.code = opts.code || null;
    this.fetchImpl = opts.fetchImpl ?? ((...a) => fetch(...a));
    this.storage = opts.storage === undefined ? safeStorage() : opts.storage;
    this.now = opts.now ?? Date.now;
  }

  /** 요청 없이 알 수 있는 것: 바꿀 부트 코드나 만료 전 열쇠가 있나. */
  available(): boolean {
    return this.code !== null || this.pending !== null || this.current() !== null;
  }

  /** 쓸 열쇠. 없으면 null(요청을 보내지 않는다). 부트 코드를 바꾸지 못했으면(닿지 않음) 던진다. */
  key(): Promise<string | null> {
    if (this.pending) return this.pending;
    if (this.code !== null) {
      const code = this.code;
      const p = this.exchange(code).finally(() => {
        if (this.pending === p) this.pending = null;
      });
      this.pending = p;
      return p;
    }
    return Promise.resolve(this.current()?.session ?? null);
  }

  /** 401 을 받은 열쇠를 지운다. 그사이 다른 열쇠로 바뀌었으면 그대로 둔다. */
  invalidate(used: string): void {
    const cur = this.current();
    if (cur && cur.session !== used) return;
    this.mem = null;
    clearStoredSession(this.storage);
  }

  private current(): StoredSession | null {
    const now = this.now();
    if (this.mem && this.mem.expires_ms > now) return this.mem;
    this.mem = null;
    const stored = readStoredSession(this.storage, now);
    if (stored) this.mem = stored;
    return stored;
  }

  private lose(): null {
    this.code = null;
    this.mem = null;
    clearStoredSession(this.storage);
    return null;
  }

  private async exchange(code: string): Promise<string | null> {
    const ctrl = new AbortController();
    const timer = setTimeout(() => ctrl.abort(), 8_000);
    let res: Response;
    try {
      res = await this.fetchImpl(`${this.base}/v1/session`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ code }),
        credentials: 'omit',
        cache: 'no-store',
        signal: ctrl.signal,
      });
    } finally {
      clearTimeout(timer);
    }
    if (res.status === 401) return this.lose();
    if (!res.ok) throw new ApiError(res.status, `POST /v1/session → ${res.status}`);
    let body: unknown;
    try {
      body = await res.json();
    } catch {
      body = null;
    }
    // 코드는 이미 쓰였다. 답이 이상하면 다시 보내 봐야 401 이므로 여기서 끝낸다.
    if (!isStoredSession(body) || body.expires_ms <= this.now()) return this.lose();
    this.code = null;
    this.mem = { session: body.session, expires_ms: body.expires_ms };
    writeStoredSession(this.storage, this.mem);
    return body.session;
  }
}
