import { IconRefresh, IconWarn } from './Icons.tsx';

/**
 * 세션 쿠키가 없거나 만료됐을 때(401, §12.1-4). 얼굴은 스스로 열쇠를 얻을 수 없으므로
 * 사람이 트레이나 터미널에서 곁을 다시 열게 안내한다. 셸 안이면 셸이 새 열쇠로 창을 다시 연다.
 */
export function AuthNotice({ onRetry }: { onRetry: () => void }) {
  return (
    <div class="auth-notice" role="alert">
      <IconWarn size={16} />
      <p class="auth-notice__text">
        열쇠가 만료됐어요 — 트레이의 ‘곁 열기’를 누르거나 터미널에서 <code>gyeotd --open</code>을 실행하세요.
      </p>
      <button type="button" class="btn btn--xs" onClick={onRetry}>
        <IconRefresh size={12} /> 다시 확인
      </button>
    </div>
  );
}
