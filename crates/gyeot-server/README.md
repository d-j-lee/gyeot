# gyeot-server

곁 데몬의 몸통. `127.0.0.1`에만 묶이는 HTTP·SSE 서버다(`docs/contracts.md` §7·§8).

- Host(→421, 읽을 수 없는 Host 머리도 421)·Origin(→403, 같은 출처만, CORS 헤더 없음)·인증(→401)·열쇠의 권한(→403) 검사, 본문 1 MiB 제한.
- 인증은 `Authorization: Bearer <값>` 하나다. 쿠키·쿼리 토큰은 받지 않고, 어떤 응답에도 `Set-Cookie`가 없다(G-030).
  - **토큰**(`<데이터>/token`): 모든 `/v1/*`.
  - **세션 열쇠**(얼굴용): 계약 §7 표의 "세션: 예" 경로만. `/v1/boot`·`/v1/ingest/*`·`/v1/channel/*`와 모르는 경로는 `403 {"ok":false,"error":"이 열쇠로는 쓸 수 없어요"}`(G-031).
- 얼굴 열기(§12.1): `POST /v1/boot`(토큰)가 120초 일회용 코드를 내고, 얼굴 스크립트가 `POST /v1/session {"code"}`(인증 없음)로 바꿔 `{"session","expires_ms"}`를 받는다. 틀림·지남·재사용은 401. 세션은 만든 때부터 7일, 최대 20개, **데몬 메모리에만** 세션 값의 SHA-256으로 둔다(§14.1, G-036). `sessions.json`은 쓰지도 읽지도 않고, 시작할 때 남아 있으면(쓰다 만 `sessions.tmp-*`도) 지운다. 데몬을 다시 띄우면 이전 열쇠는 모두 401이다(셸은 `renew_session`으로 저절로, 브라우저는 `gyeotd --open`을 다시).
- 데몬 확인(§13.1·§14.1): `GET /v1/hello?n=<16–128자 hex>`(인증 없음) → `{"v":1,"proof":…,"face":[…]}`.
  - `proof` = hex(HMAC-SHA256(토큰, `"gyeot-hello/v1|<실제 포트>|<n>"`)). 토큰을 보내는 쪽이 먼저 부른다(`auth::hello_proof`).
  - `face` = 살아 있는 세션마다 hex(HMAC-SHA256(key = SHA-256(세션 값) 32바이트, `"gyeot-face-hello/v1|<실제 포트>|<n>"`)), 순서를 섞고 최대 20개, 세션이 없으면 `[]`. 얼굴은 열쇠를 보내기 전에 자기 열쇠로 같은 값을 계산해 목록에서 찾는다(`auth::face_proof`).
- 얼굴 `index.html`(`/`·`/index.html`)과 `/assets/*`는 인증 없이 그대로 준다(비밀이 없다). 서버는 `?boot=`를 읽지 않는다. 얼굴 쪽 응답에는 CSP·`X-Frame-Options`·`Referrer-Policy`·`nosniff`를 붙인다. 얼굴 폴더가 없으면 빌드 안내 쪽.
- `/v1/channel/reply`는 그 channel에 발급한 `chat_id`(24시간, channel마다 256개)만 받는다(§12.4).
- 훅 심·하네스 로그·channel의 이벤트를 `gyeot-core` 엔진에 넣고, 바뀐 것을 얼굴에 SSE로 보낸다.
- 정규화한 이벤트를 `<데이터>/events/YYYY-MM-DD.jsonl`에 덧붙이고, 다시 뜰 때 최근 48시간을 재생해 상태를 되살린다.
- 스풀 비우기(시작할 때·30초마다), `[[tail]]` JSONL 따라 읽기(2초마다), `learn.json` 저장.
- Unix(§13.4): 데이터 폴더와 `events`·`spool`·`open`은 0700, `token`·`learn.json`·`tails.json`·`gyeotd.log`·`events/*.jsonl`·스풀 파일은 0600으로 만들고, 이미 있던 것도 시작할 때 맞춘다(링크는 따라가지 않는다). Windows는 설치 스크립트가 맡는다.

```rust
let cfg = gyeot_server::ServerConfig::from_env()?;
gyeot_server::serve(cfg, async { tokio::signal::ctrl_c().await.ok(); }).await?;
```

시험: `cargo test -p gyeot-server`(임시 `GYEOT_HOME`과 빈 포트로 서버를 띄운다).
