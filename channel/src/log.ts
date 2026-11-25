// stdout은 MCP 프로토콜 전용이다. 기록은 모두 stderr로 보낸다.

export type Level = 'info' | 'warn' | 'error';

export function log(level: Level, message: string): void {
  try {
    process.stderr.write(`[gyeot-channel] ${level}: ${message}\n`);
  } catch {
    // stderr가 닫혀 있어도 프로토콜을 깨뜨리지 않는다.
  }
}

/** 의존성이 실수로 console.log를 불러도 stdout을 더럽히지 않게 한다. */
export function routeConsoleToStderr(): void {
  const toStderr = (...args: unknown[]): void => {
    log('info', args.map((a) => (typeof a === 'string' ? a : safeInspect(a))).join(' '));
  };
  console.log = toStderr;
  console.info = toStderr;
  console.debug = toStderr;
}

function safeInspect(value: unknown): string {
  if (value instanceof Error) return value.stack ?? value.message;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

/** 오류를 사람이 읽을 짧은 문구로 바꾼다(원인 사슬의 코드 포함). */
export function describeError(err: unknown): string {
  if (err instanceof Error) {
    const cause = (err as { cause?: unknown }).cause;
    const code = (cause as { code?: unknown } | undefined)?.code ?? (err as { code?: unknown }).code;
    return typeof code === 'string' ? `${err.message} (${code})` : err.message;
  }
  return String(err);
}
