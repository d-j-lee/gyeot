// 얼굴을 실제로 띄워(main.tsx) 시작 순서를 본다: 주소에서 boot 지우기 → 세션 열쇠 받기 → 그 열쇠로만 요청.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const CODE = 'ef'.repeat(32);
const KEY = 'ab'.repeat(32);

type Call = { url: string; init: RequestInit; search: string };

function stubFetch(handler: (url: string) => Response) {
  const calls: Call[] = [];
  const fetchSpy = vi.fn(async (url: string, init: RequestInit = {}) => {
    calls.push({ url, init, search: location.search });
    return handler(url);
  });
  vi.stubGlobal('fetch', fetchSpy);
  return calls;
}

const sseOpen = () =>
  new Response(new ReadableStream({ start: () => {} }), { status: 200, headers: { 'Content-Type': 'text/event-stream' } });

async function boot(path: string) {
  history.replaceState(null, '', path);
  document.body.innerHTML = '<div id="app"></div>';
  vi.resetModules();
  await import('./main.tsx');
  await vi.waitFor(() => expect(document.querySelector('#app')!.children.length).toBeGreaterThan(0));
}

beforeEach(() => {
  sessionStorage.clear();
  window.requestAnimationFrame = ((cb: FrameRequestCallback) => setTimeout(() => cb(0), 0)) as unknown as typeof requestAnimationFrame;
  window.cancelAnimationFrame = ((id: number) => clearTimeout(id)) as unknown as typeof cancelAnimationFrame;
});
afterEach(() => {
  window.dispatchEvent(new Event('pagehide')); // 띄운 얼굴의 스트림을 멈춘다
  vi.unstubAllGlobals();
  sessionStorage.clear();
  history.replaceState(null, '', '/');
});

describe('얼굴 시작(§12.1-3)', () => {
  it('주소에서 boot 만 지운 뒤(다른 쿼리 유지) 그 코드를 /v1/session 으로 바꾸고, 열쇠는 sessionStorage 에만', async () => {
    const calls = stubFetch((url) =>
      url === '/v1/session'
        ? new Response(JSON.stringify({ session: KEY, expires_ms: Date.now() + 7 * 86_400_000 }))
        : sseOpen(),
    );
    await boot(`/?mode=overlay&boot=${CODE}&theme=dark`);
    expect(location.search).toBe('?mode=overlay&theme=dark');
    expect(location.href).not.toContain(CODE);
    await vi.waitFor(() => expect(calls.map((c) => c.url)).toEqual(['/v1/session', '/v1/stream']));
    // 첫 요청을 보낼 때 이미 주소에서 코드가 빠져 있었다
    expect(calls[0]!.search).toBe('?mode=overlay&theme=dark');
    expect(JSON.parse(calls[0]!.init.body as string)).toEqual({ code: CODE });
    expect(calls[1]!.init.headers).toMatchObject({ Authorization: `Bearer ${KEY}` });
    expect(JSON.parse(sessionStorage.getItem('gyeot.session')!).session).toBe(KEY);
    expect(document.cookie).toBe('');
    expect(document.documentElement.classList.contains('mode-overlay')).toBe(true);
    expect(document.documentElement.dataset.theme).toBe('dark');
  });

  it('열쇠도 boot 도 없으면 요청을 보내지 않고 안내를 보인다', async () => {
    const calls = stubFetch(() => sseOpen());
    await boot('/?mode=overlay');
    await vi.waitFor(() => expect(document.querySelector('.ov-avatar')!.getAttribute('aria-label')).toContain('열쇠가 없거나 만료됐어요'));
    (document.querySelector('.ov-avatar') as HTMLButtonElement).click();
    await vi.waitFor(() => expect(document.querySelector('.auth-notice')).not.toBeNull());
    await new Promise((r) => setTimeout(r, 50));
    expect(calls).toEqual([]);
  });

  it('데모(?demo=)는 boot 가 붙어 와도 주소에서 지우기만 하고 요청을 보내지 않는다', async () => {
    const calls = stubFetch(() => sseOpen());
    await boot(`/?demo=panel&boot=${CODE}`);
    expect(location.search).toBe('?demo=panel');
    await new Promise((r) => setTimeout(r, 50));
    expect(calls).toEqual([]);
    expect(sessionStorage.getItem('gyeot.session')).toBeNull();
  });
});
