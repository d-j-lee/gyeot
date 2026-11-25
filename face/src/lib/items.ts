import type { Item, ItemKind } from '../types.ts';

/**
 * 당신 차례 정렬: 점수 내림차순. 같으면 막힌 것(blocking) 먼저,
 * 그다음 오래 기다린 것(created_ms 오름차순), 마지막으로 id.
 */
export function compareItems(a: Item, b: Item): number {
  const sa = Number.isFinite(a.score) ? a.score : -Infinity;
  const sb = Number.isFinite(b.score) ? b.score : -Infinity;
  if (sb !== sa) return sb - sa;
  if (a.blocking !== b.blocking) return a.blocking ? -1 : 1;
  if (a.created_ms !== b.created_ms) return a.created_ms - b.created_ms;
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

export function sortItems(items: Iterable<Item>): Item[] {
  return [...items].sort(compareItems);
}

/** 화면에 보일 점수: 소수 한 자리, 정수면 그대로. */
export function formatScore(score: number): string {
  if (!Number.isFinite(score)) return '–';
  const r = Math.round(score * 10) / 10;
  return Number.isInteger(r) ? String(r) : r.toFixed(1);
}

/** §3 종류별 기본 점수. */
export const BASE_SCORE: Record<ItemKind, number> = {
  approval: 95,
  merge: 90,
  decision: 85,
  failed: 80,
  volatile: 78,
  stall: 70,
  followup: 60,
  unread: 55,
  context: 45,
};

export const BLOCKING_KINDS: readonly ItemKind[] = ['approval', 'merge', 'decision'];

/** §3 대기 가산 = min(15, 3·log2(대기시간(시간)+1)). */
export function waitBonus(waitedMs: number): number {
  const hours = Math.max(0, waitedMs) / 3_600_000;
  return Math.min(15, 3 * Math.log2(hours + 1));
}

/** §3 감쇠 = 7일이 지나면 하루 2점, 최대 40(failed 는 감쇠 없음). */
export function decay(kind: ItemKind, ageMs: number): number {
  if (kind === 'failed') return 0;
  const days = Math.max(0, ageMs) / 86_400_000;
  if (days <= 7) return 0;
  return Math.min(40, 2 * Math.floor(days - 7));
}

/** §3 id = it_ + FNV-1a 64비트 hex(session + "|" + kind). */
export function itemId(session: string, kind: ItemKind): string {
  return `it_${fnv1a64(`${session}|${kind}`)}`;
}

export function fnv1a64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let h = 0xcbf29ce484222325n;
  const prime = 0x100000001b3n;
  const mask = 0xffffffffffffffffn;
  for (const b of bytes) {
    h ^= BigInt(b);
    h = (h * prime) & mask;
  }
  return h.toString(16).padStart(16, '0');
}

/** 본문이 길어 접어 보여야 하는지. */
export function isLongBody(body: string): boolean {
  if (!body) return false;
  const lines = body.split('\n').length;
  return body.length > 140 || lines > 3;
}
