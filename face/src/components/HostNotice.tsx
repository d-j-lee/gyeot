import { IconWarn } from './Icons.tsx';

/**
 * 배포 얼굴이 127.0.0.1 이 아닌 주소(예: localhost)에서 열렸을 때(§15.5, G-042). 얼굴은 아무 요청도 보내지 않는다.
 * 다시 시도해도 풀리지 않으므로 단추를 두지 않는다. 문구는 lib/origin.ts 의 HOST_NOTICE 와 같다.
 */
export function HostNotice() {
  return (
    <div class="host-notice" role="alert">
      <IconWarn size={16} />
      <p class="host-notice__text">
        127.0.0.1 주소로 열어 주세요 — 트레이의 ‘곁 열기’나 <code>gyeotd --open</code>은 늘 그 주소로 엽니다
      </p>
    </div>
  );
}
