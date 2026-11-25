// 가짜 데몬의 열쇠 보관함 — docs/contracts.md §7·§12.1 을 그대로 흉내 낸다.
//   부트 코드: 32바이트 hex, 120초 동안 한 번만, 동시에 8개까지(넘으면 가장 오래된 것부터 버림)
//   세션: 32바이트 hex 쿠키, 30일, 20개까지(가장 오래 안 쓴 것부터 버림). 값은 SHA-256 만 둔다.
// 가짜 데몬이라 파일(sessions.json)에 쓰지 않는다 — 다시 띄우면 세션이 모두 사라진다(얼굴의 401 안내를 보기 좋다).
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto';

export const BOOT_TTL_MS = 120_000;
export const MAX_BOOT_CODES = 8;
export const MAX_SESSIONS = 20;
export const SESSION_MAX_AGE_S = 30 * 24 * 60 * 60;
export const SESSION_COOKIE = 'gyeot_session';

const HEX64 = /^[0-9a-f]{64}$/;

const sha256 = (v: string) => createHash('sha256').update(v).digest('hex');

export interface BootCode {
  code: string;
  expires_ms: number;
}

export class AuthStore {
  /** 코드 → 만료 시각. Map 은 넣은 차례를 지키므로 맨 앞이 가장 오래된 것이다. */
  private readonly codes = new Map<string, number>();
  /** sha256(세션 값) → 만든 시각·마지막으로 쓴 시각 */
  private readonly sessions = new Map<string, { created: number; used: number }>();
  private readonly token: Buffer;
  private readonly now: () => number;

  constructor(token: string, now: () => number = Date.now) {
    this.token = Buffer.from(token, 'utf8');
    this.now = now;
  }

  /** `Authorization: Bearer <토큰>` — 상수 시간 비교. */
  bearerOk(header: string | undefined): boolean {
    const m = /^Bearer\s+(\S+)$/i.exec(header ?? '');
    if (!m) return false;
    const got = Buffer.from(m[1]!, 'utf8');
    return got.length === this.token.length && timingSafeEqual(got, this.token);
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

  /** 코드를 쓰고(맞든 지났든 지운다) 새 세션 값을 돌려준다. 틀리거나 지났으면 null. */
  redeem(code: string): string | null {
    if (!HEX64.test(code)) return null;
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
    const value = randomBytes(32).toString('hex');
    this.sessions.set(sha256(value), { created: now, used: now });
    return value;
  }

  /** Cookie 머리에 살아 있는 세션이 있으면 마지막 사용 시각을 갱신하고 true. */
  sessionOk(cookieHeader: string | undefined): boolean {
    const now = this.now();
    for (const part of (cookieHeader ?? '').split(';')) {
      const [name, ...rest] = part.trim().split('=');
      if (name !== SESSION_COOKIE) continue;
      const value = rest.join('=');
      if (!HEX64.test(value)) continue;
      const h = sha256(value);
      const s = this.sessions.get(h);
      if (!s) continue;
      if (now - s.created >= SESSION_MAX_AGE_S * 1000) {
        this.sessions.delete(h);
        continue;
      }
      s.used = now;
      return true;
    }
    return false;
  }

  cookieFor(value: string): string {
    return `${SESSION_COOKIE}=${value}; HttpOnly; SameSite=Strict; Path=/; Max-Age=${SESSION_MAX_AGE_S}`;
  }

  /** 시험·로그용: 보관한 것(세션은 해시만). */
  dump(): { codes: number; sessions: string[] } {
    return { codes: this.codes.size, sessions: [...this.sessions.keys()] };
  }
}
