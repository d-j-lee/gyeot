// 사람에게 보이는 시각 문구. 계약의 시각은 모두 UTC ms 이고, 화면에서만 현지 시각으로 바꾼다.
// 결정적으로 시험할 수 있게 지금 시각과 UTC 오프셋(분)을 인자로 받는다.

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

/** 브라우저의 현지 UTC 오프셋(분, 동쪽이 +). 서울은 540. */
export function localOffsetMinutes(atMs: number = Date.now()): number {
  return -new Date(atMs).getTimezoneOffset();
}

/** 현지 달력 날짜의 일련번호(1970-01-01 = 0). */
function localDayIndex(ms: number, offsetMin: number): number {
  return Math.floor((ms + offsetMin * MIN) / DAY);
}

/**
 * 기다린 시간을 한국어로: "방금", "4분째", "3시간째", "어제", "3일째", "11월 2일".
 * - 1분 안: 방금
 * - 1시간 안: N분째
 * - 같은 날이거나 12시간 안: N시간째
 * - 어제(현지 달력): 어제
 * - 7일 안: N일째
 * - 그보다 오래: M월 D일 (해가 다르면 YYYY년 M월 D일)
 */
export function relativeKo(fromMs: number, nowMs: number, offsetMin: number = localOffsetMinutes(nowMs)): string {
  const diff = nowMs - fromMs;
  if (!Number.isFinite(diff) || diff < MIN) return '방금';
  if (diff < HOUR) return `${Math.floor(diff / MIN)}분째`;
  const dayGap = localDayIndex(nowMs, offsetMin) - localDayIndex(fromMs, offsetMin);
  if (dayGap === 0 || diff < 12 * HOUR) return `${Math.floor(diff / HOUR)}시간째`;
  if (dayGap === 1) return '어제';
  if (dayGap < 7) return `${dayGap}일째`;
  return calendarDateKo(fromMs, nowMs, offsetMin);
}

function parts(ms: number, offsetMin: number) {
  const d = new Date(ms + offsetMin * MIN);
  return {
    y: d.getUTCFullYear(),
    mo: d.getUTCMonth() + 1,
    d: d.getUTCDate(),
    h: d.getUTCHours(),
    mi: d.getUTCMinutes(),
  };
}

export function calendarDateKo(ms: number, nowMs: number, offsetMin: number = localOffsetMinutes(ms)): string {
  const a = parts(ms, offsetMin);
  const b = parts(nowMs, offsetMin);
  return a.y === b.y ? `${a.mo}월 ${a.d}일` : `${a.y}년 ${a.mo}월 ${a.d}일`;
}

/** "14:05" 처럼 24시간제 현지 시각. */
export function clockKo(ms: number, offsetMin: number = localOffsetMinutes(ms)): string {
  const p = parts(ms, offsetMin);
  return `${String(p.h).padStart(2, '0')}:${String(p.mi).padStart(2, '0')}`;
}

/** 남은 시간: "곧 끝나요", "23분 남음", "1시간 5분 남음", "3시간 남음". */
export function remainingKo(untilMs: number, nowMs: number): string {
  const left = untilMs - nowMs;
  if (!Number.isFinite(left) || left <= 0) return '곧 끝나요';
  const totalMin = Math.ceil(left / MIN);
  if (totalMin < 60) return `${totalMin}분 남음`;
  const h = Math.floor(totalMin / 60);
  const m = totalMin % 60;
  return m === 0 ? `${h}시간 남음` : `${h}시간 ${m}분 남음`;
}

/** 오늘(현지) 자정까지 남은 분. 집중 모드 "오늘" 단추에 쓴다. 최소 1분. */
export function minutesUntilLocalMidnight(nowMs: number, offsetMin: number = localOffsetMinutes(nowMs)): number {
  const nextDayStart = (localDayIndex(nowMs, offsetMin) + 1) * DAY - offsetMin * MIN;
  return Math.max(1, Math.ceil((nextDayStart - nowMs) / MIN));
}

/** 지난 일: "방금", "4분 전", "3시간 전", "어제", "3일 전", "11월 2일". */
export function agoKo(fromMs: number, nowMs: number, offsetMin: number = localOffsetMinutes(nowMs)): string {
  return relativeKo(fromMs, nowMs, offsetMin).replace(/째$/, ' 전');
}
