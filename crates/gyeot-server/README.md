# gyeot-server

곁 데몬의 몸통. `127.0.0.1`에만 묶이는 HTTP·SSE 서버다(`docs/contracts.md` §7·§8).

- Host(→421, 읽을 수 없는 Host 머리도 421)·Origin(→403, 같은 출처만, CORS 헤더 없음)·인증(→401) 검사, 본문 1 MiB 제한.
- 인증은 `Authorization: Bearer <토큰>` 또는 세션 쿠키 `gyeot_session`(쿼리 토큰 없음). `POST /v1/boot`(Bearer만)가 120초 일회용 코드를 내고, `GET /?boot=<코드>`가 세션 쿠키를 달아 303으로 보낸다. 세션은 `<데이터>/sessions.json`에 SHA-256만(0600), 최대 20개·30일(§12.1).
- 얼굴 `index.html`은 쿠키가 있을 때만, 토큰 없이 보낸다. 없으면 한국어 안내 쪽(401). 얼굴 쪽 응답에는 CSP·`X-Frame-Options`·`Referrer-Policy`·`nosniff`를 붙인다.
- `/v1/channel/reply`는 그 channel에 발급한 `chat_id`(24시간, channel마다 256개)만 받는다(§12.4).
- 훅 심·하네스 로그·channel의 이벤트를 `gyeot-core` 엔진에 넣고, 바뀐 것을 얼굴에 SSE로 보낸다.
- 정규화한 이벤트를 `<데이터>/events/YYYY-MM-DD.jsonl`에 덧붙이고, 다시 뜰 때 최근 48시간을 재생해 상태를 되살린다.
- 스풀 비우기(시작할 때·30초마다), `[[tail]]` JSONL 따라 읽기(2초마다), `learn.json` 저장.

```rust
let cfg = gyeot_server::ServerConfig::from_env()?;
gyeot_server::serve(cfg, async { tokio::signal::ctrl_c().await.ok(); }).await?;
```

시험: `cargo test -p gyeot-server`(임시 `GYEOT_HOME`과 빈 포트로 서버를 띄운다).
