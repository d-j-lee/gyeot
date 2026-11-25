import { defineConfig } from 'vitest/config';

// 곁 데몬(또는 mock)의 주소. dev 서버는 /v1·/health 와 부트 주소(?boot=)를 여기로 넘긴다.
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
      // §12.1: 부트 코드는 데몬이 세션 쿠키로 바꾸고 303 으로 boot 만 뺀 주소(이 개발 서버)로 돌려보낸다.
      // 쿠키는 포트를 가리지 않으므로 그 뒤 /v1 프록시 요청에도 실린다. ^ 로 시작하면 vite 가 경로+쿼리에 정규식으로 맞춘다.
      '^/[^?#]*\\?(?:[^#]*&)?boot=': { target: daemon, changeOrigin: true },
    },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.{ts,tsx}', 'mock/**/*.test.ts'],
    env: { TZ: 'Asia/Seoul' },
    restoreMocks: true,
  },
});
