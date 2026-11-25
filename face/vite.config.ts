import { defineConfig } from 'vitest/config';

// 곁 데몬(또는 mock)의 주소. dev 서버는 /v1 과 /health 를 여기로 넘긴다.
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
    include: ['src/**/*.test.{ts,tsx}'],
    env: { TZ: 'Asia/Seoul' },
    restoreMocks: true,
  },
});
