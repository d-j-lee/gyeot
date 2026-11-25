// 데몬 없이 화면을 보고 시험하기 위한 표본 데이터. 얼굴의 ?demo= 와 mock 서버가 함께 쓴다.
// Node 가 그대로 읽을 수 있게 타입은 import type 으로만, 값은 .ts 경로로 가져온다.
import type { Delivery, Item, ItemKind, Mood, Presence, Proposal, Reply, Session, Settings, Snapshot } from '../types.ts';
import { BASE_SCORE, BLOCKING_KINDS, itemId, waitBonus } from '../lib/items.ts';

const MIN = 60_000;
const HOUR = 60 * MIN;

export interface DemoSessionDef {
  id: string;
  agent: 'claude-code' | 'codex';
  project: string;
  channel: string | null;
}

export const DEMO_SESSIONS: readonly DemoSessionDef[] = [
  { id: 'a1f0c2d9-aok', agent: 'claude-code', project: 'agent-ops-kit', channel: 'ch_aok01' },
  { id: 'b7e41a03-gyt', agent: 'claude-code', project: 'gyeot', channel: 'ch_gyt02' },
  { id: 'c3d95e11-dja', agent: 'claude-code', project: 'dj-assis', channel: 'ch_dja03' },
  { id: 'd88b2f47-fsm', agent: 'claude-code', project: 'fsmap', channel: null },
  { id: 'e2a6c930-wat', agent: 'codex', project: 'work-atlas', channel: null },
  { id: 'f51d7b08-inf', agent: 'claude-code', project: 'infra-scripts', channel: null },
];

interface ItemDef {
  session: string;
  kind: ItemKind;
  title: string;
  body: string;
  /** 지금부터 몇 ms 전에 열렸나 */
  ago: number;
  reason0: string;
  options?: string[];
  decision_id?: string;
}

function startOfLocalDay(now: number, offsetMin: number): number {
  const day = 86_400_000;
  return Math.floor((now + offsetMin * MIN) / day) * day - offsetMin * MIN;
}

function itemDefs(now: number, offsetMin: number): ItemDef[] {
  // 어제 17:40(현지)에 끝난 Codex 턴 — "어제"로 보인다.
  const yesterdayEvening = startOfLocalDay(now, offsetMin) - 24 * HOUR + 17 * HOUR + 40 * MIN;
  return [
    {
      session: 'a1f0c2d9-aok',
      kind: 'approval',
      title: '권한 승인 대기 · Bash',
      body: 'npm install -D vitest',
      ago: 4 * MIN + 10_000,
      reason0: '권한 승인 대기: 기본 95',
    },
    {
      session: 'b7e41a03-gyt',
      kind: 'merge',
      title: '병합 승인 대기 · Bash',
      body: 'gh pr merge 128 --squash --delete-branch',
      ago: 9 * MIN,
      reason0: '병합 명령 승인 대기: 기본 90',
    },
    {
      session: 'c3d95e11-dja',
      kind: 'decision',
      title: '결정 요청 · 마이그레이션',
      body: '마이그레이션 0042를 지금 적용할까요, 아니면 리뷰가 끝난 뒤로 미룰까요?',
      ago: 2 * MIN + 30_000,
      reason0: 'Claude가 곁에 결정을 요청함: 기본 85',
      options: ['지금 적용', '리뷰 뒤에', '취소'],
      decision_id: 'dc_5b1e9a',
    },
    {
      session: 'd88b2f47-fsm',
      kind: 'failed',
      title: '게이트 실패 · fsmap-harness',
      body: 'gate → block (1,842ms). 지도와 맞지 않는 경로 2개가 있어 커밋이 막혔어요.',
      ago: 12 * MIN,
      reason0: '게이트 결과 block: 기본 80',
    },
    {
      session: 'b7e41a03-gyt',
      kind: 'volatile',
      title: '커밋 안 된 변경 · push',
      body: 'push-uncommitted: 커밋하지 않은 파일 3개를 둔 채 push 했어요.',
      ago: 7 * MIN,
      reason0: '게이트 결과 push-uncommitted: 기본 78',
    },
    {
      session: 'f51d7b08-inf',
      kind: 'stall',
      title: '멈춤 의심 · 35분째 조용함',
      body: '작업 중이던 세션에서 35분 동안 아무 신호가 없어요. 마지막 도구는 Bash였어요.',
      ago: 5 * MIN,
      reason0: '작업 중인데 30분 넘게 신호 없음: 기본 70',
    },
    {
      session: 'e2a6c930-wat',
      kind: 'unread',
      title: '안 읽은 결과 · Codex',
      body:
        '인덱스 재구성을 마쳤어요. 412개 파일을 다시 읽었고 경고가 2건 있어요.\n' +
        '1) docs/README.md 가 비어 있어요.\n' +
        '2) legacy/ 아래 심볼릭 링크 하나가 끊겼어요(legacy/assets → ../assets-old).\n' +
        '요약은 notes/reindex.md 에 적어 두었어요. 다음에는 캐시를 지우고 전체 시험을 다시 돌리는 게 좋겠어요.',
      ago: Math.max(now - yesterdayEvening, 13 * HOUR),
      reason0: '결과를 아직 읽지 않음: 기본 55',
    },
    {
      session: 'd88b2f47-fsm',
      kind: 'followup',
      title: '사람 후속 · 확인 필요',
      body: 'PR #57 스냅샷 갱신이 맞는지 직접 확인이 필요해요(your review 1건 남음).',
      ago: 48 * MIN,
      reason0: '마지막 답에 사람 후속 표현: 기본 60',
    },
    {
      session: 'e2a6c930-wat',
      kind: 'context',
      title: '맥락 압축 · auto',
      body: '자동 맥락 압축이 일어났어요. 앞선 지시가 요약됐을 수 있어요.',
      ago: 3 * HOUR,
      reason0: '자동 맥락 압축: 기본 45',
    },
  ];
}

function waitedText(ms: number): string {
  const m = Math.floor(ms / MIN);
  if (m < 60) return `${m}분`;
  return `${Math.floor(m / 60)}시간`;
}

function sessionOf(id: string): DemoSessionDef {
  const s = DEMO_SESSIONS.find((x) => x.id === id);
  if (!s) throw new Error(`demo session ${id} 없음`);
  return s;
}

export function cwdOf(project: string): string {
  return `C:\\work\\${project}`;
}

export function demoItems(now: number, offsetMin = 540): Item[] {
  return itemDefs(now, offsetMin).map((d) => {
    const s = sessionOf(d.session);
    const created = now - d.ago;
    const bonus = waitBonus(d.ago);
    const score = Math.round((BASE_SCORE[d.kind] + bonus) * 10) / 10;
    return {
      id: itemId(d.session, d.kind),
      kind: d.kind,
      session: d.session,
      agent: s.agent,
      project: s.project,
      cwd: cwdOf(s.project),
      title: d.title,
      body: d.body,
      blocking: BLOCKING_KINDS.includes(d.kind),
      score,
      reasons: [d.reason0, `${waitedText(d.ago)} 기다림: +${bonus.toFixed(1)}`],
      created_ms: created,
      updated_ms: created,
      signature: `${d.kind}:${created}`,
      state: 'open',
      snooze_until_ms: null,
      options: d.options ?? null,
      decision_id: d.decision_id ?? null,
    };
  });
}

export function demoSessions(now: number, items: readonly Item[] = demoItems(now)): Session[] {
  return DEMO_SESSIONS.map((s, i) => {
    const mine = items.filter((it) => it.session === s.id);
    const waiting = mine.some((it) => it.blocking);
    const working = s.id === 'f51d7b08-inf' || s.id === 'a1f0c2d9-aok';
    return {
      id: s.id,
      agent: s.agent,
      project: s.project,
      cwd: cwdOf(s.project),
      state: waiting ? 'waiting' : working ? 'working' : 'idle',
      last_ms: now - (i + 1) * 3 * MIN,
      channel: s.channel,
    };
  });
}

export const DEMO_LINES: Record<Mood, string> = {
  idle: '',
  working: 'agent-ops-kit에서 작업 중이에요',
  waiting: '권한 승인 1건이 기다려요',
  alert: 'fsmap 게이트가 실패했어요',
  done: 'gyeot 턴이 끝났어요',
  sleep: '집중 모드예요. 급한 것만 알릴게요',
  listening: '듣고 있어요',
};

export function demoPresence(now: number, mood: Mood): Presence {
  const items = demoItems(now);
  const focus = items.find((it) => it.kind === (mood === 'alert' ? 'failed' : 'approval')) ?? null;
  return {
    mood,
    level: mood === 'waiting' || mood === 'alert' ? 'nudge' : 'ambient',
    focus_item: mood === 'waiting' || mood === 'alert' ? (focus?.id ?? null) : null,
    line: DEMO_LINES[mood],
    since_ms: now - 20_000,
    focus_mode_until_ms: mood === 'sleep' ? now + 45 * MIN : null,
  };
}

export const DEMO_PROPOSALS: readonly Proposal[] = [
  {
    id: 'pr_unread_quiet',
    kind: 'unread',
    change: 'quiet',
    line: '최근 ‘안 읽은 결과’ 알림을 7번 중 6번 넘기셨어요. 이 종류는 앞으로 조용히(표정만) 알릴까요?',
    evidence: { dismissed: 6, total: 7, window_days: 14 },
  },
  {
    id: 'pr_context_quiet',
    kind: 'context',
    change: 'quiet',
    line: '‘맥락 압축’ 알림을 5번 모두 과민으로 넘기셨어요. 이 종류도 표정으로만 알릴까요?',
    evidence: { dismissed: 5, total: 5, window_days: 14 },
  },
];

export const DEMO_SETTINGS: Settings = {
  nudge_min: 80,
  interrupt_min: 95,
  stale_minutes: 30,
  quiet_hours: ['22:00-08:00'],
  quiet_kinds: ['context'],
  approval_interrupt: false,
};

export function demoSnapshot(now: number, mood: Mood = 'waiting', offsetMin = 540): Snapshot {
  const items = demoItems(now, offsetMin);
  return {
    version: '0.1.0',
    now_ms: now,
    presence: demoPresence(now, mood),
    items,
    sessions: demoSessions(now, items),
    proposals: DEMO_PROPOSALS.map((p) => ({ ...p, evidence: { ...p.evidence } })),
    budget: { nudge: 3, interrupt: 1 },
    settings: { ...DEMO_SETTINGS, quiet_hours: [...DEMO_SETTINGS.quiet_hours], quiet_kinds: [...DEMO_SETTINGS.quiet_kinds] },
  };
}

export function demoInterrupt(now: number): Delivery {
  const it = demoItems(now).find((x) => x.kind === 'approval')!;
  return {
    item: it.id,
    level: 'interrupt',
    line: 'agent-ops-kit에서 권한 승인을 4분째 기다려요',
    speak: true,
    reasons: [
      '점수 95.3이 interrupt_min 95 이상이에요',
      '막힌 일(blocking)이고 2분 넘게 기다렸어요',
      '그 세션에 최근 60초 동안 입력이 없었어요',
      'interrupt 예산이 1개 남아 있었어요',
    ],
    at_ms: now,
  };
}

export function demoNudge(now: number): Delivery {
  const it = demoItems(now).find((x) => x.kind === 'failed')!;
  return {
    item: it.id,
    level: 'nudge',
    line: 'fsmap 게이트가 block으로 멈췄어요',
    speak: false,
    reasons: ['점수 80.8이 nudge_min 80 이상이에요', '막힌 일은 아니라서 소리 없이 알려요'],
    at_ms: now,
  };
}

export function demoReply(now: number, text?: string): Reply {
  return {
    session: 'a1f0c2d9-aok',
    project: 'agent-ops-kit',
    chat_id: `c_${Math.floor(now / 1000).toString(36)}`,
    text:
      text ??
      'vitest 설치 승인이 나면 바로 시험을 돌릴게요.\n지금은 터미널에서 권한 승인을 기다리는 중이에요. 끝나면 실패한 것만 추려서 알려 드릴게요.',
    at_ms: now,
  };
}

export const CHANNEL_MISSING_REASON =
  'channel 없음 — 세션을 --dangerously-load-development-channels server:gyeot 로 시작하세요';
