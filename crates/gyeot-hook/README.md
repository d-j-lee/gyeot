# gyeot-hook

Claude Code·Codex 훅에서 부르는 작은 심(`docs/contracts.md` §9).

```
gyeot-hook claude-code          # stdin JSON(최대 1 MiB)
gyeot-hook codex                # stdin JSON
gyeot-hook codex-notify '<json>' # 마지막 인자 JSON
```

- 받은 것을 `gyeot_core::sanitize_payload`로 줄인 뒤(정규화가 쓰는 필드만, `prompt`는 같은 길이의 `x`, 글은 마스킹·4,000자, 하위 에이전트 `agent_id`는 128자, `PostToolBatch`의 `tool_calls`는 `[{"tool_name"}]` 64개, §12.5·§13.2) `{"payload","pid_hint","received_ms"}`로 감싸 `GYEOT_URL`(기본 `http://127.0.0.1:47321`)의 `/v1/ingest/<source>`로 보낸다.
- 보내기 전에 `GET /v1/hello?n=<32바이트 난수 hex>`(인증 머리 없음)로 그 포트의 서버가 같은 토큰을 아는지 확인한다(§13.1). 응답의 `proof`가 `hex(HMAC-SHA256(토큰, "gyeot-hello/v1|<심이 연결한 포트>|<n>"))`와 다르거나 오류면 토큰도 페이로드도 보내지 않고 스풀에만 쓴다. 확인은 프로세스마다 한 번이고, 스풀 비우기도 같은 확인 결과로 한다.
- 데몬과 주고받는 시간은 확인까지 다 합쳐 400ms를 넘기지 않는다(연결마다 연결 단계는 100ms까지). 못 보내면 `<데이터>/spool/`(Unix 폴더 0700·파일 0600)에 쓰고, 보내면 스풀에 남은 것을 최대 50개 이어서 보낸다.
- stdin이 1 MiB를 넘으면 앞만 쓰고 나머지는 EOF(또는 1초 마감)까지 읽어 버린다(호스트가 EPIPE를 받지 않는다).
- **stdout에 아무것도 쓰지 않고 언제나 exit 0**이다. 패닉도 삼킨다.
- `pid_hint`는 부모 PID다. 부모가 셸(sh·bash·cmd·powershell 등)이면 그 위 프로세스를 쓴다(최대 3단계).
- `GYEOT_URL`은 `http://127.0.0.1`·`http://localhost`만 받는다(토큰을 보내므로). 다른 값이면 스풀에만 쓴다.
