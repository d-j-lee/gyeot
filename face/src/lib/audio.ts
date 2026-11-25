// 부드러운 차임. 소리 파일 없이 WebAudio 로 만든다.

type Ctx = AudioContext;
let ctx: Ctx | null = null;

function getCtx(): Ctx | null {
  if (ctx) return ctx;
  const Impl: typeof AudioContext | undefined =
    typeof window === 'undefined'
      ? undefined
      : window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Impl) return null;
  try {
    ctx = new Impl();
  } catch {
    ctx = null;
  }
  return ctx;
}

/**
 * 브라우저는 사람이 한 번 누르기 전에는 소리를 막는다.
 * 첫 클릭·키 입력 때 오디오를 깨워 둔다.
 */
export function installAudioUnlock(target: EventTarget = window): () => void {
  const unlock = () => {
    const c = getCtx();
    if (c && c.state === 'suspended') void c.resume().catch(() => {});
  };
  target.addEventListener('pointerdown', unlock, { passive: true });
  target.addEventListener('keydown', unlock);
  return () => {
    target.removeEventListener('pointerdown', unlock);
    target.removeEventListener('keydown', unlock);
  };
}

/** 두 음(E5 → A5)의 짧고 부드러운 차임. 약 0.9초. */
export async function playChime(volume = 0.14): Promise<boolean> {
  const c = getCtx();
  if (!c) return false;
  try {
    if (c.state === 'suspended') await c.resume();
  } catch {
    return false;
  }
  if (c.state !== 'running') return false;

  const t0 = c.currentTime + 0.02;
  const master = c.createGain();
  master.gain.value = volume;
  const lp = c.createBiquadFilter();
  lp.type = 'lowpass';
  lp.frequency.value = 2400;
  lp.connect(master);
  master.connect(c.destination);

  const note = (freq: number, start: number, dur: number, peak: number) => {
    for (const [type, mul, g] of [
      ['sine', 1, 1],
      ['triangle', 2, 0.12],
    ] as const) {
      const osc = c.createOscillator();
      const env = c.createGain();
      osc.type = type;
      osc.frequency.value = freq * mul;
      env.gain.setValueAtTime(0.0001, start);
      env.gain.exponentialRampToValueAtTime(peak * g, start + 0.015);
      env.gain.exponentialRampToValueAtTime(0.0001, start + dur);
      osc.connect(env);
      env.connect(lp);
      osc.start(start);
      osc.stop(start + dur + 0.05);
    }
  };
  note(659.25, t0, 0.7, 1);
  note(880.0, t0 + 0.14, 0.9, 0.8);
  setTimeout(() => master.disconnect(), 1_400);
  return true;
}
