import { describe, expect, it } from 'vitest';
import { MAX_SSE_CHARS, SseParser, SseTooLarge, type SseMessage } from './sse.ts';

function parse(chunks: string[], max?: number): { events: SseMessage[]; comments: string[] } {
  const events: SseMessage[] = [];
  const comments: string[] = [];
  const p = new SseParser(
    (e) => events.push(e),
    (c) => comments.push(c),
    max,
  );
  for (const c of chunks) p.push(c);
  return { events, comments };
}

describe('SSE 해석기(§8, fetch 스트림용)', () => {
  it('event 이름과 data 를 묶어 빈 줄에서 내보낸다', () => {
    const { events } = parse(['event: presence\ndata: {"mood":"idle"}\n\n']);
    expect(events).toEqual([{ event: 'presence', data: '{"mood":"idle"}' }]);
  });

  it('여러 줄 data 는 \\n 으로 잇는다', () => {
    const { events } = parse(['event: reply\ndata: {"text":\ndata: "가"}\n\n']);
    expect(events).toEqual([{ event: 'reply', data: '{"text":\n"가"}' }]);
  });

  it('event 가 없으면 message, 콜론 뒤 공백은 하나만 뗀다, 콜론 없는 줄은 빈 값', () => {
    expect(parse(['data:  두칸\ndata:붙음\n\n']).events).toEqual([{ event: 'message', data: ' 두칸\n붙음' }]);
    expect(parse(['data\n\n']).events).toEqual([{ event: 'message', data: '' }]);
  });

  it('`:` 로 시작하는 줄(ping)은 주석이고 사건이 아니다', () => {
    const { events, comments } = parse([': ping\n\n: hello\n\nevent: snapshot\ndata: {}\n\n']);
    expect(comments).toEqual([' ping', ' hello']);
    expect(events).toEqual([{ event: 'snapshot', data: '{}' }]);
  });

  it('id 는 없어도 되고, 있어도 사건 모양을 바꾸지 않는다. retry·모르는 필드는 버린다', () => {
    const { events } = parse(['id: 7\nretry: 10\nfoo: bar\nevent: item\ndata: 1\n\nevent: item\ndata: 2\n\n']);
    expect(events).toEqual([
      { event: 'item', data: '1' },
      { event: 'item', data: '2' },
    ]);
  });

  it('data 없는 사건(event 만)은 내보내지 않고, 다음 사건에 이름이 새지 않는다', () => {
    const { events } = parse(['event: deliver\n\ndata: x\n\n']);
    expect(events).toEqual([{ event: 'message', data: 'x' }]);
  });

  it('빈 줄로 끝나지 않은 사건은 내보내지 않는다(조각이 더 오기를 기다린다)', () => {
    expect(parse(['event: item\ndata: {"id":"a"}\n']).events).toEqual([]);
  });

  it('CRLF·CR·LF 를 모두 줄 끝으로 본다(조각 경계의 CR|LF 포함)', () => {
    const { events } = parse(['data: 1\r', '\ndata: 2\r\r', 'data: 3\n', '\n']);
    expect(events.map((e) => e.data)).toEqual(['1\n2', '3']);
  });

  it('조각이 아무 데서나 잘려도 결과가 같다', () => {
    const raw =
      ': hello\n\nevent: snapshot\r\ndata: {"items":[],\r\ndata: "line":"가나다"}\r\n\r\n: ping\n\nevent: presence\ndata: {"mood":"done"}\n\n';
    const whole = parse([raw]);
    expect(whole.events).toEqual([
      { event: 'snapshot', data: '{"items":[],\n"line":"가나다"}' },
      { event: 'presence', data: '{"mood":"done"}' },
    ]);
    expect(whole.comments).toEqual([' hello', ' ping']);
    for (let i = 1; i < raw.length; i++) {
      expect(parse([raw.slice(0, i), raw.slice(i)]), `잘린 곳 ${i}`).toEqual(whole);
    }
    // 한 글자씩
    expect(parse([...raw])).toEqual(whole);
  });

  it('스트림 맨 앞의 BOM 하나는 버린다', () => {
    expect(parse(['﻿event: item\ndata: 1\n\n']).events).toEqual([{ event: 'item', data: '1' }]);
  });

  it('너무 큰 줄·사건은 SseTooLarge(메모리를 끝없이 쓰지 않는다)', () => {
    expect(MAX_SSE_CHARS).toBe(1024 * 1024);
    expect(() => parse([`data: ${'x'.repeat(50)}\n`], 20)).toThrow(SseTooLarge);
    expect(() => parse(['x'.repeat(50)], 20)).toThrow(SseTooLarge);
    expect(() => parse(['data: 123456789\ndata: 123456789\ndata: 1\n'], 20)).toThrow(SseTooLarge);
  });
});
