import { useEffect, useState } from 'preact/hooks';
import type { Mood } from '../types.ts';
import type { ConnState } from '../state/reducer.ts';
import { moodLabel } from '../lib/labels.ts';
import { remainingKo } from '../lib/time.ts';
import { LEAF_COLORS } from './Avatar.tsx';
import { IconRefresh } from './Icons.tsx';

export function connText(conn: ConnState, localNow: number): string {
  switch (conn.status) {
    case 'open':
      return '연결됨';
    case 'demo':
      return '데모 모드 — 데몬 없이 표본 데이터로 보여요';
    case 'auth':
      return '열쇠가 없거나 만료됐어요';
    case 'host':
      return '127.0.0.1 주소로 열어 주세요';
    case 'retrying': {
      if (conn.retryAt === null) return '곁 데몬에 연결 중…';
      const s = Math.max(0, Math.ceil((conn.retryAt - localNow) / 1000));
      return s > 0 ? `곁 데몬에 연결 중… ${s}초 뒤 다시 시도` : '곁 데몬에 연결 중…';
    }
    case 'connecting':
    default:
      return '곁 데몬에 연결 중…';
  }
}

export interface StatusLineProps {
  mood: Mood;
  line: string;
  focusUntil: number | null;
  now: number;
  conn: ConnState;
  onRetry: () => void;
}

export function StatusLine({ mood, line, focusUntil, now, conn, onRetry }: StatusLineProps) {
  const [tick, setTick] = useState(() => Date.now());
  const counting = conn.status === 'retrying' && conn.retryAt !== null;
  useEffect(() => {
    if (!counting) return undefined;
    const t = setInterval(() => setTick(Date.now()), 1_000);
    return () => clearInterval(t);
  }, [counting, conn.retryAt]);

  const focusOn = focusUntil !== null && focusUntil > now;
  const offline = conn.status !== 'open' && conn.status !== 'demo';
  // 열쇠·주소 문제는 기다려도 풀리지 않는다 — 돌아가는 표시와 지금 다시 단추를 두지 않는다(패널 위 안내가 할 일을 알린다).
  const stuck = conn.status === 'auth' || conn.status === 'host';

  return (
    <div class="status">
      <p class="status__mood">
        <span class="status__dot" style={{ background: LEAF_COLORS[mood] }} aria-hidden="true" />
        <span class="status__mood-text">{moodLabel(mood)}</span>
        {focusOn && (
          <span class="status__focus" title="집중 모드">
            집중 모드 {remainingKo(focusUntil!, now)}
          </span>
        )}
      </p>
      {line && !offline && <p class="status__line">{line}</p>}
      {offline && (
        <p class={`status__conn status__conn--${conn.status}`} role="status">
          {offline && !stuck && <span class="spinner" aria-hidden="true" />}
          <span>{connText(conn, tick)}</span>
          {/* 열쇠 만료면 패널 위 안내(AuthNotice)가 다시 확인 단추를 단다. 주소 문제(HostNotice)는 단추가 없다 */}
          {offline && !stuck && (
            <button type="button" class="btn btn--xs btn--ghost" onClick={onRetry}>
              <IconRefresh size={12} /> 지금 다시
            </button>
          )}
        </p>
      )}
    </div>
  );
}
