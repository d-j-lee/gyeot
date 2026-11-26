// 배포 얼굴은 127.0.0.1 출처에서만 돈다(§15.5, G-042).
// 얼굴 확인(§14.1)은 포트만 묶는다. `http://localhost:<포트>`로 연 얼굴은 localhost 가 ::1 로 풀릴 때
// 그 주소의 같은 포트를 잡은 서버가 hello 를 진짜 데몬에 중계해 확인을 통과할 수 있다(VR7 L3).
// 셸과 `gyeotd --open` 은 늘 127.0.0.1 로 연다. 개발 빌드(npm run dev)와 데모는 예외다.

/** 사람에게 보일 안내(화면에서는 `gyeotd --open` 을 코드로 그린다). */
export const HOST_NOTICE = '127.0.0.1 주소로 열어 주세요 — 트레이의 ‘곁 열기’나 gyeotd --open은 늘 그 주소로 엽니다';

/** 127.0.0.1 이 아닌 주소라 요청을 보내지 않았다. */
export class WrongHost extends Error {
  constructor() {
    super('127.0.0.1 이 아닌 주소라 보내지 않음');
    this.name = 'WrongHost';
  }
}

/** 이 주소에서 데몬에 요청해도 되나. 배포 빌드는 hostname 이 정확히 127.0.0.1 일 때만. */
export function faceHostAllowed(loc: Pick<Location, 'href'> = location, dev: boolean = import.meta.env.DEV): boolean {
  // 배포 빌드에서는 import.meta.env.DEV 가 false 로 박혀 예외 분기가 통째로 빠진다(dev 를 넘겨도 못 켠다).
  if (import.meta.env.DEV && dev) return true;
  return new URL(loc.href).hostname === '127.0.0.1';
}
