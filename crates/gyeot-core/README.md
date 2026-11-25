# gyeot-core

곁의 두뇌. 시계·파일·네트워크·스레드를 쓰지 않는 순수한 라이브러리다(`wasm32-unknown-unknown`에서도 빌드된다).
지금 시각은 늘 인자(`now_ms`)로 받는다.

- `normalize` — Claude Code·Codex 훅, codex-notify, AOK·fsmap 하네스 로그를 `Event`로 바꾼다(마스킹 포함, `docs/contracts.md` §2).
- `Engine` — 이벤트를 차례로 적용해 당신 차례 항목(§3), 존재감·전달 판단(§4), 제안(§5)을 만든다. 같은 이벤트 로그를 다시 적용하면 같은 상태가 나온다.
- `Learn` — 종류별 반응 집계와 받아들인 조정. 서버가 `learn.json`으로 저장한다.

시험: `cargo test -p gyeot-core`
