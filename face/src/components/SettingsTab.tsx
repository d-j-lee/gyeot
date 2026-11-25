import { useEffect, useState } from 'preact/hooks';
import type { Proposal, Settings } from '../types.ts';
import type { Prefs } from '../lib/prefs.ts';
import { kindLabel } from '../lib/labels.ts';
import { clockKo, minutesUntilLocalMidnight, remainingKo } from '../lib/time.ts';
import { koreanVoices, listVoices, onVoicesChanged, speechAvailable } from '../lib/voice.ts';

export const FOCUS_PRESETS = [
  { id: 'off', label: '끄기' },
  { id: '30', label: '30분', minutes: 30 },
  { id: '60', label: '1시간', minutes: 60 },
  { id: '120', label: '2시간', minutes: 120 },
  { id: 'today', label: '오늘' },
] as const;

export type FocusPresetId = (typeof FOCUS_PRESETS)[number]['id'];

export function presetMinutes(id: FocusPresetId, nowMs: number): number | null {
  if (id === 'off') return null;
  if (id === 'today') return minutesUntilLocalMidnight(nowMs);
  return Number(id);
}

export interface SettingsTabProps {
  now: number;
  focusUntil: number | null;
  prefs: Prefs;
  proposals: Proposal[];
  settings: Settings | null;
  version: string | null;
  onFocus: (minutes: number | null) => void;
  onPrefs: (patch: Partial<Prefs>) => void;
  onProposal: (id: string, accept: boolean) => void;
  onTestChime: () => void;
  onTestVoice: () => void;
}

function Switch({ id, checked, onChange, label }: { id: string; checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button
      type="button"
      id={id}
      role="switch"
      aria-checked={checked}
      aria-label={label}
      class={`switch${checked ? ' switch--on' : ''}`}
      onClick={() => onChange(!checked)}
    >
      <span class="switch__knob" aria-hidden="true" />
    </button>
  );
}

function useKoreanVoices() {
  const [voices, setVoices] = useState(() => koreanVoices(listVoices()));
  useEffect(() => onVoicesChanged(() => setVoices(koreanVoices(listVoices()))), []);
  return voices;
}

function quietHoursText(list: string[]): string {
  if (list.length === 0) return '없음';
  return list.map((r) => r.replace('-', '–')).join(', ');
}

export function SettingsTab(p: SettingsTabProps) {
  const voices = useKoreanVoices();
  const [lastPreset, setLastPreset] = useState<FocusPresetId | null>(null);
  const focusOn = p.focusUntil !== null && p.focusUntil > p.now;
  const canSpeak = speechAvailable();
  const chosenVoice = voices.find((v) => v.voiceURI === p.prefs.voiceURI) ?? voices[0] ?? null;

  return (
    <div class="settings">
      <section class="section" aria-labelledby="set-focus">
        <h3 class="section__title" id="set-focus">
          집중 모드
        </h3>
        <p class="section__desc">켜 두면 급한 승인만 말풍선으로 알리고, 소리와 음성은 내지 않아요.</p>
        <div class="segmented" role="group" aria-label="집중 모드 길이">
          {FOCUS_PRESETS.map((f) => {
            const pressed = f.id === 'off' ? !focusOn : focusOn && lastPreset === f.id;
            return (
              <button
                key={f.id}
                type="button"
                class="segmented__btn"
                aria-pressed={pressed}
                onClick={() => {
                  setLastPreset(f.id === 'off' ? null : f.id);
                  p.onFocus(presetMinutes(f.id, p.now));
                }}
              >
                {f.label}
              </button>
            );
          })}
        </div>
        <p class="section__status" aria-live="polite">
          {focusOn ? (
            <>
              <span class="dot dot--focus" aria-hidden="true" /> {clockKo(p.focusUntil!)}까지 켜짐 · {remainingKo(p.focusUntil!, p.now)}
            </>
          ) : (
            '꺼져 있어요'
          )}
        </p>
      </section>

      <section class="section" aria-labelledby="set-sound">
        <h3 class="section__title" id="set-sound">
          소리와 음성
        </h3>
        <div class="row">
          <div class="row__text">
            <label class="row__label" for="pref-sound">
              차임
            </label>
            <p class="row__desc">급한 알림(interrupt)에 부드러운 차임을 울려요.</p>
          </div>
          <button type="button" class="btn btn--sm btn--ghost" onClick={p.onTestChime}>
            들어 보기
          </button>
          <Switch id="pref-sound" checked={p.prefs.sound} onChange={(v) => p.onPrefs({ sound: v })} label="차임" />
        </div>

        <div class="row">
          <div class="row__text">
            <label class="row__label" for="pref-voice">
              음성 읽기
            </label>
            <p class="row__desc">급한 알림의 문장을 한국어 목소리로 읽어요.</p>
          </div>
          <button type="button" class="btn btn--sm btn--ghost" onClick={p.onTestVoice} disabled={!canSpeak || voices.length === 0}>
            들어 보기
          </button>
          <Switch id="pref-voice" checked={p.prefs.voice} onChange={(v) => p.onPrefs({ voice: v })} label="음성 읽기" />
        </div>

        <div class={`subrows${p.prefs.voice ? '' : ' subrows--off'}`}>
          {!canSpeak || voices.length === 0 ? (
            <p class="inline-hint">
              한국어 목소리를 찾지 못했어요. Windows 설정 › 시간 및 언어 › 음성에서 한국어 음성을 추가하세요.
            </p>
          ) : (
            <>
              <div class="field">
                <label for="pref-voice-name">목소리</label>
                <select
                  id="pref-voice-name"
                  class="select"
                  value={chosenVoice?.voiceURI ?? ''}
                  onChange={(e) => p.onPrefs({ voiceURI: (e.currentTarget as HTMLSelectElement).value || null })}
                >
                  {voices.map((v, i) => (
                    <option key={v.voiceURI} value={v.voiceURI}>
                      {v.name}
                      {i === 0 ? ' (추천)' : ''}
                    </option>
                  ))}
                </select>
              </div>
              <div class="field">
                <label for="pref-rate">빠르기</label>
                <div class="range">
                  <input
                    id="pref-rate"
                    type="range"
                    min="0.7"
                    max="1.6"
                    step="0.1"
                    value={p.prefs.rate}
                    aria-valuetext={`${p.prefs.rate.toFixed(1)}배`}
                    onInput={(e) => p.onPrefs({ rate: Number((e.currentTarget as HTMLInputElement).value) })}
                  />
                  <output for="pref-rate">{p.prefs.rate.toFixed(1)}배</output>
                </div>
              </div>
            </>
          )}
        </div>
      </section>

      <section class="section" aria-labelledby="set-proposals">
        <h3 class="section__title" id="set-proposals">
          곁의 제안
        </h3>
        <p class="section__desc">곁은 설정을 스스로 바꾸지 않아요. 넘기기 기록을 보고 이렇게 제안만 해요.</p>
        {p.proposals.length === 0 ? (
          <p class="muted small">지금은 제안이 없어요.</p>
        ) : (
          <ul class="proposals">
            {p.proposals.map((pr) => (
              <li key={pr.id} class="proposal">
                <p class="proposal__line">{pr.line}</p>
                <p class="proposal__evidence">
                  {kindLabel(pr.kind)} · 최근 {pr.evidence.window_days}일 동안 {pr.evidence.total}번 중 {pr.evidence.dismissed}번 넘김
                </p>
                <div class="proposal__actions">
                  <button type="button" class="btn btn--sm btn--primary" onClick={() => p.onProposal(pr.id, true)}>
                    받아들이기
                  </button>
                  <button type="button" class="btn btn--sm" onClick={() => p.onProposal(pr.id, false)}>
                    거절
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section class="section" aria-labelledby="set-daemon">
        <h3 class="section__title" id="set-daemon">
          데몬 설정 <span class="tag">읽기 전용</span>
        </h3>
        {p.settings ? (
          <dl class="kv">
            <div>
              <dt>말풍선 기준</dt>
              <dd>
                {p.settings.nudge_min}점 이상 <code>nudge_min</code>
              </dd>
            </div>
            <div>
              <dt>소리·음성 기준</dt>
              <dd>
                {p.settings.interrupt_min}점 이상 <code>interrupt_min</code>
              </dd>
            </div>
            <div>
              <dt>멈춤 의심</dt>
              <dd>
                {p.settings.stale_minutes}분 동안 조용하면 <code>stale_minutes</code>
              </dd>
            </div>
            <div>
              <dt>조용한 시간</dt>
              <dd>
                {quietHoursText(p.settings.quiet_hours)} <code>quiet_hours</code>
              </dd>
            </div>
            <div>
              <dt>표정으로만 알리는 종류</dt>
              <dd>
                {p.settings.quiet_kinds.length ? p.settings.quiet_kinds.map(kindLabel).join(', ') : '없음'} <code>quiet_kinds</code>
              </dd>
            </div>
          </dl>
        ) : (
          <p class="muted small">데몬에 연결되면 보여요.</p>
        )}
        <p class="section__note">
          이 값들은 <code>config.toml</code>에서 바꿔요(Windows: <code>%LOCALAPPDATA%\gyeot\config.toml</code>).
          {p.version && <> 데몬 버전 {p.version}.</>}
        </p>
      </section>
    </div>
  );
}
