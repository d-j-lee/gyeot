# 연결 견본

`scripts/install.ps1`이 아래 조각을 **실제 절대 경로로 채워서** 출력하고, `%LOCALAPPDATA%\gyeot\snippets\`에도 저장합니다. 이 폴더의 파일은 모양을 보여 주는 견본이라 경로가 `C:\Users\<사용자>\AppData\Local\gyeot\bin\…`(= `%LOCALAPPDATA%\gyeot\bin\…`)으로 적혀 있습니다. exec 형식 훅은 셸을 거치지 않아 `%LOCALAPPDATA%` 같은 환경 변수가 펼쳐지지 않으니, 직접 옮길 때는 꼭 펼친 경로로 바꾸세요.

| 파일 | 넣을 곳 | 메모 |
|---|---|---|
| `claude-settings.hooks.json` | `%USERPROFILE%\.claude\settings.json`의 `"hooks"`에 합침 | exec 형식(`args`) + `async: true`. 셸(Git Bash·PowerShell)을 거치지 않고, Claude Code를 기다리게 하지 않음. `install.ps1 -ApplyClaudeHooks`가 백업 후 자동으로 합침(기존 훅은 그대로). 기본 13개 이벤트(`PostToolBatch` 포함) |
| `codex-hooks.json` | `%USERPROFILE%\.codex\hooks.json` | Codex 훅은 `command` 문자열 하나(인자 배열 없음). 입력 필드는 실측 전(F3) |
| `codex-config.notify.toml` | `%USERPROFILE%\.codex\config.toml` 맨 위 | 페이로드가 마지막 인자로 온다는 것은 실측 전(F4) |
| `mcp.json` | 프로젝트의 `.mcp.json`, 또는 `claude mcp add --scope user gyeot -- node "<경로>\gyeot-channel.mjs"` | 세션은 `claude --dangerously-load-development-channels server:gyeot`로 시작 |
| `gyeot-config.toml` | `%LOCALAPPDATA%\gyeot\config.toml` | 모든 항목 선택. 하네스 로그 `[[tail]]` 견본 포함 |

## Claude Code 훅 이벤트

기본 13개: `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `Notification`, `PermissionRequest`, `Stop`, `StopFailure`, `SubagentStart`, `SubagentStop`, `PostToolUseFailure`, `TaskCompleted`, `PreCompact`, `PostToolBatch`.

- matcher를 쓰는 이벤트에는 `"matcher": "*"`를, 쓰지 않는 이벤트(`UserPromptSubmit`·`Stop`·`TaskCompleted`·`PostToolBatch`)에는 matcher를 넣지 않습니다.
- `PostToolBatch`는 병렬 도구 묶음이 끝날 때마다 한 번 불립니다(승인을 기다린 도구 포함). 사람이 터미널에서 승인하면 이 신호로 곁의 승인 대기가 닫힙니다(docs/contracts.md §12.3). 도구 묶음마다 불려 잦지만 비동기라 Claude Code를 기다리게 하지 않습니다.
- 빼고 싶으면 `install.ps1 -ApplyClaudeHooks -NoActivityHook`(이미 넣은 곁의 `PostToolBatch` 항목만 뺌). 빼면 터미널에서 승인해도 곁의 승인 대기가 턴이 끝날 때까지 남을 수 있고, 곁은 그 세션의 승인 대기를 말풍선까지만 알립니다.
- 곁의 훅은 비동기라 다른 훅(fsmap·AOK ⑧ 등)의 판정에 끼지 않고, 실패해도 Claude Code를 막지 않습니다. `uninstall.ps1 -RemoveClaudeHooks`는 곁의 항목(`PostToolBatch` 포함)만 뺍니다.

## 얼굴 열기

얼굴 주소(`http://127.0.0.1:47321/`)에는 토큰이 없습니다. 셸 트레이의 **곁 열기**, 또는 `gyeotd --open`(기본 브라우저)으로 여세요. 둘 다 토큰으로 일회용 부트 코드를 받아 세션 쿠키로 붙습니다(docs/contracts.md §12.1). 주소만 복사해 열면 안내 쪽이 보입니다.
