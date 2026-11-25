// 음성 읽기(speechSynthesis). 한국어 목소리 가운데 가장 나은 것을 고른다.

export interface VoiceLike {
  name: string;
  lang: string;
  voiceURI: string;
  localService: boolean;
  default?: boolean;
}

export function isKorean(v: VoiceLike): boolean {
  return /^ko(\b|[-_])/i.test(v.lang) || v.lang.toLowerCase() === 'ko';
}

/**
 * 한국어 목소리 순위. 신경망(Natural·Neural) > 온라인 > Google > 그 밖.
 * Windows 의 Edge·WebView2 는 "Microsoft SunHi Online (Natural)" 같은 이름을 준다.
 */
export function voiceScore(v: VoiceLike): number {
  if (!isKorean(v)) return -1;
  let s = 0;
  if (/natural|neural/i.test(v.name)) s += 100;
  if (/online/i.test(v.name)) s += 30;
  if (/google/i.test(v.name)) s += 40;
  if (/sunhi|injoon|hyunsu|seoyeon|yuna/i.test(v.name)) s += 10;
  if (/^ko-KR$/i.test(v.lang.replace('_', '-'))) s += 5;
  if (v.default) s += 1;
  return s;
}

export function koreanVoices<T extends VoiceLike>(voices: readonly T[]): T[] {
  return voices.filter(isKorean).sort((a, b) => voiceScore(b) - voiceScore(a) || a.name.localeCompare(b.name));
}

/** 고른 목소리가 있으면 그것, 없으면 가장 나은 한국어 목소리. */
export function pickVoice<T extends VoiceLike>(voices: readonly T[], preferredURI?: string | null): T | null {
  const ko = koreanVoices(voices);
  if (preferredURI) {
    const hit = ko.find((v) => v.voiceURI === preferredURI);
    if (hit) return hit;
  }
  return ko[0] ?? null;
}

export function speechAvailable(): boolean {
  return typeof window !== 'undefined' && 'speechSynthesis' in window && typeof SpeechSynthesisUtterance !== 'undefined';
}

export function listVoices(): SpeechSynthesisVoice[] {
  if (!speechAvailable()) return [];
  try {
    return window.speechSynthesis.getVoices();
  } catch {
    return [];
  }
}

/** 목소리 목록이 늦게 채워지는 브라우저를 위해 변경을 구독한다. */
export function onVoicesChanged(cb: () => void): () => void {
  if (!speechAvailable()) return () => {};
  const synth = window.speechSynthesis;
  synth.addEventListener?.('voiceschanged', cb);
  return () => synth.removeEventListener?.('voiceschanged', cb);
}

export function speak(text: string, opts: { voiceURI?: string | null; rate?: number } = {}): boolean {
  if (!speechAvailable() || !text.trim()) return false;
  const voice = pickVoice(listVoices(), opts.voiceURI);
  const u = new SpeechSynthesisUtterance(text);
  u.lang = voice?.lang ?? 'ko-KR';
  if (voice) u.voice = voice;
  u.rate = clampRate(opts.rate ?? 1);
  try {
    window.speechSynthesis.cancel();
    window.speechSynthesis.speak(u);
    return true;
  } catch {
    return false;
  }
}

export function clampRate(rate: number): number {
  if (!Number.isFinite(rate)) return 1;
  return Math.min(1.6, Math.max(0.7, rate));
}
