// @vitest-environment-options {"url": "http://127.0.0.1:47321/"}
import { describe, expect, it, vi } from 'vitest';
import { checkPeer, faceProof, newNonce, PeerUnverified, peerPort } from './peer.ts';

// 서버 담당과 같은 공유 시험 값(§14.1). 기대값은 파이썬 hashlib·hmac 과 node:crypto 로 따로 계산했다:
//   key = sha256("0123456789abcdef"*4 의 UTF-8) = a8ae6e6e…77c8e
//   hex(HMAC-SHA256(key, "gyeot-face-hello/v1|47321|" + "ab"*32))
const SHARED_SESSION = '0123456789abcdef'.repeat(4);
const SHARED_N = 'ab'.repeat(32);
const SHARED_47321 = '607b8f06e715b5b23515fbb572a63617cf35019b687b1444f4433e27a7d368e4';
const SHARED_47322 = 'fcb875c58193eea573405310509b4ded38cafc4c0296e5189bad09d3a6d83f91';

const KEY = '1a2b3c4d'.repeat(8); // n("ab"*32)과 겹치지 않게
const OTHER = 'cd'.repeat(32);

function hello(body: unknown, status = 200) {
  return vi.fn(async (_url: string, _init?: RequestInit) => new Response(typeof body === 'string' ? body : JSON.stringify(body), { status }));
}

describe('얼굴 확인 값(§14.1, WebCrypto)', () => {
  it('공유 시험 값과 같다 — key = SHA-256(열쇠 UTF-8), msg = "gyeot-face-hello/v1|<포트>|<n>"', async () => {
    expect(await faceProof(SHARED_SESSION, '47321', SHARED_N)).toBe(SHARED_47321);
    expect(await faceProof(SHARED_SESSION, 47322, SHARED_N)).toBe(SHARED_47322);
  });

  it('HMAC 열쇠는 SHA-256(세션 값)의 32바이트다(hex 글자 64개가 아니다) — 서버 담당의 중간값과 같다', async () => {
    const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(SHARED_SESSION)));
    expect([...digest].map((b) => b.toString(16).padStart(2, '0')).join('')).toBe('a8ae6e6ee929abea3afcfc5258c8ccd6f85273e0d4626d26c7279f3250f77c8e');
    // hex 글자를 열쇠로 쓰면 다른 값이 나온다(그렇게 구현하면 공유 값과 어긋난다)
    const wrongKey = await crypto.subtle.importKey('raw', new TextEncoder().encode('a8ae6e6ee929abea3afcfc5258c8ccd6f85273e0d4626d26c7279f3250f77c8e'), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
    const wrong = new Uint8Array(await crypto.subtle.sign('HMAC', wrongKey, new TextEncoder().encode(`gyeot-face-hello/v1|47321|${SHARED_N}`)));
    expect([...wrong].map((b) => b.toString(16).padStart(2, '0')).join('')).not.toBe(SHARED_47321);
  });

  it('n 은 32바이트 난수 hex(64자), 매번 다르다', () => {
    const a = newNonce();
    expect(a).toMatch(/^[0-9a-f]{64}$/);
    expect(newNonce()).not.toBe(a);
  });

  it('포트는 API 출처의 포트(같은 출처면 location 의 포트), 없으면 기본 포트', () => {
    expect(peerPort('', new URL('http://127.0.0.1:47321/?mode=overlay'))).toBe('47321');
    expect(peerPort('http://127.0.0.1:47399', new URL('http://127.0.0.1:5173/'))).toBe('47399');
    expect(peerPort('', new URL('http://127.0.0.1/'))).toBe('80');
  });
});

describe('보내기 전에 상대 확인(checkPeer)', () => {
  it('GET /v1/hello?n=<n> 을 인증 머리·쿠키 없이 보내고, 내 값이 face 에 있으면 known', async () => {
    const mine = await faceProof(KEY, '47321', SHARED_N);
    const fetchImpl = hello({ v: 1, proof: '0'.repeat(64), face: ['1'.repeat(64), mine] });
    expect(await checkPeer({ base: 'http://d', key: KEY, port: '47321', fetchImpl: fetchImpl as unknown as typeof fetch, nonce: () => SHARED_N })).toBe('known');
    const [url, init] = fetchImpl.mock.calls[0]!;
    expect(url).toBe(`http://d/v1/hello?n=${SHARED_N}`);
    expect(init!.method).toBe('GET');
    expect(init!.credentials).toBe('omit');
    expect(JSON.stringify(init!.headers ?? {})).not.toMatch(/authorization|cookie/i);
    expect(JSON.stringify([url, init])).not.toContain(KEY);
  });

  it('face 에 없으면 unknown(다른 열쇠·다른 포트·다른 n 으로 계산한 값은 통하지 않는다)', async () => {
    for (const face of [
      [],
      [await faceProof(OTHER, '47321', SHARED_N)],
      [await faceProof(KEY, '47322', SHARED_N)],
      [await faceProof(KEY, '47321', 'cd'.repeat(32))],
    ]) {
      const fetchImpl = hello({ v: 1, proof: 'x', face });
      expect(await checkPeer({ base: '', key: KEY, port: '47321', fetchImpl: fetchImpl as unknown as typeof fetch, nonce: () => SHARED_N })).toBe('unknown');
    }
  });

  it('기본 포트는 location 의 포트, n 은 새 난수', async () => {
    let n = '';
    const fetchImpl = vi.fn(async (url: string) => {
      n = new URL(url, 'http://x').searchParams.get('n')!;
      return new Response(JSON.stringify({ v: 1, proof: 'x', face: [await faceProof(KEY, '47321', n)] }));
    });
    expect(await checkPeer({ base: '', key: KEY, fetchImpl: fetchImpl as unknown as typeof fetch })).toBe('known');
    expect(n).toMatch(/^[0-9a-f]{64}$/);
  });

  it('닿지 않음·5xx·4xx·꼴 이상이면 PeerUnverified(끊긴 것으로 본다) — known/unknown 이 아니다', async () => {
    const cases: (() => Promise<Response>)[] = [
      async () => Promise.reject(new TypeError('fetch failed')),
      async () => new Response('', { status: 503 }),
      async () => new Response('', { status: 404 }),
      async () => new Response('{oops'),
      async () => new Response(JSON.stringify({ v: 1, proof: 'x' })), // face 없는 옛 데몬·가짜 서버
      async () => new Response(JSON.stringify({ v: 1, proof: 'x', face: 'nope' })),
      async () => new Response(JSON.stringify({ v: 1, proof: 'x', face: [1, 2] })),
    ];
    for (const c of cases) {
      const fetchImpl = vi.fn(c);
      await expect(checkPeer({ base: '', key: KEY, port: '47321', fetchImpl: fetchImpl as unknown as typeof fetch })).rejects.toBeInstanceOf(PeerUnverified);
    }
  });
});
