// fetch 스트림용 작은 SSE(text/event-stream) 해석기(§8). EventSource 는 Authorization 머리를 실을 수 없어서 쓰지 않는다(§12.1-3).
// WHATWG HTML "Server-sent events" 해석 규칙을 따른다.
// - 줄 끝: CRLF, LF, CR(조각 경계에 걸쳐도 된다). 스트림 맨 앞 BOM 하나는 버린다.
// - ':' 로 시작하는 줄은 주석(데몬의 15초 ping)
// - 'data' 는 여러 줄을 '\n' 으로 잇고, 빈 줄에서 내보낸다. data 가 없으면 내보내지 않는다.
// - 'event' 가 없으면 'message'. 'id'·'retry'·모르는 필드는 버린다(다시 잇기는 얼굴이 정하고, 다시 이으면 snapshot 이 온다).
// - 끝까지 빈 줄이 오지 않은 사건은 버린다.

export interface SseMessage {
  readonly event: string;
  readonly data: string;
}

export class SseTooLarge extends Error {
  constructor(limit: number) {
    super(`SSE 사건이 너무 커요(${limit}자 넘음)`);
    this.name = 'SseTooLarge';
  }
}

/** 한 줄·한 사건의 최대 크기. 데몬 본문 한도(1 MiB)와 맞춘다. */
export const MAX_SSE_CHARS = 1024 * 1024;

export class SseParser {
  private line = '';
  private skipLf = false;
  private started = false;
  private data = '';
  private hasData = false;
  private type = '';
  private readonly onMessage: (m: SseMessage) => void;
  private readonly onComment: (text: string) => void;
  private readonly max: number;

  constructor(onMessage: (m: SseMessage) => void, onComment: (text: string) => void = () => {}, max: number = MAX_SSE_CHARS) {
    this.onMessage = onMessage;
    this.onComment = onComment;
    this.max = max;
  }

  /** 디코딩한 글 조각을 넣는다. 사건이 끝날 때마다 onMessage 가 불린다. */
  push(chunk: string): void {
    let text = chunk;
    if (!this.started && text.length > 0) {
      this.started = true;
      if (text.charCodeAt(0) === 0xfeff) text = text.slice(1);
    }
    let start = 0;
    let i = 0;
    if (this.skipLf && text.length > 0) {
      if (text.charCodeAt(0) === 0x0a) start = i = 1;
      this.skipLf = false;
    }
    for (; i < text.length; i++) {
      const c = text.charCodeAt(i);
      if (c !== 0x0a && c !== 0x0d) continue;
      const line = this.line + text.slice(start, i);
      this.line = '';
      if (c === 0x0d) {
        if (i + 1 < text.length) {
          if (text.charCodeAt(i + 1) === 0x0a) i++;
        } else {
          this.skipLf = true;
        }
      }
      start = i + 1;
      this.processLine(line);
    }
    this.line += text.slice(start);
    if (this.line.length > this.max) {
      this.line = '';
      throw new SseTooLarge(this.max);
    }
  }

  private processLine(line: string): void {
    if (line === '') {
      this.dispatch();
      return;
    }
    if (line.charCodeAt(0) === 0x3a /* : */) {
      this.onComment(line.slice(1));
      return;
    }
    const colon = line.indexOf(':');
    const field = colon === -1 ? line : line.slice(0, colon);
    let value = colon === -1 ? '' : line.slice(colon + 1);
    if (value.charCodeAt(0) === 0x20) value = value.slice(1);
    if (field === 'event') {
      this.type = value;
    } else if (field === 'data') {
      this.data = this.hasData ? `${this.data}\n${value}` : value;
      this.hasData = true;
      if (this.data.length > this.max) {
        this.data = '';
        this.hasData = false;
        throw new SseTooLarge(this.max);
      }
    }
  }

  private dispatch(): void {
    const { hasData, data, type } = this;
    this.data = '';
    this.hasData = false;
    this.type = '';
    if (!hasData) return;
    this.onMessage({ event: type || 'message', data });
  }
}
