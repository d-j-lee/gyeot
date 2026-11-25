import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/preact';
import { App } from './App.tsx';
import { Controller } from './app/controller.ts';
import { DemoBackend } from './demo/backend.ts';
import { DEFAULT_PREFS } from './lib/prefs.ts';
import { demoInterrupt, demoReply, demoSnapshot } from './demo/data.ts';
import { resetTauriMemo } from './lib/tauri.ts';

type TauriWin = Window & { __TAURI__?: { core: { invoke: ReturnType<typeof vi.fn> } } };

function mount(mode: 'overlay' | 'default', open = false) {
  const backend = new DemoBackend({ scene: 'mood', mood: 'waiting', latency: 0 });
  const ctl = new Controller(backend, { prefs: { ...DEFAULT_PREFS, sound: false }, persistPrefs: false, chime: () => {} }, 'demo');
  const utils = render(<App ctl={ctl} config={{ mode, initialTab: 'turn', initialOpen: open }} />);
  act(() => ctl.start());
  return { ctl, backend, ...utils };
}

beforeEach(() => {
  resetTauriMemo();
  // jsdom 에는 없는 것들
  window.requestAnimationFrame = ((cb: FrameRequestCallback) => setTimeout(() => cb(0), 0)) as unknown as typeof requestAnimationFrame;
  window.cancelAnimationFrame = ((id: number) => clearTimeout(id)) as unknown as typeof cancelAnimationFrame;
  Element.prototype.scrollIntoView = () => {};
});
afterEach(() => {
  cleanup();
  delete (window as TauriWin).__TAURI__;
});

describe('오버레이', () => {
  it('아바타를 누르면 패널이 펼쳐지고 Esc 로 접힌다', async () => {
    mount('overlay');
    const avatar = screen.getByRole('button', { name: /곁 — .*패널 열기/ });
    expect(avatar.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByRole('tablist')).toBeNull();
    fireEvent.click(avatar);
    expect(screen.getByRole('tablist')).toBeTruthy();
    expect(document.querySelector('.ov-avatar')!.getAttribute('aria-expanded')).toBe('true');
    expect(document.querySelector('.ov-avatar')!.getAttribute('aria-label')).toMatch(/패널 접기/);
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByRole('tablist')).toBeNull();
  });

  it('바깥을 누르면 접히고, 패널 안을 누르면 그대로', () => {
    mount('overlay', true);
    fireEvent.pointerDown(screen.getByRole('tablist'));
    expect(screen.queryByRole('tablist')).not.toBeNull();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole('tablist')).toBeNull();
  });

  it('말풍선의 "보기"는 패널을 펼치고 그 항목을 찾아 준다', () => {
    const { ctl } = mount('overlay');
    act(() => ctl.onEvent({ event: 'deliver', data: demoInterrupt(Date.now()) }));
    expect(screen.getByText('agent-ops-kit에서 권한 승인을 4분째 기다려요')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: '보기' }));
    expect(screen.getByRole('tablist')).toBeTruthy();
    expect(document.querySelector('.card--highlight')?.getAttribute('data-kind')).toBe('approval');
  });

  it('Tauri 안이면 펼침 상태와 클릭 영역(아바타+패널을 감싼 사각형)을 셸에 알린다', async () => {
    // jsdom 은 배치를 하지 않으므로 오버레이 창(440×780)의 자리를 흉내 낸다.
    const boxes: Record<string, [number, number, number, number]> = {
      'ov-avatar': [312, 656, 424, 768],
      'ov-panel': [24, 50, 424, 650],
    };
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
      const hit = Object.entries(boxes).find(([c]) => this.classList.contains(c));
      const [l, t, r, b] = hit ? hit[1] : [0, 0, 0, 0];
      return { left: l, top: t, right: r, bottom: b, width: r - l, height: b - t, x: l, y: t, toJSON: () => ({}) } as DOMRect;
    });
    const invoke = vi.fn(async (_cmd: string, _args?: unknown) => undefined);
    (window as TauriWin).__TAURI__ = { core: { invoke } };
    mount('overlay');
    await act(async () => {
      await new Promise((r) => setTimeout(r, 5));
    });
    expect(invoke).toHaveBeenCalledWith('set_expanded', { expanded: false });
    fireEvent.click(screen.getByRole('button', { name: /패널 열기/ }));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 5));
    });
    expect(invoke).toHaveBeenCalledWith('set_expanded', { expanded: true });
    const rects = invoke.mock.calls.filter(([cmd]) => cmd === 'set_hit_rect').map(([, a]) => a);
    expect(rects[0]).toEqual({ x: 308, y: 652, width: 120, height: 120 }); // 접힘: 아바타만(+4px)
    expect(rects.at(-1)).toEqual({ x: 20, y: 46, width: 408, height: 726 }); // 펼침: 패널까지
  });

  it('Tauri 가 없으면 아무것도 부르지 않는다(브라우저)', () => {
    expect(() => mount('overlay')).not.toThrow();
  });

  it('셸이 명령을 거절해도 화면은 멀쩡하다', async () => {
    (window as TauriWin).__TAURI__ = { core: { invoke: vi.fn(async () => Promise.reject(new Error('unknown command'))) } };
    mount('overlay');
    fireEvent.click(screen.getByRole('button', { name: /패널 열기/ }));
    await act(async () => {
      await new Promise((r) => setTimeout(r, 5));
    });
    expect(screen.getByRole('tablist')).toBeTruthy();
  });
});

describe('집중 모드의 답(§12.4)', () => {
  const quiet = (ctl: Controller) =>
    act(() => ctl.onEvent({ event: 'presence', data: { ...demoSnapshot(Date.now()).presence, mood: 'waiting', focus_mode_until_ms: Date.now() + 30 * 60_000 } }));
  const LINE = 'vitest 설치 승인이 나면 바로 시험을 돌릴게요.';

  it('오버레이: 말풍선 없이 아바타와 대화 탭에 배지, 대화 탭을 열면 지운다', () => {
    const { ctl } = mount('overlay');
    quiet(ctl);
    act(() => ctl.onEvent({ event: 'reply', data: demoReply(Date.now()) }));
    expect(screen.queryByText(LINE)).toBeNull();
    const avatar = document.querySelector('.ov-avatar')!;
    expect(avatar.querySelector('.ov-unread')).not.toBeNull();
    expect(avatar.getAttribute('aria-label')).toContain('안 읽은 답 1개');
    fireEvent.click(avatar);
    const chatTab = screen.getByRole('tab', { name: /대화/ });
    expect(chatTab.querySelector('.tab__count--unread')?.textContent).toBe('1');
    expect(document.querySelector('.ov-unread')).toBeNull(); // 펼치면 아바타 점은 거둔다(탭에 있다)
    fireEvent.click(chatTab);
    expect(chatTab.querySelector('.tab__count--unread')).toBeNull();
    expect(ctl.state.get().unreadReplies).toBe(0);
    expect(screen.getByText(/vitest 설치 승인이 나면/)).toBeTruthy(); // 기록에는 있다
  });

  it('기본 화면: 대화 탭에 배지, 집중이 아니면 전처럼 말풍선', () => {
    const { ctl } = mount('default');
    quiet(ctl);
    act(() => ctl.onEvent({ event: 'reply', data: demoReply(Date.now()) }));
    expect(document.querySelector('.bubble')).toBeNull();
    expect(screen.getByRole('tab', { name: /대화/ }).querySelector('.tab__count--unread')?.textContent).toBe('1');
    act(() => ctl.onEvent({ event: 'presence', data: { ...demoSnapshot(Date.now()).presence, mood: 'idle', focus_mode_until_ms: null } }));
    act(() => ctl.onEvent({ event: 'reply', data: { ...demoReply(Date.now()), chat_id: 'm99', text: '두 번째 답' } }));
    expect(screen.getAllByText('두 번째 답').length).toBeGreaterThan(0);
    expect(document.querySelector('.bubble')).not.toBeNull();
  });

  it('대화 탭을 보고 있을 때 온 답은 배지를 남기지 않는다', () => {
    const { ctl } = mount('default');
    fireEvent.click(screen.getByRole('tab', { name: /대화/ }));
    quiet(ctl);
    act(() => ctl.onEvent({ event: 'reply', data: demoReply(Date.now()) }));
    expect(document.querySelector('.tab__count--unread')).toBeNull();
  });
});

const AUTH = { status: 'auth', attempt: 1, retryAt: null } as const;
const AUTH_TEXT = '열쇠가 없거나 만료됐어요 — 트레이의 ‘곁 열기’를 누르거나 터미널에서 gyeotd --open을 실행하세요.';

describe('열쇠가 없거나 만료됐을 때(§12.1-5)', () => {
  it('기본 화면: 안내를 보이고, 명령은 코드로, 다시 확인은 다시 잇는다', () => {
    const { ctl, backend } = mount('default');
    const retry = vi.spyOn(backend, 'retryNow');
    expect(screen.queryByText(/열쇠가 없거나 만료됐어요/)).toBeNull();
    act(() => ctl.dispatch({ type: 'conn', conn: AUTH }));
    const notice = document.querySelector('.auth-notice') as HTMLElement;
    expect(notice).not.toBeNull();
    expect(notice.getAttribute('role')).toBe('alert');
    expect(notice.querySelector('.auth-notice__text')!.textContent).toBe(AUTH_TEXT);
    expect(notice.querySelector('code')!.textContent).toBe('gyeotd --open');
    fireEvent.click(within(notice).getByRole('button', { name: '다시 확인' }));
    expect(retry).toHaveBeenCalledTimes(1);
    act(() => ctl.dispatch({ type: 'conn', conn: { status: 'open', attempt: 0, retryAt: null } }));
    expect(document.querySelector('.auth-notice')).toBeNull();
  });

  it('오버레이: 접혀 있어도 아바타가 오프라인으로 보이고 이름에 알린다, 펼치면 안내', () => {
    const { ctl } = mount('overlay');
    act(() => ctl.dispatch({ type: 'conn', conn: AUTH }));
    const avatar = document.querySelector('.ov-avatar')!;
    expect(avatar.classList.contains('ov-avatar--offline')).toBe(true);
    expect(avatar.getAttribute('aria-label')).toContain('열쇠가 없거나 만료됐어요');
    fireEvent.click(avatar);
    expect(document.querySelector('.auth-notice .auth-notice__text')!.textContent).toBe(AUTH_TEXT);
  });
});

describe('기본 화면', () => {
  it('패널이 늘 보이고 세션 목록이 있다', () => {
    mount('default');
    expect(screen.getByRole('tablist')).toBeTruthy();
    expect(screen.getByRole('heading', { name: /세션/ })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /패널 접기/ })).toBeNull();
  });

  it('탭은 화살표 키로 옮긴다', () => {
    mount('default');
    const turn = screen.getByRole('tab', { name: /당신 차례/ });
    fireEvent.keyDown(turn, { key: 'ArrowRight' });
    expect(screen.getByRole('tab', { name: '대화' }).getAttribute('aria-selected')).toBe('true');
    fireEvent.keyDown(screen.getByRole('tab', { name: '대화' }), { key: 'End' });
    expect(screen.getByRole('tab', { name: '설정' }).getAttribute('aria-selected')).toBe('true');
  });

  it('세션을 누르면 그 세션으로 대화 탭을 연다', () => {
    mount('default');
    fireEvent.click(screen.getByRole('button', { name: /work-atlas/ }));
    expect(screen.getByRole('tab', { name: '대화' }).getAttribute('aria-selected')).toBe('true');
    expect((screen.getByLabelText('보낼 곳') as HTMLSelectElement).value).toBe('e2a6c930-wat');
    expect(screen.getByText('연결 안 됨')).toBeTruthy();
  });

  it('승인 카드 어디에도 승인 단추가 없다(앱 전체로 다시 확인)', () => {
    mount('default');
    const approval = document.querySelector('article[data-kind="approval"]')!;
    const labels = [...approval.querySelectorAll('button')].map((b) => `${b.textContent} ${b.getAttribute('aria-label') ?? ''}`);
    expect(labels.join('|')).not.toMatch(/승인|approve|allow/i);
    expect(approval.textContent).toContain('승인은 터미널에서 직접 하세요');
  });
});
