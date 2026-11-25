import { describe, expect, it } from 'vitest';
import { resolveBackoffUnitMs, resolveDaemonUrl, resolveDataDir, tokenPath } from '../src/config.js';
import { helloProblem, helloProof } from '../src/daemon.js';
import { backoffDelay } from '../src/link.js';
import { SseParser, SseSizeError, type SseEvent } from '../src/sse.js';
import { checkDecision, checkMessage, checkReply, clip, sanitizeMeta } from '../src/validate.js';

function parse(chunks: string[], max?: number): { events: SseEvent[]; comments: string[] } {
  const events: SseEvent[] = [];
  const comments: string[] = [];
  const p = new SseParser(
    (e) => events.push(e),
    (c) => comments.push(c),
    max,
  );
  for (const c of chunks) p.push(c);
  return { events, comments };
}

describe('SseParser', () => {
  it('여러 줄 data를 \\n으로 잇고 event 이름을 붙인다', () => {
    const { events } = parse(['event: message\ndata: a\ndata: b\n\n']);
    expect(events).toEqual([{ event: 'message', data: 'a\nb', id: undefined }]);
  });

  it('event가 없으면 message, 콜론 뒤 공백은 하나만 뗀다', () => {
    const { events } = parse(['data:  두칸\ndata:붙음\n\n']);
    expect(events).toEqual([{ event: 'message', data: ' 두칸\n붙음', id: undefined }]);
  });

  it('CRLF·CR·LF를 모두 줄 끝으로 본다(조각 경계의 CR|LF 포함)', () => {
    const { events } = parse(['data: 1\r', '\ndata: 2\r\r', 'data: 3\n', '\n']);
    expect(events.map((e) => e.data)).toEqual(['1\n2', '3']);
  });

  it('조각이 아무 데서나 잘려도 같다', () => {
    const raw = 'event: message\r\ndata: {"a":"가나다"}\r\n\r\n: ping\n\nid: 7\ndata: x\n\n';
    const whole = parse([raw]);
    for (let i = 1; i < raw.length; i++) {
      expect(parse([raw.slice(0, i), raw.slice(i)])).toEqual(whole);
    }
    expect(whole.events).toEqual([
      { event: 'message', data: '{"a":"가나다"}', id: undefined },
      { event: 'message', data: 'x', id: '7' },
    ]);
    expect(whole.comments).toEqual([' ping']);
  });

  it('주석은 사건이 아니고, data 없는 빈 사건은 내보내지 않는다', () => {
    const { events, comments } = parse([': hello\n\nevent: ping\n\n']);
    expect(events).toEqual([]);
    expect(comments).toEqual([' hello']);
  });

  it('콜론 없는 줄은 값이 빈 필드다(data → 빈 data 사건)', () => {
    const { events } = parse(['data\n\n']);
    expect(events).toEqual([{ event: 'message', data: '', id: undefined }]);
  });

  it('빈 줄 없이 끝난 사건은 내보내지 않는다', () => {
    const { events } = parse(['data: 끝나지 않음\n']);
    expect(events).toEqual([]);
  });

  it('너무 큰 사건은 SseSizeError', () => {
    expect(() => parse([`data: ${'x'.repeat(50)}\n`], 20)).toThrow(SseSizeError);
    expect(() => parse(['x'.repeat(50)], 20)).toThrow(SseSizeError);
  });
});

describe('재연결 대기', () => {
  it('1→2→4→8→16→30→30 단위', () => {
    expect([0, 1, 2, 3, 4, 5, 6, 20].map((a) => backoffDelay(a, 1000))).toEqual([
      1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000,
    ]);
  });

  it('GYEOT_CHANNEL_BACKOFF_MS는 범위 밖이면 기본 1000', () => {
    expect(resolveBackoffUnitMs({})).toBe(1000);
    expect(resolveBackoffUnitMs({ GYEOT_CHANNEL_BACKOFF_MS: '50' })).toBe(50);
    expect(resolveBackoffUnitMs({ GYEOT_CHANNEL_BACKOFF_MS: '0' })).toBe(1000);
    expect(resolveBackoffUnitMs({ GYEOT_CHANNEL_BACKOFF_MS: 'abc' })).toBe(1000);
  });
});

describe('경로(contracts §1)', () => {
  it('GYEOT_HOME이 먼저다', () => {
    expect(resolveDataDir({ GYEOT_HOME: '/x/y', XDG_STATE_HOME: '/s' }, 'linux', '/home/u')).toBe('/x/y');
  });

  it('Windows는 %LOCALAPPDATA%\\gyeot, 없으면 홈\\AppData\\Local\\gyeot', () => {
    expect(resolveDataDir({ LOCALAPPDATA: 'C:\\Users\\dj\\AppData\\Local' }, 'win32', 'C:\\Users\\dj')).toBe(
      'C:\\Users\\dj\\AppData\\Local\\gyeot',
    );
    expect(resolveDataDir({}, 'win32', 'C:\\Users\\dj')).toBe('C:\\Users\\dj\\AppData\\Local\\gyeot');
    expect(tokenPath('C:\\Users\\dj\\AppData\\Local\\gyeot', 'win32')).toBe('C:\\Users\\dj\\AppData\\Local\\gyeot\\token');
  });

  it('그 밖은 $XDG_STATE_HOME/gyeot(절대 경로일 때만), 아니면 ~/.local/state/gyeot', () => {
    expect(resolveDataDir({ XDG_STATE_HOME: '/s' }, 'linux', '/home/u')).toBe('/s/gyeot');
    expect(resolveDataDir({ XDG_STATE_HOME: 'rel' }, 'linux', '/home/u')).toBe('/home/u/.local/state/gyeot');
    expect(resolveDataDir({ XDG_STATE_HOME: '  ' }, 'darwin', '/Users/u')).toBe('/Users/u/.local/state/gyeot');
  });
});

describe('GYEOT_URL', () => {
  it('기본은 http://127.0.0.1:47321', () => {
    expect(resolveDaemonUrl({})).toEqual({ ok: true, origin: 'http://127.0.0.1:47321' });
  });

  it('루프백 http만 받는다', () => {
    // localhost는 127.0.0.1로 잇는다: 데몬은 127.0.0.1에만 묶인다(§7). localhost가 ::1로 풀리면 그 주소의 같은
    // 포트를 잡은 서버가 127.0.0.1의 진짜 데몬으로 확인을 중계해 증명을 통과할 수 있다(포트가 같아서).
    expect(resolveDaemonUrl({ GYEOT_URL: 'http://localhost:5000/x' })).toEqual({ ok: true, origin: 'http://127.0.0.1:5000' });
    expect(resolveDaemonUrl({ GYEOT_URL: 'http://LOCALHOST:5000' })).toEqual({ ok: true, origin: 'http://127.0.0.1:5000' });
    expect(resolveDaemonUrl({ GYEOT_URL: 'http://localhost' })).toEqual({ ok: true, origin: 'http://127.0.0.1' });
    for (const bad of ['https://127.0.0.1:1', 'http://example.com', 'http://192.168.0.2:47321', 'http://[::1]:1', '127.0.0.1:47321']) {
      expect(resolveDaemonUrl({ GYEOT_URL: bad }).ok, bad).toBe(false);
    }
    expect(resolveDaemonUrl({ GYEOT_URL: 'http://u:p@127.0.0.1:1' }).ok).toBe(false);
  });
});

describe('입력 검사', () => {
  it('reply', () => {
    expect(checkReply({ chat_id: 'c_1', text: '안녕' })).toEqual({ ok: true, value: { chat_id: 'c_1', text: '안녕' } });
    expect(checkReply({ chat_id: '', text: 'x' }).ok).toBe(false);
    expect(checkReply({ chat_id: 'a b', text: 'x' }).ok).toBe(false);
    expect(checkReply({ chat_id: 'a<b', text: 'x' }).ok).toBe(false);
    expect(checkReply({ chat_id: 'x'.repeat(129), text: 'x' }).ok).toBe(false);
    expect(checkReply({ chat_id: 'c', text: 'x'.repeat(16_001) }).ok).toBe(false);
    expect(checkReply(null).ok).toBe(false);
  });

  it('request_decision: 2~4개, 비지 않음, 중복 없음, 짧음', () => {
    expect(checkDecision({ question: ' q ', options: [' a', 'b '] })).toEqual({
      ok: true,
      value: { question: 'q', options: ['a', 'b'] },
    });
    expect(checkDecision({ question: 'q', options: ['a', 'b', 'c', 'd'] }).ok).toBe(true);
    for (const options of [['a'], ['a', 'b', 'c', 'd', 'e'], ['a', 'a'], ['a', ''], ['a', 1], [], 'ab', ['a', 'x'.repeat(61)]]) {
      expect(checkDecision({ question: 'q', options }).ok, JSON.stringify(options)).toBe(false);
    }
    expect(checkDecision({ question: '', options: ['a', 'b'] }).ok).toBe(false);
  });

  it('데몬 message: kind는 say·choice, 그 밖은 say', () => {
    expect(checkMessage({ chat_id: 'c', text: 't', kind: 'choice' })).toEqual({
      ok: true,
      value: { chat_id: 'c', text: 't', kind: 'choice' },
    });
    expect(checkMessage({ chat_id: 'c', text: 't', kind: 'weird' })).toMatchObject({ value: { kind: 'say' } });
    expect(checkMessage({ chat_id: 'c', text: '' }).ok).toBe(false);
    const long = checkMessage({ chat_id: 'c', text: '가'.repeat(32_005), kind: 'say' });
    expect(long.ok && long.value.text.startsWith('가'.repeat(32_000) + '\n…(')).toBe(true);
    expect(long.ok && long.value.text).toContain('5자를 잘랐습니다');
    expect(checkMessage({ text: 't' }).ok).toBe(false);
  });
});

describe('clip', () => {
  it('서로게이트 쌍을 가르지 않는다', () => {
    const s = 'ab😀cd';
    expect(clip(s, 3).startsWith('ab\n')).toBe(true);
    expect(clip(s, 4).startsWith('ab😀\n')).toBe(true);
    expect(clip(s, 10)).toBe(s);
  });
});

describe('meta', () => {
  it('식별자 키와 문자열 값만 남긴다', () => {
    expect(
      sanitizeMeta({ chat_id: 'c', 'chat-id': 'x', 'a.b': 'y', kind: 'say', n: 1 as unknown as string, 한글: 'z', ok_1: 'v' }),
    ).toEqual({ chat_id: 'c', kind: 'say', ok_1: 'v' });
  });
});

describe('데몬 확인(contracts §13.1)', () => {
  const TOKEN = '0123456789abcdef'.repeat(4);
  const N = 'ab'.repeat(32);
  const body = (proof: unknown): unknown => ({ v: 1, proof });

  it('증명은 공유 시험 값과 같다(HMAC-SHA256, 열쇠 = 토큰 UTF-8, 글 = "gyeot-hello/v1|<포트>|<n>")', () => {
    expect(helloProof(TOKEN, 47321, N)).toBe('cb2a3490aa613ccf14c93af5162080aff69a29f105ef62e3ef4777d7f85bd4c5');
    expect(helloProof(TOKEN, 47322, N)).toBe('c909b0dde49a2587904b41e63cd46b3faa9674ea124aa03d002b4b1235a683a1');
  });

  it('내 토큰·내가 연결한 포트·이번 난수의 증명만 받는다', () => {
    const good = helloProof(TOKEN, 47321, N);
    expect(helloProblem(body(good), TOKEN, 47321, N)).toBeUndefined();
    expect(helloProblem(body(good.toUpperCase()), TOKEN, 47321, N)).toBeUndefined();
    // 다른 포트(중계)·다른 토큰(다른 사용자)·다른 난수(옛 응답 되풀이)
    expect(helloProblem(body(helloProof(TOKEN, 47322, N)), TOKEN, 47321, N)).toMatch(/맞지 않/);
    expect(helloProblem(body(helloProof('f'.repeat(64), 47321, N)), TOKEN, 47321, N)).toMatch(/맞지 않/);
    expect(helloProblem(body(helloProof(TOKEN, 47321, 'cd'.repeat(32))), TOKEN, 47321, N)).toMatch(/맞지 않/);
    for (const bad of [
      { v: 2, proof: good },
      { proof: good },
      { v: 1 },
      body(good.slice(0, 63)),
      body(`${good}0`),
      body(`${good.slice(0, 63)}g`),
      body(12),
      { ok: true, version: '0.1.0' },
      null,
      '문자열',
      [good],
    ]) {
      expect(helloProblem(bad, TOKEN, 47321, N), JSON.stringify(bad)).toBeTypeOf('string');
    }
  });
});
