# 곁 설치·업데이트 — Claude Code에 맡기기

업무 PC에서 할 일은 아래 한 줄을 Claude Code에 붙여 넣는 것뿐이다. 처음이면 설치하고, 이미 있으면 최신 릴리스로 바꾼다.

```
곁(d-j-lee/gyeot)을 설치해 줘(이미 있으면 업데이트). curl.exe -fsSL https://github.com/d-j-lee/gyeot/releases/latest/download/INSTALL-WITH-CLAUDE.md 로 받은 설치 문서 전문을 읽고 그 순서를 그대로 따르고, 내 설정을 바꾸기 전에는 미리보기를 보여 주고 확인을 받아.
```

이 문서의 나머지는 그 한 줄을 받은 Claude Code가 따르는 순서다. 사람이 PowerShell 창에서 직접 따라 해도 된다. 이 문서는 릴리스 파일이라 같은 릴리스의 bootstrap·zip과 판이 맞는다.

## 전제

- Windows 10·11과 Windows PowerShell 5.1(기본으로 들어 있다). 관리자 권한은 필요 없다.
- Claude Code CLI(`claude`). 곁은 Claude Code 훅으로 소식을 받는다.
- 인터넷에서 `github.com`의 릴리스 파일을 받을 수 있어야 한다. 저장소는 공개라 로그인이 필요 없다.
- 선택: `gh`(GitHub CLI)가 github.com에 로그인돼 있으면 "이 저장소의 GitHub Actions가 만든 파일인지"(출처 증명)까지 확인한다. 없으면 SHA-256 무결성만 확인한다.
- 선택: Node.js 22 이상. 곁과 대화(channel)할 때만 쓴다. 없으면 그 기능만 빠진다.

## Claude Code가 따를 순서

아래 명령 블록은 모두 PowerShell이다. 블록의 줄은 저마다 완결된 문장이고, 작은따옴표와 `\"`를 쓰지 않았다.

- 셸 도구가 Bash(Git Bash)이면 블록의 줄을 `; `로 이어 한 줄로 만들고 이렇게 실행한다. 작은따옴표 안이라 Bash가 `$env:`·`$LASTEXITCODE`를 풀지 않고, `MSYS_NO_PATHCONV=1`이 Git Bash의 경로 바꾸기를 끈다.
  ```
  MSYS_NO_PATHCONV=1 powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '[Console]::OutputEncoding = [Text.Encoding]::UTF8; <그 한 줄>; exit $LASTEXITCODE'
  ```
- 셸 도구가 PowerShell이면 블록 앞에 `Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass -Force; [Console]::OutputEncoding = [Text.Encoding]::UTF8`를 붙여 실행한다. 그 프로세스에만 걸리고 저장되지 않는다.
- 3·4단계는 몇 분 걸릴 수 있다. 셸 도구의 시간 한도를 10분(600000ms)으로 준다.
- 1단계는 종료 코드와 상관없이 출력만 읽는다(설치돼 있으면 점검 결과가 실패여도 정상이다). 나머지 단계는 종료 코드 0이 성공이다.

### 1. 확인 — 아무것도 바꾸지 않는다

```powershell
"PowerShell $($PSVersionTable.PSVersion) / $($ExecutionContext.SessionState.LanguageMode)"
foreach ($c in @("gh", "claude", "node")) { $x = Get-Command $c -ErrorAction SilentlyContinue; if ($x) { "$c = $($x.Source)" } else { "$c = 없음" } }
if (Get-Command node -ErrorAction SilentlyContinue) { "node " + (node --version) }
if (Get-Command gh -ErrorAction SilentlyContinue) { $env:GH_HOST = "github.com"; gh auth status --hostname github.com 2>&1 | Select-Object -First 2 }
$doctor = Join-Path $env:LOCALAPPDATA "gyeot\scripts\doctor.ps1"
if (Test-Path $doctor) { & (Join-Path $env:LOCALAPPDATA "gyeot\bin\gyeotd.exe") --version; & $doctor } else { "곁 없음: 새로 설치" }
```

이 결과로 정한다.

- 둘째 값이 `FullLanguage`가 아니면(회사 정책의 제한 언어 모드) 멈추고 사람에게 알린다. 다른 방법으로 실행해 보지 않는다.
- `node`가 없거나 22보다 낮으면 "대화 기능만 빠진다"고 알리고 3단계부터 `-RegisterChannel`을 뺀다. `claude`를 찾지 못해도 뺀다.
- `gh`가 없거나 로그인돼 있지 않으면 "출처 증명은 건너뛰고 SHA-256만 확인한다"고 알린다. 멈추지는 않는다.
- 이미 설치돼 있으면 마지막 점검 표에서 지금 쓰는 선택을 읽는다. "Claude 훅"이 ok면 `-ApplyClaudeHooks`, "활동 훅이 없어요" 경고가 있으면 `-NoActivityHook`도, "로그인 시 시작"이 ok면 `-RegisterStartup`, "channel"이 ok면 `-RegisterChannel`. 업데이트는 이 선택을 그대로 쓴다.

### 2. bootstrap 받기·무결성 확인

```powershell
$ErrorActionPreference = "Stop"
$d = Join-Path $env:TEMP "gyeot-boot"; New-Item -ItemType Directory -Force -Path $d | Out-Null
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; $ProgressPreference = "SilentlyContinue"
$base = "https://github.com/d-j-lee/gyeot/releases/latest/download"
foreach ($f in @("bootstrap.ps1", "SHA256SUMS.txt")) { Invoke-WebRequest -UseBasicParsing -Uri "$base/$f" -OutFile (Join-Path $d $f) }
$want = ((Select-String -Path (Join-Path $d "SHA256SUMS.txt") -Pattern "  bootstrap\.ps1$").Line -split "\s+")[0]
$got = (Get-FileHash -Algorithm SHA256 (Join-Path $d "bootstrap.ps1")).Hash.ToLowerInvariant()
if ($want -and $got -eq $want) { "bootstrap.ps1 SHA-256 일치" } else { throw "bootstrap.ps1 SHA-256 이 맞지 않아요: $got / $want" }
```

- `gh`가 로그인돼 있으면 이어서 출처도 본다: `$env:GH_HOST = "github.com"; gh attestation verify (Join-Path $env:TEMP "gyeot-boot\bootstrap.ps1") --repo d-j-lee/gyeot`. 실패하면 멈추고 알린다.
- 특정 판으로 가려면 `$base`를 `https://github.com/d-j-lee/gyeot/releases/download/v0.2.0`처럼 바꾸고, 3·4단계에 `-Tag v0.2.0`을 더한다.

### 3. 미리보기 — 아직 바꾸지 않는다

1단계에서 정한 선택을 붙인다. 처음 설치의 기본은 아래와 같다.

```powershell
& (Join-Path $env:TEMP "gyeot-boot\bootstrap.ps1") -ApplyClaudeHooks -RegisterStartup -RegisterChannel
```

종료 코드가 0이고 출력 끝이 "미리보기만 했어요"여야 성공이다. 출력에 나오는 Codex 훅·`.mcp.json` 조각은 참고용이라 이 순서에서 쓰지 않는다.

- 출력의 `version:` 줄이 1단계의 `gyeotd --version`과 같으면 "이미 최신"이라고 알리고, 사람이 다시 깔라고 하지 않는 한 6단계 점검만 한다.
- 아니면 사람에게 아래 표를 보여 주고 무엇을 할지 묻는다.

| 바뀌는 것 | 선택 | 되돌리는 법 |
|---|---|---|
| `%LOCALAPPDATA%\gyeot\`에 실행 파일·얼굴·스크립트를 둔다. 받은 파일의 인터넷 표시를 지운다(`Unblock-File`). 이미 있으면 실행 중인 곁을 멈추고 바꾼 뒤 다시 띄운다 | 늘 | `uninstall.ps1`(먼저 `-DryRun`으로 볼 수 있다) |
| `~\.claude\settings.json`을 `settings.json.bak-<시각>`으로 백업한 뒤 곁의 훅 13개를 더한다. 기존 훅과 설정은 그대로 둔다 | `-ApplyClaudeHooks` | `uninstall.ps1 -RemoveClaudeHooks`, 또는 백업 파일로 되돌리기 |
| 로그인할 때 곁 데몬을 창 없이 띄운다(작업 스케줄러 `gyeot-daemon`, 안 되면 사용자 Run 값 `gyeotd`) | `-RegisterStartup` | `Unregister-ScheduledTask -TaskName gyeot-daemon -Confirm:$false`, Run 값이면 `Remove-ItemProperty -Path HKCU:\Software\Microsoft\Windows\CurrentVersion\Run -Name gyeotd` |
| `claude mcp`의 사용자 범위에 `gyeot` channel을 등록한다 | `-RegisterChannel` | `claude mcp remove gyeot --scope user` |

대답에서 빼라는 선택은 4단계에서 뺀다. 묻지 않은 선택을 더하지 않는다. `-RegisterStartup`은 권한다. 데몬이 이 설치 명령과 상관없이 작업 스케줄러에서 돌고, 다음 로그인에도 뜬다.

### 4. 설치

3단계에서 고른 선택에 `-StartShell -Install`을 붙인다.

```powershell
& (Join-Path $env:TEMP "gyeot-boot\bootstrap.ps1") -ApplyClaudeHooks -RegisterStartup -RegisterChannel -StartShell -Install
```

- 종료 코드가 0이고 끝에 "설치·점검을 마쳤어요"가 나오면 성공이다.
- 실패하면 출력에서 처음 나온 `✗` 줄이나 "하지 못했어요" 줄을 사람에게 그대로 보이고 멈춘다. 고쳐 보려고 다른 길로 돌아가지 않는다.

### 5. 보고

`bootstrap.ps1`이 마지막에 `doctor.ps1`로 점검한 표를 요약해 사람에게 보고한다. 실패가 0이면 완료이고, 경고는 이유와 함께 적는다. 그리고 알린다.

- 화면 오른쪽 아래의 아바타가 곁이다. `Ctrl+Alt+Space`로 보이고 숨긴다. 트레이 메뉴의 "로그인 시 시작"을 켜면 아바타도 로그인할 때 뜬다.
- 브라우저로 열려면 트레이의 "곁 열기", 또는 `& "$env:LOCALAPPDATA\gyeot\bin\gyeotd.exe" --open`.
- 곁의 훅은 Claude Code 세션을 새로 시작하면 확실히 붙는다. 지금 설치를 맡은 이 세션도 마찬가지다.
- 곁과 대화하려면 세션을 `claude --dangerously-load-development-channels server:gyeot`로 시작한다.
- 곁은 기본으로 소리를 내지 않는다. 승인 대기를 소리로도 받으려면 `%LOCALAPPDATA%\gyeot\config.toml`에 `approval_interrupt = true`를 적는다.

### 6. 점검만 다시 하기

```powershell
& (Join-Path $env:LOCALAPPDATA "gyeot\scripts\doctor.ps1")
```

아무것도 바꾸지 않는다. 파일·판·토큰, 데몬이 내 곁 데몬인지(포트를 넣은 HMAC 확인), 얼굴 열기, Claude 훅, 훅 실행, channel, 로그인 시 시작, 폴더 권한을 본다. 실패가 있으면 종료 코드 1이다.

## 업데이트

위의 한 줄 프롬프트를 다시 쓰면 된다. Claude Code는 1~5단계를 그대로 따르고, 1단계에서 읽은 지금 선택을 미리보기에 쓴다.

손으로 할 때는 설치된 bootstrap으로 최신 릴리스를 받는다. 먼저 미리보기를 보고, 맞으면 같은 줄에 `-Install`을 붙인다. 선택은 처음 설치 때와 같게 준다(아래는 기본값).

```powershell
& (Join-Path $env:LOCALAPPDATA "gyeot\scripts\bootstrap.ps1") -ApplyClaudeHooks -RegisterStartup -RegisterChannel
& (Join-Path $env:LOCALAPPDATA "gyeot\scripts\bootstrap.ps1") -ApplyClaudeHooks -RegisterStartup -RegisterChannel -Install
```

## 지우기

```powershell
& (Join-Path $env:LOCALAPPDATA "gyeot\scripts\uninstall.ps1") -RemoveClaudeHooks
claude mcp remove gyeot --scope user
```

먼저 `-DryRun`을 붙이면 무엇을 지울지만 보인다. 데이터(토큰·설정·이벤트 기록)까지 지우려면 `-PurgeData`를 더한다. 훅을 넣기 전의 Claude 설정은 `~\.claude\settings.json.bak-<시각>`에 있다.

## Claude Code가 지킬 것

- 실행 정책은 명령마다 `-ExecutionPolicy Bypass`로만 준다. 시스템이나 사용자의 실행 정책을 바꾸지 않는다.
- 실행 정책·제한 언어 모드·백신·SmartScreen이 막으면 멈추고 사람에게 알린다. 다른 방법으로 돌아가 실행하지 않는다.
- `~\.claude\settings.json`을 손으로 고치지 않는다. 바꾸는 것은 `install.ps1`(bootstrap이 부른다)뿐이다.
- SHA-256이나 출처 증명이 맞지 않거나, 점검이 "다른 프로그램이 쓰고 있어요"·"내 토큰을 몰라요"라고 하면 보안 경계다. 멈추고 사람에게 알린다.
- `-SkipAttestation`은 사람이 이유를 알고 고른 때만 쓴다.
- 토큰 파일 내용과 얼굴 열기 주소(`?boot=…`)를 대화나 기록에 옮기지 않는다.

## 문제가 생기면

| 증상 | 할 일 |
|---|---|
| `curl.exe`나 `Invoke-WebRequest`가 인증서·프록시 오류 | 회사망이 github.com을 막거나 검사한다. 사람에게 알린다. `gh auth login`이 되는 PC면 bootstrap이 gh로 받는다. 안 되면 사람이 브라우저로 릴리스 페이지에서 `gyeot-windows-x64.zip`·`SHA256SUMS.txt`·`bootstrap.ps1`을 한 폴더에 받고, 3·4단계를 그 폴더의 `bootstrap.ps1`로 `-ZipPath "<그 폴더>\gyeot-windows-x64.zip"`을 더해 돌린다 |
| "SHA-256 이 맞지 않아요" | 다시 받는다. 그래도 같으면 멈추고 알린다 |
| "출처 증명을 확인하지 못했어요" | 멈추고 알린다. 회사망이 확인 주소를 막는 경우가 있다. 사람이 고르면 `-SkipAttestation`(SHA-256만 확인) |
| "127.0.0.1:47321 에 닿지 않아요" | 트레이 아바타를 띄우거나 `gyeotd.exe`를 실행한 뒤 점검을 다시 한다. 그래도 안 되면 `%LOCALAPPDATA%\gyeot\gyeotd.log`를 사람에게 보인다 |
| "다른 프로그램이 쓰고 있어요"·"내 토큰을 몰라요" | 다른 프로그램이나 다른 사용자가 그 포트를 쓴다. 멈추고 알린다 |
| "없는 파일을 부르는 곁 훅" | 옛 설치 경로를 부르는 훅이다. `-ApplyClaudeHooks`로 다시 설치하면 고친다. 손으로 넣은 셸 형식 줄이면 사람이 그 줄을 지운다 |
| channel 경고(Node 없음·낮음) | 대화만 빠진다. Node 22를 깐 뒤 업데이트 명령에 `-RegisterChannel`을 넣어 다시 하면 된다 |
