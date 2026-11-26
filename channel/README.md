# gyeot-channel — 곁의 Claude Code channel

곁(아바타)과 지금 열려 있는 Claude Code 세션을 잇는 MCP 서버다. Claude Code가 stdio로 띄우고, 이 서버는 로컬 곁 데몬(`127.0.0.1:47321`)과 HTTP로 말한다. 규격은 `docs/contracts.md` §7(데몬 끝점)·§10(channel)이 정본이다.

- dj가 곁에 말하거나 결정을 고르면 → 데몬 → 이 서버 → Claude Code 세션에 `<channel source="gyeot" …>`로 들어간다.
- Claude는 `reply` 도구로 답하고, `request_decision` 도구로 dj에게 2~4개 선택지를 띄운다.
- **권한 승인은 이 channel로 하지 않는다.** `claude/channel/permission`을 선언하지 않으므로 Claude Code가 승인 요청을 이쪽으로 보내지 않는다. 승인·거절은 dj가 터미널에서 직접 한다.

## 동작

1. 시작하면 Claude Code와 MCP 연결을 맺는다(데몬이 없어도 연결은 된다).
2. 초기화가 끝나면 먼저 그 포트의 서버가 **내 곁 데몬인지 확인**하고(아래 "데몬 확인"), `POST /v1/channel/register {pid, ppid, cwd}`로 `cid`를 받고, `GET /v1/channel/stream?cid=…`(SSE)를 열어 둔다. 토큰은 `Authorization: Bearer` 머리로만 보낸다(주소에 싣지 않는다).
   - 끊기면 1→2→4→8→16→30초 간격으로 다시 잇는다(건강하게 버틴 연결 뒤에는 1초부터). 다시 이을 때마다 다시 확인한다.
   - 확인에 실패하면 연결하지 않고(토큰도 페이로드도 보내지 않는다) stderr에 이유를 한 번 적은 뒤 같은 간격으로 다시 시도한다.
   - 데몬이 cid를 모른다고 하면(404·410, 데몬 재시작) 바로 다시 등록한다.
   - 데몬이 늦게 떠도 같은 간격으로 계속 시도한다.
3. `message` 사건 `{chat_id, text, kind}`를 받을 때마다 Claude Code에 `notifications/claude/channel`을 보낸다.
   Claude에게는 `<channel source="gyeot" chat_id="…" from="dj" via="gyeot" kind="say|choice">내용</channel>`로 보인다.
4. stdin이 닫히거나 SIGINT·SIGTERM·SIGHUP을 받으면 `POST /v1/channel/unregister`(최대 250ms 기다림)를 보내고 끝난다.

stdout에는 MCP 프로토콜만 쓴다. 기록은 모두 stderr로 간다(Claude Code 디버그 로그에서 볼 수 있다). 토큰은 기록·오류 문구에 싣지 않는다.

### 도구

| 도구 | 입력 | 데몬 호출 |
|---|---|---|
| `reply` | `chat_id`(받은 태그의 값), `text`(1~16000자, 첫 줄 = 소리 내어 읽을 한 문장 요약) | `POST /v1/channel/reply {cid, chat_id, text}` |
| `request_decision` | `question`(1~300자), `options`(2~4개, 각 1~60자, 중복 없음) | `POST /v1/channel/decision {cid, question, options}` → `decision_id` |

- 입력이 틀리면 `isError: true`와 "입력이 올바르지 않음: …"을 돌려준다(데몬에 보내지 않는다).
- 데몬에 닿지 않으면 `isError: true`와 "곁 데몬에 닿지 않음: …"을 돌려준다. 데몬 확인에 실패해도 같다("곁 데몬에 닿지 않음: 이 포트의 서버는 내 곁 데몬이 아니에요…").
- dj가 결정을 고르면 `kind="choice"`인 `"[선택] <선택지>"` 메시지가 같은 세션으로 돌아온다.

## 데몬 확인 — 토큰을 보내기 전에 (docs/contracts.md §13.1, G-032)

`47321`은 모든 OS 사용자가 같이 쓰는 포트라서, 내 데몬이 없을 때 다른 사용자가 먼저 잡을 수 있다. 그래서 토큰이 실린 요청(등록·스트림·`reply`·`request_decision`·등록 해제)을 보내기 전에 확인한다.

1. `crypto.randomBytes(32)`로 난수 `n`(소문자 hex 64자)을 만들어 `GET /v1/hello?n=<n>`을 보낸다. 인증 머리도 토큰도 싣지 않는다.
2. 답 `{"v":1,"proof":"<64 hex>"}`의 `proof`가 `hex(HMAC-SHA256(열쇠 = 토큰 문자열의 UTF-8 바이트, 글 = "gyeot-hello/v1|" + <연결한 포트> + "|" + n))`(`crypto.createHmac`)와 같은지 `crypto.timingSafeEqual`로 비교한다. 포트가 들어 있어서 다른 포트의 곁 데몬으로 중계하는 서버도 걸러진다.
3. 다르거나(`v`가 1이 아님, 증명 모양이 틀림, 값이 다름, `/v1/hello`가 200이 아님) 오류면 그 서버를 곁 데몬으로 보지 않는다. stderr에 `곁 연결 실패(다시 시도함): 이 포트의 서버는 내 곁 데몬이 아니에요(다른 사용자나 프로그램일 수 있어요) — 포트 N: <까닭>. 내 토큰 파일(<경로>)이 …`를 적고, 토큰 파일을 다음 시도에서 다시 읽는다.
4. 확인은 연결마다 한다: 처음 등록 전, 스트림이 끊겨 다시 이을 때마다, 요청이 실패한 뒤(거절 응답 `4xx`는 빼고). 스트림이 열려 있는 동안의 도구 호출은 그 확인을 이어 쓴다.

`GYEOT_URL`에 `localhost`를 적어도 `127.0.0.1`로 잇는다. 데몬은 `127.0.0.1`에만 묶이는데, `localhost`가 `::1`로 풀리면 `[::1]`의 같은 포트를 잡은 다른 사용자의 서버가 진짜 데몬에 확인을 중계해 증명을 통과할 수 있기 때문이다(증명에는 주소가 아니라 포트만 들어 있다).

## 설치 (Windows)

1. 빌드: 이 폴더에서 `npm ci` → `npm run build`. 결과는 의존성을 모두 묶은 파일 하나 `dist/gyeot-channel.mjs`다.
2. 복사: `dist/gyeot-channel.mjs`를 `%LOCALAPPDATA%\gyeot\bin\gyeot-channel.mjs`에 둔다. 실행에는 Node.js 22 이상만 있으면 된다.
3. 등록: 쓰려는 프로젝트 폴더의 `.mcp.json`에 넣는다(`<you>`는 Windows 사용자 이름). Windows에서는 실행 파일이 있어야 하므로 `node` + 스크립트 경로로 적는다.

   ```json
   {
     "mcpServers": {
       "gyeot": {
         "command": "node",
         "args": ["C:/Users/<you>/AppData/Local/gyeot/bin/gyeot-channel.mjs"]
       }
     }
   }
   ```

   모든 프로젝트에서 쓰려면 사용자 범위로 넣어도 된다(공식 문서는 이때 절대 경로를 쓰라고 한다. 사용자 범위에서 `server:gyeot`이 잡히는지는 아직 실측하지 않았다):

   ```
   claude mcp add --scope user gyeot -- node C:/Users/<you>/AppData/Local/gyeot/bin/gyeot-channel.mjs
   ```

4. 실행: 곁 데몬을 켠 뒤, 그 프로젝트에서

   ```
   claude --dangerously-load-development-channels server:gyeot
   ```

   - 처음에 개발용 channel 경고 화면이 뜬다. **I am using this for local development**를 고른다.
   - 프로젝트 `.mcp.json`을 처음 쓰면 "New MCP server found in this project: gyeot" 확인이 뜬다. **Use this MCP server**를 고른다.
   - 시작 배너 아래에 `Channels (experimental) messages from server:gyeot inject directly in this session …`가 보이면 연결된 것이다.
   - Claude가 처음 `reply`를 부를 때 일반 도구처럼 권한 확인이 뜰 수 있다. 매번 묻지 않게 하려면 dj가 직접 설정의 `permissions.allow`에 `mcp__gyeot__reply`, `mcp__gyeot__request_decision`을 넣는다(선택).

### 환경 변수 (모두 선택)

| 이름 | 뜻 | 기본 |
|---|---|---|
| `GYEOT_URL` | 데몬 주소. 토큰을 보내므로 `http://127.0.0.1:<포트>`·`http://localhost:<포트>`만 받는다(`localhost`도 `127.0.0.1`로 잇는다) | `http://127.0.0.1:47321` |
| `GYEOT_HOME` | 데이터 폴더(토큰은 `<데이터>/token`) | Windows `%LOCALAPPDATA%\gyeot`, 그 밖 `$XDG_STATE_HOME/gyeot` 또는 `~/.local/state/gyeot` |
| `GYEOT_CHANNEL_BACKOFF_MS` | 재연결 대기의 단위(시험용) | `1000` |

기본값을 바꿨다면 `.mcp.json` 항목의 `"env"`에 넣는다(Claude Code가 MCP 서버에 넘기는 환경이 셸과 같다고 가정하지 않는다).

## 한계

- **연구 미리보기.** channel은 Claude Code의 research preview 기능이다. 직접 만든 channel은 허용 목록에 없어서 `--dangerously-load-development-channels server:gyeot`로만 돈다. 플래그 문법과 규약이 바뀔 수 있다. 두 플래그 모두 `claude --help`에 나오지 않는다.
- **세션이 열려 있어야 한다.** 메시지는 그 플래그로 시작한 세션이 열려 있을 때만 들어간다. `.mcp.json`에만 있고 플래그 없이 시작한 세션은 서버를 띄우고 도구도 쓰지만, 들어오는 메시지는 Claude Code가 소리 없이 버린다(이 서버는 그것을 알 수 없다).
- **조직 정책.** claude.ai Team·Enterprise 조직은 Owner가 channel을 켜야 한다(관리 설정 `channelsEnabled: true`, 관리 화면 Admin settings → Claude Code → Channels). 꺼져 있으면 개발 플래그로도 메시지가 오지 않는다. Pro·Max 개인 계정은 이 검사가 없다. claude.ai 로그인이나 Console API 키로 인증해야 하고, Amazon Bedrock·Google Cloud Agent Platform·Microsoft Foundry에서는 channel을 쓸 수 없다.
- **MCP 프로토콜 판.** Claude Code의 v2 MCP 런타임은 2026-07-28 판으로 협상한 channel 서버를 channel로 등록하지 않는다. 이 서버는 MCP TypeScript SDK 1.x(최신 판 2025-11-25)로 만들어 그 판을 협상하지 않는다(시험으로 지킨다). SDK를 2.x로 올리기 전에 다시 확인한다.
- **Windows 종료.** Claude Code(v2.1.296 이상)는 stdin을 닫고 300ms 뒤 프로세스 트리를 끝낸다. unregister는 250ms 안에 보내려 한다. 그보다 오래된 Claude Code는 바로 끝내므로 unregister가 가지 않는다 — 데몬은 스트림이 끊긴 것으로도 channel이 닫혔다고 봐야 한다.
- 데몬이 모르는 cid에 404·410을 돌려준다고 가정한다(계약에 아직 적혀 있지 않다).
- 실제 Claude Code 세션에서는 아직 실측하지 않았다. 시험은 MCP SDK 클라이언트와 가짜 데몬으로 했다.
- **확인과 전송 사이.** Node `fetch`는 연결을 직접 고르지 못해서, 확인한 TCP 연결로 토큰을 보낸다고 보장하지 못한다(같은 출처의 연결을 이어 쓰는 경우가 많다). 스트림이 끊기는 순간과 channel이 그것을 알아채는 순간 사이에 다른 서버가 포트를 잡으면, 그 틈에 나간 도구 호출 하나가 그 서버로 갈 수 있다. 데몬이 죽은 직후의 아주 짧은 틈이다.
- 실제 곁 데몬의 `/v1/hello`와는 아직 맞춰 보지 않았다(서버 구현과 끝-끝 실측 전). 시험 값(`proof` 공유 벡터)은 `test/unit.test.ts`에 있다.

## 개발

```
npm ci
npm run typecheck   # tsc --noEmit (Node 22 타입으로 검사)
npm test            # 빌드 → vitest(단위 + 묶은 파일을 띄우는 통합 시험)
npm run build       # dist/gyeot-channel.mjs (라이선스 고지는 파일 끝 주석)
```

- 개발·시험과 실행 모두 Node.js 22 이상(LTS)을 쓴다. Node 20은 2026-04에 지원이 끝났다.
  다른 node로 channel을 띄워 시험하려면 `GYEOT_TEST_NODE=<node 경로> npx vitest run`.
- `zod`는 MCP SDK의 peer 의존성이라 함께 둔다.
- 구조: `src/main.ts`(연결·종료), `src/server.ts`(capability·도구), `src/link.ts`(등록·스트림·재연결), `src/sse.ts`(SSE 해석기), `src/daemon.ts`(데몬 HTTP), `src/config.ts`(경로·주소), `src/validate.ts`(입력 검사), `src/instructions.ts`(Claude에게 주는 안내문).
