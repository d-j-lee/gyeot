import { describe, expect, it } from 'vitest';
import {
  agentLabel,
  ALL_KINDS,
  ALL_MOODS,
  DISMISS_REASONS,
  isTerminalOnly,
  KIND_LABELS,
  kindLabel,
  moodLabel,
  sessionStateLabel,
} from './labels.ts';

describe('종류 이름(§3)', () => {
  it('아홉 종류 모두 정해진 한국어 이름', () => {
    expect(KIND_LABELS).toEqual({
      approval: '권한 승인',
      merge: '병합',
      decision: '결정',
      failed: '실패',
      volatile: '미저장',
      stall: '멈춤 의심',
      followup: '후속',
      unread: '안 읽음',
      context: '맥락',
    });
    expect(ALL_KINDS).toHaveLength(9);
  });

  it('모르는 종류는 원래 이름 그대로', () => {
    expect(kindLabel('approval')).toBe('권한 승인');
    expect(kindLabel('brand-new')).toBe('brand-new');
    expect(kindLabel('toString')).toBe('toString');
  });

  it('승인·병합만 "터미널에서 직접"', () => {
    expect(ALL_KINDS.filter(isTerminalOnly)).toEqual(['approval', 'merge']);
  });
});

describe('기분·에이전트·세션 이름', () => {
  it('일곱 기분 모두 한국어', () => {
    expect(ALL_MOODS).toEqual(['idle', 'working', 'waiting', 'alert', 'done', 'sleep', 'listening']);
    for (const m of ALL_MOODS) expect(moodLabel(m)).toMatch(/[가-힣]/);
    expect(moodLabel('weird')).toBe(moodLabel('idle'));
  });

  it('에이전트', () => {
    expect(agentLabel('claude-code')).toBe('Claude Code');
    expect(agentLabel('codex')).toBe('Codex');
    expect(agentLabel(null)).toBe('알 수 없는 에이전트');
  });

  it('세션 상태', () => {
    expect(sessionStateLabel('waiting')).toBe('기다림');
    expect(sessionStateLabel('odd')).toBe('odd');
  });

  it('넘기기 이유는 계약의 세 가지', () => {
    expect(DISMISS_REASONS.map((r) => r.value)).toEqual(['과민', '이미 처리', '무관']);
  });
});
