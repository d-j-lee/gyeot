import { defineConfig } from 'vitest/config';

// 곁 데몬(또는 mock)의 주소. dev 서버는 /v1·/health 를 여기로 넘긴다(같은 출처로 보이게).
// 부트 주소(http://127.0.0.1:5173/?boot=<코드>)는 넘기지 않는다 — 개발 서버의 얼굴이 주소에서 boot 를 지우고
// POST /v1/session(이 프록시)으로 세션 열쇠를 받아 그 탭의 sessionStorage 에 둔다(§12.1-3). 쿠키는 쓰지 않는다.
const daemon = process.env.GYEOT_URL ?? 'http://127.0.0.1:47321';

export default defineConfig({
  base: '/',
  oxc: {
    jsx: { runtime: 'automatic', importSource: 'preact' },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'es2022',
    sourcemap: false,
  },
  server: {
    host: '127.0.0.1',
    port: 5173,
    proxy: {
      '/v1': { target: daemon, changeOrigin: true },
      '/health': { target: daemon, changeOrigin: true },
    },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.{ts,tsx}', 'mock/**/*.test.ts'],
    env: { TZ: 'Asia/Seoul' },
    restoreMocks: true,
  },
});
