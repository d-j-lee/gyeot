import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, render, screen } from '@testing-library/preact';
import { SettingsTab } from './SettingsTab.tsx';
import { DEFAULT_PREFS } from '../lib/prefs.ts';
import { DEMO_SETTINGS } from '../demo/data.ts';
import type { Settings } from '../types.ts';

afterEach(cleanup);

const NOW = Date.UTC(2026, 10, 26, 5, 30);

function mount(settings: Settings | null) {
  return render(
    <SettingsTab
      now={NOW}
      focusUntil={null}
      prefs={DEFAULT_PREFS}
      proposals={[]}
      settings={settings}
      version="0.1.0"
      onFocus={() => {}}
      onPrefs={() => {}}
      onProposal={() => {}}
      onTestChime={() => {}}
      onTestVoice={() => {}}
    />,
  );
}

function row(name: string): HTMLElement {
  const dt = screen.getByText(name, { selector: 'dt' });
  return dt.parentElement as HTMLElement;
}

describe('데몬 설정: 승인 대기 소리 알림(§13.3 approval_interrupt)', () => {
  it('꺼져 있으면 꺼짐과 켜는 방법(config.toml 의 approval_interrupt = true)을 보인다', () => {
    mount({ ...DEMO_SETTINGS, approval_interrupt: false });
    const r = row('승인 대기 소리 알림');
    expect(r.querySelector('dd')!.textContent).toBe('꺼짐 — 켜려면 config.toml에 approval_interrupt = true');
    expect(r.querySelector('dd code')!.textContent).toBe('approval_interrupt = true');
  });

  it('켜져 있으면 켜짐', () => {
    mount({ ...DEMO_SETTINGS, approval_interrupt: true });
    const dd = row('승인 대기 소리 알림').querySelector('dd')!;
    expect(dd.textContent).toMatch(/^켜짐 — /);
    expect(dd.textContent).toContain('approval_interrupt');
  });

  it('읽기 전용이다 — 바꾸는 단추·스위치가 없다', () => {
    mount({ ...DEMO_SETTINGS, approval_interrupt: false });
    const r = row('승인 대기 소리 알림');
    expect(r.querySelectorAll('button, input, select, [role="switch"]')).toHaveLength(0);
  });

  it('데모 설정은 기본값(꺼짐)이다', () => {
    expect(DEMO_SETTINGS.approval_interrupt).toBe(false);
  });
});
