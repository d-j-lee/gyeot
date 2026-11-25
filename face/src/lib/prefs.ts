// 이 기기에서만 쓰는 얼굴 설정(소리·음성). 데몬 설정(config.toml)과 별개다.
// 대화 본문은 저장하지 않는다.

export interface Prefs {
  sound: boolean;
  voice: boolean;
  voiceURI: string | null;
  rate: number;
}

export const DEFAULT_PREFS: Prefs = { sound: true, voice: false, voiceURI: null, rate: 1 };

const KEY = 'gyeot-face-prefs';

export function loadPrefs(storage: Storage | null = safeLocal()): Prefs {
  try {
    const raw = storage?.getItem(KEY);
    if (!raw) return { ...DEFAULT_PREFS };
    const v = JSON.parse(raw) as Partial<Prefs>;
    return {
      sound: typeof v.sound === 'boolean' ? v.sound : DEFAULT_PREFS.sound,
      voice: typeof v.voice === 'boolean' ? v.voice : DEFAULT_PREFS.voice,
      voiceURI: typeof v.voiceURI === 'string' ? v.voiceURI : null,
      rate: typeof v.rate === 'number' && Number.isFinite(v.rate) ? v.rate : DEFAULT_PREFS.rate,
    };
  } catch {
    return { ...DEFAULT_PREFS };
  }
}

export function savePrefs(p: Prefs, storage: Storage | null = safeLocal()): void {
  try {
    storage?.setItem(KEY, JSON.stringify(p));
  } catch {
    /* 저장 못 해도 이번 실행에서는 쓴다 */
  }
}

function safeLocal(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}
