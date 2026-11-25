# CLAUDE.md — 곁(gyeot) 작업 규칙

곁은 dj의 Claude Code·Codex 생태계 위에 얹는 로컬 AI 동료 아바타다. 무엇·왜는 `docs/concept.md`, 구조는 `docs/architecture.md`, 부품 사이 규격은 `docs/contracts.md`에 있다.

## 바꿀 때
- 부품 사이 모양(이벤트·항목·API·SSE·channel)을 바꾸려면 `docs/contracts.md`를 먼저 고치고, 양쪽 시험을 함께 고친다.
- 고친 동작마다 시험을 붙이고, 고친 부분을 되돌리면 그 시험이 실패하는지 확인한다.
- 통과 기준: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. `face/`와 `channel/`은 `npm run typecheck`, `npm test`, `npm run build`.
- 결정은 `docs/decisions.md`에 한 줄(결정 | 근거 | 버린 대안)로 적는다. 뒤집혀도 지우지 않는다.
- 사용자에게 보이는 문구와 문서는 한국어로 쓴다.

## 지켜야 할 불변식
- 곁은 권한 승인·병합·삭제를 대신하지 않는다. 승인 버튼을 만들지 않고, channel에 `claude/channel/permission`을 선언하지 않는다(G-005).
- 곁은 먼저 브리핑을 띄우지 않는다. 모든 말풍선에 넘기기가 있다(G-006).
- 학습은 제안으로만 낸다. 스스로 설정을 바꾸지 않는다(G-007).
- `gyeot-core`는 시계·파일·네트워크를 쓰지 않는다(결정적, wasm 호환).
- 훅 심은 stdout에 아무것도 쓰지 않고 언제나 exit 0으로 끝난다. 400ms 넘게 기다리지 않는다.
- 데몬은 `127.0.0.1`에만 묶이고 Host·Origin·토큰을 검사한다. 이 검사를 느슨하게 하는 변경은 사람에게 먼저 묻는다.
- 프롬프트 본문과 트랜스크립트를 저장하지 않는다. 미리보기는 마스킹한 뒤 자른다.

## 사실과 추정
- `docs/roadmap.md`의 F 목록은 실측 전이다. 확인하면 `docs/field-notes.md`에 먼저 적고, 그 기록을 근거로 문서를 고친다.
- Claude Code·Codex의 훅·channel 동작은 자주 바뀐다. 고치기 전에 설치된 버전의 공식 문서를 다시 확인한다.
