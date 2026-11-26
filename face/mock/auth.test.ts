// @vitest-environment node
import { describe, expect, it } from 'vitest';
import { createHmac } from 'node:crypto';
import { AuthStore, BOOT_TTL_MS, faceProofOf, helloProof, isHelloNonce, MAX_BOOT_CODES, MAX_SESSIONS, SESSION_TTL_MS } from './auth.ts';

const TOKEN = 'ab'.repeat(32);

function store(start = 1_000_000) {
  let now = start;
  const s = new AuthStore(TOKEN, () => now);
  return { s, tick: (ms: number) => (now += ms), at: () => now };
}

const bearer = (v: string) => `Bearer ${v}`;

describe('가짜 데몬의 열쇠 보관함(§7·§12.1)', () => {
  it('계약의 값', () => {
    expect(BOOT_TTL_MS).toBe(120_000);
    expect(MAX_BOOT_CODES).toBe(8);
    expect(MAX_SESSIONS).toBe(20);
    expect(SESSION_TTL_MS).toBe(7 * 24 * 60 * 60 * 1000);
  });

  it('Bearer 가 토큰이면 token — 정확히 같을 때만(길이가 달라도 던지지 않는다)', () => {
    const { s } = store();
    expect(s.classify(bearer(TOKEN))).toBe('token');
    expect(s.classify(`bearer ${TOKEN}`)).toBe('token');
    expect(s.classify(bearer(`${TOKEN}x`))).toBeNull();
    expect(s.classify('Bearer ')).toBeNull();
    expect(s.classify(TOKEN)).toBeNull();
    expect(s.classify(undefined)).toBeNull();
  });

  it('코드는 120초 동안 한 번만', () => {
    const { s, tick } = store();
    const a = s.issueCode();
    expect(s.redeem(a.code)?.session).toMatch(/^[0-9a-f]{64}$/);
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

  it('세션 열쇠: 만든 때부터 7일, Bearer 로 확인하면 session', () => {
    const { s, tick, at } = store();
    const r = s.redeem(s.issueCode().code)!;
    expect(r.expires_ms).toBe(at() + SESSION_TTL_MS);
    expect(s.classify(bearer(r.session))).toBe('session');
    tick(SESSION_TTL_MS - 1);
    expect(s.classify(bearer(r.session))).toBe('session');
    tick(1);
    expect(s.classify(bearer(r.session))).toBeNull();
  });

  it('세션 열쇠를 쿠키 꼴로 줘도 받지 않는다(쿠키는 인증이 아니다)', () => {
    const { s } = store();
    const r = s.redeem(s.issueCode().code)!;
    expect(s.classify(`gyeot_session=${r.session}`)).toBeNull();
    expect(s.classify(r.session)).toBeNull();
  });

  it('이상한 꼴의 코드는 보지도 않는다', () => {
    const { s } = store();
    expect(s.redeem('')).toBeNull();
    expect(s.redeem('../../x')).toBeNull();
    expect(s.classify(bearer('nope'))).toBeNull();
  });

  it('세션은 20개까지 — 가장 오래 안 쓴 것부터 버린다', () => {
    const { s, tick } = store();
    const vals = Array.from({ length: MAX_SESSIONS }, () => {
      tick(1);
      return s.redeem(s.issueCode().code)!.session;
    });
    tick(1);
    expect(s.classify(bearer(vals[0]!))).toBe('session'); // 첫 것을 써서 가장 최근으로
    tick(1);
    s.redeem(s.issueCode().code); // 21번째
    expect(s.classify(bearer(vals[0]!))).toBe('session');
    expect(s.classify(bearer(vals[1]!))).toBeNull();
  });

  it('세션 값 자체는 보관하지 않는다(SHA-256 만)', () => {
    const { s } = store();
    const v = s.redeem(s.issueCode().code)!.session;
    expect(JSON.stringify(s.dump())).not.toContain(v);
    expect(s.dump().sessions).toHaveLength(1);
  });
});

describe('데몬 확인(§13.1 /v1/hello)', () => {
  it('proof = hex(HMAC-SHA256(토큰, "gyeot-hello/v1|<포트>|<n>"))', () => {
    const n = '0123456789abcdef'.repeat(4);
    const want = createHmac('sha256', TOKEN).update(`gyeot-hello/v1|47321|${n}`).digest('hex');
    expect(helloProof(TOKEN, 47321, n)).toBe(want);
    expect(helloProof(TOKEN, 47322, n)).not.toBe(want);
  });

  it('n 은 16–128자 hex', () => {
    expect(isHelloNonce('a'.repeat(16))).toBe(true);
    expect(isHelloNonce('A0'.repeat(64))).toBe(true);
    expect(isHelloNonce('a'.repeat(15))).toBe(false);
    expect(isHelloNonce('a'.repeat(129))).toBe(false);
    expect(isHelloNonce('zz'.repeat(10))).toBe(false);
    expect(isHelloNonce(null)).toBe(false);
  });
});

describe('얼굴 확인 목록(§14.1 face)', () => {
  // 서버 담당과 같은 공유 시험 값. 기대값은 파이썬 hashlib·hmac 으로 따로 계산했다.
  const SHARED_SESSION = '0123456789abcdef'.repeat(4);
  const SHARED_N = 'ab'.repeat(32);

  it('공유 시험 값: HMAC-SHA256(key = SHA-256(세션 UTF-8), "gyeot-face-hello/v1|47321|" + n)', () => {
    expect(faceProofOf(SHARED_SESSION, 47321, SHARED_N)).toBe('607b8f06e715b5b23515fbb572a63617cf35019b687b1444f4433e27a7d368e4');
    expect(faceProofOf(SHARED_SESSION, 47322, SHARED_N)).toBe('fcb875c58193eea573405310509b4ded38cafc4c0296e5189bad09d3a6d83f91');
  });

  it('살아 있는 세션마다 하나(해시만으로 계산), 끝난 세션·모르는 열쇠는 없다', () => {
    const { s, tick } = store();
    const a = s.redeem(s.issueCode().code)!.session;
    tick(SESSION_TTL_MS - 10);
    const b = s.redeem(s.issueCode().code)!.session;
    const n = 'cd'.repeat(32);
    expect(s.faceList(47321, n).sort()).toEqual([faceProofOf(a, 47321, n), faceProofOf(b, 47321, n)].sort());
    expect(s.faceList(47321, n)).not.toContain(faceProofOf('ef'.repeat(32), 47321, n));
    tick(10); // a 가 7일을 다 채웠다
    expect(s.faceList(47321, n)).toEqual([faceProofOf(b, 47321, n)]);
  });

  it('최대 20개, 순서는 섞는다', () => {
    const { s, tick } = store();
    const keys = Array.from({ length: MAX_SESSIONS }, () => {
      tick(1);
      return s.redeem(s.issueCode().code)!.session;
    });
    const n = 'ab'.repeat(32);
    const inOrder = keys.map((k) => faceProofOf(k, 47321, n));
    const lists = Array.from({ length: 5 }, () => s.faceList(47321, n));
    for (const l of lists) {
      expect(l).toHaveLength(MAX_SESSIONS);
      expect([...l].sort()).toEqual([...inOrder].sort());
    }
    expect(lists.some((l) => l.join() !== inOrder.join())).toBe(true);
  });

  it('보관한 것에 세션 값이 없어도 계산한다(값은 SHA-256 만 둔다)', () => {
    const { s } = store();
    const v = s.redeem(s.issueCode().code)!.session;
    expect(JSON.stringify(s.dump())).not.toContain(v);
    expect(s.faceList(1, 'a'.repeat(16))).toEqual([faceProofOf(v, 1, 'a'.repeat(16))]);
  });
});
