# 연결 견본

`scripts/install.ps1`이 아래 조각을 **실제 절대 경로로 채워서** 출력하고, `%LOCALAPPDATA%\gyeot\snippets\`에도 저장합니다. 이 폴더의 파일은 모양을 보여 주는 견본이라 경로가 `C:\Users\<사용자>\AppData\Local\gyeot\bin\…`(= `%LOCALAPPDATA%\gyeot\bin\…`)으로 적혀 있습니다. exec 형식 훅은 셸을 거치지 않아 `%LOCALAPPDATA%` 같은 환경 변수가 펼쳐지지 않으니, 직접 옮길 때는 꼭 펼친 경로로 바꾸세요.

| 파일 | 넣을 곳 | 메모 |
|---|---|---|
| `claude-settings.hooks.json` | `%USERPROFILE%\.claude\settings.json`의 `"hooks"`에 합침 | exec 형식(`args`) + `async: true`. 셸(Git Bash·PowerShell)을 거치지 않고, Claude Code를 기다리게 하지 않음. `install.ps1 -ApplyClaudeHooks`가 백업 후 자동으로 합침(기존 훅은 그대로). 기본 13개 이벤트(`PostToolBatch` 포함) |
| `codex-hooks.json` | `%USERPROFILE%\.codex\hooks.json` | Codex 훅은 `command` 문자열 하나(인자 배열 없음). 입력 필드는 실측 전(F3) |
| `codex-config.notify.toml` | `%USERPROFILE%\.codex\config.toml` 맨 위 | 페이로드가 마지막 인자로 온다는 것은 실측 전(F4) |
| `mcp.json` | 프로젝트의 `.mcp.json`, 또는 `claude mcp add --scope user gyeot -- node "<경로>\gyeot-channel.mjs"` | 세션은 `claude --dangerously-load-development-channels server:gyeot`로 시작 |
| `gyeot-config.toml` | `%LOCALAPPDATA%\gyeot\config.toml` | 모든 항목 선택(주석의 값이 기본값). 승인·병합 대기의 소리 알림 `approval_interrupt`(기본 `false`, §13.3), 하네스 로그 `[[tail]]` 견본 포함 |

## Claude Code 훅 이벤트

기본 13개: `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `Notification`, `PermissionRequest`, `Stop`, `StopFailure`, `SubagentStart`, `SubagentStop`, `PostToolUseFailure`, `TaskCompleted`, `PreCompact`, `PostToolBatch`.

- matcher를 쓰는 이벤트에는 `"matcher": "*"`를, 쓰지 않는 이벤트(`UserPromptSubmit`·`Stop`·`TaskCompleted`·`PostToolBatch`)에는 matcher를 넣지 않습니다.
- `PostToolBatch`는 병렬 도구 묶음이 끝날 때마다 한 번 불립니다(승인을 기다린 도구 포함). 사람이 터미널에서 승인하면 이 신호로 곁의 승인 대기가 닫힙니다(docs/contracts.md §12.3). 도구 묶음마다 불려 잦지만 비동기라 Claude Code를 기다리게 하지 않습니다.
- 빼고 싶으면 `install.ps1 -ApplyClaudeHooks -NoActivityHook`(이미 넣은 곁의 `PostToolBatch` 항목만 뺌). 빼면 터미널에서 승인해도 곁의 승인 대기가 턴이 끝날 때까지 남을 수 있고, 곁은 그 세션의 승인 대기를 말풍선까지만 알립니다.
- 곁의 훅은 비동기라 다른 훅(fsmap·AOK ⑧ 등)의 판정에 끼지 않고, 실패해도 Claude Code를 막지 않습니다. `uninstall.ps1 -RemoveClaudeHooks`는 곁의 항목(`PostToolBatch` 포함)만 뺍니다.

## 설정: 승인·병합 대기의 소리 알림

`approval_interrupt`(맨 위 키, 기본 `false`)는 승인·병합 대기를 소리·음성(interrupt)까지 올릴지 정합니다(docs/contracts.md §13.3). 승인 순간을 알리는 훅이 없어서, 사람이 터미널에서 승인한 도구가 2분 넘게 돌면 곁은 아직 기다린다고 잘못 알 수 있습니다. 그래서 기본은 말풍선(nudge)까지만 올립니다. 소리까지 원하면 `config.toml`의 맨 위(`[[tail]]` 같은 표보다 앞)에 `approval_interrupt = true`를 넣고 데몬을 다시 시작하세요. 데몬은 지금 값을 `Snapshot.settings.approval_interrupt`로 얼굴에 알립니다.

## 얼굴 열기

얼굴 주소(`http://127.0.0.1:47321/`)에는 토큰이 없습니다. 셸 트레이의 **곁 열기**, 또는 `gyeotd --open`(기본 브라우저)으로 여세요. 둘 다 그 포트의 서버가 내 곁 데몬인지 확인한 뒤(§13.1) 토큰으로 일회용 부트 코드를 받고, 얼굴이 그 코드를 세션 열쇠로 바꿔 붙습니다(docs/contracts.md §12.1). 주소만 복사해 열면 "열쇠가 없거나 만료됐어요" 안내가 보입니다.

## 설치 폴더 권한 (Windows)

기본 설치 폴더 `%LOCALAPPDATA%\gyeot`은 그 사용자 전용 권한을 이어받습니다. `install.ps1 -InstallRoot C:\gyeot`처럼(또는 `GYEOT_HOME`을) `%LOCALAPPDATA%` 밖에 두면, 설치 스크립트가 그 폴더의 상속을 끊고 지금 사용자와 SYSTEM만 모든 권한을 갖게 좁힌 뒤(`icacls … /inheritance:r /grant:r …:(OI)(CI)F`) 결과를 보여 줍니다(docs/contracts.md §13.4). 다른 사용자가 `token`을 읽거나 Claude Code가 실행하는 `gyeot-hook.exe`를 바꿔치지 못하게 하려는 것입니다. 폴더 주인이 다른 사용자면 설치하지 않습니다. `-DryRun`은 할 `icacls` 명령만 보여 줍니다.
