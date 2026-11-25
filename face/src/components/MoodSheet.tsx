import { Avatar } from './Avatar.tsx';
import { ALL_MOODS, moodLabel } from '../lib/labels.ts';

/** ?demo=sheet — 일곱 기분을 한 화면에(디자인 검토용). */
export function MoodSheet() {
  return (
    <main class="sheet">
      <h1 class="sheet__title">곁 · 기분 일곱 가지</h1>
      <ul class="sheet__grid">
        {ALL_MOODS.map((m) => (
          <li key={m} class="sheet__cell">
            <Avatar mood={m} size={120} />
            <p class="sheet__name">
              <code>{m}</code>
              <span>{moodLabel(m)}</span>
            </p>
          </li>
        ))}
      </ul>
    </main>
  );
}
