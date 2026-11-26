# 계약 (contracts) — 곁의 부품들이 공유하는 단일 규격

이 문서가 정본이다. 코어·서버·훅 심·channel·얼굴·셸은 모두 여기 적힌 이름과 모양만 믿는다.
바꾸려면 이 문서를 먼저 고치고, 관련 시험을 함께 고친다.

- 시간: 모든 시각은 **UTC 밀리초 epoch 정수**(`*_ms`)다. 사람에게 보일 때만 얼굴이 현지 시각으로 바꾼다.
- 문자열: UTF-8. 사람에게 보이는 문구는 한국어다.
- 코어(`gyeot-core`)는 시계·파일·네트워크를 직접 쓰지 않는다. 지금 시각은 인자로 받는다(결정적, wasm 호환).

## 1. 경로·포트·토큰

| 항목 | 값 |
|---|---|
| 데이터 폴더 | Windows `%LOCALAPPDATA%\gyeot\`, 그 밖 `$XDG_STATE_HOME/gyeot` 또는 `~/.local/state/gyeot`. 환경 변수 `GYEOT_HOME`이 있으면 그것 |
| 토큰 | `<데이터>/token` — 32바이트 난수의 소문자 hex 64자. 데몬이 처음 뜰 때 만든다(Unix 권한 0600) |
| 설정 | `<데이터>/config.toml`(없으면 기본값). 아래 §6 |
| 이벤트 로그 | `<데이터>/events/YYYY-MM-DD.jsonl` — 정규화된 이벤트 한 줄씩(UTC 날짜). 14일 보관 |
| 스풀 | `<데이터>/spool/<ts_ms>-<pid>-<n>.json` — 데몬에 못 닿은 훅 요청. 데몬이 뜰 때와 30초마다 비운다 |
| 학습 기록 | `<데이터>/learn.json` — 종류별 반응 집계와 사용자가 받아들인 조정 |
| 포트 | `127.0.0.1:47321`. 환경 변수 `GYEOT_PORT` 또는 설정 `port` |
| 데몬 주소(심·channel) | 환경 변수 `GYEOT_URL`(기본 `http://127.0.0.1:47321`) |

## 2. 정규화 이벤트 (`Event`)

```json
{
  "id": "1764126720123-7",
  "ts_ms": 1764126720123,
  "source": "claude-code",
  "agent": "claude-code",
  "session": "abc123",
  "cwd": "C:\\work\\proj",
  "pid_hint": 4321,
  "kind": "waiting_permission",
  "raw_event": "Notification",
  "detail": { "title": "Permission needed", "message": "Claude needs your permission" }
}
```

- `source`: `claude-code` · `codex` · `codex-notify` · `aok-harness` · `fsmap-harness` · `gyeot`(사용자 동작·channel) · `custom`
- `agent`: `claude-code` · `codex` · `null`
- `pid_hint`: 훅을 띄운 프로세스(에이전트)의 PID. 심이 자기 부모 PID를 넣는다. 없으면 `null`
- `kind`와 `detail`:

| kind | 뜻 | detail 필드 |
|---|---|---|
| `session_start` | 세션 시작·재개 | `source_kind`(startup·resume·clear·compact·fork), `model?`, `title?` |
| `session_end` | 세션 종료 | `reason` |
| `prompt` | 사람이 그 세션에 입력함 | `chars`(길이만, 본문은 저장하지 않음) |
| `waiting_permission` | 권한 승인 대기 알림 | `title?`, `message` |
| `permission_request` | 승인 요청의 내용 | `tool`, `preview`(명령·경로 요약, 마스킹·300자) |
| `waiting_input` | 입력·선택 대기(elicitation·agent_needs_input·idle_prompt) | `notification_type`, `message` |
| `turn_done` | 턴 끝남(Stop) | `last_message`(마스킹·280자), `asks_question`(bool), `human_followup`(bool) |
| `turn_failed` | 턴이 API 오류로 끝남(StopFailure) | `error_type` |
| `tool_failed` | 도구 실패(PostToolUseFailure) | `tool`, `error`(120자) |
| `activity` | 작업 중 신호(PreToolUse·PostToolUse·PostToolBatch·UserPromptExpansion) | `tool?` |
| `subagent_start` · `subagent_stop` | 하위 에이전트 | `agent_type?` |
| `task_done` | 할 일 완료(TaskCompleted) | `subject`(120자) |
| `context_compact` | 맥락 압축 직전(PreCompact) | `trigger`(manual·auto) |
| `gate` | 하네스 게이트 결과(fsmap·AOK 로그) | `result`, `harness`(fsmap·aok), `event`(원래 이름) |
| `notice` | 그 밖의 알림 | `notification_type?`, `message` |
| `user_action` | 사람이 곁에서 한 일(재생용) | `item`, `action`, `reason?`, `minutes?` |
| `channel_open` · `channel_close` | channel 연결 | `cid`, `pid`, `ppid`, `cwd` |
| `decision_open` | Claude가 곁에 결정을 요청함 | `decision_id`, `question`, `options`[] |

### 2.1 Claude Code 훅 → kind (공식 문서 기준, 2026-11 확인)

공통 입력: `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `permission_mode` 등.

| hook_event_name | 조건 | kind |
|---|---|---|
| `SessionStart` | | `session_start`(`source`) |
| `SessionEnd` | | `session_end`(`reason`) |
| `UserPromptSubmit` | | `prompt`(`prompt` 길이만) |
| `Notification` | `notification_type` = `permission_prompt` | `waiting_permission` |
| `Notification` | `idle_prompt`·`agent_needs_input`·`elicitation_dialog`·`elicitation_url_dialog` | `waiting_input` |
| `Notification` | 그 밖(`auth_success`, `agent_completed`, `quota_*`, `plugin_notification` …) | `notice` |
| `PermissionRequest` | `tool_name`, `tool_input` | `permission_request` |
| `Stop` | `last_assistant_message` | `turn_done` |
| `SubagentStop` | | `subagent_stop` |
| `SubagentStart` | | `subagent_start` |
| `StopFailure` | 오류 종류는 matcher 값과 같은 필드(`error_type`·`error`·`reason` 중 있는 것) | `turn_failed` |
| `PostToolUseFailure` | `tool_name`, `error` | `tool_failed` |
| `PreToolUse`·`PostToolUse`·`PostToolBatch` | | `activity` |
| `TaskCompleted` | `task_subject` | `task_done` |
| `PreCompact` | `trigger` 또는 matcher | `context_compact` |
| 그 밖 | | 버린다(로그에도 남기지 않음) |

`asks_question`: 마지막 비어 있지 않은 줄이 `?`·`？`로 끝나거나 `할까요`·`할지`·`원하시`·`선택해`·`알려 주세요`·`which`·`should I`·`do you want`를 포함.
`human_followup`: AOK §3a 사람 후속 표현(`확인 필요`, `남음`, `남은`, `직접`, `설치해`, `사용자 몫`, `결정 \d+건`, `left to`, `your review`, `you need to`)을 포함하되 완료 표현(`병합 완료`, `nothing left`, `모두 끝`)은 먼저 지운다.

### 2.2 Codex

- 훅(`~/.codex/hooks.json`, command 핸들러): 입력은 Claude Code와 같은 꼴이라고 **가정**한다(`hook_event_name`, `session_id`, `cwd`). 같은 표로 정규화하고 `agent`=`codex`. 실측 전(F3).
- notify(`config.toml`의 `notify = [...]`): JSON이 **마지막 명령줄 인자**로 온다(제3자 구현 근거, 실측 전 F4). `type`=`agent-turn-complete` → `turn_done`(`last-assistant-message`), `approval-requested` → `waiting_permission`. 세션 키는 `thread-id`·`session_id`·`turn-id` 순.

### 2.3 하네스 로그 (사용자 레포 실측 근거)

- AOK ⑧ `<git-dir>/aok-harness/log.jsonl`: `{"t": 초(실수), "event": "stop"|"pre-bash"|"post-edit"|"session-start"|"worktree", "result": …}`.
  `stop`의 `result` ∈ `pass`·`fail`·`cannot`·`skip-*`·`released`.
- fsmap `<git-dir>/fsmap-harness/log.jsonl`: `{"t": 초(정수), "event": "gate", "session": sid, "result": "pass"|"block"|"released"|"unchanged"|"skip-background"|"push-error"|"push-uncommitted"|"push-bypass", "ms": n}`.
- 둘 다 `gate`로 정규화. 세션 키는 `session` 필드, 없으면 로그 파일이 있는 저장소 경로(`cwd`)로 매칭한다.

### 2.4 마스킹(코어의 정규화에서 한다)

저장·표시 전에: `sk-…`, `sk-ant-…`, `ghp_…`, `gho_…`, `github_pat_…`, `xox[abpr]-…`, `AKIA[0-9A-Z]{16}`, `AIza…`, `-----BEGIN … PRIVATE KEY-----` 블록, `Bearer <긴 토큰>`, URL의 `user:pass@` → `[가림]`. 프롬프트 본문·트랜스크립트는 저장하지 않는다. 미리보기는 공백을 한 칸으로 접고 길이를 자른다.

## 3. 당신 차례 항목 (`Item`)

```json
{
  "id": "it_3f9c2a1b7e5d4c60",
  "kind": "approval",
  "session": "abc123",
  "agent": "claude-code",
  "project": "proj",
  "cwd": "C:\\work\\proj",
  "title": "권한 승인 대기 · Bash",
  "body": "npm install --save-dev vitest",
  "blocking": true,
  "score": 95.3,
  "reasons": ["권한 승인 대기: 기본 95", "4분 기다림: +0.3"],
  "created_ms": 1764126720123,
  "updated_ms": 1764126720123,
  "signature": "approval:1764126720123",
  "state": "open",
  "snooze_until_ms": null,
  "options": null,
  "decision_id": null
}
```

- `id` = `it_` + FNV-1a 64비트 hex(`session` + `|` + `kind`). 한 세션·한 종류에 항목 하나.
- 한 세션은 결정류(approval → merge → decision) 가운데 처음 맞는 하나만 연다(AOK §3a 판정 순서). 결정류가 열리면 같은 세션의 `unread`·`followup`은 닫는다.
- 종류·기본 점수(AOK 관제실 §3a와 같은 이름·점수):

| kind | 여는 신호 | 기본 | blocking |
|---|---|---|---|
| `approval` | `waiting_permission`(+ 직전 10초의 `permission_request`로 제목·본문 보강) | 95 | 예 |
| `merge` | `permission_request`의 미리보기가 `gh pr merge`·`git merge`·`git push … main` | 90 | 예 |
| `decision` | `waiting_input`(idle 제외), `turn_done.asks_question`, `decision_open` | 85 | 예 |
| `failed` | `turn_failed`, `gate` 결과 `fail`·`cannot`·`block`·`push-error` | 80 | 아니오(감쇠 없음) |
| `volatile` | `gate` 결과 `push-uncommitted`(측정) | 78 | 아니오 |
| `stall` | 세션이 작업 중(마지막 이벤트가 `activity`·`prompt`·`subagent_*`)인데 `stale_minutes`(기본 30) 넘게 아무 이벤트 없음 | 70 | 아니오 |
| `followup` | `turn_done.human_followup` | 60 | 아니오 |
| `unread` | `turn_done`(질문·후속 아님), `waiting_input`의 `idle_prompt`는 기존 `unread`를 갱신 | 55 | 아니오 |
| `context` | `context_compact`(`auto`) | 45 | 아니오 |

- 점수 = 기본 + 대기 가산 − 감쇠. 대기 가산 = `min(15, 3·log2(대기시간(시간)+1))`, 대기시간은 `updated_ms`부터. 감쇠 = 7일이 지나면 하루 2점, 최대 40(`failed`는 감쇠 없음). AOK와 같은 식이다.
- 닫힘(자동): 그 세션에 `prompt`가 오면 결정류·`unread`·`followup`이 닫힌다(사람이 답했다). `activity`가 오면 `approval`·`merge`가 닫힌다(승인됨·거절됨). `session_end`는 그 세션의 모든 항목을 닫는다. `gate`가 `pass`면 같은 세션의 게이트 `failed`가 닫힌다. `turn_done`·`activity`는 `stall`을 닫는다.
- 사람 동작: `open`(열어 봄 — 상태 유지, 반응 기록), `done`(처리함), `dismiss`(넘기기, 이유 `과민`·`이미 처리`·`무관` 선택), `snooze`(분, 기본 1440 = 내일 다시). 처리·넘기기는 `signature`에 묶인다. 서명이 바뀌면(새 신호) 다시 열린다.

## 4. 존재감 (`Presence`)과 전달 (`Delivery`)

```json
{ "mood": "waiting", "level": "nudge", "focus_item": "it_…", "line": "권한 승인 1건이 기다려요", "since_ms": 1764126720123, "focus_mode_until_ms": null }
```

- `mood` 우선순위: `alert`(10분 안에 열린 `failed`) > `waiting`(열린 blocking 항목) > `listening`(얼굴이 입력 중이라고 알림) > `working`(60초 안에 `activity`가 있는 세션) > `done`(5초 동안, `task_done`·`turn_done` 직후) > `sleep`(집중 모드·조용한 시간이고 blocking 없음) > `idle`.
- 전달 수준: `ambient`(표정·자세만) · `nudge`(말풍선, 소리 없음) · `interrupt`(말풍선 + 소리·음성).
- 전달 판단(항목이 처음 열리거나 서명이 바뀔 때, 그리고 `tick`에서 대기 승격 때):
  1. 같은 세션에 60초 안에 `prompt`가 있으면 `ambient`(사람이 이미 거기 있다).
  2. 사용자가 받아들인 종류 조정이 `quiet`이면 `ambient`.
  3. 점수 < `nudge_min`(기본 80) → `ambient`.
  4. 집중 모드·조용한 시간: 점수 ≥ 95 이고 3분 넘게 기다린 blocking만 `nudge`, 나머지는 `ambient`. `interrupt` 없음.
  5. 점수 ≥ `interrupt_min`(기본 95) 이고 blocking이고 2분 넘게 기다렸으면 `interrupt` 후보, 아니면 `nudge` 후보.
  6. 예산: `nudge` 통 용량 4·15분마다 1개, `interrupt` 통 용량 1·30분마다 1개. 통이 비면 한 단계 내린다.
  - 판단마다 `reasons`(한국어)를 남긴다. 얼굴의 "왜?"가 이것을 보여 준다.
- 곁은 먼저 브리핑을 띄우지 않는다(dj-assis D-013). 요약은 사람이 부를 때만 만든다.

```json
{ "item": "it_…", "level": "interrupt", "line": "proj에서 권한 승인을 4분째 기다려요", "speak": true, "reasons": ["…"], "at_ms": 1764126720123 }
```

## 5. 제안 (`Proposal`) — 스스로 바꾸지 않는다

- 종류별로 14일 동안 `dismiss`가 5번 이상이고 넘기기 비율이 0.7 이상이면 제안을 연다: "최근 ‘안 읽은 결과’ 알림을 7번 중 6번 넘기셨어요. 이 종류는 앞으로 조용히(표정만) 알릴까요?"
- 받아들이면 그 종류에 `quiet` 조정이 생긴다. 거절하면 14일 동안 같은 제안을 다시 열지 않는다. 조정은 설정 화면에서 언제든 되돌린다.
- 넘기기 이유 `과민`만 셈한다. `이미 처리`·`무관`은 집계만 한다(AOK §3a: 무시 기록은 임계 조정을 제안하는 데만 쓴다).

```json
{ "id": "pr_unread_quiet", "kind": "unread", "change": "quiet", "line": "…", "evidence": { "dismissed": 6, "total": 7, "window_days": 14 } }
```

## 6. 설정 (`config.toml`, 모두 선택)

```toml
port = 47321
stale_minutes = 30
nudge_min = 80
interrupt_min = 95
approval_interrupt = false      # 승인·병합 대기를 소리(interrupt)까지 올릴지(§13.3)
quiet_hours = ["22:00-08:00"]   # 현지 시각, 빈 배열이면 없음
utc_offset_minutes = 540        # 조용한 시간 계산용(Windows 현지 오프셋을 데몬이 채움)
retention_days = 14
[[tail]]                        # 하네스 로그 등 따라 읽을 JSONL
path = "C:/work/proj/.git/fsmap-harness/log.jsonl"
source = "fsmap-harness"
cwd = "C:/work/proj"
```

## 7. 로컬 HTTP API (`gyeot-server`)

- 바인드는 `127.0.0.1`만. 요청 `Host`가 `127.0.0.1:<port>`·`localhost:<port>`가 아니면 `421`(DNS 리바인딩 방어). `Host` 머리가 있는데 읽을 수 없으면(비UTF-8 등) 요청줄의 주소로 넘어가지 않고 `421`.
- `Origin` 헤더가 있으면 `http://127.0.0.1:<port>`·`http://localhost:<port>`만 허용, 그 밖(`null`·Tauri 출처 포함)은 `403`. CORS 헤더는 내지 않는다(같은 출처만).
- 인증은 `Authorization: Bearer <값>` 하나다. 값은 둘 중 하나다. 쿼리(`?t=`)·쿠키는 받지 않는다(URL과 브라우저가 저절로 싣는 곳에 비밀을 두지 않는다).
  - **토큰**: 토큰 파일의 값. 상수 시간 비교. 훅 심·channel·셸·`gyeotd --open`이 쓴다. 모든 `/v1/*`를 쓸 수 있다.
  - **세션 열쇠**: 얼굴용. 일회용 부트 코드를 `POST /v1/session`으로 바꿔 받는다(§12.1). 얼굴에 필요한 경로만 쓸 수 있다(아래 표의 "세션" 열). 그 밖은 `403 {"ok":false,"error":"이 열쇠로는 쓸 수 없어요"}`.
  - 없거나 틀리면 `401`.
- 토큰을 보내는 쪽은 보내기 전에 `GET /v1/hello`로 그 포트의 서버가 같은 토큰을 아는지 확인한다(§13.1). 확인이 안 되면 토큰도 페이로드도 보내지 않는다.
- 본문 최대 1 MiB.
- 얼굴 응답(`/`·`/assets/*`)에는 `Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; media-src 'self' data: blob:; connect-src 'self' ipc: http://ipc.localhost; frame-ancestors 'none'; base-uri 'none'; form-action 'none'`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, `X-Content-Type-Options: nosniff`, `Cache-Control: no-store`(index)를 붙인다.

| 메서드·경로 | 토큰 | 세션 | 본문 → 응답 |
|---|---|---|---|
| `GET /health` | 없어도 됨 | 없어도 됨 | `{"ok":true,"version":"0.2.0"}`. 곁 데몬인지는 이것으로 판단하지 않는다(§13.1) |
| `GET /v1/hello?n=<hex>` | 없어도 됨 | 없어도 됨 | → `{"v":1,"proof":"<64 hex>"}`(§13.1). `n`이 16–128자 hex가 아니면 `400` |
| `GET /`·`/index.html` | 없어도 됨 | 없어도 됨 | 얼굴 `index.html`. 비밀이 없다. 서버는 `?boot=`를 읽지 않는다(얼굴 스크립트가 쓴다) |
| `GET /assets/*` | 없어도 됨 | 없어도 됨 | 얼굴 정적 파일 |
| `POST /v1/boot` | 예 | 아니오 | `{}` → `{"code":str,"url":"http://127.0.0.1:<port>/?boot=<code>","expires_ms":n}`. 본문을 끝까지 읽은 뒤 답한다 |
| `POST /v1/session` | 없어도 됨 | 없어도 됨 | `{"code":str}` → `{"session":"<64 hex>","expires_ms":n}`. 코드가 틀렸거나 지났거나 이미 썼으면 `401` |
| `POST /v1/ingest/{source}` | 예 | 아니오 | 원본 페이로드 JSON. 심은 `{"payload":…, "pid_hint":n, "received_ms":n}` 봉투로 보낸다 → `202 {"accepted":k}` |
| `GET /v1/state` | 예 | 예 | `Snapshot` |
| `GET /v1/stream` | 예 | 예 | SSE(§8) |
| `POST /v1/items/{id}/action` | 예 | 예 | `{"action":"open"\|"done"\|"dismiss"\|"snooze","reason"?:str,"minutes"?:n}` → `{"ok":true}` |
| `POST /v1/focus` | 예 | 예 | `{"on":bool,"minutes"?:n}` → `{"ok":true,"until_ms":n\|null}` |
| `POST /v1/presence/listening` | 예 | 예 | `{"on":bool}` → `{"ok":true}` |
| `POST /v1/say` | 예 | 예 | `{"text":str,"session"?:str}` → `{"chat_id":str\|null,"delivered_to":str\|null,"reason":str}` |
| `POST /v1/decisions/{decision_id}` | 예 | 예 | `{"choice":str}` → `{"delivered_to":str\|null}`. 선택지에 없는 답 `400`, 이미 답함 `409` |
| `POST /v1/proposals/{id}` | 예 | 예 | `{"accept":bool}` → `{"ok":true}` |
| `POST /v1/adjustments/{kind}` | 예 | 예 | `{"quiet":false}` → `{"ok":true}`. 받아들인 조정을 되돌린다. 켜기(`true`)는 제안으로만 하므로 `400` |
| `POST /v1/channel/register` | 예 | 아니오 | `{"pid":n,"ppid":n,"cwd":str}` → `{"cid":str,"session":str\|null}` |
| `GET /v1/channel/stream?cid=` | 예 | 아니오 | SSE: `message` 이벤트 `{"chat_id":str,"text":str,"kind":"say"\|"choice"}` |
| `POST /v1/channel/reply` | 예 | 아니오 | `{"cid":str,"chat_id":str,"text":str}` → `{"ok":true}` |
| `POST /v1/channel/decision` | 예 | 아니오 | `{"cid":str,"question":str,"options":[str]}`(2–4개) → `{"decision_id":str}` |
| `POST /v1/channel/unregister` | 예 | 아니오 | `{"cid":str}` → `{"ok":true}` |

`Snapshot`:

```json
{
  "version": "0.2.0",
  "now_ms": 1764126720123,
  "presence": { "mood": "idle", "level": "ambient", "focus_item": null, "line": "", "since_ms": 0, "focus_mode_until_ms": null },
  "items": [ "Item…" ],
  "sessions": [ { "id": "abc123", "agent": "claude-code", "project": "proj", "cwd": "…", "state": "working", "last_ms": 0, "channel": "ch_1" } ],
  "proposals": [ "Proposal…" ],
  "budget": { "nudge": 4, "interrupt": 1 },
  "settings": { "nudge_min": 80, "interrupt_min": 95, "stale_minutes": 30, "quiet_hours": [], "quiet_kinds": [], "approval_interrupt": false }
}
```

세션 `state` ∈ `working`·`waiting`(열린 blocking)·`idle`·`ended`. 세션과 channel은 `pid_hint == channel.ppid`로 먼저 잇고, 못 이으면 같은 `cwd`의 가장 최근 세션(channel이 없는 것)으로 잇는다.

`POST /v1/say`의 대상: `session`이 있으면 그 세션의 channel, 없으면 channel이 있는 세션 중 가장 최근에 움직인 것. channel이 없으면 `delivered_to:null`, `reason:"channel 없음 — 세션을 --dangerously-load-development-channels server:gyeot 로 시작하세요"`.

## 8. SSE (`/v1/stream`)

이벤트 이름과 `data`(JSON):

| event | data |
|---|---|
| `snapshot` | `Snapshot`(연결 직후 한 번, 그 뒤 재동기화가 필요할 때) |
| `item` | `Item`(새로 열림·갱신) |
| `item_removed` | `{"id":str}`(닫힘·처리·넘기기·미룸) |
| `presence` | `Presence` |
| `deliver` | `Delivery` |
| `reply` | `{"session":str\|null,"project":str\|null,"chat_id":str,"text":str,"at_ms":n}` — Claude가 channel로 보낸 답 |
| `proposal` | `Proposal` |
| `sessions` | `[Session]` |

15초마다 주석 줄(`: ping`)을 보낸다. 응답 머리는 `Content-Type: text/event-stream`이다. 얼굴은 `fetch` 스트림으로 읽고(`Authorization` 머리), 45초 동안 아무것도 받지 못하면 끊긴 것으로 보고 다시 잇는다.

## 9. 훅 심 (`gyeot-hook`)

- 사용: `gyeot-hook <source>`(stdin JSON) 또는 `gyeot-hook codex-notify <json>`(인자 JSON).
- stdin은 최대 1 MiB까지 읽는다. **stdout에 아무것도 쓰지 않는다**(Claude Code가 훅 출력으로 해석한다). 늘 종료 코드 0.
- 데몬에 400ms 제한으로 `POST /v1/ingest/<source>`. 실패하면 스풀에 쓴다. 성공하면 스풀에 남은 것 가운데 최대 50개를 이어서 보낸다.
- 토큰은 `<데이터>/token`에서 읽는다. 없으면 스풀에만 쓴다.

## 10. channel (`channel/`)

- MCP 서버 이름 `gyeot`. capability `experimental['claude/channel'] = {}`, `tools = {}`. **`claude/channel/permission`은 선언하지 않는다**(권한 승인은 터미널에서 사람이 한다, dj-assis D-009·AOK v2).
- 시작하면 `POST /v1/channel/register`(pid·ppid·cwd) → `cid`, 그 뒤 `GET /v1/channel/stream?cid=`를 열어 둔다(끊기면 1→2→4…최대 30초 간격으로 다시).
- `message`를 받으면 `notifications/claude/channel` `{content:text, meta:{chat_id, from:"dj", via:"gyeot", kind}}`.
- 도구:
  - `reply {chat_id, text}` → `POST /v1/channel/reply`.
  - `request_decision {question, options[2..4]}` → `POST /v1/channel/decision`. 사람이 고르면 `kind:"choice"` 메시지 `"[선택] <옵션>"`가 같은 세션으로 돌아온다.
- 데몬이 없을 때 도구 호출은 `isError`로 "곁 데몬에 닿지 않음"을 돌려준다.

### 10.1 보완 (2026-11-26, channel 구현 때 정함)

- **모르는 cid**: 데몬은 `/v1/channel/stream`·`/reply`·`/decision`에서 모르는 `cid`에 `404`로 답한다. channel은 `404`·`410`을 받으면 한 번 다시 등록한다.
- **스트림 ping**: 데몬은 channel 스트림에도 15초마다 `: ping`을 보낸다(Node fetch는 300초 조용하면 끊는다).
- **닫힘 판정**: Windows의 오래된 Claude Code는 unregister 없이 프로세스를 끝낼 수 있다. 데몬은 channel 스트림이 끊기고 10초 안에 같은 cid로 다시 붙지 않으면 그 channel을 닫힌 것으로 본다(`channel_close`).
- **`chat_id` 형식**: `[A-Za-z0-9_-]{1,64}`. 데몬은 이 꼴로만 만든다(예: `m17`). channel은 다른 꼴을 버리고, 도구 입력에서도 거절한다.
- **한도**: 들어오는 text 32,000자(넘으면 자르고 표시), reply 16,000자, question 300자, 선택지 각 60자·중복 금지.
- **`GYEOT_URL`**: channel·심은 `http://127.0.0.1`·`http://localhost`만 받는다(토큰을 보내므로).
- **등록만 하고 스트림을 열지 않은 channel**은 60초 뒤 닫는다. 데몬이 다시 뜨면 재생으로 되살아난 channel은 모두 닫고 새 등록을 기다린다.
- **전달 확인의 한계**: 세션을 개발 채널 플래그 없이 시작해도 channel 프로세스는 뜨고 등록된다. 그때 Claude Code는 메시지를 조용히 버리지만 데몬은 알 수 없다. 그래서 `/v1/say`의 `delivered_to`는 "channel에 넘김"이지 "Claude가 읽음"이 아니다. 얼굴은 이 차이를 문구로 밝힌다("세션에 전달 요청함 — 답이 없으면 그 세션이 channel 플래그로 시작됐는지 확인하세요").

## 11. 구현 보완 (2026-11-26, v0.1 구현 때 정함)

계약 원문에 없거나 모호해서 구현이 정한 것이다. 시험이 이 동작을 지킨다.

**항목·턴**
- `turn_done`·`turn_failed`는 같은 세션의 `approval`·`merge`와 channel이 아닌 `decision`을 닫는다(턴이 끝나면 더 기다리지 않는다). channel이 연 결정은 `turn_done`이 닫거나 덮지 않는다.
- `activity`는 도구가 같거나 알 수 없을 때만 `approval`을 닫는다. 병렬로 돈 다른 도구 때문에 진짜 승인 대기를 놓치지 않기 위해서다(§3 원문 "activity가 오면 닫힘"보다 좁다).
- 병합처럼 보이는 `permission_request`는 10초 동안 보류한다. 그 안에 승인 알림이 오면 `approval`(병합 미리보기로 보강), 오지 않으면 `merge`를 연다. 알림과 요청 어느 쪽이 먼저 와도 보강된다.
- `idle_prompt`는 열린 `unread`를 그대로 두고 새로 열지 않는다. 미룬 항목은 새 신호가 오면 다시 열리고, 미룸이 끝나면 전달 판단을 다시 한다.
- 감쇠는 7일이 지난 뒤 다 채운 하루마다 2점씩이다.
- 세션도 저장소 경로도 맞지 않는 하네스 이벤트는 임시 세션 `cwd:<경로>`에 둔다.
- 끝난 세션은 1시간 뒤, 조용한 세션은 48시간 뒤 목록에서 뺀다(열린 항목이 없을 때). `Item`·`Session`의 `project`·`cwd`는 모르면 `""`다.

**전달·존재감·제안**
- 예산을 적용하기 전 후보 수준이 올라갈 때만 전달 판단을 다시 한다. 예산이 없어 한 단계 내린 것은 다시 시도하지 않는다. `interrupt`가 `nudge`로 내려가면 `nudge` 예산을 쓴다. `ambient` 판단도 `deliver`로 보낸다(얼굴의 "왜?"용).
- 점수는 소수 첫째 자리로 반올림한 값으로 판단한다. 예산은 GCRA 한 값으로 계산한다(확인 횟수와 상관없이 같은 결과).
- 시간에 따라 바뀌는 일(멈춤 의심, 미룸 끝, 대기 승격, 기분 만료, 조용한 시간 경계)은 모두 예정 시각에 처리한다. `tick`을 얼마나 자주 부르든 재생 결과가 같다.
- `listening`은 2분 뒤 저절로 풀린다. 집중 모드는 분을 주지 않으면 60분이다(1–1440). 미룸은 1–43200분이다.
- `Proposal`에는 `opened_ms`가 있다. 근거의 `total`은 14일 동안 연 알림 수, `dismissed`는 `과민` 넘기기 수다. 학습 집계는 (항목·서명·동작) 열쇠로 중복을 막는다. 그래야 `learn.json`과 48시간 재생이 같은 동작을 두 번 세지 않는다.

**재생용 `user_action` 동작**: `open`·`done`·`dismiss`·`snooze`에 더해 `focus_on`(분)·`focus_off`, `listening_on`·`listening_off`, `proposal_accept`·`proposal_reject`(item에 제안 id), `decide`(item에 decision_id), `unquiet`(item에 kind 이름).

**저장·수집**
- 로그 파일 날짜는 쓴 시각(UTC) 기준이다. 그래서 로그 순서가 적용 순서와 같다. 시작할 때 48시간을 재생한다.
- 스풀 파일 내용은 `{"source","payload","pid_hint","received_ms"}`다. 집어 간 뒤 5분이 지난 파일은 다시 집는다. 심은 10,000개 넘게 쌓지 않는다.
- `custom` 소스는 이미 정규화된 꼴(`kind`·`detail`)을 받는다. 내부 전용 kind는 거절하고 글자 필드는 모두 마스킹한다.
- `UserPromptExpansion`은 `activity`로 본다. `notification_type`이 없으면 문구로 짐작한다.
- tail은 처음 보는 기존 파일을 끝에서부터 읽는다(옛 기록은 들이지 않는다). 파일이 바뀌었는지는 첫 줄 지문으로 알아챈다. 위치는 `<데이터>/tails.json`에 둔다.
- 설정의 모르는 키는 경고만 하고, 잘못된 값은 시작을 막는다. 데몬 로그는 `<데이터>/gyeotd.log`(5 MiB 넘으면 시작할 때 비움)다.

**훅 심**
- `pid_hint`: 부모가 셸(sh·bash·cmd·powershell 등)이면 최대 3단계 위까지 올라간다.
- stdin은 최대 1초 기다린다(호스트가 stdin을 닫지 않을 때만 걸린다). 1 MiB가 넘는 페이로드는 정규화에 쓰는 필드만 남긴다.
- `GYEOT_URL`이 루프백이 아니면 보내지 않고 스풀에만 쓴다.

**셸·설치** (`shell/README.md`, `scripts/`)
- 셸 로그는 `<데이터>\shell.log`, 설치 스크립트 사본은 `<데이터>\scripts\`, 출력한 설정 조각은 `<데이터>\snippets\`에 둔다.
- 로그인 시작 작업의 이름은 `gyeot-daemon`, 레지스트리 Run 값의 이름은 `gyeotd`·`gyeot-shell`이다.

## 12. 보안·정확성 개정 (2026-11-26, 독립 검증 R1 반영)

### 12.1 얼굴 열기 — 일회용 부트 코드와 세션 열쇠 (R1 H1·M1·L3, R3 H1로 개정)

얼굴 페이지에는 비밀을 싣지 않고, 브라우저가 저절로 싣는 곳(쿠키)에도 두지 않는다. 쿠키는 포트를 가리지 않아 같은 호스트의 다른 포트 서버로 샌다(R3에서 실측, G-030).

1. 토큰을 가진 쪽(셸·`gyeotd --open`)이 `POST /v1/boot`(토큰)로 코드를 받는다. 코드는 32바이트 난수 hex, **120초 동안 한 번만** 쓸 수 있다. 동시에 열린 코드는 최대 8개(넘으면 가장 오래된 것부터 버린다).
2. 창이 `http://127.0.0.1:<port>/?boot=<code>&…`를 연다(`?mode=overlay` 등은 그대로 둔다). 서버는 `index.html`을 그대로 준다.
3. 얼굴 스크립트는 시작하자마자 `history.replaceState`로 주소에서 `boot`만 지우고, `POST /v1/session {"code"}`로 세션 열쇠를 받는다. 열쇠는 `sessionStorage`(출처 = 스킴·호스트·**포트**마다 갈리고 탭이 닫히면 사라진다)의 `gyeot.session`에 `{"session","expires_ms"}`로 둔다. 그 뒤 모든 요청은 `Authorization: Bearer <세션 열쇠>`로 한다. SSE는 `EventSource`가 머리를 실을 수 없으므로 `fetch` 스트림으로 읽는다(형식은 §8 그대로).
4. 세션은 만든 때부터 **7일** 쓸 수 있고, **데몬 메모리에만** 있다(§14.1). 데몬이 다시 뜨면 모든 세션 열쇠가 무효다. 마지막 사용 시각은 개수 상한에서 버릴 순서에만 쓴다. 세션은 최대 20개, 서버는 세션 값의 SHA-256만 들고 있다.
5. 얼굴이 `401`을 받으면 열쇠를 지우고, 셸 안이면 IPC `renew_session`을 부르고(셸이 새 코드로 같은 창을 다시 연다, 30초에 한 번까지), 브라우저면 "열쇠가 없거나 만료됐어요 — 트레이의 곁 열기 또는 `gyeotd --open`" 안내를 보인다. 열쇠도 `boot`도 없이 열면 같은 안내를 보인다.
6. `?api=`(다른 API 주소)는 개발 빌드(`import.meta.env.DEV`)에서 루프백 주소일 때만 받는다. 배포 빌드는 늘 같은 출처다.

`gyeotd --open [--print]`: 실행 중인 데몬을 §13.1로 확인하고 `POST /v1/boot` 한다. 코드가 든 주소를 프로그램 인자에 싣지 않는다(Linux·macOS에서는 다른 사용자가 `ps`·`/proc`로 읽어 먼저 쓸 수 있다, R2 N1). 대신 `<데이터>/open/open-<난수>.html`(폴더 0700, 파일 0600)에 그 주소로 넘어가는 쪽을 쓰고, 그 **파일 경로**로 기본 브라우저를 연다. Windows는 `rundll32 url.dll,FileProtocolHandler <경로>`, Linux는 `xdg-open`, macOS는 `open`. 5분 넘은 `open-*.html`은 실행할 때마다 지운다. `--print`면 주소를 표준 출력에만 쓴다.
셸: §13.1로 확인한 뒤 `POST /v1/boot`로 코드를 받아 `http://127.0.0.1:<port>/?mode=overlay&boot=<code>`를 연다. 원격 IPC 허용 명령은 `set_hit_rect`·`set_expanded`·`renew_session` 셋이다.

### 12.2 Tauri 출처 허용 삭제 (R1 M2)
셸은 데몬 주소를 직접 열므로 페이지 출처가 같은 출처다. 셸에 묶인 대체 화면은 데몬 API를 부르지 않는다. 그래서 Tauri 출처(`tauri://localhost`·`http(s)://tauri.localhost`)를 허용하지 않고, CORS 반사도 하지 않는다.

### 12.3 승인 대기와 작업 신호 (R1 H2)
- 기본 설치는 `PostToolBatch` 훅을 넣는다(병렬 도구 묶음이 끝나면 한 번, 승인 대기 중인 도구를 포함한다). 설치 스크립트의 `-NoActivityHook`을 주면 뺀다.
- 코어: 한 번도 `activity`를 낸 적이 없는 세션은 사람이 승인했는지 알 수 없다. 그런 세션의 `approval`·`merge`는 대기 시간으로 `interrupt`까지 올리지 않는다(`nudge`가 최대). 이유 줄: "이 세션은 작업 신호가 없어 승인 여부를 알 수 없어요 → 말풍선까지만".
- `PostToolBatch`는 병렬 도구 묶음의 모든 호출이 끝난 뒤, 다음 모델 호출 전에 한 번 온다(공식 hooks 레퍼런스, 2026-11-26 열람). 그 세션의 `approval`·`merge`를 닫는다. 입력의 `tool_calls`(도구마다 `tool_input`·`tool_response`가 들어 있다)는 심이 줄일 때 버리므로, 정규화는 이것을 도구 이름 없는 activity, 곧 G-016의 '알 수 없는 도구'로 본다.

### 12.4 Claude의 답(reply) (R1 L1)
- `/v1/channel/reply`는 그 channel에 데몬이 발급한 `chat_id`(say의 `m<n>`, 결정의 `d…`)만 받는다. 발급 뒤 24시간 안, channel마다 최근 256개. 그 밖은 `400 {"ok":false,"error":"발급하지 않은 chat_id"}`(다른 API 오류와 같은 모양). 검사 순서는 모르는 cid `404` → 꼴 `400` → 빈 글·길이 `400` → 발급 여부 `400`. 같은 id로 여러 번 답해도 된다. channel이 다시 등록되거나 데몬이 다시 뜨면 이전 cid에 발급한 id는 쓸 수 없다.
- 얼굴: 집중 모드이거나 존재감이 `sleep`이면 reply를 말풍선으로 띄우지 않고 대화 탭 기록과 배지로만 보인다.

### 12.5 훅 심의 스풀 (R1 M3·L6)
- 심은 보내기 전에 페이로드를 줄인다(`gyeot_core::sanitize_payload`, 순수 함수). 이벤트마다 정규화가 읽는 필드만 남긴다.
  - `prompt`는 글자 수가 같은 `x` 문자열로 바꾼다(정규화는 길이만 쓴다).
  - 미리보기로만 쓰는 글(`message` 등)은 마스킹 → 공백 접기 → 4,000자로 자른다.
  - `last_assistant_message`는 줄 구조가 질문 판단에 쓰이므로 마스킹만 한다. 4,000자가 넘으면 앞 2,000자와 끝 1,999자를 줄바꿈으로 잇는다.
  - `tool_input`은 정규화가 미리보기로 고르는 필드 **하나**만 남긴다(예: Bash는 `command`). 고를 필드가 없으면(MCP 도구 등) `tool_input` JSON을 마스킹해 300자로 자른 글로 바꾼다. activity 이벤트(`PreToolUse`·`PostToolUse`·`PostToolBatch`)는 `tool_name`만 남긴다.
  - 그 밖(`transcript_path`·`tool_response`·`tool_calls`·`content`·`old_string` 등)은 버린다. 식별자(`session_id`·`cwd`·`hook_event_name`)는 그대로 둔다.
  - 4,000자 이하 입력에서는 `normalize(원본) == normalize(줄인 것)`이다. 모르는 source는 보내지도 스풀에 쓰지도 않는다.
- 스풀 파일 권한은 Unix 0600, 폴더 0700(Windows는 `%LOCALAPPDATA%`의 사용자 전용 권한을 이어받는다).
- stdin이 1 MiB를 넘으면 나머지를 마감 시각까지 읽어 버린다(호스트에 EPIPE를 내지 않는다).

### 12.6 병합 감지 (R1 L5)
`permission_request` 미리보기가 다음 가운데 하나면 병합이다: `gh pr merge`, `git merge`(옵션 사이에 `-C <경로>` 같은 앞 옵션 허용), `git … push` 뒤 브랜치 자리가 정확히 `main`·`master`(앞은 공백·`:`, 뒤는 공백·끝·`:`). `feature/main-fix`처럼 단어 일부는 병합이 아니다.

### 12.7 남은 위험 (알고 받아들인 것)
- **1 MiB가 넘는 훅 입력**은 앞부분에서 최상위 문자열 필드만 건진다. 필드 순서가 `tool_calls` 뒤라면 건지지 못하고 그 신호를 잃는다(Claude Code의 필드 순서는 실측 전). 잃으면 승인 대기가 다음 `PostToolBatch`·`Stop`까지 남는다.
- **늦게 도착한 옛 대기 신호**(시각이 이른 `Notification`이 닫힌 뒤에 도착)는 승인 대기를 다시 열 수 있다(R3 L2). 데몬 재시작 경로에서는 스풀을 먼저 재생해 재현되지 않았다. 심의 시간 초과처럼 좁은 경우만 남는다.
- **세션 열쇠를 가진 쪽은 얼굴이 할 수 있는 일(말 걸기 포함)을 할 수 있다.** 열쇠는 그 출처의 `sessionStorage`에만 있고 CSP가 외부 스크립트를 막지만, 얼굴에 스크립트 주입이 생기면 그만큼 쓸 수 있다.

## 13. 보안·정확성 개정 2 (2026-11-26, 독립 검증 R2·R3 반영)

### 13.1 데몬 확인 — 토큰을 보내기 전에 (R3 M2)
`47321`은 모든 OS 사용자가 같이 쓰는 포트다. 내 데몬이 없을 때 다른 사용자가 그 포트를 먼저 잡으면, 심·`--open`·셸·channel이 그쪽에 토큰과 페이로드를 보낸다. 그래서 토큰을 쓰는 쪽은 먼저 서버가 같은 토큰을 아는지 확인한다.

- 클라이언트는 늘 `127.0.0.1`로 연결한다(`localhost`도 `127.0.0.1`로 바꾼다). `localhost`가 `::1`로 풀리면 그 주소의 같은 포트를 잡은 서버가 진짜 데몬에 hello를 중계해 확인을 통과할 수 있어서다.
- 클라이언트: 32바이트 난수 `n`(hex 64자)을 만들어 `GET /v1/hello?n=<n>`을 보낸다(인증 머리 없음). 셸은 확인한 바로 그 연결(keep-alive)로 토큰 요청을 보낸다. 서버는 hello 응답 뒤 연결을 닫지 않는다.
- 서버: `proof = hex(HMAC-SHA256(key = 토큰 문자열의 UTF-8 바이트, msg = "gyeot-hello/v1|" + <서버가 실제로 묶인 포트의 10진수> + "|" + n))`을 `{"v":1,"proof":…}`로 준다.
- 클라이언트: 자기 토큰과 **자기가 연결한 포트**로 같은 값을 계산해 상수 시간으로 비교한다. 다르거나 오류면 그 서버를 곁 데몬으로 보지 않는다(포트를 넣어 다른 포트로 중계하는 서버도 걸러낸다).
  - 훅 심: 보내지 않고 스풀에 남긴다(400ms 통신 한도 안에서 확인과 전송을 함께 한다).
  - `gyeotd --open`: "이 포트의 서버는 내 곁 데몬이 아니에요(다른 사용자나 프로그램일 수 있어요)"와 exit 1.
  - 셸: 데몬이 없는 것과 같이 다루되, 대체 화면과 `shell.log`에 같은 이유를 적는다. 그 포트의 페이지를 창에 열지 않는다.
  - channel: 연결하지 않고 stderr에 이유를 적은 뒤 다시 시도 간격을 따른다.
- 확인은 프로세스(또는 연결)마다 한 번이면 된다. 셸·channel은 다시 연결할 때마다 다시 한다.

### 13.2 승인 대기를 닫는 신호 — 에이전트를 가린다 (R3 M3, L1)
- 심은 모든 Claude Code 이벤트에서 `agent_id`(128자까지)를 남긴다. `PostToolBatch`에서는 `tool_calls`를 `[{"tool_name":…}]`(64개까지)로 줄여 남긴다(`tool_input`·`tool_response`는 버린다).
- 정규화: 이벤트 최상위에 `agent_id`를 싣는다(값이 있을 때만 키가 있다; `agent`는 이미 claude-code·codex 구분에 쓰인다). `PostToolBatch`의 activity는 `detail.tools`(도구 이름 목록, 비어 있으면 키가 없다)를 싣는다. 두 필드가 없던 옛 로그도 그대로 재생된다.
- 코어: `approval`·`merge` 항목은 연 이벤트의 `agent_id`를 기억한다(엔진 안에서만, Item 모양은 그대로). 작업 신호(activity)는 **같은 세션·같은 `agent_id`**(둘 다 없으면 같다)이고, 도구가 맞거나(G-016) 모를 때만 닫는다. `tools`가 있으면 그 가운데 하나가 맞아야 한다. `agent_id`가 없는 `Notification(permission_prompt)`은 그 세션의 열린 승인에 붙고 그 승인의 `agent_id`를 바꾸지 않는다. 알림이 먼저 와서 에이전트를 모르는 승인은 10초 안에 온 요청이 에이전트를 정해 준다.
- 같은 세션·같은 `agent`·같은 도구의 `PostToolUseFailure`도 승인 대기를 닫는다.

### 13.3 승인·병합 대기의 소리 알림은 켜야 난다 (R2 N2)
승인 순간을 알리는 훅이 없다. `PostToolBatch`는 묶음이 다 끝난 뒤에야 온다. 그래서 사람이 승인한 도구가 2분 넘게 돌면(빌드·시험) 곁은 아직 기다린다고 잘못 안다.
- 설정 `approval_interrupt`(최상위 키, 기본 `false`). 끄면 `approval`·`merge`는 대기 시간으로 `nudge`(말풍선)까지만 올라간다. 이유 줄: "승인했는지 곁이 알 수 없어 말풍선까지만 알려요".
- 켜면 지금까지의 규칙(2분 → `interrupt`, 작업 신호가 없는 세션은 `nudge`까지)을 따른다.
- `Snapshot.settings.approval_interrupt`로 얼굴에 보인다.

### 13.4 디스크에 남는 것 (R3 L3·L4, R2 N5)
- 마스킹에 일반 `키=값` 규칙을 더한다. 키 이름에 `pass`·`passwd`·`password`·`secret`·`token`·`api_key`·`apikey`·`access_key`·`private_key`가 들어 있고(대소문자 무시, `_`·`-` 허용) 뒤에 `=` 또는 `:`가 오면 값(따옴표 묶음 또는 공백 전까지)을 `[가림]`으로 바꾼다. 두 번 가려도 같다.
- Unix: 데이터 폴더와 그 아래 폴더는 0700, 파일(`events/*.jsonl`·`gyeotd.log`·`token`·`learn.json`·스풀)은 0600으로 만든다. 이미 있는 것도 시작할 때 맞춘다.
- Windows: 설치 폴더나 데이터 폴더가 `%LOCALAPPDATA%` 밖이면 설치 스크립트가 상속을 끊고 그 사용자와 SYSTEM만 남긴다(`icacls … /inheritance:r /grant:r <사용자>:(OI)(CI)F /grant:r *S-1-5-18:(OI)(CI)F`). 결과를 출력한다(R3 M4).

### 13.5 결정 카드는 권한 승인이 아니다 (R3 L5)
channel이 만든 결정 카드(`decision`이고 선택지가 있는 것)에는 질문 위에 "Claude가 묻는 거예요 — 권한 승인이 아니에요"를 늘 보인다. 승인 단추처럼 보이는 색을 쓰지 않는다.

### 13.6 병합 감지 보강 (R2 N3)
§12.6에 더해: 브랜치 자리 앞은 공백·`:`·`+`·따옴표, `refs/heads/`를 허용한다. 뒤는 공백·끝·`:`·`;`·`&`·`|`·따옴표·`)`. 예: `git push origin +main`, `HEAD:refs/heads/main`, `git push origin main;`, `"main"`은 병합이다. `feature/main-fix`·`main-fix`·`mainline`·`git merge-base`는 아니다.

## 14. 보안·정확성 개정 3 (2026-11-26, 독립 검증 R3 회귀·R5 반영)

### 14.1 얼굴도 보내기 전에 상대를 확인한다, 세션은 메모리에만 (R5 H1)
데몬이 멈춘 사이 다른 OS 사용자가 포트를 잡으면, 다시 잇는 얼굴이 그 서버에 세션 열쇠를 보냈고, 세션이 디스크에 남아 다시 뜬 진짜 데몬에서 그 열쇠가 통했다(R5 실측). 두 겹으로 막는다.
- **세션은 메모리에만 둔다.** `sessions.json`을 쓰지 않고, 시작할 때 남아 있으면 지운다. 데몬이 다시 뜨면 이전 열쇠는 모두 `401`이다. 셸은 `renew_session`으로 저절로 다시 열고, 브라우저는 `gyeotd --open`을 다시 한다.
- **얼굴 확인**: `GET /v1/hello?n=<n>` 응답에 `"face":[…]`를 더한다. 살아 있는 세션마다 `hex(HMAC-SHA256(key = SHA-256(세션 값)의 32바이트, msg = "gyeot-face-hello/v1|" + <포트> + "|" + n))`이고 순서는 섞는다(최대 20개). 얼굴은 열쇠를 보내기 전에(시작할 때, 스트림을 새로 열 때마다) 자기 열쇠로 같은 값을 계산해(WebCrypto) 목록에 있는지 본다. 없으면 열쇠를 보내지 않고 401과 같이 다룬다(열쇠를 지우고 셸이면 `renew_session`, 브라우저면 안내). hello 자체가 실패하면 끊긴 것으로 보고 다시 시도 간격을 따른다.
- 셸: 감시가 데몬 `Down`을 보면 기다리지 않고 대체 화면으로 바꾼다(그 포트의 다른 서버 페이지가 창에 다시 뜨지 않게).

### 14.2 승인 요청은 하나씩 따로 센다 (R3 회귀 N1·N2, R5 M1·L1)
한 세션에서 승인 대기가 겹칠 수 있다(병렬 도구, 하위 에이전트). 하나가 끝났다고 남은 대기를 닫으면 사람 차례를 놓친다.
- 코어는 세션마다 **열린 요청 목록**을 둔다. 요청 = (`agent_id`, 도구, 미리보기, 시각). `permission_request`가 하나를 더하고, `agent_id` 없는 `waiting_permission`은 10초 안의 가장 최근 요청에, 없으면 가장 최근에 열린 요청에 붙는다.
- `approval` 항목은 세션마다 하나로 보이되(제목·본문은 가장 최근 요청), **목록이 비어야 닫힌다.** 본문 끝에 "외 n건"을 붙인다.
- 요청을 지우는 신호:
  - activity: 같은 `agent_id`이고 도구가 맞거나 모르면 그 에이전트의 가장 오래된 맞는 요청 하나를 지운다. `PostToolBatch`는 `tools`의 이름마다 하나씩(같은 에이전트) 지운다. `tools`가 없으면 그 에이전트의 요청을 모두 지운다.
  - `tool_failed`: 같은 `agent_id`·같은 도구이고 **미리보기가 같을 때만** 그 요청을 지운다(심은 `PostToolUseFailure`에서도 `tool_input`의 미리보기 필드 하나를 남긴다).
  - `subagent_stop`(`agent_id` 있음): 그 에이전트의 요청을 모두 지운다.
  - 턴 끝(`stop`)·`prompt`: `agent_id`가 없는(주 에이전트) 요청만 지운다. 하위 에이전트의 요청은 그 에이전트의 신호로만 지운다.
  - `session_end`: 모두 지운다.
- `merge`도 같은 규칙이다(요청 목록을 함께 쓴다).
- 잘린 입력(1 MiB 초과)에서 건진 페이로드에는 심이 `"gyeot_partial": true`를 붙인다. 정규화는 그 activity에 `partial`을 싣고, 코어는 partial activity로 요청을 지우지 않는다(작업 신호로만 센다). 심의 건지기는 최상위 필드만 본다(중괄호·따옴표 깊이를 센다).

### 14.3 병합 보류가 알림 없이 끝나면 조용히 (R5 M2)
병합처럼 보이는 요청을 10초 보류했는데 승인 알림(`permission_prompt`, 약 6초 뒤에 온다)이 오지 않았다면 사람이 이미 답한 것이다. 그때 여는 `merge`는 전달 수준 `ambient`(말풍선 없음)다. 알림이 오면 지금처럼 `approval`로 보강된다.

### 14.4 소리는 대기 시간으로 생기지 않는다 (R5 M3)
대기 가산은 순서와 말풍선 판단에만 쓴다. 전달 판단 5번의 `interrupt` 후보는 **기본 점수** ≥ `interrupt_min`이고 blocking이고 2분 넘게 기다린 항목이다. 지금 기본 점수가 95 이상인 종류는 `approval`뿐이고, §13.3에 따라 `approval_interrupt = true`일 때만 소리까지 간다(그때 `merge`도 같이 따른다). 그래서 기본 설정에서 곁은 소리를 내지 않는다. 밤새 남은 질문이 새벽에 소리를 내지 않는다.

### 14.5 마스킹 보강 (R3 회귀 N5, R5 L2)
§13.4에 더한다(값을 `[가림]`으로).
- 값을 공백 뒤에 받는 플래그: `--password`·`--passwd`·`--token`·`--secret`·`--api-key`·`--access-token`·`--auth` 다음 낱말.
- `mysql`·`mysqldump`·`mysqladmin` 명령의 `-p<값>`(붙여 쓴 것만).
- `-u`·`--user` 다음 `이름:비밀`의 비밀, URL의 `사용자:비밀@`(이미 있음).
- `Authorization: Basic …`·`Cookie: …`·`X-Api-Key: …` 머리 값(닫는 따옴표는 남긴다).
- 접두어: `npm_`·`dckr_pat_`·`glpat-`·`sk_live_`·`rk_live_`·`xox[abpr]-`, Slack 웹훅 `hooks.slack.com/services/…`, JWT(`eyJ…\.eyJ…\.…`).
- 키 이름 규칙에 `pwd`·`pw`(키가 정확히 그것일 때)·`auth_key`·`credential(s)`를 더한다.
- 두 번 가려도 같고, 평범한 글(`mkdir -p dir`, `cp -pr`, `--token-limit`)은 그대로다.

### 14.6 남은 위험에 더한다
- 셸 창이 확인한 연결과 다른 새 연결로 얼굴을 연다. 그 사이 몇 ms 안에 데몬이 죽고 다른 사용자가 포트를 잡으면 그 페이지가 창에 뜰 수 있다. 그 페이지가 부를 수 있는 것은 허용 명령 셋뿐이고 `renew_session`은 셸의 확인에서 거절된다(R3 회귀 N6).
- Linux에서 `gyeotd --open`이 숨김 폴더의 파일을 브라우저에 넘기므로 snap·flatpak 브라우저가 열지 못할 수 있다. 실패하면 `--open --print`를 안내한다(R3 회귀 N7, 실측 전).
- 늦게 도착한 옛 대기 신호(§12.7)는 이제 소리가 아니라 말풍선까지만 낸다.
- 얼굴은 부트 코드를 `/v1/session`에 확인 없이 보낸다(아직 열쇠가 없어 확인할 값이 없다). 또 확인된 스트림이 열린 채 데몬이 죽고 다른 서버가 포트를 잡으면, 끊김을 알아채기 전에 보낸 REST 요청 하나는 그 서버로 갈 수 있다. 어느 쪽이든 코드·열쇠는 죽은 데몬의 메모리에만 있던 것이라 다시 뜬 데몬에서 쓸 수 없다.
- 셸은 확인한 연결로 `GET /v1/stream`(토큰)을 하나 붙여 두고 데몬 생존 신호로 쓴다. 서버는 데몬이 살아 있는 동안 이 응답을 끝내지 않고 15초마다 ping을 보낸다(§8).


## 15. 정확성 개정 4 (2026-11-26, 독립 검증 R5 회귀·VR7 반영)

### 15.1 사람이 새로 말하면 모든 승인 요청을 지운다 (R5 회귀 R-M1, VR7 H1·L1)
§14.2의 "턴 끝·`prompt`는 주 에이전트 요청만 지운다"를 고친다. `prompt`(사람이 입력창에 새로 씀)는 그 시각까지 온 그 세션의 **모든** 요청(에이전트 상관없이, 보류 중인 병합 포함)을 지운다. 근거: 앞쪽 권한 창은 입력을 막으므로 사람이 입력했다면 기다리는 창이 없다. 백그라운드 하위 에이전트는 창을 띄우지 못해 그 요청을 자동으로 거절한다(공식 hooks 문서의 PermissionRequest 설명). 그래서 하위 에이전트가 중단돼 `SubagentStop`이 오지 않아도 대기가 영영 남지 않는다. `stop`은 지금처럼 주 에이전트 요청만 지운다.

### 15.2 승인이 열려 있어도 질문·결정은 따로 연다 (VR7 H1)
결정류의 판정 순서(§3)는 결정끼리 덮어쓰는 데만 쓴다. `approval`·`merge`가 열려 있어도 `decision`(턴 끝 질문, elicitation, 입력 대기)은 따로 연다. 승인이 묵어 있는 동안 진짜 질문을 놓치지 않기 위해서다. channel 결정을 턴 끝 질문이 덮어쓰지 않는 규칙은 그대로다.

### 15.3 같은 이름의 다른 호출은 지우지 않는다 (VR7 M1)
심은 `PostToolUse`에서도 `tool_input`의 미리보기 필드 하나를 남긴다(§12.5의 고르는 규칙, 마스킹). 정규화는 그 activity에 `preview`를 싣는다. activity가 요청을 지울 때 둘 다 미리보기가 있으면 같아야 한다. 미리보기가 없는 activity(`PostToolBatch`·옛 로그)는 지금처럼 이름으로 맞춘다. `PostToolBatch`는 묶음의 모든 호출이 끝난 뒤에 오므로 이름으로 맞춰도 기다리는 호출을 지우지 않는다.

### 15.4 마스킹 보강 3 (R5 회귀 R-L1, VR7 M2)
§13.4·§14.5에 더한다.
- 공백 뒤에 값을 받는 짧은 플래그를 명령으로 가려서: `docker login`·`podman login`·`sshpass`·`az login`의 `-p <값>`, `redis-cli`의 `-a <값>`, `openssl`의 `-k <값>`·`-pass <값>`.
- 긴 플래그에 `--client-secret`·`--secret-key`·`--secret-access-key`·`--private-key`·`--passphrase`를 더한다(값이 공백 뒤든 `=` 뒤든).
- 키 이름 규칙에 공백 구분을 더한다: `aws configure set <키> <값>`에서 키가 비밀 꼴(§13.4)이면 값을 가린다.
- Discord 웹훅 `discord(app)?.com/api/webhooks/…`.
- 자유 글("password hunter2" 같은 오류 문장)은 꼴이 없어 가리지 못한다. 남은 위험으로 둔다(파일은 0600, 보존 14일).

### 15.5 배포 얼굴은 127.0.0.1에서만 돈다 (VR7 L3)
얼굴 확인(§14.1)은 포트만 묶는다. `http://localhost:<포트>`로 연 얼굴은 `localhost`가 `::1`로 풀릴 때 그 주소의 같은 포트를 잡은 서버가 hello를 진짜 데몬에 중계해 확인을 통과할 수 있다. 그래서 배포 빌드의 얼굴은 `location.hostname`이 `127.0.0.1`이 아니면 아무 요청도 보내지 않고 "127.0.0.1 주소로 열어 주세요 — 트레이의 곁 열기나 `gyeotd --open`은 늘 그 주소로 엽니다" 안내를 보인다(`boot`가 있어도 교환하지 않는다). 개발 빌드는 예외다.

### 15.6 남은 위험에 더한다 (VR7 회귀)
- 앞쪽 권한 창이 떠 있는 동안에도 `UserPromptSubmit`이 처리된다면(창이 입력을 막는다는 §15.1의 전제가 틀리면) 그 대기가 지워지고 다시 열어 줄 신호가 없다. 실측 전이다(roadmap F13).
- 사람이 권한 창에서 입력을 고쳐 승인하면 `PostToolUse`의 미리보기가 요청과 달라 그 요청이 다음 `PostToolBatch`(기본 설치)·턴 끝·입력까지 남는다.
- 순서가 바뀌어 늦게 온 `PermissionRequest`는 항목을 다시 열지 않는다(알림이 붙어야 연다).
- 마스킹 거짓 양성: 키 이름에 `secret`이 든 평범한 옵션(`--secret-scanning=false`)도 값을 가린다.

