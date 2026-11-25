import { render } from 'preact';
import './styles/theme.css';
import './styles/avatar.css';
import './styles/app.css';
import { App } from './App.tsx';
import { MoodSheet } from './components/MoodSheet.tsx';
import { Controller } from './app/controller.ts';
import { HttpBackend } from './app/backend.ts';
import { DemoBackend } from './demo/backend.ts';
import { parseBoot } from './boot.ts';
import { resolveApiBase, resolveToken } from './lib/api.ts';
import { installAudioUnlock } from './lib/audio.ts';

const boot = parseBoot(location.search);
const root = document.documentElement;
root.classList.add(boot.mode === 'overlay' ? 'mode-overlay' : 'mode-default');
if (boot.theme) root.dataset.theme = boot.theme;
if (boot.reduceMotion) root.dataset.motion = 'reduce';

const mount = document.getElementById('app')!;

if (boot.demo?.scene === 'sheet') {
  render(<MoodSheet />, mount);
} else {
  const backend = boot.demo ? new DemoBackend(boot.demo) : new HttpBackend(resolveApiBase(), resolveToken());
  const ctl = new Controller(backend, { persistPrefs: !boot.demo }, boot.demo ? 'demo' : 'connecting');
  installAudioUnlock();
  render(<App ctl={ctl} config={{ mode: boot.mode, initialTab: boot.tab, initialOpen: boot.open }} />, mount);
  ctl.start();
  window.addEventListener('pagehide', () => ctl.stop());
}
