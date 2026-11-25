import type { BootConfig } from '../boot.ts';
import type { Backend } from './backend.ts';
import { HttpBackend } from './backend.ts';
import { DemoBackend } from '../demo/backend.ts';
import { resolveApiBase } from '../lib/api.ts';

/**
 * 주소에 맞는 백엔드. 데모(`?demo=`)는 네트워크를 쓰지 않으므로 열쇠도 필요 없다.
 * 실제 데몬은 같은 출처 세션 쿠키로만 인증한다(§12.1) — 토큰을 찾지 않는다.
 */
export function createBackend(boot: Pick<BootConfig, 'demo'>, loc: Pick<Location, 'href'> = location): Backend {
  return boot.demo ? new DemoBackend(boot.demo) : new HttpBackend(resolveApiBase(loc));
}
