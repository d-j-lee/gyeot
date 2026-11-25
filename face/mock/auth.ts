// 가짜 데몬의 열쇠 보관함 — docs/contracts.md §7·§12.1·§13.1 을 그대로 흉내 낸다.
//   인증: `Authorization: Bearer <값>` 하나. 값은 토큰(모든 /v1/*) 또는 세션 열쇠(얼굴이 쓰는 경로만, G-031). 쿠키·?t= 는 받지 않는다.
//   부트 코드: 32바이트 hex, 120초 동안 한 번만, 동시에 8개까지(넘으면 가장 오래된 것부터 버림)
//   세션 열쇠: 32바이트 hex, 만든 때부터 7일, 20개까지(가장 오래 안 쓴 것부터 버림). 값은 SHA-256 만 둔다.
// 가짜 데몬이라 파일(sessions.json)에 쓰지 않는다 — 다시 띄우면 세션이 모두 사라진다(얼굴의 401 안내를 보기 좋다).
import { createHash, createHmac, randomBytes, timingSafeEqual } from 'node:crypto';

export const BOOT_TTL_MS = 120_000;
export const MAX_BOOT_CODES = 8;
export const MAX_SESSIONS = 20;
export const SESSION_TTL_MS = 7 * 24 * 60 * 60 * 1000;

const HEX64 = /^[0-9a-f]{64}$/;

const sha256 = (v: string) => createHash('sha256').update(v).digest('hex');

export interface BootCode {
  code: string;
  expires_ms: number;
}

export interface SessionKey {
  session: string;
  expires_ms: number;
}

export type Credential = 'token' | 'session';

/** §13.1: n 은 16–128자 hex */
export function isHelloNonce(n: string | null | undefined): n is string {
  return typeof n === 'string' && /^[0-9a-fA-F]{16,128}$/.test(n);
}

/** §13.1: proof = hex(HMAC-SHA256(key = 토큰 UTF-8, msg = "gyeot-hello/v1|<포트>|<n>")) */
export function helloProof(token: string, port: number, n: string): string {
  return createHmac('sha256', Buffer.from(token, 'utf8')).update(`gyeot-hello/v1|${port}|${n}`).digest('hex');
}

export class AuthStore {
  /** 코드 → 만료 시각. Map 은 넣은 차례를 지키므로 맨 앞이 가장 오래된 것이다. */
  private readonly codes = new Map<string, number>();
  /** sha256(세션 열쇠) → 만든 시각·마지막으로 쓴 시각 */
  private readonly sessions = new Map<string, { created: number; used: number }>();
  private readonly token: Buffer;
  private readonly now: () => number;

  constructor(token: string, now: () => number = Date.now) {
    this.token = Buffer.from(token, 'utf8');
    this.now = now;
  }

  /** `Authorization` 머리 → 토큰(상수 시간 비교)·살아 있는 세션 열쇠·없음. 세션이면 마지막 사용 시각을 갱신한다. */
  classify(header: string | undefined): Credential | null {
    const m = /^Bearer\s+(\S+)$/i.exec(header ?? '');
    if (!m) return null;
    const value = m[1]!;
    const got = Buffer.from(value, 'utf8');
    if (got.length === this.token.length && timingSafeEqual(got, this.token)) return 'token';
    if (!HEX64.test(value)) return null;
    const h = sha256(value);
    const s = this.sessions.get(h);
    if (!s) return null;
    const now = this.now();
    if (now - s.created >= SESSION_TTL_MS) {
      this.sessions.delete(h);
      return null;
    }
    s.used = now;
    return 'session';
  }

  issueCode(): BootCode {
    const now = this.now();
    for (const [c, exp] of this.codes) if (exp <= now) this.codes.delete(c);
    while (this.codes.size >= MAX_BOOT_CODES) this.codes.delete(this.codes.keys().next().value!);
    const code = randomBytes(32).toString('hex');
    const expires_ms = now + BOOT_TTL_MS;
    this.codes.set(code, expires_ms);
    return { code, expires_ms };
  }

  /** 코드를 쓰고(맞든 지났든 지운다) 새 세션 열쇠를 돌려준다. 틀리거나 지났으면 null. */
  redeem(code: unknown): SessionKey | null {
    if (typeof code !== 'string' || !HEX64.test(code)) return null;
    const exp = this.codes.get(code);
    this.codes.delete(code);
    const now = this.now();
    if (exp === undefined || exp <= now) return null;
    while (this.sessions.size >= MAX_SESSIONS) {
      let oldest: string | null = null;
      let oldestUsed = Infinity;
      for (const [h, s] of this.sessions) {
        if (s.used < oldestUsed) {
          oldest = h;
          oldestUsed = s.used;
        }
      }
      this.sessions.delete(oldest!);
    }
    const session = randomBytes(32).toString('hex');
    this.sessions.set(sha256(session), { created: now, used: now });
    return { session, expires_ms: now + SESSION_TTL_MS };
  }

  /** 시험·로그용: 보관한 것(세션은 해시만). */
  dump(): { codes: number; sessions: string[] } {
    return { codes: this.codes.size, sessions: [...this.sessions.keys()] };
  }
}
