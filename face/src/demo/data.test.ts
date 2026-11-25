import { describe, expect, it } from 'vitest';
import { applyEvent, initialState, visibleItems } from '../state/reducer.ts';
import { ALL_KINDS, ALL_MOODS, isKind } from '../lib/labels.ts';
import { BASE_SCORE, BLOCKING_KINDS, itemId, waitBonus } from '../lib/items.ts';
import {
  DEMO_PROPOSALS,
  DEMO_SESSIONS,
  demoInterrupt,
  demoItems,
  demoNudge,
  demoReply,
  demoSnapshot,
} from './data.ts';

const NOW = Date.UTC(2026, 10, 26, 5, 30);

describe('데모 데이터가 계약을 지킨다', () => {
  const items = demoItems(NOW, 540);

  it('아홉 종류를 하나씩 담는다', () => {
    expect(items.map((i) => i.kind).sort()).toEqual([...ALL_KINDS].sort());
  });

  it('id = it_ + FNV-1a(session|kind), 겹치지 않는다', () => {
    for (const it of items) expect(it.id).toBe(itemId(it.session, it.kind));
    expect(new Set(items.map((i) => i.id)).size).toBe(items.length);
  });

  it('점수 = 기본 + 대기 가산(소수 한 자리), blocking 은 결정류만', () => {
    for (const it of items) {
      expect(isKind(it.kind)).toBe(true);
      expect(it.score).toBeCloseTo(BASE_SCORE[it.kind] + waitBonus(NOW - it.updated_ms), 1);
      expect(it.blocking).toBe(BLOCKING_KINDS.includes(it.kind));
      expect(it.reasons.length).toBeGreaterThan(0);
      for (const r of it.reasons) expect(r).toMatch(/[가-힣]/);
      expect(it.state).toBe('open');
    }
  });

  it('한 세션에 결정류(approval·merge·decision)는 하나만 열린다', () => {
    const per = new Map<string, number>();
    for (const it of items) if (it.blocking) per.set(it.session, (per.get(it.session) ?? 0) + 1);
    for (const n of per.values()) expect(n).toBe(1);
  });

  it('결정류가 열린 세션에는 unread·followup 이 없다(§3)', () => {
    const blockingSessions = new Set(items.filter((i) => i.blocking).map((i) => i.session));
    for (const it of items) {
      if (it.kind === 'unread' || it.kind === 'followup') expect(blockingSessions.has(it.session)).toBe(false);
    }
  });

  it('선택지는 decision 에만, decision_id 와 함께, 2–4개', () => {
    for (const it of items) {
      if (it.options) {
        expect(it.kind).toBe('decision');
        expect(it.decision_id).toBeTruthy();
        expect(it.options.length).toBeGreaterThanOrEqual(2);
        expect(it.options.length).toBeLessThanOrEqual(4);
      } else {
        expect(it.decision_id).toBeNull();
      }
    }
  });

  it('항목의 세션은 모두 세션 목록에 있고, 프로젝트가 맞는다', () => {
    const snap = demoSnapshot(NOW);
    for (const it of snap.items) {
      const s = snap.sessions.find((x) => x.id === it.session);
      expect(s).toBeDefined();
      expect(s!.project).toBe(it.project);
    }
    expect(snap.sessions.filter((s) => s.state === 'waiting').map((s) => s.id).sort()).toEqual(
      [...new Set(snap.items.filter((i) => i.blocking).map((i) => i.session))].sort(),
    );
  });

  it('요청한 예시 프로젝트가 들어 있다', () => {
    const approval = items.find((i) => i.kind === 'approval')!;
    expect(approval.project).toBe('agent-ops-kit');
    expect(approval.body).toBe('npm install -D vitest');
    expect(items.find((i) => i.kind === 'failed')!.project).toBe('fsmap');
    expect(items.find((i) => i.kind === 'unread')!.project).toBe('work-atlas');
  });

  it('Codex 세션에는 channel 이 없다(channel 은 Claude Code 기능)', () => {
    for (const s of DEMO_SESSIONS) if (s.agent === 'codex') expect(s.channel).toBeNull();
  });

  it('일곱 기분 모두 스냅샷을 만들 수 있고 sleep 은 집중 모드가 켜져 있다', () => {
    for (const m of ALL_MOODS) {
      const snap = demoSnapshot(NOW, m);
      expect(snap.presence.mood).toBe(m);
      expect(snap.presence.focus_mode_until_ms !== null).toBe(m === 'sleep');
    }
  });

  it('전달·답장 표본은 데모 항목을 가리킨다', () => {
    const ids = new Set(items.map((i) => i.id));
    expect(ids.has(demoInterrupt(NOW).item)).toBe(true);
    expect(demoInterrupt(NOW).level).toBe('interrupt');
    expect(ids.has(demoNudge(NOW).item)).toBe(true);
    expect(demoReply(NOW).text.length).toBeGreaterThan(0);
  });

  it('제안은 계약 모양(근거 포함)', () => {
    for (const p of DEMO_PROPOSALS) {
      expect(p.id).toMatch(/^pr_/);
      expect(p.change).toBe('quiet');
      expect(p.evidence.dismissed).toBeLessThanOrEqual(p.evidence.total);
    }
  });

  it('리듀서에 넣으면 승인 카드가 맨 위', () => {
    const s = applyEvent(initialState(), { event: 'snapshot', data: demoSnapshot(NOW) });
    const v = visibleItems(s, NOW);
    expect(v).toHaveLength(9);
    expect(v[0]!.kind).toBe('approval');
    expect(v[v.length - 1]!.kind).toBe('context');
  });
});
