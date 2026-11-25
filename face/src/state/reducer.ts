import type {
  Budget,
  Delivery,
  Item,
  Presence,
  Proposal,
  Reply,
  Session,
  Settings,
  SseEvent,
} from '../types.ts';
import { isMood } from '../lib/labels.ts';
import { sortItems } from '../lib/items.ts';

export type ConnStatus = 'connecting' | 'open' | 'retrying' | 'auth' | 'demo';

export interface ConnState {
  status: ConnStatus;
  /** 연속 실패 횟수(0 이면 처음 연결 중). */
  attempt: number;
  /** 다음 재시도 시각(현지 ms). retrying 일 때만. */
  retryAt: number | null;
}

export interface ChatMessage {
  id: string;
  role: 'me' | 'claude';
  text: string;
  at_ms: number;
  session: string | null;
  project: string | null;
  /** 내 글의 전달 상태 */
  status?: 'sending' | 'sent' | 'undelivered' | 'error';
  deliveredTo?: string | null;
  /** delivered_to 가 null 일 때 서버가 준 이유, 또는 오류 문구 */
  reason?: string;
  chatId?: string;
  /** 보낸 뒤 그 세션에서 답(reply)이 왔다 */
  answered?: boolean;
  /** 30초 동안 답이 없어 channel 플래그를 확인하라는 안내를 보인다(§10.1) */
  noReply?: boolean;
}

/** §10.1 chat_id 형식 */
export const CHAT_ID_RE = /^[A-Za-z0-9_-]{1,64}$/;
export function isChatId(v: unknown): v is string {
  return typeof v === 'string' && CHAT_ID_RE.test(v);
}

export interface AppState {
  conn: ConnState;
  version: string | null;
  /** 서버 시계 − 현지 시계(ms). 같은 기계라 보통 0 근처. */
  clockOffsetMs: number;
  synced: boolean;
  presence: Presence;
  items: Record<string, Item>;
  /** 낙관적으로 숨긴 항목: id → 숨길 때의 signature. 서명이 바뀌면 다시 보인다. */
  hidden: Record<string, string>;
  /** "열어 봄"을 누른 항목: id → 그때의 signature. 새 신호가 오면 다시 새것으로 보인다. */
  seen: Record<string, string>;
  sessions: Session[];
  proposals: Proposal[];
  /** 낙관적으로 숨긴 제안 id */
  hiddenProposals: Record<string, true>;
  budget: Budget | null;
  settings: Settings | null;
  chat: ChatMessage[];
  /** 집중 모드·sleep 이라 말풍선 없이 들어온 Claude 답의 수(§12.4). 대화 탭을 보면 0. */
  unreadReplies: number;
  lastDelivery: Delivery | null;
  deliverySeq: number;
}

export const CHAT_LIMIT = 200;

export const EMPTY_PRESENCE: Presence = {
  mood: 'idle',
  level: 'ambient',
  focus_item: null,
  line: '',
  since_ms: 0,
  focus_mode_until_ms: null,
};

export function initialState(conn: ConnStatus = 'connecting'): AppState {
  return {
    conn: { status: conn, attempt: 0, retryAt: null },
    version: null,
    clockOffsetMs: 0,
    synced: false,
    presence: EMPTY_PRESENCE,
    items: {},
    hidden: {},
    seen: {},
    sessions: [],
    proposals: [],
    hiddenProposals: {},
    budget: null,
    settings: null,
    chat: [],
    unreadReplies: 0,
    lastDelivery: null,
    deliverySeq: 0,
  };
}

/**
 * Claude 의 답을 조용히 받을 때인가(§12.4): 집중 모드가 아직 끝나지 않았거나(서버 시계로 잰다) 기분이 sleep.
 * 그때 답은 말풍선을 띄우지 않고 대화 기록과 배지로만 보인다.
 */
export function quietReplies(state: AppState, localNowMs: number): boolean {
  if (state.presence.mood === 'sleep') return true;
  const until = state.presence.focus_mode_until_ms;
  return until !== null && until > localNowMs + state.clockOffsetMs;
}

// ── 들어온 값 검사(데몬이 이상한 값을 보내도 화면이 무너지지 않게) ─────────

function isObj(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

export function isItem(v: unknown): v is Item {
  return (
    isObj(v) &&
    typeof v.id === 'string' &&
    v.id.length > 0 &&
    typeof v.kind === 'string' &&
    typeof v.score === 'number'
  );
}

function normalizeItem(v: Item): Item {
  return {
    ...v,
    title: typeof v.title === 'string' ? v.title : '',
    body: typeof v.body === 'string' ? v.body : '',
    project: typeof v.project === 'string' ? v.project : '',
    reasons: Array.isArray(v.reasons) ? v.reasons.filter((r) => typeof r === 'string') : [],
    options: Array.isArray(v.options) ? v.options.filter((o) => typeof o === 'string') : null,
    blocking: Boolean(v.blocking),
    signature: typeof v.signature === 'string' ? v.signature : '',
    state: typeof v.state === 'string' ? v.state : 'open',
  };
}

function isOpen(item: Item): boolean {
  return item.state === 'open';
}

function normalizePresence(v: unknown, prev: Presence): Presence {
  if (!isObj(v)) return prev;
  return {
    mood: isMood(v.mood) ? v.mood : 'idle',
    level: v.level === 'nudge' || v.level === 'interrupt' ? v.level : 'ambient',
    focus_item: typeof v.focus_item === 'string' ? v.focus_item : null,
    line: typeof v.line === 'string' ? v.line : '',
    since_ms: typeof v.since_ms === 'number' ? v.since_ms : 0,
    focus_mode_until_ms: typeof v.focus_mode_until_ms === 'number' ? v.focus_mode_until_ms : null,
  };
}

function isSession(v: unknown): v is Session {
  return isObj(v) && typeof v.id === 'string';
}

function normalizeSession(v: Session): Session {
  return {
    ...v,
    project: typeof v.project === 'string' ? v.project : '',
    channel: typeof v.channel === 'string' && v.channel ? v.channel : null,
    last_ms: typeof v.last_ms === 'number' ? v.last_ms : 0,
  };
}

function isProposal(v: unknown): v is Proposal {
  return isObj(v) && typeof v.id === 'string' && typeof v.line === 'string';
}

function isDelivery(v: unknown): v is Delivery {
  return isObj(v) && typeof v.line === 'string' && typeof v.level === 'string';
}

function isReply(v: unknown): v is Reply {
  return isObj(v) && typeof v.text === 'string' && isChatId(v.chat_id);
}

/** 숨김 표 정리: 사라진 항목은 빼고, 서명이 바뀐 항목은 다시 보이게 한다. */
function pruneHidden(hidden: Record<string, string>, items: Record<string, Item>): Record<string, string> {
  let changed = false;
  const next: Record<string, string> = {};
  for (const [id, sig] of Object.entries(hidden)) {
    const it = items[id];
    if (it && it.signature === sig) next[id] = sig;
    else changed = true;
  }
  return changed ? next : hidden;
}

// ── SSE 이벤트 ──────────────────────────────────────────────────────────

/**
 * SSE 이벤트 하나를 상태에 적용한다(순수 함수).
 * @param receivedLocalMs snapshot 을 받은 현지 시각. 서버 시계와의 차이를 잰다.
 */
export function applyEvent(state: AppState, ev: SseEvent, receivedLocalMs: number = Date.now()): AppState {
  switch (ev.event) {
    case 'snapshot': {
      const s = ev.data as unknown;
      if (!isObj(s)) return state;
      const items: Record<string, Item> = {};
      if (Array.isArray(s.items)) {
        for (const raw of s.items) {
          if (!isItem(raw)) continue;
          const it = normalizeItem(raw);
          if (isOpen(it)) items[it.id] = it;
        }
      }
      const proposals = Array.isArray(s.proposals) ? s.proposals.filter(isProposal) : [];
      const proposalIds = new Set(proposals.map((p) => p.id));
      const hiddenProposals: Record<string, true> = {};
      for (const id of Object.keys(state.hiddenProposals)) if (proposalIds.has(id)) hiddenProposals[id] = true;
      return {
        ...state,
        version: typeof s.version === 'string' ? s.version : state.version,
        clockOffsetMs: typeof s.now_ms === 'number' ? s.now_ms - receivedLocalMs : state.clockOffsetMs,
        synced: true,
        presence: normalizePresence(s.presence, state.presence),
        items,
        hidden: pruneHidden(state.hidden, items),
        seen: pruneHidden(state.seen, items),
        sessions: Array.isArray(s.sessions) ? s.sessions.filter(isSession).map(normalizeSession) : [],
        proposals,
        hiddenProposals,
        budget: isObj(s.budget) ? (s.budget as unknown as Budget) : state.budget,
        settings: isObj(s.settings) ? normalizeSettings(s.settings) : state.settings,
      };
    }
    case 'item': {
      if (!isItem(ev.data)) return state;
      const it = normalizeItem(ev.data);
      if (!isOpen(it)) return removeItem(state, it.id);
      const items = { ...state.items, [it.id]: it };
      return {
        ...state,
        items,
        hidden: dropIfResigned(state.hidden, it),
        seen: dropIfResigned(state.seen, it),
      };
    }
    case 'item_removed': {
      const d = ev.data as unknown;
      if (!isObj(d) || typeof d.id !== 'string') return state;
      return removeItem(state, d.id);
    }
    case 'presence':
      return { ...state, presence: normalizePresence(ev.data, state.presence) };
    case 'deliver': {
      if (!isDelivery(ev.data)) return state;
      return { ...state, lastDelivery: ev.data, deliverySeq: state.deliverySeq + 1 };
    }
    case 'reply': {
      if (!isReply(ev.data)) return state;
      const r = ev.data;
      const id = `r:${r.chat_id ?? ''}:${r.at_ms ?? 0}`;
      if (state.chat.some((m) => m.id === id)) return state;
      const msg: ChatMessage = {
        id,
        role: 'claude',
        text: r.text,
        at_ms: typeof r.at_ms === 'number' ? r.at_ms : receivedLocalMs + state.clockOffsetMs,
        session: typeof r.session === 'string' ? r.session : null,
        project: typeof r.project === 'string' ? r.project : null,
        chatId: r.chat_id,
      };
      // 그 세션으로 보낸 내 글은 답을 받은 것으로 본다(안내를 거둔다).
      const chat = msg.session
        ? state.chat.map((m) =>
            m.role === 'me' && m.status === 'sent' && m.deliveredTo === msg.session && !m.answered
              ? { ...m, answered: true, noReply: false }
              : m,
          )
        : state.chat;
      const unreadReplies = quietReplies(state, receivedLocalMs) ? state.unreadReplies + 1 : state.unreadReplies;
      return { ...state, chat: capChat([...chat, msg]), unreadReplies };
    }
    case 'proposal': {
      if (!isProposal(ev.data)) return state;
      const p = ev.data;
      const i = state.proposals.findIndex((x) => x.id === p.id);
      const proposals = i < 0 ? [...state.proposals, p] : state.proposals.map((x, j) => (j === i ? p : x));
      return { ...state, proposals };
    }
    case 'sessions': {
      if (!Array.isArray(ev.data)) return state;
      return { ...state, sessions: ev.data.filter(isSession).map(normalizeSession) };
    }
    default:
      return state;
  }
}

function normalizeSettings(s: Record<string, unknown>): Settings {
  const num = (v: unknown, d: number) => (typeof v === 'number' && Number.isFinite(v) ? v : d);
  const strs = (v: unknown) => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []);
  return {
    nudge_min: num(s.nudge_min, 80),
    interrupt_min: num(s.interrupt_min, 95),
    stale_minutes: num(s.stale_minutes, 30),
    quiet_hours: strs(s.quiet_hours),
    quiet_kinds: strs(s.quiet_kinds) as Settings['quiet_kinds'],
    // §13.3: 기본은 꺼짐. 정확히 true 일 때만 켜졌다고 보인다.
    approval_interrupt: s.approval_interrupt === true,
  };
}

function dropIfResigned(map: Record<string, string>, it: Item): Record<string, string> {
  const sig = map[it.id];
  if (sig === undefined || sig === it.signature) return map;
  const next = { ...map };
  delete next[it.id];
  return next;
}

function removeItem(state: AppState, id: string): AppState {
  if (!(id in state.items) && !(id in state.hidden) && !(id in state.seen)) return state;
  const items = { ...state.items };
  delete items[id];
  const hidden = { ...state.hidden };
  delete hidden[id];
  const seen = { ...state.seen };
  delete seen[id];
  return { ...state, items, hidden, seen };
}

function capChat(chat: ChatMessage[]): ChatMessage[] {
  return chat.length > CHAT_LIMIT ? chat.slice(chat.length - CHAT_LIMIT) : chat;
}

// ── 얼굴 안의 동작(낙관적 UI·연결 상태·대화) ─────────────────────────────

export type LocalAction =
  | { type: 'conn'; conn: ConnState }
  | { type: 'hideItem'; id: string }
  | { type: 'unhideItem'; id: string }
  | { type: 'markSeen'; id: string }
  | { type: 'hideProposal'; id: string }
  | { type: 'unhideProposal'; id: string }
  | { type: 'removeProposal'; id: string }
  | { type: 'chatAdd'; msg: ChatMessage }
  | { type: 'chatUpdate'; id: string; patch: Partial<ChatMessage> }
  | { type: 'chatSeen' }
  | { type: 'focusUntil'; until_ms: number | null };

export type Action = { type: 'sse'; ev: SseEvent; at?: number } | LocalAction;

export function reduce(state: AppState, action: Action): AppState {
  switch (action.type) {
    case 'sse':
      return applyEvent(state, action.ev, action.at);
    case 'conn':
      return { ...state, conn: action.conn };
    case 'hideItem': {
      const it = state.items[action.id];
      if (!it) return state;
      return { ...state, hidden: { ...state.hidden, [action.id]: it.signature } };
    }
    case 'unhideItem': {
      if (!(action.id in state.hidden)) return state;
      const hidden = { ...state.hidden };
      delete hidden[action.id];
      return { ...state, hidden };
    }
    case 'markSeen': {
      const it = state.items[action.id];
      if (!it) return state;
      return { ...state, seen: { ...state.seen, [action.id]: it.signature } };
    }
    case 'hideProposal':
      return { ...state, hiddenProposals: { ...state.hiddenProposals, [action.id]: true } };
    case 'unhideProposal': {
      const hp = { ...state.hiddenProposals };
      delete hp[action.id];
      return { ...state, hiddenProposals: hp };
    }
    case 'removeProposal': {
      const hp = { ...state.hiddenProposals };
      delete hp[action.id];
      return { ...state, proposals: state.proposals.filter((p) => p.id !== action.id), hiddenProposals: hp };
    }
    case 'chatAdd':
      return { ...state, chat: capChat([...state.chat, action.msg]) };
    case 'chatUpdate':
      return {
        ...state,
        chat: state.chat.map((m) => (m.id === action.id ? { ...m, ...action.patch } : m)),
      };
    case 'chatSeen':
      return state.unreadReplies === 0 ? state : { ...state, unreadReplies: 0 };
    case 'focusUntil':
      return { ...state, presence: { ...state.presence, focus_mode_until_ms: action.until_ms } };
    default:
      return state;
  }
}

// ── 고르개(selector) ────────────────────────────────────────────────────

/** 화면에 보일 당신 차례: 숨기지 않았고, 미뤄 두지 않았고, 열린 것. 점수순. */
export function visibleItems(state: AppState, nowMs: number): Item[] {
  const out: Item[] = [];
  for (const it of Object.values(state.items)) {
    if (it.id in state.hidden) continue;
    if (it.snooze_until_ms != null && it.snooze_until_ms > nowMs) continue;
    out.push(it);
  }
  return sortItems(out);
}

export function visibleProposals(state: AppState): Proposal[] {
  return state.proposals.filter((p) => !state.hiddenProposals[p.id]);
}

/** 대화 대상 후보: channel 이 있는 세션 먼저, 그다음 최근 움직인 순. 끝난 세션은 channel 이 없으면 뺀다. */
export function chatTargets(sessions: Session[]): Session[] {
  return sessions
    .filter((s) => s.state !== 'ended' || s.channel)
    .sort((a, b) => {
      const ca = a.channel ? 1 : 0;
      const cb = b.channel ? 1 : 0;
      if (ca !== cb) return cb - ca;
      return b.last_ms - a.last_ms;
    });
}
