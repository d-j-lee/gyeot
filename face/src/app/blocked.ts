import type { DecisionResult, FocusResult, SayResult } from '../types.ts';
import type { Backend, BackendSink } from './backend.ts';
import { WrongHost } from '../lib/origin.ts';

/**
 * 배포 얼굴이 127.0.0.1 이 아닌 주소에서 열렸을 때(§15.5, G-042). 데몬에 아무 요청도 보내지 않고(부트 코드 교환·hello·스트림·REST 모두),
 * 셸 IPC(`renew_session`)도 부르지 않는다. 화면은 conn.status = 'host' 로 "127.0.0.1 주소로 열어 주세요" 안내를 보인다.
 */
export class HostBlockedBackend implements Backend {
  start(sink: BackendSink): void {
    sink.status({ status: 'host', attempt: 0, retryAt: null });
  }
  stop(): void {}
  retryNow(): void {}
  itemAction(): Promise<void> {
    return Promise.reject(new WrongHost());
  }
  focus(): Promise<FocusResult> {
    return Promise.reject(new WrongHost());
  }
  listening(): Promise<void> {
    return Promise.reject(new WrongHost());
  }
  say(): Promise<SayResult> {
    return Promise.reject(new WrongHost());
  }
  decide(): Promise<DecisionResult> {
    return Promise.reject(new WrongHost());
  }
  proposal(): Promise<void> {
    return Promise.reject(new WrongHost());
  }
}
