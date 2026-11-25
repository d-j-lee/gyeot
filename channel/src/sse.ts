// 작은 SSE(text/event-stream) 해석기. WHATWG HTML "Server-sent events" 해석 규칙을 따른다.
// - 줄 끝: CRLF, LF, CR(조각 경계에 걸쳐도 된다)
// - ':'로 시작하는 줄은 주석
// - 'data'는 여러 줄을 '\n'으로 잇는다, 빈 줄에서 내보낸다
// - 끝까지 빈 줄이 오지 않은 사건은 버린다

export interface SseEvent {
  readonly event: string;
  readonly data: string;
  readonly id: string | undefined;
}

export class SseSizeError extends Error {
  constructor(limit: number) {
    super(`SSE 사건이 너무 큽니다(${limit}자 초과)`);
    this.name = 'SseSizeError';
  }
}

/** 한 사건(또는 한 줄)의 최대 크기. 데몬 본문 한도(1 MiB)와 맞춘다. */
export const MAX_EVENT_CHARS = 1024 * 1024;

export class SseParser {
  private line = '';
  private skipLf = false;
  private data = '';
  private type = '';
  private lastId: string | undefined;

  constructor(
    private readonly onEvent: (event: SseEvent) => void,
    private readonly onComment: (text: string) => void = () => {},
    private readonly maxChars: number = MAX_EVENT_CHARS,
  ) {}

  /** 디코딩한 글 조각을 넣는다. 한 사건이 끝날 때마다 onEvent가 불린다. */
  push(chunk: string): void {
    let start = 0;
    let i = 0;
    if (this.skipLf && chunk.length > 0) {
      if (chunk.charCodeAt(0) === 0x0a) start = i = 1;
      this.skipLf = false;
    }
    for (; i < chunk.length; i++) {
      const c = chunk.charCodeAt(i);
      if (c !== 0x0a && c !== 0x0d) continue;
      const line = this.line + chunk.slice(start, i);
      this.line = '';
      if (c === 0x0d) {
        if (i + 1 < chunk.length) {
          if (chunk.charCodeAt(i + 1) === 0x0a) i++;
        } else {
          this.skipLf = true;
        }
      }
      start = i + 1;
      this.processLine(line);
    }
    this.line += chunk.slice(start);
    if (this.line.length > this.maxChars) throw new SseSizeError(this.maxChars);
  }

  /** 스트림이 끝났다. 마치지 않은 사건은 버린다(규격). */
  reset(): void {
    this.line = '';
    this.skipLf = false;
    this.data = '';
    this.type = '';
  }

  get lastEventId(): string | undefined {
    return this.lastId;
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
    let field: string;
    let value: string;
    if (colon === -1) {
      field = line;
      value = '';
    } else {
      field = line.slice(0, colon);
      value = line.slice(colon + 1);
      if (value.charCodeAt(0) === 0x20) value = value.slice(1);
    }
    switch (field) {
      case 'event':
        this.type = value;
        break;
      case 'data':
        this.data += value + '\n';
        if (this.data.length > this.maxChars) {
          this.data = '';
          throw new SseSizeError(this.maxChars);
        }
        break;
      case 'id':
        if (!value.includes('\0')) this.lastId = value;
        break;
      default:
        // retry 및 모르는 필드는 무시한다(재연결 간격은 클라이언트가 정한다).
        break;
    }
  }

  private dispatch(): void {
    if (this.data === '') {
      this.type = '';
      return;
    }
    const data = this.data.endsWith('\n') ? this.data.slice(0, -1) : this.data;
    const event: SseEvent = { event: this.type || 'message', data, id: this.lastId };
    this.data = '';
    this.type = '';
    this.onEvent(event);
  }
}
