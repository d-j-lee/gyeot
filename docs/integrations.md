# 연동

상태: **구현**(이 저장소에 코드와 시험이 있음) · **계획**(설계만 있음) · **시드**(아이디어만 남겨 둠).
근거: [확인] 이번에 실행·열람함 · [문서] 문서에 그렇게 적혀 있음 · [미확인] 실측 전.

## Claude Code — 훅 (구현)

- 받는 이벤트: `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `Notification`, `PermissionRequest`, `Stop`, `StopFailure`, `SubagentStart`, `SubagentStop`, `PostToolUseFailure`, `TaskCompleted`, `PreCompact`, 그리고 선택으로 `PostToolBatch`(작업 중 표시용, 잦음).
- 설정: `examples/claude-settings.hooks.json`. exec 형식이라 Windows에서 셸을 거치지 않는다. `"async": true`라 기다리지 않는다[문서: hooks 레퍼런스].
- `Notification`의 `notification_type`으로 승인 대기(`permission_prompt`)와 입력 대기(`idle_prompt`·`agent_needs_input`·`elicitation_*`)를 가른다[문서].
- 이미 쓰는 훅(fsmap·AOK ⑧·work-atlas)과 함께 둔다. Claude Code는 같은 이벤트의 훅을 모두 실행한다. 곁의 훅은 비동기라 다른 훅의 판정에 끼지 않는다.
- `install.ps1 -ApplyClaudeHooks`는 `~/.claude/settings.json`을 백업한 뒤, 기존 훅을 지우지 않고 곁의 항목만 더한다. 기본값은 조각을 출력만 하는 것이다.
- [미확인 F1] exec 형식 훅의 부모 프로세스가 `claude.exe`인지. 아니면 channel 연결은 `cwd`로 잇는다.

## Claude Code — channel (구현, 연구 프리뷰)

- `gyeot-channel.mjs`는 MCP 서버다. 곁 → 세션 방향은 `notifications/claude/channel`, 세션 → 곁 방향은 `reply`·`request_decision` 도구다[문서: channels reference].
- 연구 프리뷰 동안 직접 만든 channel은 허용 목록에 없어서 이렇게 띄운다: `claude --dangerously-load-development-channels server:gyeot`. 처음 한 번 경고 화면과 MCP 서버 동의가 나온다[문서].
- 세션이 열려 있을 때만 메시지가 도착한다. 세션이 그 서버를 channel로 띄우지 않았으면 메시지는 조용히 버려진다[문서]. 그래서 곁은 channel 등록이 있는 세션에만 보내고, 없으면 그 이유를 그대로 보여 준다.
- 권한 중계는 선언하지 않는다(G-005).
- Team·Enterprise 조직에서는 관리자가 channel을 켜야 한다[문서]. 이 계정은 개인 계정이라 해당하지 않는다고 보지만, 실측 전이다[미확인 F2].

## Codex (구현, 페이로드는 미확인)

- 훅: `~/.codex/hooks.json`. 구조(이벤트 → matcher → command 핸들러)는 Claude Code와 같다[문서: Codex hooks]. 입력 필드가 Claude Code와 같다는 것은 가정이다[미확인 F3]. 정규화는 공통 필드가 없어도 버티게 했다.
- notify: `~/.codex/config.toml`의 `notify`. 페이로드 JSON이 마지막 인자로 온다는 것은 제3자 구현의 기록이다[미확인 F4]. `gyeot-hook codex-notify`가 받는다.
- Codex는 Windows 네이티브 샌드박스가 있다. 반면 Claude Code는 네이티브 Windows에서 셸 명령을 샌드박스 없이 실행한다[문서]. 곁은 이 차이를 바꾸지 않는다. 다만 승인 대기를 놓치지 않게 하는 것이 네이티브 Windows Claude Code 사용의 안전에도 도움이 된다[추론].

## fsmap · AOK ⑧ 하네스 (구현)

- `config.toml`의 `[[tail]]`에 각 저장소의 `<git-dir>/fsmap-harness/log.jsonl`, `<git-dir>/aok-harness/log.jsonl`을 적으면 따라 읽는다.
- 게이트 `fail`·`cannot`·`block`·`push-error`는 `failed`(80)가 되고, `pass`가 오면 닫힌다. `push-uncommitted`는 `volatile`(78)이 된다. 레코드 모양은 두 저장소의 소스에서 확인했다[확인: `aok_harness.py` `log()`, fsmap `hooks.rs` `log_gate`].

## AOK 관제실 (계획 v0.2)

- 관제실은 클라우드 세션을, 곁은 로컬 세션을 본다. 점수 체계가 같아서 사람에게는 하나의 "당신 차례"로 읽힌다(G-008).
- 다음 단계: `gyeot-core`를 wasm으로 빌드해 관제실 콘솔이 같은 규칙을 쓰게 한다. AOK의 19개 사례(`tests/console/atlas_cases.json`)로 콘솔 JS 판정과 코어 판정을 나란히 돌려 비교하는 차등 시험을 먼저 만든다(AOK R3의 방식).
- 곁에서 관제실로 가는 링크(클라우드 세션은 `https://claude.ai/code/<id>`)는 시드다.

## dj-assis (원칙 공유, 표면 분담)

- dj-assis는 폰과 음성, 곁은 Windows 데스크톱이 표면이다. 결정 카드, 넘기기, 승인 대행 금지 같은 원칙을 같이 쓴다.
- 시드: 곁의 "당신 차례" 요약을 폰으로 넘기기(PushNotification 도달은 아직 실측 전, dj-assis F16).

## DocBench · work-atlas · agent-office (시드)

- DocBench `127.0.0.1:4317`의 SSE `/events`·`/notify` → 곁 항목("볼 것"을 곁으로 합치기).
- work-atlas 맥락(결함이 몰린 파일, 같이 바뀌는 파일) → 승인 카드의 근거 줄로 붙이기.
- agent-office spool·API(`127.0.0.1:55433`) → 같은 수집 경로로 받기.
