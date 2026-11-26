import { afterEach, describe, expect, it, vi } from 'vitest';
import { faceHostAllowed, HOST_NOTICE } from './origin.ts';

const loc = (href: string) => new URL(href) as unknown as Location;

afterEach(() => vi.unstubAllEnvs());

describe('배포 얼굴은 127.0.0.1 에서만(§15.5, G-042)', () => {
  it('배포 빌드: 127.0.0.1 만 받는다', () => {
    expect(faceHostAllowed(loc('http://127.0.0.1:47321/?mode=overlay'), false)).toBe(true);
    for (const href of [
      'http://localhost:47321/',
      'http://[::1]:47321/',
      'http://127.0.0.2:47321/',
      'http://127.0.0.1.evil.example:47321/',
      'http://0.0.0.0:47321/',
      'http://tauri.localhost/',
      'http://evil.example/',
    ]) {
      expect(faceHostAllowed(loc(href), false), href).toBe(false);
    }
  });

  it('개발 빌드(import.meta.env.DEV)는 예외 — localhost 도 받는다', () => {
    vi.stubEnv('DEV', true);
    expect(faceHostAllowed(loc('http://localhost:5173/'))).toBe(true);
    vi.stubEnv('DEV', false);
    expect(faceHostAllowed(loc('http://localhost:5173/'))).toBe(false);
  });

  it('배포 빌드에서는 dev=true 를 넘겨도 예외가 없다(분기가 빌드에서 빠진다)', () => {
    vi.stubEnv('DEV', false);
    expect(faceHostAllowed(loc('http://localhost:47321/'), true)).toBe(false);
  });

  it('안내 문구', () => {
    expect(HOST_NOTICE).toBe('127.0.0.1 주소로 열어 주세요 — 트레이의 ‘곁 열기’나 gyeotd --open은 늘 그 주소로 엽니다');
  });
});
