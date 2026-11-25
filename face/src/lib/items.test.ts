import { describe, expect, it } from 'vitest';
import type { Item } from '../types.ts';
import { compareItems, decay, fnv1a64, formatScore, isLongBody, itemId, sortItems, waitBonus } from './items.ts';

function it_(id: string, score: number, over: Partial<Item> = {}): Item {
  return {
    id,
    kind: 'unread',
    session: 's',
    agent: 'codex',
    project: 'p',
    cwd: '',
    title: '',
    body: '',
    blocking: false,
    score,
    reasons: [],
    created_ms: 1000,
    updated_ms: 1000,
    signature: '',
    state: 'open',
    snooze_until_ms: null,
    options: null,
    decision_id: null,
    ...over,
  };
}

describe('정렬', () => {
  it('점수 내림차순', () => {
    const out = sortItems([it_('a', 55), it_('b', 95.3), it_('c', 80)]);
    expect(out.map((i) => i.id)).toEqual(['b', 'c', 'a']);
  });

  it('점수가 같으면 막힌 것, 그다음 오래 기다린 것, 그다음 id', () => {
    const out = sortItems([
      it_('d', 80, { created_ms: 5 }),
      it_('c', 80, { created_ms: 1 }),
      it_('b', 80, { blocking: true, created_ms: 9 }),
      it_('a', 80, { created_ms: 5 }),
    ]);
    expect(out.map((i) => i.id)).toEqual(['b', 'c', 'a', 'd']);
  });

  it('숫자가 아닌 점수는 맨 뒤', () => {
    const out = sortItems([it_('nan', Number.NaN), it_('x', 1)]);
    expect(out.map((i) => i.id)).toEqual(['x', 'nan']);
    expect(compareItems(it_('a', 1), it_('a', 1))).toBe(0);
  });

  it('원래 배열은 바꾸지 않는다', () => {
    const arr = [it_('a', 1), it_('b', 2)];
    sortItems(arr);
    expect(arr.map((i) => i.id)).toEqual(['a', 'b']);
  });
});

describe('점수 식(§3)', () => {
  it('대기 가산 = min(15, 3·log2(시간+1))', () => {
    expect(waitBonus(0)).toBe(0);
    expect(waitBonus(4 * 60_000)).toBeCloseTo(0.279, 3);
    expect(waitBonus(3_600_000)).toBeCloseTo(3, 6);
    expect(waitBonus(1000 * 3_600_000)).toBe(15);
  });

  it('감쇠: 7일 뒤 하루 2점, 최대 40, failed 는 없음', () => {
    const day = 86_400_000;
    expect(decay('unread', 7 * day)).toBe(0);
    expect(decay('unread', 10 * day)).toBe(6);
    expect(decay('unread', 100 * day)).toBe(40);
    expect(decay('failed', 100 * day)).toBe(0);
  });

  it('점수 표시는 소수 한 자리', () => {
    expect(formatScore(95.33)).toBe('95.3');
    expect(formatScore(85)).toBe('85');
    expect(formatScore(Number.NaN)).toBe('–');
  });
});

describe('항목 id(§3, FNV-1a 64)', () => {
  it('알려진 시험 벡터와 맞다', () => {
    expect(fnv1a64('')).toBe('cbf29ce484222325');
    expect(fnv1a64('a')).toBe('af63dc4c8601ec8c');
    expect(fnv1a64('foobar')).toBe('85944171f73967e8');
  });

  it('it_ + hex16, 세션·종류마다 다르다', () => {
    const a = itemId('abc123', 'approval');
    expect(a).toMatch(/^it_[0-9a-f]{16}$/);
    expect(a).toBe(`it_${fnv1a64('abc123|approval')}`);
    expect(itemId('abc123', 'merge')).not.toBe(a);
  });
});

describe('긴 본문', () => {
  it('140자 넘거나 네 줄 이상이면 접는다', () => {
    expect(isLongBody('짧다')).toBe(false);
    expect(isLongBody('가'.repeat(141))).toBe(true);
    expect(isLongBody('1\n2\n3\n4')).toBe(true);
    expect(isLongBody('')).toBe(false);
  });
});
