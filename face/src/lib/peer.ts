// 얼굴 확인(§14.1, G-036): 세션 열쇠를 보내기 전에 그 포트의 서버가 내 세션을 아는지 본다.
// 데몬이 멈춘 사이 다른 OS 사용자가 같은 포트를 잡으면, 다시 잇는 얼굴이 그 서버에 열쇠를 보낼 수 있었다(R5 H1).
//   GET /v1/hello?n=<32바이트 난수 hex>(인증 머리 없음) → {"v":1,"proof":…,"face":[…]}
//   face 에는 살아 있는 세션마다 hex(HMAC-SHA256(key = SHA-256(세션 값 UTF-8)의 32바이트, msg = "gyeot-face-hello/v1|<포트>|<n>"))가 섞여 있다.
//   얼굴은 자기 열쇠로 같은 값을 WebCrypto 로 계산해 목록에 있는지 본다. 열쇠는 이 확인에 실리지 않는다.

export const FACE_HELLO_PREFIX = 'gyeot-face-hello/v1';

/** hello 를 끝내 확인하지 못했다(닿지 않음·5xx·4xx·꼴 이상). 끊긴 것으로 보고 다시 시도 간격을 따른다. 열쇠 문제가 아니다. */
export class PeerUnverified extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'PeerUnverified';
  }
}

const enc = new TextEncoder();

function hex(buf: ArrayBuffer): string {
  return [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

function subtleCrypto(): SubtleCrypto {
  const s = globalThis.crypto?.subtle;
  // crypto.subtle 은 안전한 문맥에만 있다. http://127.0.0.1·localhost 는 안전한 문맥이다.
  if (!s) throw new PeerUnverified('이 창에서는 WebCrypto 를 쓸 수 없어요');
  return s;
}

/** hex(HMAC-SHA256(key = SHA-256(열쇠 UTF-8), msg = "gyeot-face-hello/v1|<포트>|<n>")) */
export async function faceProof(key: string, port: string | number, n: string): Promise<string> {
  const subtle = subtleCrypto();
  const raw = await subtle.digest('SHA-256', enc.encode(key));
  const hmacKey = await subtle.importKey('raw', raw, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  return hex(await subtle.sign('HMAC', hmacKey, enc.encode(`${FACE_HELLO_PREFIX}|${port}|${n}`)));
}

/** 32바이트 난수 hex(64자) */
export function newNonce(): string {
  const b = new Uint8Array(32);
  globalThis.crypto.getRandomValues(b);
  return [...b].map((x) => x.toString(16).padStart(2, '0')).join('');
}

/** 확인에 넣을 포트: API 출처의 포트(같은 출처면 이 페이지의 포트). 주소에 포트가 없으면 기본 포트. */
export function peerPort(base: string, loc: Pick<Location, 'href'> = location): string {
  const u = new URL(base || loc.href, loc.href);
  return u.port || (u.protocol === 'https:' ? '443' : '80');
}

export interface CheckPeerOptions {
  base: string;
  key: string;
  port?: string;
  fetchImpl?: typeof fetch;
  /** 시험용: n 을 정한다 */
  nonce?: () => string;
  signal?: AbortSignal;
}

/**
 * 그 포트의 서버가 내 세션을 아는가.
 * - 'known': 목록에 내 값이 있다 → 열쇠를 보내도 된다.
 * - 'unknown': 목록에 없다(다른 사용자의 서버, 다시 떠서 세션을 잃은 데몬) → 열쇠를 보내지 않고 401 과 같이 다룬다.
 * - 던짐(PeerUnverified): 닿지 않음·5xx·4xx·꼴 이상 → 끊긴 것으로 다룬다. 열쇠는 보내지도 지우지도 않는다.
 */
export async function checkPeer(opts: CheckPeerOptions): Promise<'known' | 'unknown'> {
  const fetchImpl = opts.fetchImpl ?? ((...a) => fetch(...a));
  const n = (opts.nonce ?? newNonce)();
  const port = opts.port ?? peerPort(opts.base);
  const ctrl = new AbortController();
  const onAbort = () => ctrl.abort();
  if (opts.signal?.aborted) ctrl.abort();
  else opts.signal?.addEventListener('abort', onAbort, { once: true });
  const timer = setTimeout(() => ctrl.abort(), 8_000);
  let body: unknown;
  try {
    let res: Response;
    try {
      res = await fetchImpl(`${opts.base}/v1/hello?n=${n}`, { method: 'GET', credentials: 'omit', cache: 'no-store', signal: ctrl.signal });
    } catch (e) {
      throw new PeerUnverified(`hello 에 닿지 않음: ${(e as Error)?.message ?? e}`);
    }
    if (!res.ok) {
      void res.body?.cancel().catch(() => {});
      throw new PeerUnverified(`hello → ${res.status}`);
    }
    try {
      body = await res.json();
    } catch {
      throw new PeerUnverified('hello 답이 JSON 이 아님');
    }
  } finally {
    clearTimeout(timer);
    opts.signal?.removeEventListener('abort', onAbort);
  }
  const face = typeof body === 'object' && body !== null ? (body as { face?: unknown }).face : undefined;
  if (!Array.isArray(face) || !face.every((f) => typeof f === 'string')) throw new PeerUnverified('hello 답에 face 목록이 없음');
  const mine = await faceProof(opts.key, port, n);
  return (face as string[]).some((f) => f.toLowerCase() === mine) ? 'known' : 'unknown';
}
