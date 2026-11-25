import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { RENEW_MIN_INTERVAL_MS, renewSession, resetTauriMemo } from './tauri.ts';

type TauriWin = Window & { __TAURI__?: { core: { invoke: (...a: unknown[]) => Promise<unknown> } } };

beforeEach(() => {
  resetTauriMemo();
  sessionStorage.clear();
});
afterEach(() => {
  delete (window as TauriWin).__TAURI__;
  sessionStorage.clear();
});

describe('셸에 새 열쇠 청하기(§12.1-4 renew_session)', () => {
  it('셸 밖(브라우저)에서는 아무것도 부르지 않는다', () => {
    expect(renewSession(100_000)).toBe(false);
  });

  it('셸 안에서는 renew_session 을 부르고, 30초에 한 번까지만', () => {
    const invoke = vi.fn(async () => undefined);
    (window as TauriWin).__TAURI__ = { core: { invoke } };
    expect(RENEW_MIN_INTERVAL_MS).toBe(30_000);
    expect(renewSession(100_000)).toBe(true);
    expect(invoke).toHaveBeenCalledWith('renew_session');
    expect(renewSession(100_000 + 29_999)).toBe(false);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(renewSession(100_000 + 30_000)).toBe(true);
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it('셸이 창을 다시 열어도(같은 창의 sessionStorage) 30초 제한이 이어진다 — 다시 열기 고리를 막는다', () => {
    const invoke = vi.fn(async () => undefined);
    (window as TauriWin).__TAURI__ = { core: { invoke } };
    expect(renewSession(100_000)).toBe(true);
    resetTauriMemo(); // 새로 고침: 메모리는 비고 sessionStorage 만 남는다
    expect(renewSession(110_000)).toBe(false);
    expect(renewSession(130_000)).toBe(true);
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it('시계가 뒤로 갔으면 기록을 믿지 않는다(영영 막히지 않게)', () => {
    const invoke = vi.fn(async () => undefined);
    (window as TauriWin).__TAURI__ = { core: { invoke } };
    expect(renewSession(500_000)).toBe(true);
    expect(renewSession(100_000)).toBe(true);
  });

  it('셸이 명령을 거절해도 던지지 않는다', async () => {
    (window as TauriWin).__TAURI__ = { core: { invoke: vi.fn(async () => Promise.reject(new Error('not allowed'))) } };
    expect(() => renewSession(100_000)).not.toThrow();
    await new Promise((r) => setTimeout(r, 0));
  });
});
