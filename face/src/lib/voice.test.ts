import { describe, expect, it } from 'vitest';
import { clampRate, koreanVoices, pickVoice, type VoiceLike } from './voice.ts';
import { DEFAULT_PREFS, loadPrefs, savePrefs } from './prefs.ts';

const v = (name: string, lang: string, extra: Partial<VoiceLike> = {}): VoiceLike => ({
  name,
  lang,
  voiceURI: name,
  localService: true,
  ...extra,
});

const voices = [
  v('Microsoft David - English (United States)', 'en-US'),
  v('Microsoft Heami - Korean (Korean)', 'ko-KR'),
  v('Google 한국의', 'ko-KR', { localService: false }),
  v('Microsoft SunHi Online (Natural) - Korean (Korea)', 'ko-KR', { localService: false }),
  v('Yuna', 'ko_KR'),
];

describe('한국어 목소리 고르기', () => {
  it('한국어만, 신경망(Natural) > Google > 그 밖', () => {
    expect(koreanVoices(voices).map((x) => x.name)).toEqual([
      'Microsoft SunHi Online (Natural) - Korean (Korea)',
      'Google 한국의',
      'Yuna',
      'Microsoft Heami - Korean (Korean)',
    ]);
    expect(pickVoice(voices)?.name).toContain('SunHi');
  });

  it('사람이 고른 목소리가 있으면 그것', () => {
    expect(pickVoice(voices, 'Microsoft Heami - Korean (Korean)')?.name).toContain('Heami');
    expect(pickVoice(voices, 'Microsoft David - English (United States)')?.name).toContain('SunHi');
  });

  it('한국어 목소리가 없으면 null', () => {
    expect(pickVoice([voices[0]!])).toBeNull();
  });

  it('빠르기는 0.7–1.6', () => {
    expect(clampRate(3)).toBe(1.6);
    expect(clampRate(0.1)).toBe(0.7);
    expect(clampRate(Number.NaN)).toBe(1);
  });
});

describe('이 기기의 설정', () => {
  it('저장한 값을 읽고, 깨졌으면 기본값', () => {
    const m = new Map<string, string>();
    const store = { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, val: string) => void m.set(k, val) } as Storage;
    savePrefs({ sound: false, voice: true, voiceURI: 'x', rate: 1.3 }, store);
    expect(loadPrefs(store)).toEqual({ sound: false, voice: true, voiceURI: 'x', rate: 1.3 });
    m.set('gyeot-face-prefs', '{broken');
    expect(loadPrefs(store)).toEqual(DEFAULT_PREFS);
    expect(loadPrefs(null)).toEqual(DEFAULT_PREFS);
  });
});
