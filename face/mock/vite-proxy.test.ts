// @vitest-environment node
// 개발 서버(npm run dev)의 프록시: API 만 데몬으로 넘긴다. 부트 주소(?boot=)는 개발 서버의 얼굴이 직접 읽는다(§12.1-3).
import { describe, expect, it } from 'vitest';
import config from '../vite.config.ts';

describe('vite 개발 서버 프록시', () => {
  const proxy = (config.server?.proxy ?? {}) as Record<string, { target: string; changeOrigin?: boolean }>;

  it('/v1 과 /health 만 데몬으로 넘긴다(부트 주소·쿠키 길은 없다)', () => {
    expect(Object.keys(proxy).sort()).toEqual(['/health', '/v1']);
    for (const p of Object.values(proxy)) {
      expect(p.target).toMatch(/^http:\/\/(127\.0\.0\.1|localhost)(:\d+)?$/);
      expect(p.changeOrigin).toBe(true); // Host 를 데몬 주소로(§7 Host 검사)
    }
  });

  it('개발 서버는 127.0.0.1 에만 묶인다', () => {
    expect(config.server?.host).toBe('127.0.0.1');
  });
});
