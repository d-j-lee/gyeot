import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError, createApi, describeError, resolveApiBase, resolveToken, DEFAULT_DAEMON } from './api.ts';

function docWithMeta(content: string | null): Document {
  const doc = document.implementation.createHTMLDocument('t');
  if (content !== null) {
    const m = doc.createElement('meta');
    m.setAttribute('name', 'gyeot-token');
    m.setAttribute('content', content);
    doc.head.appendChild(m);
  }
  return doc;
}

function memStorage(): Storage {
  const m = new Map<string, string>();
  return {
    get length() {
      return m.size;
    },
    clear: () => m.clear(),
    getItem: (k) => m.get(k) ?? null,
    key: (i) => [...m.keys()][i] ?? null,
    removeItem: (k) => void m.delete(k),
    setItem: (k, v) => void m.set(k, String(v)),
  };
}

const loc = (href: string) => new URL(href) as unknown as Location;

afterEach(() => {
  history.replaceState(null, '', '/');
});

describe('토큰 찾기', () => {
  it('데몬이 채운 meta 를 먼저 쓴다', () => {
    expect(resolveToken(docWithMeta('a'.repeat(64)), loc('http://127.0.0.1:47321/?t=dev'), memStorage())).toBe('a'.repeat(64));
  });

  it('meta 가 비었으면 ?t= 를 쓰고 기억한 뒤 주소창에서 지운다', () => {
    history.replaceState(null, '', '/?mode=overlay&t=dev');
    const store = memStorage();
    expect(resolveToken(docWithMeta(''), location, store)).toBe('dev');
    expect(location.search).toBe('?mode=overlay');
    // 새로 고쳐도 같은 탭이면 기억한 토큰을 쓴다
    expect(resolveToken(docWithMeta(''), location, store)).toBe('dev');
  });

  it('아무것도 없으면 빈 문자열', () => {
    expect(resolveToken(docWithMeta(null), loc('http://x/'), memStorage())).toBe('');
  });
});

describe('API 주소', () => {
  it('같은 출처가 기본, ?api= 로 바꾸고, Tauri 묶음이면 데몬 기본 주소', () => {
    expect(resolveApiBase(loc('http://127.0.0.1:47321/'))).toBe('');
    expect(resolveApiBase(loc('http://localhost:5173/?api=http://127.0.0.1:47399/'))).toBe('http://127.0.0.1:47399');
    expect(resolveApiBase(loc('http://tauri.localhost/?mode=overlay'))).toBe(DEFAULT_DAEMON);
  });
});

describe('요청(§7)', () => {
  function api(status = 200, body: unknown = { ok: true }) {
    const fetchImpl = vi.fn(async (_url: string, _init?: RequestInit) => new Response(JSON.stringify(body), { status }));
    return { a: createApi('http://d', 'tok', fetchImpl as unknown as typeof fetch), fetchImpl };
  }

  it('모든 요청에 Bearer 토큰, 본문은 JSON', async () => {
    const { a, fetchImpl } = api();
    await a.itemAction('it_1', 'dismiss', { reason: '과민' });
    const [url, init] = fetchImpl.mock.calls[0]!;
    expect(url).toBe('http://d/v1/items/it_1/action');
    expect(init!.method).toBe('POST');
    expect((init!.headers as Record<string, string>).Authorization).toBe('Bearer tok');
    expect(JSON.parse(init!.body as string)).toEqual({ action: 'dismiss', reason: '과민' });
  });

  it('경로와 본문이 계약과 같다', async () => {
    const { a, fetchImpl } = api(200, { delivered_to: null, reason: 'r', ok: true, until_ms: null });
    await a.itemAction('it_1', 'snooze', { minutes: 1440 });
    await a.focus(true, 30);
    await a.focus(false);
    await a.listening(true);
    await a.say('안녕', 's1');
    await a.say('안녕');
    await a.decide('dc 1', '지금 적용');
    await a.proposal('pr_x', false);
    await a.state();
    const calls = fetchImpl.mock.calls.map(([u, i]) => [u.replace('http://d', ''), i!.method, i!.body ? JSON.parse(i!.body as string) : undefined]);
    expect(calls).toEqual([
      ['/v1/items/it_1/action', 'POST', { action: 'snooze', minutes: 1440 }],
      ['/v1/focus', 'POST', { on: true, minutes: 30 }],
      ['/v1/focus', 'POST', { on: false }],
      ['/v1/presence/listening', 'POST', { on: true }],
      ['/v1/say', 'POST', { text: '안녕', session: 's1' }],
      ['/v1/say', 'POST', { text: '안녕' }],
      ['/v1/decisions/dc%201', 'POST', { choice: '지금 적용' }],
      ['/v1/proposals/pr_x', 'POST', { accept: false }],
      ['/v1/state', 'GET', undefined],
    ]);
  });

  it('EventSource 주소는 ?t= 로 토큰을 싣는다', () => {
    expect(createApi('', 'a b').streamUrl()).toBe('/v1/stream?t=a%20b');
  });

  it('실패 상태는 ApiError, 문구는 한국어', async () => {
    const { a } = api(401);
    await expect(a.state()).rejects.toBeInstanceOf(ApiError);
    expect(describeError(new ApiError(401, ''))).toContain('토큰');
    expect(describeError(new ApiError(421, ''))).toContain('출처');
    expect(describeError(new TypeError('x'))).toBe('곁 데몬에 닿지 않아요');
  });
});
