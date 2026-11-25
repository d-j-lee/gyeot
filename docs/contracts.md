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
quiet_hours = ["22:00-08:00"]   # 현지 시각, 빈 배열이면 없음
utc_offset_minutes = 540        # 조용한 시간 계산용(Windows 현지 오프셋을 데몬이 채움)
retention_days = 14
[[tail]]                        # 하네스 로그 등 따라 읽을 JSONL
path = "C:/work/proj/.git/fsmap-harness/log.jsonl"
source = "fsmap-harness"
cwd = "C:/work/proj"
```

## 7. 로컬 HTTP API (`gyeot-server`)

- 바인드는 `127.0.0.1`만. 요청 `Host`가 `127.0.0.1:<port>`·`localhost:<port>`가 아니면 `421`(DNS 리바인딩 방어).
- `Origin` 헤더가 있으면 `http://127.0.0.1:<port>`·`http://localhost:<port>`·`tauri://localhost`·`http://tauri.localhost`·`https://tauri.localhost`만 허용, 그 밖은 `403`.
- 인증: `Authorization: Bearer <토큰>` 또는 쿼리 `?t=<토큰>`(EventSource용). 상수 시간 비교. 실패 `401`.
- 본문 최대 1 MiB. CORS 헤더는 내지 않는다(같은 출처만), 단 Tauri 출처에는 반사한다.

| 메서드·경로 | 인증 | 본문 → 응답 |
|---|---|---|
| `GET /health` | 아니오 | `{"ok":true,"version":"0.1.0"}` |
| `GET /` · `GET /assets/*` | 아니오 | 얼굴 정적 파일. `index.html`의 `<meta name="gyeot-token" content="">`에 토큰을 채워 보낸다(같은 출처·Host 검사로 보호) |
| `POST /v1/ingest/{source}` | 예 | 원본 페이로드 JSON. 심은 `{"payload":…, "pid_hint":n, "received_ms":n}` 봉투로 보낸다 → `202 {"accepted":k}` |
| `GET /v1/state` | 예 | `Snapshot` |
| `GET /v1/stream` | 예 | SSE(§8) |
| `POST /v1/items/{id}/action` | 예 | `{"action":"open"\|"done"\|"dismiss"\|"snooze","reason"?:str,"minutes"?:n}` → `{"ok":true}` |
| `POST /v1/focus` | 예 | `{"on":bool,"minutes"?:n}` → `{"ok":true,"until_ms":n\|null}` |
| `POST /v1/presence/listening` | 예 | `{"on":bool}` → `{"ok":true}` |
| `POST /v1/say` | 예 | `{"text":str,"session"?:str}` → `{"chat_id":str\|null,"delivered_to":str\|null,"reason":str}` |
| `POST /v1/decisions/{decision_id}` | 예 | `{"choice":str}` → `{"delivered_to":str\|null}`. 선택지에 없는 답 `400`, 이미 답함 `409` |
| `POST /v1/proposals/{id}` | 예 | `{"accept":bool}` → `{"ok":true}` |
| `POST /v1/adjustments/{kind}` | 예 | `{"quiet":false}` → `{"ok":true}`. 받아들인 조정을 되돌린다. 켜기(`true`)는 제안으로만 하므로 `400` |
| `POST /v1/channel/register` | 예 | `{"pid":n,"ppid":n,"cwd":str}` → `{"cid":str,"session":str\|null}` |
| `GET /v1/channel/stream?cid=` | 예 | SSE: `message` 이벤트 `{"chat_id":str,"text":str,"kind":"say"\|"choice"}` |
| `POST /v1/channel/reply` | 예 | `{"cid":str,"chat_id":str,"text":str}` → `{"ok":true}` |
| `POST /v1/channel/decision` | 예 | `{"cid":str,"question":str,"options":[str]}`(2–4개) → `{"decision_id":str}` |
| `POST /v1/channel/unregister` | 예 | `{"cid":str}` → `{"ok":true}` |

`Snapshot`:

```json
{
  "version": "0.1.0",
  "now_ms": 1764126720123,
  "presence": { "mood": "idle", "level": "ambient", "focus_item": null, "line": "", "since_ms": 0, "focus_mode_until_ms": null },
  "items": [ "Item…" ],
  "sessions": [ { "id": "abc123", "agent": "claude-code", "project": "proj", "cwd": "…", "state": "working", "last_ms": 0, "channel": "ch_1" } ],
  "proposals": [ "Proposal…" ],
  "budget": { "nudge": 4, "interrupt": 1 },
  "settings": { "nudge_min": 80, "interrupt_min": 95, "stale_minutes": 30, "quiet_hours": [], "quiet_kinds": [] }
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

15초마다 주석 줄(`: ping`)을 보낸다.

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
