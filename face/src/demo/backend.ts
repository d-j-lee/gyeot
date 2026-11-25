import type { DecisionResult, FocusResult, Mood, SayResult } from '../types.ts';
import type { Backend, BackendSink } from '../app/backend.ts';
import {
  CHANNEL_MISSING_REASON,
  DEMO_SESSIONS,
  demoInterrupt,
  demoNudge,
  demoReply,
  demoSnapshot,
} from './data.ts';
import { localOffsetMinutes } from '../lib/time.ts';

export type DemoScene = 'mood' | 'panel' | 'bubble' | 'nudge' | 'reply' | 'sheet';

export interface DemoOptions {
  mood: Mood;
  scene: DemoScene;
  /** 응답 지연(ms). 시험에서는 0. */
  latency?: number;
}

const wait = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** 데몬 없이 도는 가짜 백엔드. 동작은 늘 성공하고, 대화에는 짧은 답을 돌려준다. */
export class DemoBackend implements Backend {
  private sink: BackendSink | null = null;
  private timers: ReturnType<typeof setTimeout>[] = [];
  private readonly opts: DemoOptions;

  constructor(opts: DemoOptions) {
    this.opts = opts;
  }

  private get latency(): number {
    return this.opts.latency ?? 220;
  }

  start(sink: BackendSink): void {
    this.sink = sink;
    const now = Date.now();
    sink.status({ status: 'demo', attempt: 0, retryAt: null });
    sink.event({ event: 'snapshot', data: demoSnapshot(now, this.opts.mood, localOffsetMinutes(now)) });
    const { scene } = this.opts;
    if (scene === 'bubble') sink.event({ event: 'deliver', data: demoInterrupt(now) });
    if (scene === 'nudge') sink.event({ event: 'deliver', data: demoNudge(now) });
    if (scene === 'reply') sink.event({ event: 'reply', data: demoReply(now) });
    if (scene === 'panel') {
      sink.event({ event: 'reply', data: demoReply(now - 60_000) });
    }
  }

  stop(): void {
    for (const t of this.timers) clearTimeout(t);
    this.timers = [];
    this.sink = null;
  }

  retryNow(): void {}

  async itemAction(): Promise<void> {
    await wait(this.latency);
  }

  async focus(on: boolean, minutes?: number): Promise<FocusResult> {
    await wait(this.latency);
    const until = on ? Date.now() + (minutes ?? 60) * 60_000 : null;
    return { ok: true, until_ms: until };
  }

  async listening(): Promise<void> {}

  async say(text: string, session?: string | null): Promise<SayResult> {
    await wait(this.latency);
    const target = session
      ? DEMO_SESSIONS.find((s) => s.id === session)
      : DEMO_SESSIONS.find((s) => s.channel !== null);
    if (!target || !target.channel) return { delivered_to: null, reason: CHANNEL_MISSING_REASON };
    const t = setTimeout(() => {
      const r = demoReply(Date.now(), `‘${shorten(text)}’ 받았어요. 지금 하던 일을 마무리하고 바로 볼게요.`);
      this.sink?.event({ event: 'reply', data: { ...r, session: target.id, project: target.project } });
    }, 1_400);
    this.timers.push(t);
    return { delivered_to: target.id, reason: `${target.project} 세션으로 보냈어요` };
  }

  async decide(): Promise<DecisionResult> {
    await wait(this.latency);
    return { delivered_to: 'c3d95e11-dja' };
  }

  async proposal(): Promise<void> {
    await wait(this.latency);
  }
}

function shorten(text: string): string {
  const t = text.replace(/\s+/g, ' ').trim();
  return [...t].length > 24 ? `${[...t].slice(0, 23).join('')}…` : t;
}
