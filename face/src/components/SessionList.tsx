import type { Session } from '../types.ts';
import { agentLabel, sessionStateLabel } from '../lib/labels.ts';
import { agoKo } from '../lib/time.ts';

export interface SessionListProps {
  sessions: Session[];
  now: number;
  onPick: (s: Session) => void;
}

/** 기본 화면 옆칸: 세션 한눈에 보기. 누르면 그 세션으로 대화를 연다. */
export function SessionList({ sessions, now, onPick }: SessionListProps) {
  const live = sessions.filter((s) => s.state !== 'ended');
  return (
    <section class="sessions" aria-labelledby="sessions-title">
      <h2 class="sessions__title" id="sessions-title">
        세션 <span class="sessions__count">{live.length}</span>
      </h2>
      {live.length === 0 ? (
        <p class="muted small">아직 움직이는 세션이 없어요.</p>
      ) : (
        <ul class="sessions__list">
          {live.map((s) => (
            <li key={s.id}>
              <button
                type="button"
                class="session"
                onClick={() => onPick(s)}
                title={`${s.cwd}\n누르면 이 세션과 대화해요`}
              >
                <span class={`session__state session__state--${s.state}`} aria-hidden="true" />
                <span class="session__main">
                  <span class="session__name">{s.project || s.id}</span>
                  <span class="session__sub">
                    {agentLabel(s.agent)} · {sessionStateLabel(s.state)}
                    {s.last_ms > 0 && <> · {agoKo(s.last_ms, now)}</>}
                  </span>
                </span>
                {s.channel && <span class="pill pill--ok pill--tiny">연결됨</span>}
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
