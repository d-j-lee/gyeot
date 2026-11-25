import type { Delivery, Item, Reply } from '../types.ts';

export type BubbleKind = 'nudge' | 'interrupt' | 'reply';

export interface Bubble {
  /** 중복 막기 열쇠(재연결 뒤 같은 전달이 다시 와도 한 번만 보인다) */
  key: string;
  kind: BubbleKind;
  line: string;
  reasons: string[];
  item: string | null;
  project: string | null;
  session: string | null;
  at_ms: number;
  speak: boolean;
}

/** 말풍선이 저절로 사라지기까지(ms). interrupt 는 사람이 넘길 때까지 남는다. */
export const BUBBLE_MS: Record<Exclude<BubbleKind, 'interrupt'>, number> = {
  nudge: 8_000,
  reply: 12_000,
};

export const MAX_QUEUE = 3;
const SEEN_LIMIT = 300;
const REPLY_LINE_MAX = 90;

/** Delivery → 말풍선. ambient 는 말풍선을 띄우지 않는다(표정만). */
export function bubbleFromDelivery(d: Delivery, item?: Item | null): Bubble | null {
  if (d.level !== 'nudge' && d.level !== 'interrupt') return null;
  const line = (d.line ?? '').trim();
  if (!line) return null;
  return {
    key: `d:${d.item ?? ''}:${d.at_ms ?? 0}:${d.level}`,
    kind: d.level,
    line,
    reasons: Array.isArray(d.reasons) ? d.reasons.filter((r) => typeof r === 'string') : [],
    item: typeof d.item === 'string' && d.item ? d.item : null,
    project: item?.project ?? null,
    session: item?.session ?? null,
    at_ms: d.at_ms ?? 0,
    speak: Boolean(d.speak),
  };
}

/** 답장의 첫 줄(비어 있지 않은 줄)을 말풍선 길이로 자른다. */
export function firstLine(text: string, max: number = REPLY_LINE_MAX): string {
  const line = (text ?? '').split(/\r?\n/).map((l) => l.trim()).find((l) => l.length > 0) ?? '';
  const chars = [...line];
  return chars.length > max ? `${chars.slice(0, max - 1).join('')}…` : line;
}

export function bubbleFromReply(r: Reply): Bubble | null {
  if (typeof r.chat_id !== 'string' || !/^[A-Za-z0-9_-]{1,64}$/.test(r.chat_id)) return null; // §10.1
  const line = firstLine(r.text);
  if (!line) return null;
  const where = r.project ? `‘${r.project}’ 세션의 Claude가 channel로 보낸 답이에요` : 'Claude가 channel로 보낸 답이에요';
  return {
    key: `r:${r.chat_id ?? ''}:${r.at_ms ?? 0}`,
    kind: 'reply',
    line,
    reasons: [where, '전체 답은 ‘대화’ 탭에 있어요'],
    item: null,
    project: r.project ?? null,
    session: r.session ?? null,
    at_ms: r.at_ms ?? 0,
    speak: false,
  };
}

/**
 * 말풍선 차례 관리.
 * - interrupt 는 지금 떠 있는 nudge·reply 를 밀어내고 사람이 넘길 때까지 남는다.
 * - nudge(8초)·reply(12초)는 저절로 사라지고, 떠 있는 것이 있으면 뒤에 줄 선다(최대 3개).
 * - 같은 항목의 새 전달은 줄을 새로 서지 않고 그 자리의 말풍선을 바꾼다.
 * - 마우스를 올리거나 "왜?"를 펼치면(hold) 시계를 멈춘다.
 */
export class BubbleQueue {
  current: Bubble | null = null;
  queue: Bubble[] = [];
  private timer: ReturnType<typeof setTimeout> | null = null;
  private remainingMs = 0;
  private startedAt = 0;
  private holds = new Set<string>();
  private seen: string[] = [];
  private seenSet = new Set<string>();
  private readonly onChange: (current: Bubble | null) => void;
  private readonly maxQueue: number;

  constructor(onChange: (current: Bubble | null) => void, opts: { maxQueue?: number } = {}) {
    this.onChange = onChange;
    this.maxQueue = opts.maxQueue ?? MAX_QUEUE;
  }

  /** 새 말풍선을 넣는다. 이미 본 것이면 false. */
  push(b: Bubble): boolean {
    if (this.seenSet.has(b.key)) return false;
    this.remember(b.key);

    // 같은 항목의 말풍선이 떠 있으면 그 자리에서 바꾼다(덜 급한 것으로 내리지는 않는다).
    if (b.item && this.current?.item === b.item) {
      const kind: BubbleKind = this.current.kind === 'interrupt' ? 'interrupt' : b.kind;
      this.show({ ...b, kind });
      return true;
    }
    if (b.item) {
      const i = this.queue.findIndex((q) => q.item === b.item);
      if (i >= 0) {
        const prev = this.queue[i]!;
        this.queue[i] = { ...b, kind: prev.kind === 'interrupt' ? 'interrupt' : b.kind };
        return true;
      }
    }

    if (!this.current) {
      this.show(b);
      return true;
    }
    if (b.kind === 'interrupt') {
      if (this.current.kind !== 'interrupt') {
        // 급한 말이 오면 지나가는 말은 내려놓는다.
        this.show(b);
      } else {
        const firstCalm = this.queue.findIndex((q) => q.kind !== 'interrupt');
        if (firstCalm < 0) this.queue.push(b);
        else this.queue.splice(firstCalm, 0, b);
        this.trim();
      }
      return true;
    }
    this.queue.push(b);
    this.trim();
    return true;
  }

  /** 지금 말풍선을 넘긴다(넘기기 단추). 다음 차례가 있으면 띄운다. */
  ack(): void {
    if (!this.current) return;
    this.next();
  }

  /** 항목이 닫혔으면 그 항목의 말풍선은 더 볼 까닭이 없다. */
  removeItem(itemId: string): void {
    this.queue = this.queue.filter((q) => q.item !== itemId);
    if (this.current?.item === itemId) this.next();
  }

  /** 시계 멈춤 사유를 켜고 끈다(hover, focus, reasons 등). */
  hold(source: string, on: boolean): void {
    const before = this.holds.size;
    if (on) this.holds.add(source);
    else this.holds.delete(source);
    if (before === 0 && this.holds.size > 0) this.pauseTimer();
    else if (before > 0 && this.holds.size === 0) this.resumeTimer();
  }

  get isHeld(): boolean {
    return this.holds.size > 0;
  }

  clear(): void {
    this.stopTimer();
    this.queue = [];
    if (this.current) {
      this.current = null;
      this.onChange(null);
    }
  }

  dispose(): void {
    this.stopTimer();
    this.queue = [];
    this.current = null;
  }

  // ── 안쪽 ──

  private remember(key: string): void {
    this.seen.push(key);
    this.seenSet.add(key);
    if (this.seen.length > SEEN_LIMIT) {
      const old = this.seen.shift();
      if (old !== undefined) this.seenSet.delete(old);
    }
  }

  private trim(): void {
    while (this.queue.length > this.maxQueue) {
      const i = this.queue.findIndex((q) => q.kind !== 'interrupt');
      this.queue.splice(i >= 0 ? i : 0, 1);
    }
  }

  private show(b: Bubble): void {
    this.stopTimer();
    this.current = b;
    if (b.kind !== 'interrupt') {
      this.remainingMs = BUBBLE_MS[b.kind];
      if (this.holds.size === 0) this.startTimer();
    }
    this.onChange(b);
  }

  private next(): void {
    this.stopTimer();
    const n = this.queue.shift() ?? null;
    if (n) this.show(n);
    else {
      this.current = null;
      this.onChange(null);
    }
  }

  private startTimer(): void {
    this.startedAt = Date.now();
    this.timer = setTimeout(() => {
      this.timer = null;
      this.next();
    }, this.remainingMs);
  }

  private stopTimer(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
  }

  private pauseTimer(): void {
    if (!this.timer) return;
    this.stopTimer();
    this.remainingMs = Math.max(0, this.remainingMs - (Date.now() - this.startedAt));
  }

  private resumeTimer(): void {
    if (this.timer || !this.current || this.current.kind === 'interrupt') return;
    // 손을 뗀 뒤 바로 사라지지 않게 최소 2초는 남긴다.
    this.remainingMs = Math.max(this.remainingMs, 2_000);
    this.startTimer();
  }
}
