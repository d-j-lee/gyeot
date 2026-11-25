# 곁의 얼굴 (`face/`)

곁 데몬이 `/`에서 내보내는 웹 화면과 아바타다. TypeScript + Vite + Preact, UI 키트 없이 만든다.
규격은 `../docs/contracts.md`(Snapshot·Item·Presence·Delivery·Proposal·Session, REST §7, SSE §8, 보완 §10.1),
아바타는 `../docs/avatar-spec.md`를 따른다.

## 명령

Node 22.18 이상(타입 지우기로 `.ts` 스크립트를 바로 돌린다).

| 명령 | 하는 일 |
|---|---|
| `npm install` | 의존성 설치(런타임은 `preact` 하나) |
| `npm run mock` | 가짜 곁 데몬을 `127.0.0.1:47321`에 띄운다(토큰 `dev`). `dist`가 있으면 토큰을 채워 `/`에서 얼굴을 내보낸다 |
| `npm run dev` | Vite 개발 서버 `http://localhost:5173`. `/v1`·`/health`는 `GYEOT_URL`(기본 `http://127.0.0.1:47321`)로 넘긴다 |
| `npm run build` | `dist/` 생성(데몬이 `/`에서 내보낸다, base `/`) |
| `npm test` | vitest(jsdom) 시험 |
| `npm run typecheck` | `tsc --noEmit`(strict) |
| `npm run screenshots` | `dist`를 작은 정적 서버로 띄우고 headless Chrome으로 `screenshots/*.png`를 찍는다(`CHROME=` 로 경로 변경, 인자로 이름 거르기: `npm run screenshots -- overlay-mood`) |
| `node scripts/icon.ts` | 앱 아이콘 `public/favicon.svg`·`public/app-icon.svg`(1024 원본)를 다시 만든다 |

### 데몬 없이 써 보기

```sh
npm run build && npm run mock          # http://127.0.0.1:47321/ , 오버레이는 /?mode=overlay
# 또는 고치면서
npm run mock -- --quiet                # 터미널 1 (저절로 일어나는 대본 없이)
npm run dev                            # 터미널 2 → http://localhost:5173/?t=dev
```

mock은 90초마다 작업 중 → nudge → 끝남 → 답장 → interrupt 대본을 돈다(`--quiet`로 끈다). 손으로 일으키려면:

```sh
curl "http://127.0.0.1:47321/mock/trigger?what=interrupt&t=dev"   # nudge|reply|alert|done|working|item|reset
```

`dj-assis` 세션은 channel이 있지만 답하지 않는다(개발 채널 플래그 없이 시작된 세션 흉내, §10.1). 여기로 말을 보내면 30초 뒤 안내가 뜬다.
`work-atlas`(Codex)는 channel이 없어 `delivered_to:null`과 이유를 돌려준다.

## 화면

- **기본**(브라우저 탭·앱 창): 왼쪽에 아바타·말풍선·세션 목록, 오른쪽에 패널. 820px보다 좁으면 위아래로 쌓는다. 세션을 누르면 그 세션으로 대화 탭이 열린다.
- **오버레이** `?mode=overlay`(Tauri 투명 항상 위 창): 배경이 투명하고, 오른쪽 아래에 112px 아바타, 그 위에 말풍선. 아바타를 누르면 패널이 위·왼쪽으로 펼쳐진다. Esc·바깥 누르기·(Tauri에서) 창이 초점을 잃으면 접힌다. 막힌 일이 있으면 아바타 왼쪽 아래에 건수를 단다.

패널 머리에는 기분(한국어), 데몬이 준 `presence.line`, 집중 모드 남은 시간, 연결 상태("곁 데몬에 연결 중… N초 뒤 다시 시도" + 지금 다시)를 보인다.

| 탭 | 내용 |
|---|---|
| 당신 차례 | 점수 내림차순 카드. 종류 배지·프로젝트·에이전트·"4분째/어제"·점수(누르면 이유). 처리함·넘기기(과민·이미 처리·무관 중 고름)·내일 다시(1440분)·열어 봄. 승인·병합 카드에는 **승인 단추가 없고** "승인은 터미널에서 직접 하세요"를 보인다. 결정 항목의 `options`는 칩으로 그려 `POST /v1/decisions/{decision_id}` |
| 대화 | 보낼 곳(channel 있는 세션 먼저, "연결됨"), 내 글과 Claude 답(`reply`). Enter 보내기·Shift+Enter 줄바꿈(한글 조합 중 Enter는 무시), Win+H 받아쓰기 안내. 입력창에 초점이 있으면 `POST /v1/presence/listening {on:true}`, 떠나면 `false` |
| 설정 | 집중 모드(끄기·30분·1시간·2시간·오늘), 차임 켜고 끄기·들어 보기, 음성 읽기·목소리·빠르기, 곁의 제안(받아들이기·거절), 데몬 설정(읽기 전용, `config.toml`에서 바꾼다) |

### 말풍선

- `deliver`의 `nudge`·`interrupt`, `reply`의 첫 줄만 띄운다. `ambient`와 `presence.line`은 띄우지 않는다(먼저 브리핑하지 않음).
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
| `t` | 토큰 | 개발용. 읽은 뒤 주소창에서 지우고 이 탭의 sessionStorage에만 둔다 |
| `api` | URL | API 주소 바꾸기(기본은 같은 출처, Tauri 묶음 `tauri://`·`tauri.localhost`에서 열리면 `http://127.0.0.1:47321`) |

예: `/?mode=overlay&demo=bubble`, `/?demo=panel&tab=settings&theme=dark`, `/?demo=sheet`.

## 데이터 흐름

- 토큰: `<meta name="gyeot-token">`(데몬이 채움) → `?t=` → sessionStorage 순.
- `EventSource(/v1/stream?t=…)`로 받고, 끊기면 1→2→4→…→30초 간격으로 다시 잇는다. 끊길 때 `/v1/state`로 원인을 가려 401이면 "토큰이 맞지 않아요"로 보인다. 다시 이으면 데몬이 `snapshot`을 먼저 보내 상태가 맞춰진다.
- REST는 `Authorization: Bearer`. 8초 넘게 답이 없으면 끊는다.
- 순수 리듀서 `applyEvent(state, sseEvent)`(`src/state/reducer.ts`)가 snapshot·item·item_removed·presence·deliver·reply·proposal·sessions를 처리한다. 이상한 값은 버리거나 기본값으로 바꾼다.
- 처리함·넘기기·내일 다시·제안 답·집중 모드는 낙관적으로 바로 반영하고, 실패하면 되돌리며 한국어 알림을 띄운다. 숨긴 항목은 서명(`signature`)이 바뀌면(새 신호) 다시 보인다.
- 대화 본문은 메모리에만 두고 저장하지 않는다(창을 닫으면 사라진다). 이 기기의 소리·음성 설정만 localStorage에 둔다.

## Tauri 셸과의 약속(선택)

`window.__TAURI__?.core?.invoke`가 있을 때만 부르고, 실패는 조용히 넘긴다. 브라우저에서는 아무 일도 하지 않는다.

- `invoke('set_hit_rect', {x, y, width, height})` — 클릭을 받을 영역(CSS px, 아바타+말풍선+패널을 감싼 사각형, 4px 여유). ResizeObserver·창 크기·패널·말풍선이 바뀔 때마다, 값이 달라졌을 때만. 기본 화면에서는 창 전체.
- `invoke('set_expanded', {expanded})` — 오버레이 패널을 펼치고 접을 때.

## 폴더

```
src/
  types.ts            계약 타입
  state/reducer.ts    applyEvent·낙관적 숨김·고르개
  app/                Controller(상태·말풍선·소리·낙관적 동작), Backend(HTTP), Observable
  lib/                api·stream(SSE)·bubble(차례)·time(한국어 시각)·items(정렬·점수식·FNV id)·labels·audio·voice·prefs·tauri·shape
  components/         Avatar·Bubble·Panel·TurnTab·ItemCard·ChatTab·SettingsTab·StatusLine·SessionList·MoodSheet
  demo/               표본 데이터(mock 서버와 같이 씀)·DemoBackend
  styles/             theme(색 토큰, WCAG AA 시험)·avatar(움직임)·app
mock/server.ts        가짜 데몬
scripts/              screenshots.ts·icon.ts
screenshots/          찍은 화면
```

## 시험

`npm test`가 도는 것: 리듀서, 정렬·점수식·FNV id, 한국어 상대 시각, 종류·기분 이름, 말풍선 차례(가짜 시계), 승인·병합 카드에 승인 단추 없음(G-005), 모든 말풍선에 넘기기(G-006),
대화 입력(Enter·Shift+Enter·IME), §10.1 안내, SSE 다시 잇기 간격, API 경로·본문, 컨트롤러의 낙관적 되돌림·차임·음성, 오버레이 Esc·바깥 누르기·Tauri 호출,
데모 데이터가 계약을 지키는지, 색 대비(WCAG AA, 밝은·어두운 모드), 움직임 줄이기.

## 한계

- 소리·음성·Win+H는 headless 환경에서 귀로 확인하지 않았다. Windows WebView2에서 한국어 목소리 이름과 자동 재생 정책을 실측해야 한다.
- 오버레이의 투명 창·클릭 통과·창 끌기는 Tauri 셸이 해야 한다. 얼굴은 클릭 영역과 펼침 상태만 알린다.
- 화면 사진은 리눅스 headless Chrome + Noto Sans CJK로 찍었다. Windows에서는 Pretendard(있으면) 또는 맑은 고딕으로 보인다.
- mock은 Host 검사를 하지 않는다(개발 서버 프록시 때문에). Origin·토큰 검사는 한다.
