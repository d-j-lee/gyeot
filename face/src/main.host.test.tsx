// @vitest-environment-options {"url": "http://localhost:47321/"}
// §15.5: 배포 빌드의 얼굴을 localhost 출처로 띄우면 아무 요청도 보내지 않고 127.0.0.1 안내를 보인다.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const CODE = 'ef'.repeat(32);
const HOST_TEXT = '127.0.0.1 주소로 열어 주세요 — 트레이의 ‘곁 열기’나 gyeotd --open은 늘 그 주소로 엽니다';

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
  window.dispatchEvent(new Event('pagehide'));
  vi.unstubAllGlobals();
  vi.unstubAllEnvs();
  delete (window as Window & { __TAURI__?: unknown }).__TAURI__;
  sessionStorage.clear();
  history.replaceState(null, '', '/');
});

describe('localhost 로 연 배포 얼굴(§15.5, G-042)', () => {
  it('boot 는 주소에서 지우되 교환하지 않고, 어떤 요청도 IPC 도 없이 안내만', async () => {
    vi.stubEnv('DEV', false);
    sessionStorage.setItem('gyeot.session', JSON.stringify({ session: 'ab'.repeat(32), expires_ms: Date.now() + 86_400_000 }));
    const fetchSpy = vi.fn();
    vi.stubGlobal('fetch', fetchSpy);
    const invoke = vi.fn(async (_cmd: string, _args?: unknown) => undefined);
    (window as Window & { __TAURI__?: unknown }).__TAURI__ = { core: { invoke } };
    await boot(`/?mode=overlay&boot=${CODE}`);
    expect(location.search).toBe('?mode=overlay');
    const avatar = document.querySelector('.ov-avatar')!;
    await vi.waitFor(() => expect(avatar.getAttribute('aria-label')).toContain('127.0.0.1 주소로 열어 주세요'));
    (avatar as HTMLButtonElement).click();
    await vi.waitFor(() => expect(document.querySelector('.host-notice')).not.toBeNull());
    const notice = document.querySelector('.host-notice')!;
    expect(notice.getAttribute('role')).toBe('alert');
    expect(notice.querySelector('.host-notice__text')!.textContent).toBe(HOST_TEXT);
    expect(notice.querySelector('code')!.textContent).toBe('gyeotd --open');
    expect(document.querySelector('.auth-notice')).toBeNull();
    expect(document.querySelector('.status__conn')!.textContent).toContain('127.0.0.1 주소로 열어 주세요');
    expect(document.querySelector('.status__conn .spinner')).toBeNull();
    await new Promise((r) => setTimeout(r, 50));
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(invoke.mock.calls.map(([cmd]) => cmd)).not.toContain('renew_session');
    expect(JSON.parse(sessionStorage.getItem('gyeot.session')!).session).toBe('ab'.repeat(32)); // 지우지도 않는다(다른 출처의 일)
  });

  it('개발 빌드는 예외 — localhost 에서도 데몬에 붙으려 한다', async () => {
    vi.stubEnv('DEV', true);
    const fetchSpy = vi.fn(async () => new Response('{}', { status: 503 }));
    vi.stubGlobal('fetch', fetchSpy);
    await boot(`/?boot=${CODE}`);
    await vi.waitFor(() => expect(fetchSpy).toHaveBeenCalled());
    expect(document.querySelector('.host-notice')).toBeNull();
  });

  it('데모는 localhost 에서도 그대로(요청 없음)', async () => {
    vi.stubEnv('DEV', false);
    const fetchSpy = vi.fn();
    vi.stubGlobal('fetch', fetchSpy);
    await boot('/?demo=panel');
    await new Promise((r) => setTimeout(r, 50));
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(document.querySelector('.host-notice')).toBeNull();
    expect(document.querySelectorAll('article.card').length).toBeGreaterThan(0);
  });
});
