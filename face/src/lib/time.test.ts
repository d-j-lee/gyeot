import { describe, expect, it } from 'vitest';
import { agoKo, calendarDateKo, clockKo, minutesUntilLocalMidnight, relativeKo, remainingKo } from './time.ts';

const SEOUL = 540;
// 2026-11-26 14:30 (서울) = 05:30 UTC
const NOW = Date.UTC(2026, 10, 26, 5, 30, 0);
const MIN = 60_000;
const HOUR = 60 * MIN;

describe('relativeKo — 기다린 시간', () => {
  it('1분 안은 "방금"', () => {
    expect(relativeKo(NOW - 59_000, NOW, SEOUL)).toBe('방금');
    expect(relativeKo(NOW + 5_000, NOW, SEOUL)).toBe('방금'); // 시계가 조금 어긋나도
  });

  it('1시간 안은 "N분째"', () => {
    expect(relativeKo(NOW - 4 * MIN, NOW, SEOUL)).toBe('4분째');
    expect(relativeKo(NOW - 4 * MIN - 59_000, NOW, SEOUL)).toBe('4분째');
    expect(relativeKo(NOW - 59 * MIN, NOW, SEOUL)).toBe('59분째');
  });

  it('같은 날이면 "N시간째"', () => {
    expect(relativeKo(NOW - 3 * HOUR - 10 * MIN, NOW, SEOUL)).toBe('3시간째');
  });

  it('어제(현지 달력)는 "어제"', () => {
    // 어제 09:00 서울
    expect(relativeKo(Date.UTC(2026, 10, 25, 0, 0, 0), NOW, SEOUL)).toBe('어제');
  });

  it('자정을 넘었어도 12시간 안이면 시간으로', () => {
    const now = Date.UTC(2026, 10, 25, 15, 30); // 11-26 00:30 서울
    const from = Date.UTC(2026, 10, 25, 13, 0); // 11-25 22:00 서울
    expect(relativeKo(from, now, SEOUL)).toBe('2시간째');
  });

  it('일주일 안은 "N일째", 그보다 오래면 날짜', () => {
    expect(relativeKo(NOW - 3 * 24 * HOUR, NOW, SEOUL)).toBe('3일째');
    expect(relativeKo(Date.UTC(2026, 10, 2, 3, 0), NOW, SEOUL)).toBe('11월 2일');
    expect(relativeKo(Date.UTC(2025, 11, 30, 3, 0), NOW, SEOUL)).toBe('2025년 12월 30일');
  });

  it('달력은 오프셋을 따른다(UTC 로는 같은 날이어도 서울에선 어제)', () => {
    const now = Date.UTC(2026, 10, 26, 0, 30); // 서울 09:30
    const from = Date.UTC(2026, 10, 25, 14, 0); // 서울 전날 23:00, UTC 로는 전날 14:00
    expect(relativeKo(from, now, SEOUL)).toBe('10시간째');
    expect(relativeKo(Date.UTC(2026, 10, 25, 1, 0), now, SEOUL)).toBe('어제');
  });
});

describe('agoKo — 지난 일', () => {
  it('"째" 대신 "전"', () => {
    expect(agoKo(NOW - 4 * MIN, NOW, SEOUL)).toBe('4분 전');
    expect(agoKo(NOW - 2 * HOUR, NOW, SEOUL)).toBe('2시간 전');
    expect(agoKo(NOW - 3 * 24 * HOUR, NOW, SEOUL)).toBe('3일 전');
    expect(agoKo(NOW, NOW, SEOUL)).toBe('방금');
  });
});

describe('remainingKo — 남은 시간', () => {
  it('분·시간으로', () => {
    expect(remainingKo(NOW + 23 * MIN, NOW)).toBe('23분 남음');
    expect(remainingKo(NOW + 22 * MIN + 1, NOW)).toBe('23분 남음'); // 올림
    expect(remainingKo(NOW + 65 * MIN, NOW)).toBe('1시간 5분 남음');
    expect(remainingKo(NOW + 3 * HOUR, NOW)).toBe('3시간 남음');
    expect(remainingKo(NOW - 1, NOW)).toBe('곧 끝나요');
  });
});

describe('시계·날짜', () => {
  it('clockKo 는 24시간제 현지 시각', () => {
    expect(clockKo(NOW, SEOUL)).toBe('14:30');
    expect(clockKo(Date.UTC(2026, 10, 25, 15, 5), SEOUL)).toBe('00:05');
  });

  it('calendarDateKo', () => {
    expect(calendarDateKo(NOW, NOW, SEOUL)).toBe('11월 26일');
  });

  it('오늘 자정까지 남은 분', () => {
    expect(minutesUntilLocalMidnight(NOW, SEOUL)).toBe(9 * 60 + 30);
    expect(minutesUntilLocalMidnight(Date.UTC(2026, 10, 26, 14, 59, 30), SEOUL)).toBe(1);
  });
});
