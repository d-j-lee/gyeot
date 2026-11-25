// docs/contracts.md 의 모양을 그대로 옮긴 타입. 계약이 바뀌면 여기부터 고친다.
// 이 파일은 타입만 담는다(Node 의 타입 지우기로 mock 서버가 그대로 읽는다).

export type ItemKind =
  | 'approval'
  | 'merge'
  | 'decision'
  | 'failed'
  | 'volatile'
  | 'stall'
  | 'followup'
  | 'unread'
  | 'context';

export type Mood = 'idle' | 'working' | 'waiting' | 'alert' | 'done' | 'sleep' | 'listening';

export type Level = 'ambient' | 'nudge' | 'interrupt';

export type Agent = 'claude-code' | 'codex' | null;

/** §3 당신 차례 항목 */
export interface Item {
  id: string;
  kind: ItemKind;
  session: string;
  agent: Agent;
  project: string;
  cwd: string;
  title: string;
  body: string;
  blocking: boolean;
  score: number;
  reasons: string[];
  created_ms: number;
  updated_ms: number;
  signature: string;
  state: string;
  snooze_until_ms: number | null;
  options: string[] | null;
  decision_id: string | null;
}

/** §4 존재감 */
export interface Presence {
  mood: Mood;
  level: Level;
  focus_item: string | null;
  line: string;
  since_ms: number;
  focus_mode_until_ms: number | null;
}

/** §4 전달 */
export interface Delivery {
  item: string;
  level: Level;
  line: string;
  speak: boolean;
  reasons: string[];
  at_ms: number;
}

/** §5 제안 */
export interface Proposal {
  id: string;
  kind: ItemKind;
  change: string;
  line: string;
  evidence: { dismissed: number; total: number; window_days: number };
}

export type SessionState = 'working' | 'waiting' | 'idle' | 'ended';

/** §7 Snapshot.sessions[] */
export interface Session {
  id: string;
  agent: Agent;
  project: string;
  cwd: string;
  state: SessionState;
  last_ms: number;
  channel: string | null;
}

export interface Settings {
  nudge_min: number;
  interrupt_min: number;
  stale_minutes: number;
  quiet_hours: string[];
  quiet_kinds: ItemKind[];
}

export interface Budget {
  nudge: number;
  interrupt: number;
}

/** §7 GET /v1/state, §8 snapshot */
export interface Snapshot {
  version: string;
  now_ms: number;
  presence: Presence;
  items: Item[];
  sessions: Session[];
  proposals: Proposal[];
  budget: Budget;
  settings: Settings;
}

/** §8 reply */
export interface Reply {
  session: string | null;
  project: string | null;
  chat_id: string;
  text: string;
  at_ms: number;
}

/** §8 SSE 이벤트. data 는 JSON 을 푼 값이다. */
export type SseEvent =
  | { event: 'snapshot'; data: Snapshot }
  | { event: 'item'; data: Item }
  | { event: 'item_removed'; data: { id: string } }
  | { event: 'presence'; data: Presence }
  | { event: 'deliver'; data: Delivery }
  | { event: 'reply'; data: Reply }
  | { event: 'proposal'; data: Proposal }
  | { event: 'sessions'; data: Session[] };

export type SseEventName = SseEvent['event'];

export type ItemAction = 'open' | 'done' | 'dismiss' | 'snooze';

export type DismissReason = '과민' | '이미 처리' | '무관';

/** §7 POST /v1/say 응답 */
export interface SayResult {
  delivered_to: string | null;
  reason: string;
}

/** §7 POST /v1/decisions/{id} 응답 */
export interface DecisionResult {
  delivered_to: string | null;
}

/** §7 POST /v1/focus 응답 */
export interface FocusResult {
  ok: boolean;
  until_ms: number | null;
}
