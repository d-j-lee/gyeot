import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { HttpBackend } from './backend.ts';
import { ApiError, type Api } from '../lib/api.ts';
import { resetTauriMemo } from '../lib/tauri.ts';
import type { ConnState } from '../state/reducer.ts';

class FakeES {
  static all: FakeES[] = [];
  url: string;
  readyState = 0;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  closed = false;
  constructor(url: string) {
    this.url = url;
    FakeES.all.push(this);
  }
  addEventListener() {}
  close() {
    this.closed = true;
    this.readyState = 2;
  }
  open() {
    this.readyState = 1;
    this.onopen?.();
  }
  fail() {
    this.onerror?.();
  }
}

type TauriWin = Window & { __TAURI__?: { core: { invoke: ReturnType<typeof vi.fn> } } };

function fakeApi(over: Partial<Api> = {}): Api {
  return {
    base: '',
    state: vi.fn(async () => Promise.reject(new ApiError(401, 'GET /v1/state → 401'))),
    itemAction: vi.fn(async () => {}),
    focus: vi.fn(async () => ({ ok: true, until_ms: null })),
    listening: vi.fn(async () => {}),
    say: vi.fn(async () => ({ delivered_to: null, reason: '' })),
    decide: vi.fn(async () => ({ delivered_to: null })),
    proposal: vi.fn(async () => {}),
    streamUrl: () => '/v1/stream',
    ...over,
  };
}

let backend: HttpBackend | null = null;

function setup(api: Api = fakeApi()) {
  const statuses: ConnState[] = [];
  backend = new HttpBackend('', { api, EventSourceImpl: FakeES as unknown as typeof EventSource });
  backend.start({ event: () => {}, status: (c) => statuses.push(c) });
  return { b: backend, api, statuses };
}

function inShell() {
  const invoke = vi.fn(async (_cmd: string) => undefined);
  (window as TauriWin).__TAURI__ = { core: { invoke } };
  return invoke;
}

beforeEach(() => {
  FakeES.all = [];
  resetTauriMemo();
  sessionStorage.clear();
  vi.useFakeTimers();
});
afterEach(() => {
  backend?.stop();
  backend = null;
  delete (window as TauriWin).__TAURI__;
  sessionStorage.clear();
  vi.useRealTimers();
});

describe('열쇠가 없을 때(401, §12.1-4)', () => {
  it('스트림이 쿠키 주소로 열린다(토큰 없음)', () => {
    setup();
    expect(FakeES.all[0]!.url).toBe('/v1/stream');
  });

  it('셸 안에서 스트림이 401 이면 renew_session 을 청하고, 서버를 다시 두드리지 않는다', async () => {
    const invoke = inShell();
    const { api, statuses } = setup();
    FakeES.all[0]!.fail();
    await vi.advanceTimersByTimeAsync(0);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('renew_session');
    expect(statuses.at(-1)).toMatchObject({ status: 'auth', retryAt: null });
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(FakeES.all).toHaveLength(1);
    expect(api.state).toHaveBeenCalledTimes(1);
  });

  it('30초 안에 또 401 이어도 renew 는 한 번만, 30초가 지나면 다시 청한다', async () => {
    const invoke = inShell();
    const { b } = setup();
    FakeES.all[0]!.fail();
    await vi.advanceTimersByTimeAsync(0);
    b.retryNow();
    FakeES.all[1]!.fail();
    await vi.advanceTimersByTimeAsync(0);
    expect(invoke).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(30_000);
    b.retryNow();
    FakeES.all[2]!.fail();
    await vi.advanceTimersByTimeAsync(0);
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it('브라우저(셸 밖)에서는 renew 없이 auth 로 멈춘다', async () => {
    const { statuses } = setup();
    FakeES.all[0]!.fail();
    await vi.advanceTimersByTimeAsync(0);
    expect(statuses.at(-1)?.status).toBe('auth');
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(FakeES.all).toHaveLength(1);
  });

  it('API 요청이 401 이면 열린 스트림도 멈추고 셸에 새 열쇠를 청한다(오류는 그대로 던진다)', async () => {
    const invoke = inShell();
    const { b, statuses } = setup(fakeApi({ itemAction: vi.fn(async () => Promise.reject(new ApiError(401, 'x'))) }));
    FakeES.all[0]!.open();
    await expect(b.itemAction('it_1', 'done')).rejects.toBeInstanceOf(ApiError);
    expect(FakeES.all[0]!.closed).toBe(true);
    expect(statuses.at(-1)).toMatchObject({ status: 'auth', retryAt: null });
    expect(invoke).toHaveBeenCalledWith('renew_session');
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(FakeES.all).toHaveLength(1);
  });

  it('401 이 아닌 실패는 열쇠 문제로 보지 않는다', async () => {
    const invoke = inShell();
    const { b, statuses } = setup(fakeApi({ focus: vi.fn(async () => Promise.reject(new ApiError(500, 'x'))) }));
    FakeES.all[0]!.open();
    await expect(b.focus(true, 30)).rejects.toBeInstanceOf(ApiError);
    expect(FakeES.all[0]!.closed).toBe(false);
    expect(statuses.at(-1)?.status).toBe('open');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('데몬이 꺼진 것(401 아님)은 전처럼 간격을 늘려 다시 잇는다', async () => {
    const { statuses } = setup(fakeApi({ state: vi.fn(async () => Promise.reject(new TypeError('fetch failed'))) }));
    FakeES.all[0]!.fail();
    await vi.advanceTimersByTimeAsync(0);
    expect(statuses.at(-1)).toMatchObject({ status: 'retrying', attempt: 1 });
    await vi.advanceTimersByTimeAsync(1_000);
    expect(FakeES.all).toHaveLength(2);
  });
});
