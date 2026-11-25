import type { Mood } from './types.ts';
import type { TabId } from './components/Panel.tsx';
import type { DemoOptions, DemoScene } from './demo/backend.ts';
import { isMood } from './lib/labels.ts';

export interface BootConfig {
  mode: 'default' | 'overlay';
  demo: DemoOptions | null;
  tab: TabId;
  open: boolean;
  theme: 'light' | 'dark' | null;
  reduceMotion: boolean;
}

const SCENES: Record<string, { scene: DemoScene; mood: Mood }> = {
  panel: { scene: 'panel', mood: 'waiting' },
  bubble: { scene: 'bubble', mood: 'waiting' },
  nudge: { scene: 'nudge', mood: 'alert' },
  reply: { scene: 'reply', mood: 'done' },
  sheet: { scene: 'sheet', mood: 'idle' },
  '1': { scene: 'mood', mood: 'waiting' },
  true: { scene: 'mood', mood: 'waiting' },
};

/**
 * 주소의 물음표 뒤를 읽는다.
 *   mode=overlay              투명 오버레이(Tauri 창)
 *   demo=<mood>|panel|bubble|nudge|reply|sheet   데몬 없이 표본으로
 *   tab=turn|chat|settings    처음 열 탭
 *   open=1                    오버레이에서 패널을 펼친 채로
 *   theme=light|dark          색 모드 고정(기본은 시스템 설정)
 *   motion=reduce             움직임 줄이기 강제
 */
export function parseBoot(search: string): BootConfig {
  const q = new URLSearchParams(search);
  const mode = q.get('mode') === 'overlay' ? 'overlay' : 'default';
  const d = q.get('demo');
  let demo: DemoOptions | null = null;
  if (d !== null) {
    if (isMood(d)) demo = { scene: 'mood', mood: d };
    else if (SCENES[d]) demo = { ...SCENES[d]! };
    else demo = { scene: 'mood', mood: 'waiting' };
  }
  const t = q.get('tab');
  const tab: TabId = t === 'chat' || t === 'settings' ? t : 'turn';
  const theme = q.get('theme');
  return {
    mode,
    demo,
    tab,
    open: q.get('open') === '1' || demo?.scene === 'panel',
    theme: theme === 'light' || theme === 'dark' ? theme : null,
    reduceMotion: q.get('motion') === 'reduce',
  };
}
