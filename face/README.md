# 곁의 얼굴 (`face/`)

곁 데몬이 `/`에서 내보내는 웹 화면과 아바타다. TypeScript + Vite + Preact, UI 키트 없이 만든다.
규격은 `../docs/contracts.md`(Snapshot·Item·Presence·Delivery·Proposal·Session, REST §7, SSE §8, 보완 §10.1, 열쇠·답 §12, 데몬 확인·소리 설정·결정 카드 §13, 얼굴 확인·메모리 세션 §14.1),
아바타는 `../docs/avatar-spec.md`를 따른다.

## 명령

Node 22.18 이상(타입 지우기로 `.ts` 스크립트를 바로 돌린다).

| 명령 | 하는 일 |
|---|---|
| `npm install` | 의존성 설치(런타임은 `preact` 하나) |
| `npm run mock` | 가짜 곁 데몬을 `127.0.0.1:47321`에 띄운다. 배포와 같은 열쇠 흐름(§12.1): 띄울 때 개발용 토큰과 부트 주소를 보인다. `dist`가 있으면 `/`에서 얼굴을 내보낸다(인증 없이, 페이지에 비밀 없음) |
| `npm run dev` | Vite 개발 서버 `http://127.0.0.1:5173`. `/v1`·`/health`만 `GYEOT_URL`(기본 `http://127.0.0.1:47321`)로 넘긴다. 부트 주소(`?boot=`)는 개발 서버의 얼굴이 직접 읽는다 |
| `npm run build` | `dist/` 생성(데몬이 `/`에서 내보낸다, base `/`) |
| `npm test` | vitest(jsdom) 시험 |
| `npm run typecheck` | `tsc --noEmit`(strict) |
| `npm run screenshots` | `dist`를 작은 정적 서버로 띄우고 headless Chrome으로 `screenshots/*.png`를 찍는다(`CHROME=` 로 경로 변경, 인자로 이름 거르기: `npm run screenshots -- overlay-mood`) |
| `node scripts/icon.ts` | 앱 아이콘 `public/favicon.svg`·`public/app-icon.svg`(1024 원본)를 다시 만든다 |

### 데몬 없이 써 보기

얼굴은 토큰을 모른다. 배포와 똑같이 **부트 주소**(`/?boot=<코드>`, 120초 안에 한 번)로 연다. 서버는 `index.html`을 그대로 주고(`boot`를 읽지 않는다),
얼굴 스크립트가 주소에서 `boot`만 지운 뒤 `POST /v1/session {"code"}`로 **세션 열쇠**를 받아 그 탭의 `sessionStorage`에 둔다.
mock은 띄울 때 개발용 토큰(띄울 때마다 새 난수, `GYEOT_TOKEN=`으로 고정)과 부트 주소 두 개를 보인다.

```text
가짜 곁 데몬: http://127.0.0.1:47321/
  개발용 토큰: 3f9c…  (GYEOT_TOKEN 으로 정할 수 있어요)
  얼굴 열기(dist, 120초 안에 한 번):  http://127.0.0.1:47321/?boot=8d1e…
  개발 서버(npm run dev, 120초 안에 한 번): http://127.0.0.1:5173/?boot=a07b…
    오버레이는 주소 끝에 &mode=overlay — 새 주소는 이 터미널에서 Enter
    열쇠는 그 탭의 sessionStorage 에만 있어요. mock 을 다시 띄우면 열쇠가 사라져 얼굴이 안내를 보여요.
```

```sh
npm run build && npm run mock          # '얼굴 열기' 주소를 연다. 오버레이는 그 주소 끝에 &mode=overlay
# 또는 고치면서
npm run mock -- --quiet                # 터미널 1 (저절로 일어나는 대본 없이)
npm run dev                            # 터미널 2 → 터미널 1의 '개발 서버' 주소(http://127.0.0.1:5173/?boot=…)를 연다
```

- 코드는 한 번만 쓰고 120초면 끝난다. 다 썼거나 지났으면 mock 터미널에서 **Enter**를 누르면 새 주소가 나온다. 토큰으로 직접 받아도 된다:
  `curl -s -X POST -H "Authorization: Bearer <토큰>" http://127.0.0.1:47321/v1/boot` → `{"code","url","expires_ms"}`.
- 개발 서버에서는 얼굴이 `/v1/session`·`/v1/*`을 개발 서버(같은 출처)로 부르고, `vite.config.ts` 프록시가 데몬으로 넘긴다. 열쇠는 개발 서버 출처(`127.0.0.1:5173`)의 `sessionStorage`에 생긴다.
- 열쇠는 탭마다 따로다(탭을 닫으면 사라진다). 같은 탭에서 새로 고침하면 그대로 쓴다. 새 탭은 새 부트 주소로 연다.
- 열쇠도 `boot`도 없이 열면 요청을 하나도 보내지 않고 열쇠 안내를 보인다. mock을 다시 띄워도(진짜 데몬처럼 세션을 메모리에만 둔다, §14.1) 다시 잇는 얼굴의 hello 확인에서 열쇠가 걸러져 같은 안내가 뜬다(열쇠는 보내지 않는다).
- 개발 서버 프록시는 `/v1`에 `x-forwarded-port`를 싣는다(`xfwd`). 얼굴은 자기 주소의 포트(5173)로 확인 값을 계산하므로, mock은 `x-forwarded-port: 5173`인 hello에만 5173으로 계산해 준다(가짜 데몬만의 예외, 진짜 데몬은 하지 않는다).
- 쿠키는 쓰지 않는다(요청은 `credentials: 'omit'`). `?t=`도 받지 않는다.

mock은 90초마다 작업 중 → nudge → 끝남 → 답장 → interrupt 대본을 돈다(`--quiet`로 끈다). 손으로 일으키려면(토큰만, 세션 열쇠는 403, `?t=`는 안 받음):

```sh
curl -H "Authorization: Bearer <토큰>" "http://127.0.0.1:47321/mock/trigger?what=interrupt"   # nudge|reply|alert|done|working|item|reset
```

`dj-assis` 세션은 channel이 있지만 답하지 않는다(개발 채널 플래그 없이 시작된 세션 흉내, §10.1). 여기로 말을 보내면 30초 뒤 안내가 뜬다.
`work-atlas`(Codex)는 channel이 없어 `delivered_to:null`과 이유를 돌려준다.

## 화면

- **기본**(브라우저 탭·앱 창): 왼쪽에 아바타·말풍선·세션 목록, 오른쪽에 패널. 820px보다 좁으면 위아래로 쌓는다. 세션을 누르면 그 세션으로 대화 탭이 열린다.
- **오버레이** `?mode=overlay`(Tauri 투명 항상 위 창): 배경이 투명하고, 오른쪽 아래에 112px 아바타, 그 위에 말풍선. 아바타를 누르면 패널이 위·왼쪽으로 펼쳐진다. Esc·바깥 누르기·(Tauri에서) 창이 초점을 잃으면 접힌다. 막힌 일이 있으면 아바타 왼쪽 아래에 건수를 단다.

패널 머리에는 기분(한국어), 데몬이 준 `presence.line`, 집중 모드 남은 시간, 연결 상태("곁 데몬에 연결 중… N초 뒤 다시 시도" + 지금 다시)를 보인다.
세션 열쇠가 없거나 만료되면(401, 열쇠도 `boot`도 없이 열었을 때 포함) 탭 위에 "열쇠가 없거나 만료됐어요 — 트레이의 ‘곁 열기’를 누르거나 터미널에서 `gyeotd --open`을 실행하세요." 안내와 다시 확인 단추를 띄우고, 아바타는 오프라인으로 보인다.

| 탭 | 내용 |
|---|---|
| 당신 차례 | 점수 내림차순 카드. 종류 배지·프로젝트·에이전트·"4분째/어제"·점수(누르면 이유). 처리함·넘기기(과민·이미 처리·무관 중 고름)·내일 다시(1440분)·열어 봄. 승인·병합 카드에는 **승인 단추가 없고** "승인은 터미널에서 직접 하세요"를 보인다. 결정 항목의 `options`는 칩으로 그려 `POST /v1/decisions/{decision_id}`. 선택지가 있는 결정 카드에는 질문 위에 **"Claude가 묻는 거예요 — 권한 승인이 아니에요"**를 늘 보이고, 선택지 칩은 강조색 없이 중립색(글자 `--text`·바탕 `--surface`·테두리 `--border-strong`)으로 모두 같은 꼴이다(§13.5) |
| 대화 | 보낼 곳(channel 있는 세션 먼저, "연결됨"), 내 글과 Claude 답(`reply`). Enter 보내기·Shift+Enter 줄바꿈(한글 조합 중 Enter는 무시), Win+H 받아쓰기 안내. 입력창에 초점이 있으면 `POST /v1/presence/listening {on:true}`, 떠나면 `false` |
| 설정 | 집중 모드(끄기·30분·1시간·2시간·오늘), 차임 켜고 끄기·들어 보기, 음성 읽기·목소리·빠르기, 곁의 제안(받아들이기·거절), 데몬 설정(읽기 전용, `config.toml`에서 바꾼다. 말풍선·소리 기준, 멈춤 의심, 조용한 시간, **승인 대기 소리 알림**(`approval_interrupt`, 꺼져 있으면 "꺼짐 — 켜려면 config.toml에 `approval_interrupt = true`", §13.3), 표정으로만 알리는 종류) |

### 말풍선

- `deliver`의 `nudge`·`interrupt`, `reply`의 첫 줄만 띄운다. `ambient`와 `presence.line`은 띄우지 않는다(먼저 브리핑하지 않음).
- 집중 모드(`presence.focus_mode_until_ms`가 서버 시계로 앞날)이거나 기분이 `sleep`이면 `reply`는 말풍선을 띄우지 않는다(§12.4). 대화 탭 기록에는 남기고, 대화 탭에 "안 읽은 답" 수를, 오버레이 아바타 오른쪽 위에 작은 점을 단다. 대화 탭을 보면 지운다. `deliver`는 데몬이 정하므로 이 규칙과 상관없다.
- nudge 8초·reply 12초 뒤 흐려지며 사라지고, interrupt는 넘길 때까지 남는다. 마우스를 올리거나 초점이 있거나 "왜?"를 펼친 동안은 시계를 멈추고, 놓으면 최소 2초 더 보인다.
- interrupt는 떠 있는 nudge·reply를 밀어낸다. 그 밖에는 뒤에 줄을 선다(최대 3개). 같은 항목의 새 전달은 그 자리에서 바꾼다(interrupt를 nudge로 내리지 않음). 재연결 뒤 같은 전달은 한 번만.
- 모든 말풍선에 보기·왜?·넘기기가 있다. 말풍선의 넘기기는 **말풍선만** 닫고 항목은 당신 차례에 남긴다.
- 그 항목이 닫히면(`item_removed`, 재연결 스냅샷에 없음) 말풍선도 거둔다.
- 화면 읽기: 처음부터 있는 두 live 영역에 써 넣는다. nudge·reply는 `polite`, interrupt는 `assertive`.
- interrupt에서 차임(WebAudio로 만든 두 음, 소리 파일 없음)을 울리고, 음성 읽기를 켰고 `speak`가 참이면 차임 뒤에 `line`을 ko-KR 목소리로 읽는다. 목소리는 Natural·Neural > Google > 그 밖 순으로 고르고 설정에서 바꿀 수 있다. 브라우저는 사람이 한 번 누르기 전에는 소리를 막으므로 첫 클릭·키 입력 때 오디오를 깨운다.

### 대화의 전달 표시(§10.1)

`delivered_to`가 있어도 "channel에 넘김"일 뿐 Claude가 읽었다는 뜻이 아니다. 그래서 보낸 글 아래에는 "◯◯ 세션에 전달 요청함"이라고 쓰고, 30초 안에 그 세션의 `reply`가 없으면
"답이 없으면 그 세션을 `claude --dangerously-load-development-channels server:gyeot` 로 시작했는지 확인하세요"를 한 번 보인다(같은 세션에 이미 떠 있으면 또 띄우지 않음). 답이 오면 "답 받음"으로 바꾸고 안내를 거둔다.
`delivered_to:null`이면 데몬이 준 `reason`을 그대로 보이고 시작 명령 복사·다시 보내기를 둔다. `chat_id`가 `[A-Za-z0-9_-]{1,64}` 꼴이 아닌 `reply`는 버린다.

## 주소 매개변수

| 이름 | 값 | 뜻 |
|---|---|---|
| `mode` | `overlay` | 오버레이 화면 |
| `demo` | `idle`…`listening`(기분 7개), `panel`, `bubble`, `nudge`, `reply`, `sheet` | 데몬 없이 표본 데이터로. `panel`은 패널을 펼친 채, `bubble`은 interrupt 말풍선, `nudge`·`reply`는 그 말풍선, `sheet`는 기분 7개 모음 |
| `tab` | `turn`·`chat`·`settings` | 처음 열 탭 |
| `open` | `1` | 오버레이에서 패널을 펼친 채 시작 |
| `theme` | `light`·`dark` | 색 모드 고정(기본은 시스템 설정) |
| `motion` | `reduce` | 움직임 줄이기 강제(기본은 `prefers-reduced-motion`) |
| `boot` | 부트 코드 | 얼굴이 시작하자마자 `history.replaceState`로 주소에서 이것만 지우고(다른 매개변수는 글자 그대로 둔다) `POST /v1/session`으로 세션 열쇠로 바꾼다. 서버는 읽지 않는다(§12.1-3). 데모에서는 지우기만 한다 |
| `api` | URL | **개발 빌드(`npm run dev`)에서만**, `http://127.0.0.1`·`localhost`·`[::1]` 주소일 때만 API 출처를 바꾼다. 배포 빌드는 이 분기를 빌드에서 지우고 늘 같은 출처를 쓴다(§12.1-6). 데몬은 CORS를 내지 않으므로 다른 출처를 가리키면 `Authorization` 머리 때문에 요청이 막힌다 — 개발 서버에서는 프록시(같은 출처)를 쓴다 |

예: `/?mode=overlay&demo=bubble`, `/?demo=panel&tab=settings&theme=dark`, `/?demo=sheet`.

## 데이터 흐름

- 열쇠(§12.1, G-030): 얼굴은 토큰을 받지 않는다(meta·`?t=` 없음).
  1. 시작하자마자 주소에 `boot`가 있으면 `history.replaceState`로 `boot`만 지운다(`mode=overlay` 등은 그대로).
  2. 처음 요청이 필요할 때 `POST /v1/session {"code"}`로 세션 열쇠를 받아 `sessionStorage['gyeot.session'] = {"session","expires_ms"}`에 둔다. 동시에 여러 요청이 기다려도 코드는 한 번만 보낸다.
     `boot`가 없으면 저장된 열쇠를 쓴다(만료가 지났거나 꼴이 이상하면 버린다). 열쇠도 `boot`도 없으면 요청을 보내지 않는다.
  3. 그 뒤 모든 요청(REST·스트림)은 `Authorization: Bearer <세션 열쇠>` 하나로 인증한다. 쿠키는 싣지도 받지도 않는다(`credentials: 'omit'`). 주소에 비밀을 싣지 않는다.
  - `/v1/session`이 401(코드가 틀렸거나 지났거나 이미 씀)이면 아래 401과 같이 다룬다(저장된 열쇠도 지운다). 데몬에 닿지 않으면(네트워크·5xx) 코드를 쥔 채 다시 잇기 때 다시 바꾼다.
- **얼굴 확인(§14.1, G-036)**: 열쇠를 싣기 전에 — 시작할 때(저장된 열쇠든 방금 받은 열쇠든) 그리고 스트림을 새로 열 때마다(다시 잇기 포함) — `GET /v1/hello?n=<32바이트 난수 hex>`를 인증 머리 없이 보낸다.
  자기 열쇠로 `hex(HMAC-SHA256(key = SHA-256(열쇠 UTF-8)의 32바이트, msg = "gyeot-face-hello/v1|<포트>|<n>"))`를 WebCrypto로 계산해 응답의 `face` 목록에 있는지 본다(`src/lib/peer.ts`, 포트는 API 출처의 포트 = 같은 출처면 이 페이지의 포트).
  - 목록에 없으면(데몬이 멈춘 사이 그 포트를 잡은 다른 사용자의 서버, 다시 떠서 세션을 잃은 데몬) 열쇠를 **보내지 않고** 지운 뒤 401과 같이 다룬다.
  - hello가 닿지 않음·5xx·4xx·꼴 이상(`face` 목록이 없음 등)이면 끊긴 것으로 보고 다시 시도 간격을 따른다. 열쇠는 보내지도 지우지도 않는다.
  - 방금 `/v1/session`으로 받은 열쇠도 첫 스트림 전에 똑같이 확인한다(규칙 하나로 둔다).
  - REST 요청(항목 동작·말 걸기·집중 모드 등)은 이렇게 확인된 스트림이 열려 있는 동안에만 보낸다. 확인 전·끊긴 동안은 요청 없이 "곁 데몬과 연결되어 있지 않아요"로 실패한다.
- 스트림(§8): `EventSource`는 머리를 실을 수 없어서 `fetch` 스트림을 직접 읽는다(`src/lib/sse.ts`: `event:`·여러 줄 `data:`·`:` 주석 ping·`id` 없어도 됨·빈 줄에서 끝, CR/LF/CRLF, 조각난 청크·UTF-8 경계).
  끊기면(연결 실패·스트림 끝·오류) 1→2→4→…→30초 간격으로 다시 잇는다. 45초 동안 아무것도(ping 포함) 오지 않으면 끊고 다시 잇는다(데몬은 15초마다 ping). 다시 이으면 데몬이 `snapshot`을 먼저 보내 상태가 맞춰진다.
  연결마다 세대 번호를 늘려, 늦게 끝난 옛 연결의 머리·이벤트·401이 새 연결을 건드리지 못하게 한다.
- **401**(스트림이든 REST든 `/v1/session`이든, hello가 내 열쇠를 모르든): 그 열쇠를 지우고 스트림을 멈추며 **저절로 다시 두드리지 않는다**. 열쇠가 없으면 지금 다시·다시 확인·창 복귀·온라인 복귀도 요청을 보내지 않는다(안내가 깜빡이지 않는다).
  셸 안(`window.__TAURI__.core.invoke`가 있음)이면 `invoke('renew_session')`을 부른다 — 셸이 새 부트 코드로 이 창을 다시 연다. 30초에 한 번까지이고, 마지막으로 부른 시각을 이 창의 sessionStorage에 두어 다시 열린 창이 곧바로 또 부르지 않는다.
  브라우저면 열쇠 안내를 보인다(트레이의 ‘곁 열기’나 `gyeotd --open`이 새 탭을 연다).
  데몬이 다시 뜨면 세션이 모두 무효라(메모리에만 둔다) 이 흐름을 탄다: 스트림이 끊기고 → 다시 잇기 hello에서 걸러져 → 셸은 `renew_session` 한 번, 브라우저는 안내. 안내 뒤로 상태가 오락가락하거나 요청을 되풀이하지 않는다.
- REST는 8초 넘게 답이 없으면 끊는다.
- 순수 리듀서 `applyEvent(state, sseEvent)`(`src/state/reducer.ts`)가 snapshot·item·item_removed·presence·deliver·reply·proposal·sessions를 처리한다. 이상한 값은 버리거나 기본값으로 바꾼다.
- 처리함·넘기기·내일 다시·제안 답·집중 모드는 낙관적으로 바로 반영하고, 실패하면 되돌리며 한국어 알림을 띄운다. 숨긴 항목은 서명(`signature`)이 바뀌면(새 신호) 다시 보인다.
- 대화 본문은 메모리에만 두고 저장하지 않는다(창을 닫으면 사라진다). 이 기기의 소리·음성 설정만 localStorage에 둔다.

## Tauri 셸과의 약속(선택)

`window.__TAURI__?.core?.invoke`가 있을 때만 부르고, 실패는 조용히 넘긴다. 브라우저에서는 아무 일도 하지 않는다.

- `invoke('set_hit_rect', {x, y, width, height})` — 클릭을 받을 영역(CSS px, 아바타+말풍선+패널을 감싼 사각형, 4px 여유). ResizeObserver·창 크기·패널·말풍선이 바뀔 때마다, 값이 달라졌을 때만. 기본 화면에서는 창 전체.
- `invoke('set_expanded', {expanded})` — 오버레이 패널을 펼치고 접을 때.
- `invoke('renew_session')` — 세션 열쇠가 없거나(열쇠도 `boot`도 없이 열림) 401을 받았거나(`/v1/session`의 401 포함) hello가 내 열쇠를 모를 때(30초에 한 번까지). 셸이 새 부트 코드로 창을 다시 연다(§12.1-5). 데몬에 닿지 않는 것은 401이 아니므로 부르지 않고 다시 잇는다.
- 셸은 데몬 주소(`http://127.0.0.1:<포트>/?mode=overlay&boot=…`)를 직접 열므로 얼굴은 늘 같은 출처다. Tauri 출처(`tauri://`·`tauri.localhost`)에서 데몬을 부르지 않는다(§12.2).

## 폴더

```
src/
  types.ts            계약 타입
  state/reducer.ts    applyEvent·낙관적 숨김·고르개
  app/                Controller(상태·말풍선·소리·낙관적 동작), Backend(HTTP·401 처리), createBackend(주소→백엔드), Observable
  lib/                api(Authorization 머리)·session(부트 코드·세션 열쇠)·peer(얼굴 확인 hello)·stream(fetch 스트림)·sse(해석기)·bubble(차례)·time(한국어 시각)·items(정렬·점수식·FNV id)·labels·audio·voice·prefs·tauri·shape
  components/         Avatar·Bubble·Panel·TurnTab·ItemCard·ChatTab·SettingsTab·StatusLine·SessionList·MoodSheet·AuthNotice
  demo/               표본 데이터(mock 서버와 같이 씀)·DemoBackend
  styles/             theme(색 토큰, WCAG AA 시험)·avatar(움직임)·app
mock/server.ts        가짜 데몬(npm run mock). server.test.ts 가 실제로 띄워 HTTP 로 시험한다
mock/auth.ts          가짜 데몬의 부트 코드·세션 열쇠 보관함(§12.1, 메모리에 해시만 둔다)·데몬 확인 HMAC(§13.1)·얼굴 확인 face 목록(§14.1)
scripts/              screenshots.ts·icon.ts
screenshots/          찍은 화면
```

## 시험

`npm test`가 도는 것: 리듀서, 정렬·점수식·FNV id, 한국어 상대 시각, 종류·기분 이름, 말풍선 차례(가짜 시계), 승인·병합 카드에 승인 단추 없음(G-005), 모든 말풍선에 넘기기(G-006),
대화 입력(Enter·Shift+Enter·IME), §10.1 안내, SSE 해석기(조각난 청크·여러 줄 data·ping·CR/LF·BOM·크기 한도)와 fetch 스트림(다시 잇기 간격·세대·45초 감시·UTF-8 경계), API 경로·본문, 컨트롤러의 낙관적 되돌림·차임·음성, 오버레이 Esc·바깥 누르기·Tauri 호출,
데모 데이터가 계약을 지키는지, 색 대비(WCAG AA, 밝은·어두운 모드)와 결정 선택지 칩의 중립색, 움직임 줄이기,
§12: 토큰 없음(meta·`?t=`), 주소에서 `boot`만 지우기(`replaceState`, 다른 쿼리 글자 그대로), `/v1/session` 교환(한 번만·401·닿지 않음), `sessionStorage['gyeot.session']`, 모든 요청의 `Authorization: Bearer`, 쿠키·`EventSource` 안 씀(코드 전체 검사), 배포 빌드의 `?api=` 무시,
401이면 열쇠 지움·스트림 멈춤·`renew_session` 30초 제한·안내, 열쇠도 `boot`도 없으면 요청 0개, 집중·sleep 때 답은 배지로만, 데모는 네트워크·IPC를 안 씀(`main.tsx`를 실제로 띄워서도 본다),
§13: 결정 카드 안내(§13.5), 설정 탭의 `approval_interrupt`(§13.3),
§14.1: 얼굴 확인 값(서버 담당과 같은 공유 시험 값), hello를 먼저·인증 머리 없이·스트림마다 새 n, 모르는 서버면 열쇠 0건·지움·renew/안내, hello 실패는 끊김, 데몬 재시작 흐름, REST는 확인된 연결에서만,
가짜 데몬(`mock/`)의 부트 코드·`/v1/session`·세션 열쇠 범위(ingest·channel·boot는 403)·쿠키 거절·`/v1/hello` HMAC과 `face` 목록(섞임·최대 20·끝난 세션 없음)·보안 머리글·Host·Origin(실제로 띄워 HTTP로), vite 프록시가 `/v1`·`/health`만 넘기고 `/v1`에 `xfwd`를 싣는지.

## 한계

- 소리·음성·Win+H는 headless 환경에서 귀로 확인하지 않았다. Windows WebView2에서 한국어 목소리 이름과 자동 재생 정책을 실측해야 한다.
- 오버레이의 투명 창·클릭 통과·창 끌기는 Tauri 셸이 해야 한다. 얼굴은 클릭 영역과 펼침 상태만 알린다.
- 화면 사진은 리눅스 headless Chrome + Noto Sans CJK로 찍었다. Windows에서는 Pretendard(있으면) 또는 맑은 고딕으로 보인다.
- mock은 Host·Origin·Bearer(토큰·세션 열쇠와 그 범위)를 계약대로 검사한다. 다만 개발 서버(`127.0.0.1:5173`·`localhost:5173`) 출처는 받는다(vite 프록시가 Origin을 그대로 넘긴다). 진짜 데몬은 받지 않는다.
  그래서 개발 서버(`npm run dev`)는 mock에만 붙인다. 진짜 데몬에 붙이면 브라우저가 POST에 싣는 `Origin: http://127.0.0.1:5173` 때문에 `POST /v1/session`이 403이라 열쇠를 받지 못한다(프록시에서 Origin을 지우는 것은 검사를 느슨하게 하므로 하지 않았다).
  mock은 세션을 메모리에만 두고 `sessions.json`에 쓰지 않는다. 훅 수집·channel은 흉내 내지 않는다(토큰이면 404, 세션 열쇠면 403 — 경계만 계약대로).
- 열쇠는 출처(스킴·호스트·**포트**)마다 갈리는 `sessionStorage`에 있으므로 `127.0.0.1`과 `localhost`, 개발 서버(5173)와 데몬(47321)은 서로 다른 열쇠를 쓴다. 각자 부트 주소로 연다.
- 부트 코드는 `/v1/session`에 확인 없이 보낸다(아직 열쇠가 없어 확인할 값이 없다). 코드는 120초·한 번만이고 데몬 메모리에만 있어, 데몬이 멈춘 사이 포트를 잡은 서버가 받아도 다시 뜬 데몬에서는 쓸 수 없다.
- 확인된 스트림이 열려 있는 동안 데몬이 죽고 다른 서버가 포트를 잡으면, 얼굴이 끊김을 알아채기 전(보통 곧바로, 늦어도 45초 감시)에 보낸 REST 요청 하나는 그 서버로 갈 수 있다. 스트림은 다시 이을 때 늘 다시 확인한다.
- 세션 열쇠를 가진 페이지 스크립트는 얼굴이 할 수 있는 일을 다 할 수 있다(계약 §12.7). CSP가 외부 스크립트를 막지만, 얼굴에 스크립트 주입이 생기면 열쇠를 읽을 수 있다.
- 셸의 `renew_session`(창 다시 열기)은 얼굴 쪽 호출만 시험했다. 실제 셸(WebView2)에서 `navigate`로 다시 연 창이 같은 `sessionStorage`를 이어 쓰는지(30초 제한 기록)는 셸과 함께 확인해야 한다.
