# gyeot-server

곁 데몬의 몸통. `127.0.0.1`에만 묶이는 HTTP·SSE 서버다(`docs/contracts.md` §7·§8).

- Host(→421)·Origin(→403)·토큰(→401, 상수 시간 비교) 검사, 본문 1 MiB 제한, Tauri 출처에만 CORS 반사.
- 훅 심·하네스 로그·channel의 이벤트를 `gyeot-core` 엔진에 넣고, 바뀐 것을 얼굴에 SSE로 보낸다.
- 정규화한 이벤트를 `<데이터>/events/YYYY-MM-DD.jsonl`에 덧붙이고, 다시 뜰 때 최근 48시간을 재생해 상태를 되살린다.
- 스풀 비우기(시작할 때·30초마다), `[[tail]]` JSONL 따라 읽기(2초마다), `learn.json` 저장.

```rust
let cfg = gyeot_server::ServerConfig::from_env()?;
gyeot_server::serve(cfg, async { tokio::signal::ctrl_c().await.ok(); }).await?;
```

시험: `cargo test -p gyeot-server`(임시 `GYEOT_HOME`과 빈 포트로 서버를 띄운다).
