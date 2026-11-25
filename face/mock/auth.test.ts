// @vitest-environment node
import { describe, expect, it } from 'vitest';
import {
  AuthStore,
  BOOT_TTL_MS,
  MAX_BOOT_CODES,
  MAX_SESSIONS,
  SESSION_COOKIE,
  SESSION_MAX_AGE_S,
} from './auth.ts';

const TOKEN = 'ab'.repeat(32);

function store(start = 1_000_000) {
  let now = start;
  const s = new AuthStore(TOKEN, () => now);
  return { s, tick: (ms: number) => (now += ms), at: () => now };
}

const cookieOf = (value: string) => `${SESSION_COOKIE}=${value}`;

describe('가짜 데몬의 열쇠 보관함(§12.1)', () => {
  it('계약의 값', () => {
    expect(BOOT_TTL_MS).toBe(120_000);
    expect(MAX_BOOT_CODES).toBe(8);
    expect(MAX_SESSIONS).toBe(20);
    expect(SESSION_MAX_AGE_S).toBe(2_592_000);
    expect(SESSION_COOKIE).toBe('gyeot_session');
  });

  it('Bearer 는 정확히 같을 때만(길이가 달라도 던지지 않는다)', () => {
    const { s } = store();
    expect(s.bearerOk(`Bearer ${TOKEN}`)).toBe(true);
    expect(s.bearerOk(`bearer ${TOKEN}`)).toBe(true);
    expect(s.bearerOk(`Bearer ${TOKEN}x`)).toBe(false);
    expect(s.bearerOk('Bearer ')).toBe(false);
    expect(s.bearerOk(TOKEN)).toBe(false);
    expect(s.bearerOk(undefined)).toBe(false);
  });

  it('코드는 120초 동안 한 번만', () => {
    const { s, tick } = store();
    const a = s.issueCode();
    expect(s.redeem(a.code)).toMatch(/^[0-9a-f]{64}$/);
    expect(s.redeem(a.code)).toBeNull();
    const b = s.issueCode();
    tick(BOOT_TTL_MS - 1);
    expect(s.redeem(b.code)).not.toBeNull();
    const c = s.issueCode();
    tick(BOOT_TTL_MS);
    expect(s.redeem(c.code)).toBeNull();
  });

  it('열린 코드는 8개까지 — 넘으면 가장 오래된 것부터 버린다', () => {
    const { s } = store();
    const codes = Array.from({ length: MAX_BOOT_CODES + 1 }, () => s.issueCode().code);
    expect(s.redeem(codes[0]!)).toBeNull();
    for (const c of codes.slice(1)) expect(s.redeem(c)).not.toBeNull();
  });

  it('이상한 꼴의 코드·쿠키는 보지도 않는다', () => {
    const { s } = store();
    expect(s.redeem('')).toBeNull();
    expect(s.redeem('../../x')).toBeNull();
    expect(s.sessionOk(cookieOf('nope'))).toBe(false);
    expect(s.sessionOk(undefined)).toBe(false);
  });

  it('세션은 쿠키로 확인하고, 30일이 지나면 끝난다', () => {
    const { s, tick } = store();
    const v = s.redeem(s.issueCode().code)!;
    expect(s.sessionOk(`a=1; ${cookieOf(v)}; b=2`)).toBe(true);
    tick(SESSION_MAX_AGE_S * 1000 - 1);
    expect(s.sessionOk(cookieOf(v))).toBe(true);
    tick(1);
    expect(s.sessionOk(cookieOf(v))).toBe(false);
  });

  it('세션은 20개까지 — 가장 오래 안 쓴 것부터 버린다', () => {
    const { s, tick } = store();
    const vals = Array.from({ length: MAX_SESSIONS }, () => {
      tick(1);
      return s.redeem(s.issueCode().code)!;
    });
    tick(1);
    expect(s.sessionOk(cookieOf(vals[0]!))).toBe(true); // 첫 것을 써서 가장 최근으로
    tick(1);
    s.redeem(s.issueCode().code); // 21번째
    expect(s.sessionOk(cookieOf(vals[0]!))).toBe(true);
    expect(s.sessionOk(cookieOf(vals[1]!))).toBe(false);
  });

  it('세션 값 자체는 보관하지 않는다(SHA-256 만)', () => {
    const { s } = store();
    const v = s.redeem(s.issueCode().code)!;
    expect(JSON.stringify(s.dump())).not.toContain(v);
    expect(s.dump().sessions).toHaveLength(1);
  });

  it('쿠키 문자열은 계약과 같다', () => {
    const { s } = store();
    expect(s.cookieFor('ab'.repeat(32))).toBe(`gyeot_session=${'ab'.repeat(32)}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000`);
  });
});
