import type { BootConfig } from '../boot.ts';
import type { Backend } from './backend.ts';
import { HttpBackend } from './backend.ts';
import { DemoBackend } from '../demo/backend.ts';
import { resolveApiBase } from '../lib/api.ts';
import { faceHostAllowed } from '../lib/origin.ts';
import { HostBlockedBackend } from './blocked.ts';

/**
 * 주소에 맞는 백엔드. 데모(`?demo=`)는 네트워크·IPC 를 쓰지 않으므로 열쇠도 필요 없다(부트 코드가 와도 버린다).
 * 배포 빌드가 127.0.0.1 이 아닌 주소에서 열렸으면 아무 요청도 보내지 않고 안내만 한다(§15.5 — 부트 코드도 바꾸지 않는다).
 * 실제 데몬은 세션 열쇠로만 인증한다(§12.1) — 부트 코드가 있으면 그것으로 열쇠를 받고, 없으면 이 탭에 저장된 열쇠를 쓴다.
 */
export function createBackend(boot: Pick<BootConfig, 'demo'>, loc: Pick<Location, 'href'> = location, bootCode: string | null = null): Backend {
  if (boot.demo) return new DemoBackend(boot.demo);
  if (!faceHostAllowed(loc)) return new HostBlockedBackend();
  return new HttpBackend(resolveApiBase(loc), { bootCode });
}
