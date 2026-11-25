import { useEffect, useState } from 'preact/hooks';

/** 값 하나와 구독. 상태 라이브러리 없이 쓴다. */
export class Observable<T> {
  private value: T;
  private listeners = new Set<(v: T) => void>();

  constructor(initial: T) {
    this.value = initial;
  }

  get(): T {
    return this.value;
  }

  set(next: T): void {
    if (Object.is(next, this.value)) return;
    this.value = next;
    for (const fn of [...this.listeners]) fn(next);
  }

  update(fn: (prev: T) => T): void {
    this.set(fn(this.value));
  }

  subscribe(fn: (v: T) => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }
}

export function useObservable<T>(obs: Observable<T>): T {
  const [v, setV] = useState(() => obs.get());
  useEffect(() => {
    setV(obs.get());
    return obs.subscribe(setV);
  }, [obs]);
  return v;
}

/** 일정 간격으로 다시 그리기 위한 지금 시각(서버 시계에 맞춘 값). */
export function useNow(offsetMs: number, intervalMs = 15_000): number {
  const [now, setNow] = useState(() => Date.now() + offsetMs);
  useEffect(() => {
    setNow(Date.now() + offsetMs);
    const t = setInterval(() => setNow(Date.now() + offsetMs), intervalMs);
    return () => clearInterval(t);
  }, [offsetMs, intervalMs]);
  return now;
}
