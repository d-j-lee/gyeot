import { render } from 'preact';
import './styles/theme.css';
import './styles/avatar.css';
import './styles/app.css';
import { App } from './App.tsx';
import { MoodSheet } from './components/MoodSheet.tsx';
import { Controller } from './app/controller.ts';
import { createBackend } from './app/create.ts';
import { parseBoot } from './boot.ts';
import { installAudioUnlock } from './lib/audio.ts';
import { takeBootCode } from './lib/session.ts';

// §12.1-3: 무엇보다 먼저 주소에서 일회용 부트 코드를 지운다(다른 쿼리는 그대로). 주소창·방문 기록에 남기지 않는다.
const bootCode = takeBootCode(location, history);
const boot = parseBoot(location.search);
const root = document.documentElement;
root.classList.add(boot.mode === 'overlay' ? 'mode-overlay' : 'mode-default');
if (boot.theme) root.dataset.theme = boot.theme;
if (boot.reduceMotion) root.dataset.motion = 'reduce';

const mount = document.getElementById('app')!;

if (boot.demo?.scene === 'sheet') {
  render(<MoodSheet />, mount);
} else {
  const backend = createBackend(boot, location, bootCode);
  const ctl = new Controller(backend, { persistPrefs: !boot.demo }, boot.demo ? 'demo' : 'connecting');
  installAudioUnlock();
  render(<App ctl={ctl} config={{ mode: boot.mode, initialTab: boot.tab, initialOpen: boot.open }} />, mount);
  ctl.start();
  window.addEventListener('pagehide', () => ctl.stop());
}
