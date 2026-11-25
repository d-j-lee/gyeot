// 도구 입력과 데몬 메시지 검사. 실패 문구는 Claude가 스스로 고칠 수 있게 한국어로 짧게 쓴다.

export const LIMITS = {
  chatId: 128,
  text: 16_000,
  /** 데몬에서 오는 dj의 말. 넘치면 버리지 않고 자른 뒤 표시를 붙인다. */
  inboundText: 32_000,
  question: 300,
  option: 60,
  minOptions: 2,
  maxOptions: 4,
} as const;

/** chat_id는 `<channel chat_id="…">` 속성으로 되돌아가므로 공백·따옴표·꺾쇠·&를 받지 않는다. */
const CHAT_ID_RE = /^[^\s"'<>&\p{Cc}]+$/u;

/** meta 키는 식별자(글자·숫자·밑줄)만 된다. 다른 키는 Claude Code가 소리 없이 버린다. */
const META_KEY_RE = /^[A-Za-z0-9_]+$/;

export type Checked<T> = { ok: true; value: T } | { ok: false; error: string };

export interface ReplyInput {
  readonly chat_id: string;
  readonly text: string;
}

export interface DecisionInput {
  readonly question: string;
  readonly options: readonly string[];
}

export interface ChannelMessage {
  readonly chat_id: string;
  readonly text: string;
  readonly kind: string;
}

export function checkChatId(v: unknown): string | undefined {
  if (typeof v !== 'string' || v.length === 0) return 'chat_id는 비어 있지 않은 문자열이어야 합니다';
  if (v.length > LIMITS.chatId) return `chat_id는 ${LIMITS.chatId}자 이하여야 합니다`;
  if (!CHAT_ID_RE.test(v)) return 'chat_id에 공백·따옴표·꺾쇠·&·제어 문자를 쓸 수 없습니다';
  return undefined;
}

export function checkReply(args: unknown): Checked<ReplyInput> {
  const a = asRecord(args);
  const chatErr = checkChatId(a['chat_id']);
  if (chatErr) return fail(chatErr + ' — 받은 <channel> 태그의 chat_id를 그대로 넘기세요');
  const text = a['text'];
  if (typeof text !== 'string' || text.trim() === '') return fail('text는 비어 있지 않은 문자열이어야 합니다');
  if (text.length > LIMITS.text) return fail(`text는 ${LIMITS.text}자 이하여야 합니다`);
  return { ok: true, value: { chat_id: a['chat_id'] as string, text } };
}

export function checkDecision(args: unknown): Checked<DecisionInput> {
  const a = asRecord(args);
  const q = a['question'];
  if (typeof q !== 'string' || q.trim() === '') return fail('question은 비어 있지 않은 문자열이어야 합니다');
  if (q.trim().length > LIMITS.question) return fail(`question은 ${LIMITS.question}자 이하여야 합니다`);
  const opts = a['options'];
  if (!Array.isArray(opts)) return fail('options는 문자열 배열이어야 합니다');
  if (opts.length < LIMITS.minOptions || opts.length > LIMITS.maxOptions) {
    return fail(`options는 ${LIMITS.minOptions}~${LIMITS.maxOptions}개여야 합니다(받은 수 ${opts.length})`);
  }
  const options: string[] = [];
  for (const [i, o] of opts.entries()) {
    if (typeof o !== 'string' || o.trim() === '') return fail(`options[${i}]는 비어 있지 않은 문자열이어야 합니다`);
    const t = o.trim();
    if (t.length > LIMITS.option) return fail(`options[${i}]는 ${LIMITS.option}자 이하의 짧은 말이어야 합니다`);
    if (options.includes(t)) return fail(`options에 같은 선택지가 두 번 있습니다(${t})`);
    options.push(t);
  }
  return { ok: true, value: { question: q.trim(), options } };
}

/**
 * 데몬 `message` 사건의 data(JSON)를 검사한다(contracts §7: {chat_id, text, kind: say|choice}).
 * kind가 계약 밖이면 'say'로 본다. 너무 긴 text는 버리지 않고 잘라서 그 사실을 덧붙인다(사람의 말을 버리지 않는다).
 */
export function checkMessage(data: unknown): Checked<ChannelMessage> {
  const a = asRecord(data);
  const chatErr = checkChatId(a['chat_id']);
  if (chatErr) return fail(chatErr);
  const text = a['text'];
  if (typeof text !== 'string' || text.trim() === '') return fail('text가 비어 있습니다');
  const kind = a['kind'] === 'choice' ? 'choice' : 'say';
  return { ok: true, value: { chat_id: a['chat_id'] as string, text: clip(text, LIMITS.inboundText), kind } };
}

/** max 자(UTF-16 단위)로 자르되 서로게이트 쌍을 가르지 않는다. */
export function clip(text: string, max: number): string {
  if (text.length <= max) return text;
  let cut = max;
  const c = text.charCodeAt(cut - 1);
  if (c >= 0xd800 && c <= 0xdbff) cut -= 1;
  return `${text.slice(0, cut)}\n…(곁 channel이 뒤의 ${text.length - cut}자를 잘랐습니다)`;
}

/** meta에서 식별자가 아닌 키와 문자열이 아닌 값을 뺀다. */
export function sanitizeMeta(meta: Record<string, unknown>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(meta)) {
    if (META_KEY_RE.test(k) && typeof v === 'string') out[k] = v;
  }
  return out;
}

export function isMetaKey(k: string): boolean {
  return META_KEY_RE.test(k);
}

function asRecord(v: unknown): Record<string, unknown> {
  return v !== null && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : {};
}

function fail(error: string): { ok: false; error: string } {
  return { ok: false, error };
}
