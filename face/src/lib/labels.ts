import type { Agent, ItemKind, Mood, SessionState } from '../types.ts';

export const KIND_LABELS: Record<ItemKind, string> = {
  approval: '권한 승인',
  merge: '병합',
  decision: '결정',
  failed: '실패',
  volatile: '미저장',
  stall: '멈춤 의심',
  followup: '후속',
  unread: '안 읽음',
  context: '맥락',
};

export const ALL_KINDS = Object.keys(KIND_LABELS) as ItemKind[];

export function isKind(v: unknown): v is ItemKind {
  return typeof v === 'string' && Object.prototype.hasOwnProperty.call(KIND_LABELS, v);
}

/** 모르는 종류가 와도 화면이 깨지지 않게 원래 이름을 그대로 보인다. */
export function kindLabel(kind: string): string {
  return isKind(kind) ? KIND_LABELS[kind] : kind;
}

/**
 * 곁이 대신 승인하지 않는 종류(G-005). 이 카드에는 승인 버튼을 두지 않고
 * "승인은 터미널에서 직접 하세요"를 보인다.
 */
export function isTerminalOnly(kind: string): boolean {
  return kind === 'approval' || kind === 'merge';
}

export const TERMINAL_ONLY_NOTE = '승인은 터미널에서 직접 하세요';

export const MOOD_LABELS: Record<Mood, string> = {
  idle: '지켜보는 중',
  working: '에이전트가 작업 중',
  waiting: '당신 차례가 있어요',
  alert: '실패가 생겼어요',
  done: '방금 끝났어요',
  sleep: '조용히 있는 중',
  listening: '듣고 있어요',
};

export const ALL_MOODS = Object.keys(MOOD_LABELS) as Mood[];

export function isMood(v: unknown): v is Mood {
  return typeof v === 'string' && Object.prototype.hasOwnProperty.call(MOOD_LABELS, v);
}

export function moodLabel(mood: string): string {
  return isMood(mood) ? MOOD_LABELS[mood] : MOOD_LABELS.idle;
}

export function agentLabel(agent: Agent | string | null | undefined): string {
  if (agent === 'claude-code') return 'Claude Code';
  if (agent === 'codex') return 'Codex';
  return '알 수 없는 에이전트';
}

export const SESSION_STATE_LABELS: Record<SessionState, string> = {
  working: '작업 중',
  waiting: '기다림',
  idle: '쉼',
  ended: '끝남',
};

export function sessionStateLabel(state: string): string {
  return (SESSION_STATE_LABELS as Record<string, string>)[state] ?? state;
}

export const DISMISS_REASONS = [
  { value: '과민', hint: '이런 알림이 너무 잦아요' },
  { value: '이미 처리', hint: '벌써 해결했어요' },
  { value: '무관', hint: '나와 상관없는 일이에요' },
] as const;
