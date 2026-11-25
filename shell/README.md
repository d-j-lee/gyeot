# 곁 셸 (Windows 데스크톱, Tauri 2)

곁의 얼굴을 바탕화면 오른쪽 아래에 투명하게 띄우는 **얇은 껍데기**입니다. 서버는 들어 있지 않습니다(G-002). 이벤트를 모으고 판단하는 일은 따로 사는 데몬 `gyeotd`가 하고, 셸은 데몬이 서빙하는 얼굴(`http://127.0.0.1:47321/?mode=overlay`)을 창에 띄울 뿐입니다. 그래서 셸을 꺼도 이벤트는 계속 모이고, 셸 없이 `gyeotd --open`으로 브라우저에서도 같은 얼굴을 열 수 있습니다.

## 어떻게 도나

1. 한 번만 뜹니다(tauri-plugin-single-instance). 두 번째로 실행하면 트레이의 "곁 열기"처럼 기존 창을 보이고 끝납니다.
2. 보이지 않는 창을 만들고, 셸에 든 작은 대체 화면(`dist/index.html`, "곁을 깨우는 중…")을 먼저 읽습니다.
3. `GET /health`로 데몬을 확인합니다. 응답이 없으면 `gyeotd.exe`를 **창 없이**(`CREATE_NO_WINDOW`) 띄웁니다. 찾는 순서는 셸 실행 파일 옆 → `%LOCALAPPDATA%\gyeot\bin`(`GYEOT_HOME`이 있으면 `<GYEOT_HOME>\bin`)입니다. 데몬은 셸과 따로 살아서 셸을 꺼도 남습니다.
4. 데몬이 응답하면(최대 15초) 일회용 부트 코드를 받아 얼굴을 엽니다(아래 "얼굴 열기"). 페이지를 다 읽은 뒤에 창을 보입니다. 실패하면 이유와 "다시 시도"·"데몬 다시 시작" 단추가 있는 대체 화면을 보입니다.
5. 10초마다 데몬을 살핍니다. 두 번 연속 응답이 없으면 다시 띄우고(20초부터 두 배씩, 최대 10분 간격), 작업 스케줄러 같은 다른 경로로 살아나면 새 부트 코드로 얼굴을 다시 엽니다. 데몬은 응답하는데 얼굴을 못 열었으면(부트 코드를 못 받음 등) 바로 한 번, 그 뒤로는 20초부터 두 배씩 간격을 두고 다시 엽니다.

포트는 `GYEOT_PORT` → `config.toml`의 `port` → 47321 순서로 정합니다(docs/contracts.md §1).

## 얼굴 열기 — 부트 코드와 세션 쿠키 (docs/contracts.md §12.1)

얼굴 페이지에는 토큰이 없습니다. 셸은 이렇게 엽니다.

1. 토큰 파일(`<데이터>\token`)을 읽어 `POST /v1/boot`를 `Authorization: Bearer <토큰>`으로 부릅니다(본문 `{}`). 답은 `{code, url, expires_ms}`이고, 코드는 120초 동안 한 번만 쓸 수 있습니다.
2. 셸은 서버가 준 `url`을 쓰지 않고 자기 포트로 `http://127.0.0.1:<port>/?mode=overlay&boot=<code>`를 만들어 창을 그리로 보냅니다. 코드는 hex 32–128자만 받습니다.
3. 코드가 맞으면 데몬이 `303` + 세션 쿠키(`gyeot_session`, HttpOnly·SameSite=Strict)로 `boot`를 뺀 `/?mode=overlay`로 보냅니다. 얼굴은 그 뒤 같은 출처 `fetch`·`EventSource`로 쿠키 인증을 씁니다.
4. 다 읽은 뒤에도 주소에 `boot`가 남아 있으면 데몬이 코드를 거절한 것(안내 쪽 `401`)입니다. 셸은 이것을 기록하고 창을 보이되, 감시가 되풀이해 열지는 않습니다(고리 방지). 트레이의 "곁 열기"가 새 코드로 다시 엽니다.

부트 코드를 받지 못하면(토큰 파일 없음, `401` 토큰 불일치, `404` 부트를 모르는 옛 데몬) 대체 화면에 그 이유를 보입니다.

### `renew_session` (얼굴이 부르는 IPC)

세션 쿠키가 만료·삭제되어 얼굴의 API 호출이 `401`을 받으면, 얼굴은 `renew_session`(인자 없음)을 부릅니다. 셸은 새 부트 코드를 받아 **같은 창**을 새 부트 주소로 다시 엽니다.

- 성공하면 `null`을 돌려주고 창이 다시 읽힙니다. 코드와 주소는 페이지에 돌려주지 않습니다.
- **30초에 한 번까지**입니다. 그보다 잦으면 `"곁 열쇠를 새로 받지 못했어요: 30초에 한 번만 … N초 뒤에 다시 시도하세요"` 오류 글을 돌려줍니다. 부트 실패도 같은 머리(`곁 열쇠를 새로 받지 못했어요: <이유>`)의 오류 글입니다. 얼굴은 이때 "트레이의 곁 열기" 안내를 보이면 됩니다.
- 실패하면 셸은 그 창을 "다시 열어야 함"으로 표시해서, 트레이의 "곁 열기"가 30초 제한 없이 새 코드로 엽니다(사람이 누른 것이라서).
- 데몬 얼굴(`overlay-remote`)만 부를 수 있습니다. 셸에 든 대체 화면·다른 출처·다른 창은 거절됩니다.

## 창

- 투명, 테두리·그림자 없음, 항상 위, 작업 표시줄에 없음, 크기 조절 불가.
- 처음 크기 360×520(논리 px), **주 모니터 작업 영역**(작업 표시줄을 뺀 곳)의 오른쪽 아래에 붙습니다.
- 얼굴이 패널을 펼치면(`set_expanded {expanded: true}`) 420×680으로 커지고, 오른쪽 아래 모서리는 그대로입니다. 작업 영역보다 크면 줄입니다.
- 3초마다 작업 영역을 다시 보고(작업 표시줄 이동·해상도·배율 변경) 자리를 맞춥니다.
- Alt+F4로 닫아도 숨기기만 합니다. 끝내려면 트레이의 "종료".

## 선택적 클릭 통과

창 전체는 처음에 클릭을 통과시킵니다(`set_ignore_cursor_events(true)`). 얼굴이 "여기만 클릭을 받겠다"는 사각형을 알려 주면(`set_hit_rect {x, y, width, height}`, 웹뷰 기준 CSS px), 셸이 약 30 Hz로 전역 마우스 위치(Windows `GetCursorPos`, 물리 px)를 보면서

- 마우스가 그 사각형 안이면 클릭을 받고,
- 밖이면 다시 통과시킵니다(뒤에 있는 창이 클릭됩니다).

CSS px는 창의 위치와 배율(`scale_factor`)로 물리 px로 바꿉니다. 받는 중에 단추를 누른 채 밖으로 끌면(글자 선택 등) 놓을 때까지 받습니다. 넓이가 0인 사각형을 보내거나 페이지가 새로 읽히면 어디도 받지 않는 상태로 돌아갑니다.

## 권한(capabilities)

얼굴은 원격 출처(`http://127.0.0.1:47321`)라서 Tauri가 기본으로 아무 명령도 주지 않습니다. `src-tauri/capabilities/`에서 딱 필요한 것만 엽니다.

| capability | 출처 | 허락하는 명령 |
|---|---|---|
| `overlay-remote` | `http://127.0.0.1:47321/*`, `http://localhost:47321/*` (`local: false`), 창 `main` | `set_hit_rect`, `set_expanded`, `renew_session` |
| `overlay-local` | 셸에 든 대체 화면, 창 `main` | `set_hit_rect`, `set_expanded`, `retry_connect` |

Tauri 코어 API(창·이벤트·웹뷰)와 플러그인 API는 어느 쪽에도 주지 않습니다. 포트를 바꾸면 그 포트에도 같은 세 명령만 실행 중에 더합니다. `src/acl_tests.rs`가 가짜 런타임으로 이 표를 확인합니다(얼굴의 세 명령 허락, `renew_session` 30초 제한, 다른 포트·다른 출처·다른 창·코어 API·`retry_connect` 거절, 대체 화면의 `renew_session` 거절, 바꾼 포트의 같은 세 명령). 그 밖의 주소로 가려는 이동은 막고, `https` 주소만 기본 브라우저로 엽니다.

## 보안 메모

- 셸이 데몬에 보내는 요청은 `GET /health`(인증 없음), `POST /v1/boot`·`POST /v1/focus`(토큰) 셋뿐입니다. 토큰은 `Authorization: Bearer`로만 보냅니다. 셸의 HTTP 클라이언트는 경로에 쿼리(`?`)를 받지 않아서 `?t=` 같은 쿼리 토큰이 끼어들 수 없습니다.
- 셸은 `Origin` 머리를 보내지 않습니다. 데몬은 Tauri 출처(`tauri://localhost`·`http(s)://tauri.localhost`)를 `403`으로 막고 CORS를 반사하지 않습니다(§12.2). 셸 창은 데몬 주소를 직접 열기 때문에 얼굴은 같은 출처입니다.
- 셸에 든 대체 화면(`dist/`)은 IPC만 쓰고 데몬 API를 부르지 않습니다. 셸 페이지의 CSP도 `connect-src ipc: http://ipc.localhost`뿐입니다(`overlay::tests::bundled_fallback_never_talks_to_the_daemon`이 확인).
- 부트 코드가 든 주소는 로그에 남기지 않습니다. 거절된 코드는 이미 쓸 수 없는 값입니다.

## 트레이와 단축키

알림 영역 아이콘을 왼쪽 클릭하면 보이기/숨기기, 오른쪽 클릭하면 메뉴입니다.

| 메뉴 | 하는 일 |
|---|---|
| 곁 열기 | 창을 보이고 앞으로 가져옵니다. 얼굴이 아니거나(대체 화면), 데몬이 부트 코드를 거절했거나, 얼굴의 `renew_session`이 실패했거나, 데몬 페이지가 셸 명령을 한 번도 부르지 않았으면(안내 쪽일 수 있음) 새 부트 코드로 다시 엽니다(데몬이 없으면 띄웁니다) |
| 보이기/숨기기 | 창을 보이거나 숨깁니다. 숨긴 동안은 데몬이 페이지를 다시 읽어도 저절로 보이지 않습니다 |
| 집중 모드 30분 | 토큰 파일(`<데이터>\token`)을 읽어 `Authorization: Bearer`로 `POST /v1/focus {"on":true,"minutes":30}` |
| 집중 모드 끄기 | `POST /v1/focus {"on":false}` |
| 곁 데몬 다시 시작 | 이 사용자의 `gyeotd.exe`를 끝내고(`taskkill /F`, 종료 API가 없어서) 다시 띄운 뒤 새 부트 코드로 얼굴을 다시 엽니다 |
| 로그인 시 시작 | 셸을 로그인 때 띄웁니다(tauri-plugin-autostart, `HKCU\…\Run`의 `gyeot-shell` 값). 경로에 공백이 있어도 되게 따옴표로 감싸 다시 씁니다 |
| 종료 | 셸만 끝냅니다. 데몬은 계속 돕니다 |

결과는 트레이 아이콘의 말풍선 문구로 알립니다. **Ctrl+Alt+Space**는 어디서든 보이기/숨기기입니다. 다른 프로그램이 이미 쓰고 있으면 등록하지 못한 사실만 기록하고 넘어갑니다.

## 기록

`%LOCALAPPDATA%\gyeot\shell.log`(512 KiB가 넘으면 `shell.log.1`로 넘김). 시각은 UTC 밀리초입니다. 릴리스 빌드에는 콘솔이 없어서 문제를 볼 곳은 이 파일입니다.

## Windows에서 빌드

준비: Rust stable(MSVC), Visual Studio Build Tools의 "C++를 사용한 데스크톱 개발", Node.js 22, WebView2 런타임(Windows 10/11에는 들어 있음).

```powershell
cd shell
npm ci
npx tauri build --bundles nsis
```

- 실행 파일: `shell\src-tauri\target\release\gyeot-shell.exe`
- 설치본: `shell\src-tauri\target\release\bundle\nsis\곁_0.1.0_x64-setup.exe`(현재 사용자 설치, 관리자 권한 불필요)

NSIS 설치본에는 **셸만** 들어 있습니다. 데몬·훅 심·얼굴·channel은 배포 zip의 `scripts\install.ps1`로 `%LOCALAPPDATA%\gyeot\bin`에 설치하세요. 셸은 거기 있는 `gyeotd.exe`를 찾아 띄웁니다(zip의 `bin\`에도 `gyeot-shell.exe`가 함께 들어 있어서 설치본 없이 써도 됩니다).

개발 중에는 `npx tauri dev`로 띄웁니다. 이때 셸 옆(`target\debug`)에는 데몬이 없으니, 데몬을 먼저 띄워 두거나 `%LOCALAPPDATA%\gyeot\bin`에 설치해 두세요.

### 시험

```powershell
cd shell\src-tauri
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Linux에서도 돌릴 수 있습니다(WebKitGTK 개발 패키지 필요: `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev`). CI의 `shell` 작업은 Windows에서 같은 명령을 돌립니다.

### 아이콘

`assets/gyeot-idle-1024.png`는 `tools/make_icon.py`가 docs/avatar-spec.md의 `idle` 모습(조약돌 몸, 세이지 새싹)으로 그린 원본입니다. 다시 만들려면 `npm run icon`(Python 3 + Pillow 필요).

## 한계

- Windows가 목표입니다. 다른 OS에서도 빌드·시험은 되지만 클릭 통과의 마우스 위치는 Tauri에 묻고, 데몬 재시작은 `pkill`을 씁니다.
- 클릭 통과는 약 33ms 간격으로 바뀝니다. 아주 빠르게 들어와 바로 누르면 첫 클릭이 뒤 창으로 갈 수 있습니다. 체감은 실측 전입니다(docs/roadmap.md F6).
- 얼굴이 `set_hit_rect`를 보내지 않으면 창 전체가 통과 상태로 남습니다(바탕화면을 막지 않는 쪽을 골랐습니다). 이때는 트레이 메뉴를 쓰세요.
- 배율이 다른 모니터 사이에서 주 모니터가 바뀌면 3초 안에 자리를 다시 맞춥니다. 그 사이 잠깐 어긋날 수 있습니다.
- 전체 화면 앱(게임·발표) 위에는 Windows 정책에 따라 보이지 않을 수 있습니다.
- 서명하지 않은 실행 파일이라 SmartScreen이 처음 실행을 막을 수 있습니다.
- 데몬 다시 시작은 강제 종료입니다. 데몬은 이벤트 로그를 다시 읽어 상태를 복원합니다(이벤트 소싱).
- 세션 쿠키가 WebView2에서 `303` 뒤 실제로 붙는지(SameSite=Strict, 셸이 연 이동)는 Windows 실기기에서 확인하지 않았습니다. 붙지 않으면 창에 데몬의 안내 쪽이 보이고, "곁 열기"를 눌러도 같은 쪽이 다시 뜹니다(`shell.log`에 부트를 되풀이한 기록이 남습니다).
- 데몬의 안내 쪽이 `boot` 없는 주소에 뜨면 셸은 주소만으로 알아채지 못합니다. 그래서 "곁 열기"는 셸 명령을 부른 적이 없는 데몬 페이지를 새로 엽니다. 감시가 저절로 다시 열지는 않습니다.
