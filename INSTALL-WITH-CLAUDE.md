# 곁 설치·업데이트 — Claude Code에 맡기기

업무 PC에서 할 일은 아래 한 줄을 Claude Code에 붙여 넣는 것뿐이다. 설치가 처음이면 설치하고, 이미 있으면 최신 릴리스로 바꾼다.

```
d-j-lee/gyeot 최신 릴리스로 곁을 설치(이미 있으면 업데이트)해 줘. 순서는 `gh api repos/d-j-lee/gyeot/contents/INSTALL-WITH-CLAUDE.md -H "Accept: application/vnd.github.raw"` 로 읽은 문서를 그대로 따르고, 내 설정을 바꾸기 전에는 미리보기를 보여 주고 확인을 받아.
```

이 문서의 나머지는 그 한 줄을 받은 Claude Code가 따르는 순서다. 사람이 직접 따라 해도 된다.

## 전제

- Windows 10·11, Windows PowerShell 5.1 이상(기본으로 들어 있다). 관리자 권한은 필요 없다.
- `gh`(GitHub CLI)가 깔려 있고, 이 저장소를 볼 수 있는 계정으로 로그인돼 있다. 저장소가 비공개라서 필요하다.
- Claude Code CLI(`claude`). 곁은 Claude Code 훅으로 소식을 받는다.
- 선택: Node.js 22 이상. 곁과 대화(channel)할 때만 쓴다. 없으면 그 기능만 빠진다.

## Claude Code가 따를 순서

명령은 PowerShell 기준이다. Bash(Git Bash) 도구로 부를 때는 `powershell.exe -NoProfile -ExecutionPolicy Bypass -File "<경로>" <인자>` 꼴로 넘기고, `$env:` 같은 PowerShell 변수는 배시가 풀지 않게 작은따옴표 안에 두거나 `-File` 경로를 직접 적는다.

### 1. 확인 — 아무것도 바꾸지 않는다

```powershell
gh auth status
gh release view --repo d-j-lee/gyeot --json tagName,publishedAt
claude --version
node --version
```

- `gh`가 없거나 로그인돼 있지 않거나 릴리스가 404이면 멈추고 사람에게 알린다. 다른 길: 사람이 브라우저로 릴리스 페이지에서 `gyeot-windows-x64.zip`·`SHA256SUMS.txt`·`bootstrap.ps1`을 한 폴더에 받아 두면, 4·5단계에 `-ZipPath "<그 폴더>\gyeot-windows-x64.zip"`을 더해 같은 순서로 간다.
- `node`가 없거나 22보다 낮으면 사람에게 "대화 기능만 빠진다"고 알리고, 3단계부터 `-RegisterChannel`을 뺀다.
- 이미 설치돼 있으면 지금 판을 본다: `& "$env:LOCALAPPDATA\gyeot\bin\gyeotd.exe" --version`. 릴리스 판과 같으면 "이미 최신"이라고 알리고 6단계의 점검만 한다.

### 2. bootstrap 받기

```powershell
gh release download --repo d-j-lee/gyeot --pattern bootstrap.ps1 --dir "$env:TEMP\gyeot-boot" --clobber
```

`bootstrap.ps1`은 릴리스에서 zip과 `SHA256SUMS.txt`를 받아 해시를 확인하고, 맞지 않으면 아무것도 설치하지 않는다.

### 3. 미리보기 — 아직 바꾸지 않는다

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:TEMP\gyeot-boot\bootstrap.ps1" -ApplyClaudeHooks -RegisterStartup -RegisterChannel
```

출력 끝이 "미리보기만 했어요"여야 한다. 사람에게 아래를 그대로 보여 주고, 넷 가운데 무엇을 할지 묻는다.

| 바뀌는 것 | 옵션 | 되돌리는 법 |
|---|---|---|
| `%LOCALAPPDATA%\gyeot\`에 실행 파일·얼굴·스크립트를 둔다. 이미 있으면 실행 중인 곁을 멈추고 바꾼 뒤 다시 띄운다 | 늘 | `uninstall.ps1` |
| `~\.claude\settings.json`을 `settings.json.bak-<시각>`으로 백업한 뒤 곁의 훅 13개를 더한다. 기존 훅과 설정은 그대로 둔다 | `-ApplyClaudeHooks` | `uninstall.ps1 -RemoveClaudeHooks` |
| 로그인할 때 곁 데몬을 창 없이 띄운다(작업 스케줄러, 안 되면 사용자 Run 값) | `-RegisterStartup` | `uninstall.ps1` |
| `claude mcp`의 사용자 범위에 `gyeot` channel을 등록한다 | `-RegisterChannel` | `claude mcp remove gyeot --scope user` |

대답에서 빼라는 옵션은 4단계에서 뺀다. 묻지 않은 옵션을 더하지 않는다.

### 4. 설치

3단계와 같은 옵션에 `-StartShell -Install`을 붙인다.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:TEMP\gyeot-boot\bootstrap.ps1" -ApplyClaudeHooks -RegisterStartup -RegisterChannel -StartShell -Install
```

- 종료 코드 0이고 끝에 "설치·점검을 마쳤어요"가 나오면 성공이다.
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
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\gyeot\scripts\doctor.ps1"
```

아무것도 바꾸지 않는다. 파일·판·토큰, 데몬이 내 곁 데몬인지(포트를 넣은 HMAC 확인), 얼굴 열기, Claude 훅, 훅 실행, channel, 로그인 시 시작, 폴더 권한을 본다. 실패가 있으면 종료 코드 1이다.

## 업데이트

위의 한 줄 프롬프트를 다시 쓰면 된다. 손으로 할 때는 설치된 bootstrap으로도 최신 릴리스를 받는다.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\gyeot\scripts\bootstrap.ps1" -ApplyClaudeHooks -RegisterStartup -RegisterChannel -Install
```

특정 판으로 가려면 `-Tag v0.2.0`처럼 준다.

## 지우기

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\gyeot\scripts\uninstall.ps1" -RemoveClaudeHooks
claude mcp remove gyeot --scope user
```

데이터(토큰·설정·이벤트 기록)까지 지우려면 `-PurgeData`를 더한다. 훅을 넣기 전의 Claude 설정은 `~\.claude\settings.json.bak-<시각>`에 있다.

## Claude Code가 지킬 것

- 실행 정책은 명령마다 `-ExecutionPolicy Bypass`로만 준다. 시스템이나 사용자의 실행 정책을 바꾸지 않는다.
- `~\.claude\settings.json`을 손으로 고치지 않는다. 바꾸는 것은 `install.ps1`(bootstrap이 부른다)뿐이다.
- 해시가 맞지 않거나, 점검이 "이 포트의 서버가 내 토큰을 몰라요"라고 하면 보안 경계다. 멈추고 사람에게 알린다.
- 토큰 파일 내용과 얼굴 열기 주소(`?boot=…`)를 대화나 기록에 옮기지 않는다.
- 회사 보안 소프트웨어가 실행 파일을 막으면 우회하지 않는다. 사람에게 알린다.

## 문제가 생기면

| 증상 | 할 일 |
|---|---|
| `gh`가 404·권한 없음 | 이 저장소를 볼 수 있는 계정으로 `gh auth login`. 안 되면 1단계의 수동 받기(`-ZipPath`) |
| "SHA-256 이 맞지 않아요" | `--clobber`로 다시 받는다. 그래도 같으면 멈추고 알린다 |
| "127.0.0.1:47321 에 닿지 않아요" | 트레이 아바타를 띄우거나 `gyeotd.exe`를 실행한 뒤 점검을 다시 한다. 그래도 안 되면 `%LOCALAPPDATA%\gyeot\gyeotd.log`를 사람에게 보인다 |
| "이 포트의 서버가 내 토큰을 몰라요" | 다른 사용자나 프로그램이 그 포트를 쓴다. 멈추고 알린다 |
| channel 경고(Node 없음·낮음) | 대화만 빠진다. Node 22를 깐 뒤 `bootstrap.ps1 -RegisterChannel -Install`을 다시 하면 된다 |
| 실행 파일이 SmartScreen·백신에 막힘 | 사람에게 알린다. 회사 정책일 수 있다 |
