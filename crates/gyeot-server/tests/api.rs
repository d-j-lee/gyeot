//! 서버 통합 시험: 임시 `GYEOT_HOME`·빈 포트로 띄워 계약 §7·§8·§10.1을 확인한다.

mod common;

use common::*;
use serde_json::{Value, json};
use std::time::Duration;

fn perm_prompt(session: &str) -> Value {
    hook(
        "Notification",
        session,
        json!({"notification_type": "permission_prompt", "message": "Claude needs your permission to use Bash"}),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_needs_no_token() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let r = srv
        .raw("GET", "/health", &[("Host", &srv.host())], None)
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(r.json(), json!({"ok": true, "version": "0.1.0"}));
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn token_is_required_for_v1() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let host = srv.host();
    let r = srv.raw("GET", "/v1/state", &[("Host", &host)], None).await;
    assert_eq!(r.status, 401);
    let bad = format!("Bearer {}", "0".repeat(64));
    let r = srv
        .raw(
            "GET",
            "/v1/state",
            &[("Host", &host), ("Authorization", &bad)],
            None,
        )
        .await;
    assert_eq!(r.status, 401);
    let r = srv
        .raw("GET", "/v1/state?t=wrong", &[("Host", &host)], None)
        .await;
    assert_eq!(r.status, 401);
    // 쿼리 토큰은 받지 않는다(계약 §7: URL에 비밀을 싣지 않는다)
    for path in [
        format!("/v1/state?t={}", srv.token),
        format!("/v1/stream?t={}", srv.token),
        format!("/v1/boot?t={}", srv.token),
    ] {
        let method = if path.starts_with("/v1/boot") {
            "POST"
        } else {
            "GET"
        };
        let r = srv.raw(method, &path, &[("Host", &host)], None).await;
        assert_eq!(r.status, 401, "{path}");
    }
    let r = srv.raw("GET", "/v1/nope", &[("Host", &host)], None).await;
    assert_eq!(r.status, 401, "모르는 /v1 경로도 먼저 토큰을 본다");
    assert_eq!(srv.get("/v1/nope").await.status, 404);
    // 토큰 파일이 만들어졌다
    let t = std::fs::read_to_string(d.path().join("token")).unwrap();
    assert_eq!(t.trim(), srv.token);
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bad_host_is_421() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let auth = format!("Bearer {}", srv.token);
    for host in [
        "evil.example:47321".to_owned(),
        format!("evil.example:{}", srv.addr.port()),
        "127.0.0.1".to_owned(),
        format!("127.0.0.1:{}", srv.addr.port() as u32 + 1),
    ] {
        let r = srv
            .raw(
                "GET",
                "/v1/state",
                &[("Host", &host), ("Authorization", &auth)],
                None,
            )
            .await;
        assert_eq!(r.status, 421, "{host}");
        let r = srv.raw("GET", "/health", &[("Host", &host)], None).await;
        assert_eq!(r.status, 421, "{host}");
    }
    let lh = format!("localhost:{}", srv.addr.port());
    let r = srv.raw("GET", "/health", &[("Host", &lh)], None).await;
    assert_eq!(r.status, 200);
    srv.stop().await;
}

/// R1 M2(계약 §12.2): Tauri 출처도 403, CORS 헤더는 어떤 응답에도 없다.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn origin_allowlist_without_tauri_or_cors() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let host = srv.host();
    let auth = format!("Bearer {}", srv.token);
    let no_cors = |r: &Resp| {
        assert!(
            r.headers
                .iter()
                .all(|(k, _)| !k.starts_with("access-control-")),
            "{:?}",
            r.headers
        );
    };
    for bad in [
        "http://evil.example",
        "null",
        "https://127.0.0.1:1",
        "http://localhost",
        "tauri://localhost",
        "http://tauri.localhost",
        "https://tauri.localhost",
    ] {
        let r = srv
            .raw(
                "GET",
                "/v1/state",
                &[("Host", &host), ("Authorization", &auth), ("Origin", bad)],
                None,
            )
            .await;
        assert_eq!(r.status, 403, "{bad}");
        no_cors(&r);
        // 프리플라이트도 받지 않는다
        let r = srv
            .raw(
                "OPTIONS",
                "/v1/state",
                &[
                    ("Host", &host),
                    ("Origin", bad),
                    ("Access-Control-Request-Method", "GET"),
                ],
                None,
            )
            .await;
        assert_eq!(r.status, 403, "OPTIONS {bad}");
        no_cors(&r);
    }
    // 같은 출처: 허용하지만 CORS 헤더는 내지 않는다
    for same in [
        format!("http://127.0.0.1:{}", srv.addr.port()),
        format!("http://localhost:{}", srv.addr.port()),
    ] {
        let r = srv
            .raw(
                "GET",
                "/v1/state",
                &[("Host", &host), ("Authorization", &auth), ("Origin", &same)],
                None,
            )
            .await;
        assert_eq!(r.status, 200, "{same}");
        no_cors(&r);
        // 토큰 없는 OPTIONS는 그냥 인증 실패(204 프리플라이트가 아니다)
        let r = srv
            .raw(
                "OPTIONS",
                "/v1/state",
                &[
                    ("Host", &host),
                    ("Origin", &same),
                    ("Access-Control-Request-Method", "GET"),
                ],
                None,
            )
            .await;
        assert_eq!(r.status, 401, "OPTIONS {same}");
        no_cors(&r);
    }
    srv.stop().await;
}

/// R1 L2(계약 §7): Host 머리가 있는데 읽을 수 없으면 요청줄의 주소로 넘어가지 않고 421.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unreadable_host_is_421_without_fallback() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let port = srv.addr.port();
    let mut req = format!("GET http://127.0.0.1:{port}/health HTTP/1.1\r\nHost: ").into_bytes();
    req.extend_from_slice(b"\xff\xfe127.0.0.1");
    req.extend_from_slice(b"\r\nConnection: close\r\n\r\n");
    let r = srv.raw_bytes(&req).await;
    assert_eq!(r.status, 421, "{}", r.text());
    // 같은 요청줄에 읽을 수 있는 올바른 Host면 통과
    let ok = format!(
        "GET http://127.0.0.1:{port}/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    );
    assert_eq!(srv.raw_bytes(ok.as_bytes()).await.status, 200);
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_limit_and_bad_input() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let auth = format!("Bearer {}", srv.token);
    let big = vec![b' '; 1024 * 1024 + 1];
    let r = srv
        .raw(
            "POST",
            "/v1/ingest/claude-code",
            &[("Host", &srv.host()), ("Authorization", &auth)],
            Some(&big),
        )
        .await;
    assert_eq!(r.status, 413);
    let r = srv
        .raw(
            "POST",
            "/v1/ingest/claude-code",
            &[("Host", &srv.host()), ("Authorization", &auth)],
            Some(b"{not json"),
        )
        .await;
    assert_eq!(r.status, 400);
    assert_eq!(srv.post("/v1/ingest/gyeot", json!({})).await.status, 400);
    assert_eq!(srv.post("/v1/ingest/whatever", json!({})).await.status, 400);
    // 모르는 이벤트는 받되 버린다
    let r = srv
        .post(
            "/v1/ingest/claude-code",
            hook("SomethingNew", "s", json!({})),
        )
        .await;
    assert_eq!(r.status, 202);
    assert_eq!(r.json(), json!({"accepted": 0}));
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permission_prompt_becomes_item_in_state_and_sse() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let mut sse = srv.sse("/v1/stream").await;
    assert_eq!(sse.status, 200);
    let first = sse.next(S).await.unwrap();
    assert_eq!(first.event, "snapshot");
    let snap = first.json();
    assert_eq!(snap["version"], "0.1.0");
    assert_eq!(snap["presence"]["mood"], "idle");
    assert_eq!(snap["budget"], json!({"nudge": 4, "interrupt": 1}));
    assert_eq!(snap["settings"]["nudge_min"], 80);

    let r = srv
        .post(
            "/v1/ingest/claude-code",
            envelope(perm_prompt("abc123"), 4321),
        )
        .await;
    assert_eq!(r.status, 202);
    assert_eq!(r.json(), json!({"accepted": 1}));

    let item = sse.wait_for("item", S).await.expect("item event").json();
    assert_eq!(item["kind"], "approval");
    assert_eq!(item["session"], "abc123");
    assert_eq!(item["project"], "proj");
    assert_eq!(item["blocking"], true);
    assert_eq!(item["state"], "open");
    let deliver = sse.wait_for("deliver", S).await.expect("deliver").json();
    assert_eq!(deliver["item"], item["id"]);
    assert_eq!(deliver["level"], "nudge");
    assert_eq!(deliver["line"], "proj에서 권한 승인을 기다려요");
    let sessions = sse.wait_for("sessions", S).await.expect("sessions").json();
    assert_eq!(sessions[0]["id"], "abc123");
    assert_eq!(sessions[0]["state"], "waiting");
    let presence = sse.wait_for("presence", S).await.expect("presence").json();
    assert_eq!(presence["mood"], "waiting");

    let st = srv.state().await;
    let its = items(&st);
    assert_eq!(its.len(), 1);
    assert_eq!(its[0]["id"], item["id"]);
    assert_eq!(st["sessions"][0]["state"], "waiting");

    // 봉투 없이 원본만 보내도 받는다
    let r = srv
        .post(
            "/v1/ingest/claude-code",
            hook("Stop", "s2", json!({"last_assistant_message": "끝"})),
        )
        .await;
    assert_eq!(r.json(), json!({"accepted": 1}));
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn item_actions_dismiss_and_snooze() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    srv.post("/v1/ingest/claude-code", perm_prompt("a")).await;
    srv.post(
        "/v1/ingest/claude-code",
        hook("Stop", "b", json!({"last_assistant_message": "끝"})),
    )
    .await;
    let st = srv.state().await;
    let ids: Vec<(String, String)> = items(&st)
        .iter()
        .map(|i| {
            (
                i["kind"].as_str().unwrap().to_owned(),
                i["id"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let approval = ids.iter().find(|(k, _)| k == "approval").unwrap().1.clone();
    let unread = ids.iter().find(|(k, _)| k == "unread").unwrap().1.clone();

    let mut sse = srv.sse("/v1/stream").await;
    sse.wait_for("snapshot", S).await.unwrap();

    let p = format!("/v1/items/{approval}/action");
    let r = srv
        .post(&p, json!({"action": "dismiss", "reason": "싫음"}))
        .await;
    assert_eq!(r.status, 400);
    let r = srv.post(&p, json!({"action": "explode"})).await;
    assert_eq!(r.status, 400);
    let r = srv
        .post(
            "/v1/items/it_0000000000000000/action",
            json!({"action": "done"}),
        )
        .await;
    assert_eq!(r.status, 404);
    let r = srv
        .post(&p, json!({"action": "dismiss", "reason": "과민"}))
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(r.json(), json!({"ok": true}));
    let gone = sse.wait_for("item_removed", S).await.unwrap().json();
    assert_eq!(gone, json!({"id": approval}));

    let r = srv
        .post(
            &format!("/v1/items/{unread}/action"),
            json!({"action": "snooze", "minutes": 5}),
        )
        .await;
    assert_eq!(r.status, 200);
    assert!(items(&srv.state().await).is_empty());
    let r = srv
        .post(
            &format!("/v1/items/{unread}/action"),
            json!({"action": "open"}),
        )
        .await;
    assert_eq!(r.status, 200);
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn focus_and_listening() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let before = now_ms();
    let r = srv
        .post("/v1/focus", json!({"on": true, "minutes": 25}))
        .await;
    assert_eq!(r.status, 200);
    let v = r.json();
    assert_eq!(v["ok"], true);
    let until = v["until_ms"].as_i64().unwrap();
    assert!(until >= before + 25 * 60_000 && until <= now_ms() + 25 * 60_000);
    let st = srv.state().await;
    assert_eq!(st["presence"]["focus_mode_until_ms"], until);
    assert_eq!(st["presence"]["mood"], "sleep");
    let r = srv
        .post("/v1/focus", json!({"on": true, "minutes": 0}))
        .await;
    assert_eq!(r.status, 400);
    let r = srv.post("/v1/focus", json!({"on": false})).await;
    assert_eq!(r.json(), json!({"ok": true, "until_ms": null}));

    let r = srv
        .post("/v1/presence/listening", json!({"on": true}))
        .await;
    assert_eq!(r.json(), json!({"ok": true}));
    assert_eq!(srv.state().await["presence"]["mood"], "listening");
    srv.post("/v1/presence/listening", json!({"on": false}))
        .await;
    assert_eq!(srv.state().await["presence"]["mood"], "idle");
    assert_eq!(
        srv.post("/v1/presence/listening", json!({})).await.status,
        400
    );
    srv.stop().await;
}

async fn register(srv: &TestServer, pid: u32, ppid: u32, cwd: &str) -> (String, Value) {
    let r = srv
        .post(
            "/v1/channel/register",
            json!({"pid": pid, "ppid": ppid, "cwd": cwd}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    let v = r.json();
    (v["cid"].as_str().unwrap().to_owned(), v["session"].clone())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn say_reaches_channel_stream_and_reply_reaches_face() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    // channel이 없으면 안내
    let r = srv.post("/v1/say", json!({"text": "안녕"})).await;
    assert_eq!(r.status, 200);
    let v = r.json();
    assert_eq!(v["delivered_to"], Value::Null);
    assert_eq!(
        v["reason"],
        "channel 없음 — 세션을 --dangerously-load-development-channels server:gyeot 로 시작하세요"
    );
    assert_eq!(srv.post("/v1/say", json!({"text": "  "})).await.status, 400);

    // 세션(pid_hint 4321) → 같은 ppid의 channel이 붙는다
    srv.post(
        "/v1/ingest/claude-code",
        envelope(
            hook("SessionStart", "abc123", json!({"source": "startup"})),
            4321,
        ),
    )
    .await;
    let (cid, session) = register(&srv, 9000, 4321, "/somewhere/else").await;
    assert!(cid.starts_with("ch_"));
    assert_eq!(session, "abc123");
    let st = srv.state().await;
    assert_eq!(st["sessions"][0]["channel"], cid);

    let mut ch = srv.sse(&format!("/v1/channel/stream?cid={cid}")).await;
    assert_eq!(ch.status, 200);
    let r = srv.post("/v1/say", json!({"text": "테스트 돌려 줘"})).await;
    let v = r.json();
    assert_eq!(v["delivered_to"], "abc123");
    let msg = ch.wait_for("message", S).await.expect("message").json();
    assert_eq!(msg["text"], "테스트 돌려 줘");
    assert_eq!(msg["kind"], "say");
    let chat_id = msg["chat_id"].as_str().unwrap().to_owned();
    assert!(
        chat_id.len() <= 64
            && chat_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    );
    assert_eq!(v["chat_id"], chat_id);
    // 세션을 지정해도 같다, 모르는 세션은 안내
    let v = srv
        .post("/v1/say", json!({"text": "둘", "session": "abc123"}))
        .await
        .json();
    assert_eq!(v["delivered_to"], "abc123");
    ch.wait_for("message", S).await.unwrap();
    let v = srv
        .post("/v1/say", json!({"text": "x", "session": "nope"}))
        .await
        .json();
    assert_eq!(v["delivered_to"], Value::Null);
    // 아주 긴 글은 잘라서 밝힌다
    srv.post("/v1/say", json!({"text": "가".repeat(40_000)}))
        .await;
    let long = ch.wait_for("message", S).await.unwrap().json();
    let t = long["text"].as_str().unwrap();
    assert_eq!(t.chars().count(), 32_000);
    assert!(t.ends_with("…(잘렸어요)"));

    // reply → 얼굴 SSE
    let mut face = srv.sse("/v1/stream").await;
    face.wait_for("snapshot", S).await.unwrap();
    let r = srv
        .post(
            "/v1/channel/reply",
            json!({"cid": cid, "chat_id": chat_id, "text": "돌렸어요. 키 sk-abcdefghijklmnopqrstuv"}),
        )
        .await;
    assert_eq!(r.json(), json!({"ok": true}));
    let rep = face.wait_for("reply", S).await.expect("reply").json();
    assert_eq!(rep["session"], "abc123");
    assert_eq!(rep["project"], "proj");
    assert_eq!(rep["chat_id"], chat_id);
    assert_eq!(rep["text"], "돌렸어요. 키 [가림]");
    assert!(rep["at_ms"].as_i64().unwrap() > 0);

    // 형식 검사
    let bad = srv
        .post(
            "/v1/channel/reply",
            json!({"cid": cid, "chat_id": "has space", "text": "x"}),
        )
        .await;
    assert_eq!(bad.status, 400);
    let bad = srv
        .post(
            "/v1/channel/reply",
            json!({"cid": cid, "chat_id": "m1", "text": "x".repeat(16_001)}),
        )
        .await;
    assert_eq!(bad.status, 400);
    let ok = srv
        .post(
            "/v1/channel/reply",
            json!({"cid": cid, "chat_id": "m1", "text": "x".repeat(16_000)}),
        )
        .await;
    assert_eq!(ok.status, 200);

    // 풀기
    let r = srv
        .post("/v1/channel/unregister", json!({"cid": cid}))
        .await;
    assert_eq!(r.json(), json!({"ok": true}));
    assert!(
        ch.wait_for("message", Duration::from_secs(2))
            .await
            .is_none()
    );
    assert_eq!(srv.state().await["sessions"][0]["channel"], Value::Null);
    srv.stop().await;
}

/// R1 L1(계약 §12.4): reply는 그 channel에 데몬이 발급한 chat_id만 받는다.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reply_accepts_only_chat_ids_issued_to_that_channel() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    for (sid, pid) in [("abc123", 4321), ("s2", 77)] {
        srv.post(
            "/v1/ingest/claude-code",
            envelope(hook("SessionStart", sid, json!({"source": "startup"})), pid),
        )
        .await;
    }
    let (a, sa) = register(&srv, 9000, 4321, "/a").await;
    let (b, sb) = register(&srv, 9001, 77, "/b").await;
    assert_eq!((sa, sb), (json!("abc123"), json!("s2")));
    let _cha = srv.sse(&format!("/v1/channel/stream?cid={a}")).await;
    let _chb = srv.sse(&format!("/v1/channel/stream?cid={b}")).await;
    let reply = |cid: &str, chat_id: &str| json!({"cid": cid, "chat_id": chat_id, "text": "답"});
    let not_issued = |r: &Resp| {
        assert_eq!(r.status, 400, "{}", r.text());
        assert_eq!(r.json()["error"], "발급하지 않은 chat_id");
    };
    // 아무것도 발급하지 않았으면 그럴듯한 꼴이어도 받지 않는다
    not_issued(&srv.post("/v1/channel/reply", reply(&a, "m1")).await);
    let v = srv
        .post("/v1/say", json!({"text": "하나", "session": "abc123"}))
        .await
        .json();
    let m = v["chat_id"].as_str().unwrap().to_owned();
    assert_eq!(
        srv.post("/v1/channel/reply", reply(&a, &m)).await.status,
        200
    );
    // 같은 id를 다시 써도(여러 번 답해도) 된다
    assert_eq!(
        srv.post("/v1/channel/reply", reply(&a, &m)).await.status,
        200
    );
    // 다른 channel에 발급한 id, 발급하지 않은 id
    not_issued(&srv.post("/v1/channel/reply", reply(&b, &m)).await);
    not_issued(&srv.post("/v1/channel/reply", reply(&a, "m999")).await);
    not_issued(
        &srv.post("/v1/channel/reply", reply(&a, "dabcdef123456"))
            .await,
    );
    // 결정 id는 그 결정을 연 channel에 발급된 것이다
    let r = srv
        .post(
            "/v1/channel/decision",
            json!({"cid": b, "question": "고를까요?", "options": ["예", "아니오"]}),
        )
        .await;
    let did = r.json()["decision_id"].as_str().unwrap().to_owned();
    assert_eq!(
        srv.post("/v1/channel/reply", reply(&b, &did)).await.status,
        200
    );
    not_issued(&srv.post("/v1/channel/reply", reply(&a, &did)).await);
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_cid_is_404() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let st = srv.get("/v1/channel/stream?cid=ch_nope").await;
    assert_eq!(st.status, 404);
    let r = srv
        .post(
            "/v1/channel/reply",
            json!({"cid": "ch_nope", "chat_id": "m1", "text": "x"}),
        )
        .await;
    assert_eq!(r.status, 404);
    let r = srv
        .post(
            "/v1/channel/decision",
            json!({"cid": "ch_nope", "question": "q", "options": ["a", "b"]}),
        )
        .await;
    assert_eq!(r.status, 404);
    // 풀기는 없는 cid에도 ok(되풀이해도 안전)
    let r = srv
        .post("/v1/channel/unregister", json!({"cid": "ch_nope"}))
        .await;
    assert_eq!(r.status, 200);
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn decision_roundtrip() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    srv.post(
        "/v1/ingest/claude-code",
        envelope(
            hook("PreToolUse", "abc123", json!({"tool_name": "Bash"})),
            77,
        ),
    )
    .await;
    // PID가 안 맞아도 같은 작업 폴더로 잇는다
    let (cid, session) = register(&srv, 1, 2, "c:/work/proj").await;
    assert_eq!(session, "abc123");
    let mut ch = srv.sse(&format!("/v1/channel/stream?cid={cid}")).await;

    for (body, why) in [
        (
            json!({"cid": cid, "question": "", "options": ["a", "b"]}),
            "빈 질문",
        ),
        (
            json!({"cid": cid, "question": "x".repeat(301), "options": ["a", "b"]}),
            "긴 질문",
        ),
        (
            json!({"cid": cid, "question": "q", "options": ["a"]}),
            "선택지 1개",
        ),
        (
            json!({"cid": cid, "question": "q", "options": ["a", "b", "c", "d", "e"]}),
            "선택지 5개",
        ),
        (
            json!({"cid": cid, "question": "q", "options": ["a", "a"]}),
            "중복",
        ),
        (
            json!({"cid": cid, "question": "q", "options": ["a", "x".repeat(61)]}),
            "긴 선택지",
        ),
        (
            json!({"cid": cid, "question": "q", "options": ["a", " "]}),
            "빈 선택지",
        ),
    ] {
        let r = srv.post("/v1/channel/decision", body).await;
        assert_eq!(r.status, 400, "{why}");
    }
    let r = srv
        .post(
            "/v1/channel/decision",
            json!({"cid": cid, "question": "어느 쪽으로 병합할까요?", "options": ["squash", "merge"]}),
        )
        .await;
    assert_eq!(r.status, 200);
    let did = r.json()["decision_id"].as_str().unwrap().to_owned();
    assert!(
        did.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    );

    let st = srv.state().await;
    let dec = items(&st)
        .into_iter()
        .find(|i| i["kind"] == "decision")
        .expect("decision item");
    assert_eq!(dec["decision_id"], did);
    assert_eq!(dec["options"], json!(["squash", "merge"]));
    assert_eq!(dec["title"], "어느 쪽으로 병합할까요?");

    let p = format!("/v1/decisions/{did}");
    assert_eq!(srv.post(&p, json!({"choice": "rebase"})).await.status, 400);
    assert_eq!(
        srv.post("/v1/decisions/dnope", json!({"choice": "x"}))
            .await
            .status,
        404
    );
    let r = srv.post(&p, json!({"choice": "squash"})).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.json(), json!({"delivered_to": "abc123"}));
    let msg = ch.wait_for("message", S).await.expect("choice").json();
    assert_eq!(
        msg,
        json!({"chat_id": did, "text": "[선택] squash", "kind": "choice"})
    );
    assert!(!item_kinds(&srv.state().await).contains(&"decision".to_owned()));
    let again = srv.post(&p, json!({"choice": "squash"})).await;
    assert_eq!(again.status, 409);
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn proposal_flow_and_adjustment() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    assert_eq!(
        srv.post("/v1/proposals/pr_unread_quiet", json!({"accept": true}))
            .await
            .status,
        404
    );
    let mut face = srv.sse("/v1/stream").await;
    face.wait_for("snapshot", S).await.unwrap();
    for i in 0..5 {
        srv.post(
            "/v1/ingest/claude-code",
            hook(
                "Stop",
                "s1",
                json!({"last_assistant_message": format!("끝 {i}")}),
            ),
        )
        .await;
        let st = srv.state().await;
        let id = items(&st)[0]["id"].as_str().unwrap().to_owned();
        let r = srv
            .post(
                &format!("/v1/items/{id}/action"),
                json!({"action": "dismiss", "reason": "과민"}),
            )
            .await;
        assert_eq!(r.status, 200);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let prop = face.wait_for("proposal", S).await.expect("proposal").json();
    assert_eq!(prop["id"], "pr_unread_quiet");
    assert_eq!(
        prop["evidence"],
        json!({"dismissed": 5, "total": 5, "window_days": 14})
    );
    assert_eq!(srv.state().await["proposals"][0]["id"], "pr_unread_quiet");
    let r = srv
        .post("/v1/proposals/pr_unread_quiet", json!({"accept": true}))
        .await;
    assert_eq!(r.json(), json!({"ok": true}));
    let snap = face.wait_for("snapshot", S).await.expect("resync").json();
    assert_eq!(snap["settings"]["quiet_kinds"], json!(["unread"]));
    assert_eq!(snap["proposals"], json!([]));
    // 되돌리기(켜기는 제안으로만)
    assert_eq!(
        srv.post("/v1/adjustments/unread", json!({"quiet": true}))
            .await
            .status,
        400
    );
    assert_eq!(
        srv.post("/v1/adjustments/bogus", json!({"quiet": false}))
            .await
            .status,
        404
    );
    let r = srv
        .post("/v1/adjustments/unread", json!({"quiet": false}))
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(srv.state().await["settings"]["quiet_kinds"], json!([]));
    srv.stop().await;
    // 끌 때 learn.json을 남긴다
    let learn: Value =
        serde_json::from_slice(&std::fs::read(d.path().join("learn.json")).unwrap()).unwrap();
    assert_eq!(learn["kinds"]["unread"]["opened"], 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_restores_items_and_closes_channels() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    srv.post(
        "/v1/ingest/claude-code",
        envelope(perm_prompt("abc123"), 4321),
    )
    .await;
    srv.post(
        "/v1/ingest/claude-code",
        hook("Stop", "other", json!({"last_assistant_message": "끝"})),
    )
    .await;
    let st = srv.state().await;
    let unread = items(&st)
        .into_iter()
        .find(|i| i["kind"] == "unread")
        .unwrap();
    srv.post(
        &format!("/v1/items/{}/action", unread["id"].as_str().unwrap()),
        json!({"action": "done"}),
    )
    .await;
    let (cid, _) = register(&srv, 1, 4321, "/x").await;
    let before = srv.state().await;
    assert_eq!(
        before["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["channel"] == cid)
            .count(),
        1
    );
    let token = srv.token.clone();
    srv.stop().await;

    // 이벤트 로그가 남았다
    let logs: Vec<_> = std::fs::read_dir(d.path().join("events"))
        .unwrap()
        .collect();
    assert_eq!(logs.len(), 1);

    let srv = TestServer::start(d.path()).await;
    assert_eq!(srv.token, token, "토큰은 그대로");
    let after = srv.state().await;
    assert_eq!(item_kinds(&after), vec!["approval"]);
    assert_eq!(items(&after)[0]["id"], items(&before)[0]["id"]);
    assert_eq!(
        items(&after)[0]["signature"],
        items(&before)[0]["signature"]
    );
    // 다시 뜬 뒤에는 옛 channel이 없다
    assert!(
        after["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["channel"].is_null())
    );
    assert_eq!(
        srv.get(&format!("/v1/channel/stream?cid={cid}"))
            .await
            .status,
        404
    );
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spool_is_drained_on_startup() {
    let d = tempfile::tempdir().unwrap();
    let spool = d.path().join("spool");
    std::fs::create_dir_all(&spool).unwrap();
    let entry = json!({
        "source": "claude-code",
        "payload": perm_prompt("spooled"),
        "pid_hint": 11,
        "received_ms": now_ms() - 60_000,
    });
    std::fs::write(spool.join("1764126720123-11-0.json"), entry.to_string()).unwrap();
    std::fs::write(spool.join("1764126720124-11-0.json"), "garbage").unwrap();
    let srv = TestServer::start(d.path()).await;
    let st = srv.state().await;
    let its = items(&st);
    assert_eq!(its.len(), 1);
    assert_eq!(its[0]["session"], "spooled");
    // 받은 시각이 그대로 쓰여 1분 기다린 것으로 보인다
    assert!(its[0]["updated_ms"].as_i64().unwrap() <= now_ms() - 59_000);
    assert_eq!(std::fs::read_dir(&spool).unwrap().count(), 0);
    srv.stop().await;
}

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; media-src 'self' data: blob:; connect-src 'self' ipc: http://ipc.localhost; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

fn assert_face_headers(r: &Resp, no_store: bool, what: &str) {
    assert_eq!(r.header("content-security-policy"), Some(CSP), "{what}");
    assert_eq!(r.header("x-frame-options"), Some("DENY"), "{what}");
    assert_eq!(r.header("referrer-policy"), Some("no-referrer"), "{what}");
    assert_eq!(
        r.header("x-content-type-options"),
        Some("nosniff"),
        "{what}"
    );
    if no_store {
        assert_eq!(r.header("cache-control"), Some("no-store"), "{what}");
    }
}

fn face_dir_with_token_meta() -> tempfile::TempDir {
    let face = tempfile::tempdir().unwrap();
    std::fs::write(
        face.path().join("index.html"),
        r#"<!doctype html><html><head><meta name="gyeot-token" content=""><script src="/assets/app.js"></script></head><body>얼굴</body></html>"#,
    )
    .unwrap();
    std::fs::create_dir_all(face.path().join("assets")).unwrap();
    std::fs::write(face.path().join("assets/app.js"), "console.log(1)").unwrap();
    std::fs::write(face.path().join("favicon.svg"), "<svg/>").unwrap();
    face
}

/// R3 H1(계약 §7·§12.1, G-030): `GET /`·`/index.html`은 인증 없이 `index.html` 그대로(비밀 없음).
/// 서버는 `?boot=`를 읽지 않고(코드를 쓰지 않는다) 쿠키도 달지 않는다.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn index_is_public_and_never_contains_token() {
    let d = tempfile::tempdir().unwrap();
    let face = face_dir_with_token_meta();
    std::fs::write(d.path().join("secret.txt"), "nope").unwrap();
    let mut cfg = config(d.path());
    cfg.face_dir = Some(face.path().to_path_buf());
    let srv = TestServer::start_with(cfg).await;
    let host = srv.host();
    let code = srv.boot().await["code"].as_str().unwrap().to_owned();
    for path in [
        "/".to_owned(),
        "/index.html".to_owned(),
        "/?mode=overlay".to_owned(),
        format!("/?mode=overlay&boot={code}"),
        format!("/index.html?boot={code}"),
    ] {
        let r = srv.raw("GET", &path, &[("Host", &host)], None).await;
        assert_eq!(r.status, 200, "{path}: {}", r.text());
        let t = r.text();
        assert!(t.contains("얼굴</body>"), "{path}: {t}");
        assert!(t.contains(r#"<meta name="gyeot-token" content="">"#), "{t}");
        assert!(!t.contains(&srv.token), "{path}");
        assert!(r.header("content-type").unwrap().starts_with("text/html"));
        assert!(r.header("location").is_none(), "{path}: 넘겨주기 없음");
        assert_no_set_cookie(&r, &path);
        assert_face_headers(&r, true, &path);
    }
    // 쿠키를 실어도 같은 쪽(쿠키는 아무 뜻이 없다)
    let r = srv.with_cookie("GET", "/", &"0".repeat(64)).await;
    assert_eq!(r.status, 200);
    // 서버가 `?boot=`를 쓰지 않았으니 그 코드는 아직 세션으로 바꿀 수 있다
    let r = srv
        .session_raw(json!({ "code": code }).to_string().as_bytes())
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    // 맨 위 index.html을 인코딩한 경로로는 주지 않는다(그대로)
    let r = srv
        .raw("GET", "/%69ndex.html", &[("Host", &host)], None)
        .await;
    assert_eq!(r.status, 404);
    // 정적 파일도 인증 없이, 보안 머리를 붙인다
    let r = srv
        .raw("GET", "/assets/app.js", &[("Host", &host)], None)
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(
        r.header("content-type"),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(r.text(), "console.log(1)");
    assert_face_headers(&r, false, "/assets/app.js");
    let r = srv
        .raw("GET", "/favicon.svg", &[("Host", &host)], None)
        .await;
    assert_eq!(r.header("content-type"), Some("image/svg+xml"));
    assert_face_headers(&r, false, "/favicon.svg");
    for bad in [
        "/assets/../../secret.txt",
        "/assets/%2e%2e/%2e%2e/secret.txt",
        "/nope.js",
    ] {
        let r = srv.raw("GET", bad, &[("Host", &host)], None).await;
        assert_eq!(r.status, 404, "{bad}");
    }
    let r = srv.raw("GET", "/", &[("Host", "evil.example")], None).await;
    assert_eq!(r.status, 421);
    srv.stop().await;
}

/// R3 H1(계약 §12.1): 부트 코드(Bearer 토큰으로만 받음) → `POST /v1/session`(인증 머리 없음) → 세션 열쇠.
/// 코드는 한 번만, 틀림·재사용은 401. 열쇠는 `Authorization: Bearer`로 쓴다.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn boot_code_becomes_a_session_key_once() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let host = srv.host();
    let port = srv.addr.port();
    let before = now_ms();
    let b = srv.boot().await;
    let code = b["code"].as_str().unwrap().to_owned();
    assert_eq!(code.len(), 64);
    assert!(
        code.bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    assert_eq!(b["url"], format!("http://127.0.0.1:{port}/?boot={code}"));
    let exp = b["expires_ms"].as_i64().unwrap();
    assert!(exp >= before + 120_000 && exp <= now_ms() + 120_000, "{b}");
    assert_ne!(srv.boot().await["code"], b["code"], "코드마다 다르다");

    // 인증 머리 없이 바꾼다
    let body = json!({ "code": code }).to_string();
    let r = srv.session_raw(body.as_bytes()).await;
    assert_eq!(r.status, 200, "{}", r.text());
    assert_no_set_cookie(&r, "/v1/session");
    assert_eq!(r.header("cache-control"), Some("no-store"));
    let v = r.json();
    assert_eq!(v.as_object().unwrap().len(), 2, "{v}");
    let key = v["session"].as_str().unwrap().to_owned();
    assert_eq!(key.len(), 64);
    assert!(
        key.bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    assert_ne!(key, code);
    assert_ne!(key, srv.token);
    let sexp = v["expires_ms"].as_i64().unwrap();
    let week = 7 * 86_400_000;
    assert!(
        sexp >= before + week && sexp <= now_ms() + week,
        "세션은 만든 때부터 7일: {v}"
    );

    // 한 번만
    let r = srv.session_raw(body.as_bytes()).await;
    assert_eq!(r.status, 401, "{}", r.text());
    let e = r.json();
    assert_eq!(e["ok"], false);
    assert!(e["error"].as_str().unwrap().contains("코드"), "{e}");
    assert_no_set_cookie(&r, "used code");
    // 틀린 코드·빈 코드·대문자로 바꾼 코드
    for bad in [
        "nope".to_owned(),
        String::new(),
        "a".repeat(64),
        srv.boot().await["code"]
            .as_str()
            .unwrap()
            .to_ascii_uppercase(),
    ] {
        let r = srv
            .session_raw(json!({ "code": bad }).to_string().as_bytes())
            .await;
        assert_eq!(r.status, 401, "{bad}");
        assert_eq!(r.json()["ok"], false);
    }
    // 본문 꼴이 틀리면 400
    for bad in [&b"{not json"[..], br#"{}"#, br#"{"code":1}"#] {
        let r = srv.session_raw(bad).await;
        assert_eq!(r.status, 400, "{}", String::from_utf8_lossy(bad));
    }
    // Host·Origin 규칙은 그대로
    let fresh = srv.boot().await["code"].as_str().unwrap().to_owned();
    let fb = json!({ "code": fresh }).to_string();
    let r = srv
        .raw(
            "POST",
            "/v1/session",
            &[("Host", &host), ("Origin", "http://evil.example")],
            Some(fb.as_bytes()),
        )
        .await;
    assert_eq!(r.status, 403);
    let r = srv
        .raw(
            "POST",
            "/v1/session",
            &[("Host", "evil.example")],
            Some(fb.as_bytes()),
        )
        .await;
    assert_eq!(r.status, 421);
    // 막힌 요청은 코드를 쓰지 않았다
    let same = format!("http://127.0.0.1:{port}");
    let r = srv
        .raw(
            "POST",
            "/v1/session",
            &[("Host", &host), ("Origin", &same)],
            Some(fb.as_bytes()),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.text());

    // 열쇠로 API·SSE를 쓴다
    let r = srv.with_key("GET", "/v1/state", &key, None).await;
    assert_eq!(r.status, 200, "{}", r.text());
    let auth = format!("Bearer {key}");
    let mut face = Sse::open(srv.addr, &host, "/v1/stream", Some(&auth)).await;
    assert_eq!(face.status, 200, "열쇠로 여는 얼굴 SSE");
    assert_eq!(face.next(S).await.unwrap().event, "snapshot");
    // 틀린 열쇠·없는 열쇠는 401
    let r = srv
        .with_key("GET", "/v1/state", &"f".repeat(64), None)
        .await;
    assert_eq!(r.status, 401);
    let s = Sse::open(srv.addr, &host, "/v1/stream", None).await;
    assert_eq!(s.status, 401);
    srv.stop().await;
}

/// R2 N4·R3 M1(계약 §7 표, G-031): 세션 열쇠는 "세션: 예" 경로만 연다. 그 밖(`/v1/boot`·`/v1/ingest/*`·
/// `/v1/channel/*`·모르는 경로)은 403 `{"ok":false,"error":"이 열쇠로는 쓸 수 없어요"}`이고 아무 일도 하지 않는다.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_key_opens_only_face_routes() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let key = srv.open_session().await;
    let denied: Vec<(&str, String, Option<Value>)> = vec![
        ("POST", "/v1/boot".into(), Some(json!({}))),
        (
            "POST",
            "/v1/ingest/claude-code".into(),
            Some(perm_prompt("forged")),
        ),
        (
            "POST",
            "/v1/ingest/custom".into(),
            Some(json!({"kind": "turn_failed", "session": "forged"})),
        ),
        (
            "POST",
            "/v1/channel/register".into(),
            Some(json!({"pid": 1, "ppid": 2, "cwd": "/w"})),
        ),
        ("GET", "/v1/channel/stream?cid=ch_x".into(), None),
        (
            "POST",
            "/v1/channel/reply".into(),
            Some(json!({"cid": "ch_x", "chat_id": "m1", "text": "x"})),
        ),
        (
            "POST",
            "/v1/channel/decision".into(),
            Some(json!({"cid": "ch_x", "question": "q", "options": ["a", "b"]})),
        ),
        (
            "POST",
            "/v1/channel/unregister".into(),
            Some(json!({"cid": "ch_x"})),
        ),
        ("GET", "/v1/nope".into(), None),
        ("GET", "/v1".into(), None),
        ("POST", "/v1//boot".into(), Some(json!({}))),
        ("POST", "/v1/%62oot".into(), Some(json!({}))),
    ];
    for (m, p, body) in &denied {
        let r = srv.with_key(m, p, &key, body.clone()).await;
        assert_eq!(r.status, 403, "{m} {p}: {}", r.text());
        assert_eq!(
            r.json(),
            json!({"ok": false, "error": "이 열쇠로는 쓸 수 없어요"}),
            "{m} {p}"
        );
    }
    // 막힌 요청은 아무 일도 하지 않았다
    let st = srv.state().await;
    assert!(items(&st).is_empty(), "{st}");
    assert!(st["sessions"].as_array().unwrap().is_empty(), "{st}");
    // 얼굴이 쓰는 경로는 열린다(401·403이 아니다)
    let allowed: Vec<(&str, &str, Option<Value>)> = vec![
        ("GET", "/v1/state", None),
        (
            "POST",
            "/v1/items/nope/action",
            Some(json!({"action": "open"})),
        ),
        ("POST", "/v1/focus", Some(json!({"on": false}))),
        ("POST", "/v1/presence/listening", Some(json!({"on": false}))),
        ("POST", "/v1/say", Some(json!({"text": "안녕"}))),
        ("POST", "/v1/decisions/dnope", Some(json!({"choice": "a"}))),
        (
            "POST",
            "/v1/proposals/pnope",
            Some(json!({"accept": false})),
        ),
        (
            "POST",
            "/v1/adjustments/unread",
            Some(json!({"quiet": false})),
        ),
    ];
    for (m, p, body) in &allowed {
        let r = srv.with_key(m, p, &key, body.clone()).await;
        assert!(
            r.status != 401 && r.status != 403 && r.status != 0,
            "{m} {p}: {} {}",
            r.status,
            r.text()
        );
    }
    let auth = format!("Bearer {key}");
    let s = Sse::open(srv.addr, &srv.host(), "/v1/stream", Some(&auth)).await;
    assert_eq!(s.status, 200);
    drop(s);
    // 토큰은 막힌 경로를 모두 쓴다
    assert_eq!(srv.post("/v1/boot", json!({})).await.status, 200);
    let r = srv
        .post(
            "/v1/channel/register",
            json!({"pid": 1, "ppid": 2, "cwd": "/w"}),
        )
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(srv.get("/v1/nope").await.status, 404);
    srv.stop().await;
}

/// R3 H1(G-030): 쿠키는 어떤 `/v1`도 열지 않는다(살아 있는 세션 열쇠·토큰을 쿠키로 실어도 401).
/// 어떤 응답에도 `Set-Cookie`가 없다.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cookies_never_authenticate_and_nothing_sets_one() {
    let d = tempfile::tempdir().unwrap();
    let face = face_dir_with_token_meta();
    let mut cfg = config(d.path());
    cfg.face_dir = Some(face.path().to_path_buf());
    let srv = TestServer::start_with(cfg).await;
    let host = srv.host();
    let key = srv.open_session().await;
    let routes = [
        ("GET", "/v1/state"),
        ("GET", "/v1/stream"),
        ("POST", "/v1/say"),
        ("POST", "/v1/focus"),
        ("POST", "/v1/presence/listening"),
        ("POST", "/v1/items/x/action"),
        ("POST", "/v1/decisions/d1"),
        ("POST", "/v1/proposals/p1"),
        ("POST", "/v1/adjustments/unread"),
        ("POST", "/v1/boot"),
        ("POST", "/v1/ingest/claude-code"),
        ("POST", "/v1/channel/register"),
        ("GET", "/v1/channel/stream?cid=x"),
        ("POST", "/v1/channel/reply"),
        ("POST", "/v1/channel/decision"),
        ("POST", "/v1/channel/unregister"),
    ];
    for secret in [key.as_str(), srv.token.as_str()] {
        for (m, p) in routes {
            for cookie in [
                format!("gyeot_session={secret}"),
                format!("a=1; gyeot_session={secret}; b=2"),
                format!("session={secret}"),
                format!("token={secret}"),
            ] {
                let r = srv
                    .raw(m, p, &[("Host", &host), ("Cookie", &cookie)], None)
                    .await;
                assert_eq!(r.status, 401, "{m} {p} Cookie: {cookie}");
                assert_no_set_cookie(&r, p);
            }
        }
    }
    // 열쇠 자체는 살아 있다(쿠키라서 막혔다)
    assert_eq!(
        srv.with_key("GET", "/v1/state", &key, None).await.status,
        200
    );
    // 어떤 응답에도 Set-Cookie가 없다
    let code = srv.boot().await["code"].as_str().unwrap().to_owned();
    let mut seen = Vec::new();
    for p in [
        "/".to_owned(),
        "/index.html".to_owned(),
        format!("/?boot={code}"),
        "/?boot=nope".to_owned(),
        "/assets/app.js".to_owned(),
        "/nope".to_owned(),
        "/health".to_owned(),
        format!("/v1/hello?n={}", "ab".repeat(32)),
        "/v1/hello?n=zz".to_owned(),
        "/v1/state".to_owned(),
    ] {
        seen.push((
            p.clone(),
            srv.raw("GET", &p, &[("Host", &host)], None).await,
        ));
    }
    seen.push(("state(token)".into(), srv.get("/v1/state").await));
    seen.push((
        "state(key)".into(),
        srv.with_key("GET", "/v1/state", &key, None).await,
    ));
    seen.push(("boot".into(), srv.post("/v1/boot", json!({})).await));
    seen.push((
        "session ok".into(),
        srv.session_raw(json!({ "code": code }).to_string().as_bytes())
            .await,
    ));
    seen.push((
        "session bad".into(),
        srv.session_raw(json!({ "code": code }).to_string().as_bytes())
            .await,
    ));
    seen.push((
        "forbidden".into(),
        srv.with_key("POST", "/v1/boot", &key, Some(json!({})))
            .await,
    ));
    for (what, r) in &seen {
        assert!(r.status != 0, "{what}");
        assert_no_set_cookie(r, what);
    }
    srv.stop().await;
}

/// R3 M2(계약 §13.1, G-032): `GET /v1/hello?n=`는 인증 없이
/// `hex(HMAC-SHA256(토큰, "gyeot-hello/v1|<실제 포트>|<n>"))`를 준다. 포트·토큰·n이 바뀌면 증명도 바뀐다.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hello_proves_token_port_and_nonce() {
    let token = "c0ffee".repeat(10) + "abcd";
    assert_eq!(token.len(), 64);
    let (da, db, dc) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    std::fs::write(da.path().join("token"), &token).unwrap();
    std::fs::write(db.path().join("token"), &token).unwrap();
    let a = TestServer::start(da.path()).await;
    let b = TestServer::start(db.path()).await;
    let c = TestServer::start(dc.path()).await;
    assert_eq!(a.token, token);
    assert_eq!(b.token, token);
    assert_ne!(c.token, token);
    let (pa, pb) = (a.addr.port(), b.addr.port());
    assert_ne!(pa, pb);
    let n = "ab".repeat(32);
    let proof = |r: &Resp| -> String {
        assert_eq!(r.status, 200, "{}", r.text());
        let v = r.json();
        assert_eq!(v["v"], 1, "{v}");
        let mut keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, ["face", "proof", "v"], "{v}");
        assert!(v["face"].is_array(), "{v}");
        v["proof"].as_str().unwrap().to_owned()
    };
    // 인증 머리 없이 받는다
    let r = a.hello(&n).await;
    assert_eq!(r.header("cache-control"), Some("no-store"));
    assert_no_set_cookie(&r, "hello");
    let got_a = proof(&r);
    assert_eq!(got_a.len(), 64);
    assert_eq!(got_a, expected_proof(&token, pa, &n));
    // 같은 토큰이라도 묶인 포트가 다르면 다르다(포트 0으로 묶어도 실제 포트를 쓴다)
    let got_b = proof(&b.hello(&n).await);
    assert_eq!(got_b, expected_proof(&token, pb, &n));
    assert_ne!(got_a, got_b);
    // 토큰이 다르면 다르다
    let got_c = proof(&c.hello(&n).await);
    assert_eq!(got_c, expected_proof(&c.token, c.addr.port(), &n));
    assert_ne!(got_c, expected_proof(&token, c.addr.port(), &n));
    // n이 다르면 다르다. 경계 길이 16·128자, 대문자 hex도 받는다(그대로 묶는다)
    for m in [
        "cd".repeat(32),
        "0".repeat(16),
        "f".repeat(128),
        "AB".repeat(8),
    ] {
        let got = proof(&a.hello(&m).await);
        assert_eq!(got, expected_proof(&token, pa, &m), "{m}");
        assert_ne!(got, got_a, "{m}");
    }
    // 토큰을 붙여도 같은 답(인증과 무관)
    assert_eq!(proof(&a.get(&format!("/v1/hello?n={n}")).await), got_a);
    // 잘못된 n은 400
    let host = a.host();
    for q in [
        "/v1/hello".to_owned(),
        "/v1/hello?".to_owned(),
        "/v1/hello?n=".to_owned(),
        format!("/v1/hello?n={}", "a".repeat(15)),
        format!("/v1/hello?n={}", "a".repeat(129)),
        format!("/v1/hello?n={}", "zz".repeat(16)),
        format!("/v1/hello?n={}%20", "a".repeat(16)),
        format!("/v1/hello?m={n}"),
        format!("/v1/hello?nn={n}"),
    ] {
        let r = a.raw("GET", &q, &[("Host", &host)], None).await;
        assert_eq!(r.status, 400, "{q}: {}", r.text());
        assert_eq!(r.json()["ok"], false, "{q}");
    }
    // Host·Origin 규칙은 그대로
    let q = format!("/v1/hello?n={n}");
    let r = a.raw("GET", &q, &[("Host", "evil.example")], None).await;
    assert_eq!(r.status, 421);
    let r = a
        .raw(
            "GET",
            &q,
            &[("Host", &host), ("Origin", "http://evil.example")],
            None,
        )
        .await;
    assert_eq!(r.status, 403);
    a.stop().await;
    b.stop().await;
    c.stop().await;
}

/// 열린 코드는 8개까지(넘으면 가장 오래된 것부터 버린다).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn at_most_eight_open_boot_codes() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let mut codes = Vec::new();
    for _ in 0..9 {
        codes.push(srv.boot().await["code"].as_str().unwrap().to_owned());
    }
    let redeem = |c: &str| json!({ "code": c }).to_string();
    let r = srv.session_raw(redeem(&codes[0]).as_bytes()).await;
    assert_eq!(r.status, 401, "가장 오래된 코드는 버렸다");
    for c in &codes[1..] {
        let r = srv.session_raw(redeem(c).as_bytes()).await;
        assert_eq!(r.status, 200, "{}", r.text());
    }
    srv.stop().await;
}

/// R5 H1(계약 §14.1·§12.1 4번, G-036): 세션은 데몬 메모리에만 있다. 최대 20개, `sessions.json`을
/// 쓰지 않고, 데몬을 다시 띄우면 이전 열쇠는 모두 401이다.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_live_only_in_memory_and_die_with_the_daemon() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let mut keys = Vec::new();
    for _ in 0..21 {
        keys.push(srv.open_session().await);
    }
    // 가장 오래 안 쓴 첫 세션이 밀려났다
    assert_eq!(
        srv.with_key("GET", "/v1/state", &keys[0], None)
            .await
            .status,
        401
    );
    for k in &keys[1..] {
        assert_eq!(srv.with_key("GET", "/v1/state", k, None).await.status, 200);
    }
    assert!(
        !d.path().join("sessions.json").exists(),
        "돌 때도 쓰지 않는다"
    );
    srv.stop().await;
    // 끌 때도 쓰지 않고, 데이터 폴더 어디에도 열쇠·해시가 없다
    assert!(
        !d.path().join("sessions.json").exists(),
        "끌 때도 쓰지 않는다"
    );
    let mut stack = vec![d.path().to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let bytes = std::fs::read(&p).unwrap();
            let text = String::from_utf8_lossy(&bytes);
            for k in &keys {
                assert!(!text.contains(k.as_str()), "{}: 열쇠가 남음", p.display());
                assert!(
                    !text.contains(&sha256_hex(k)),
                    "{}: 열쇠의 해시가 남음",
                    p.display()
                );
            }
        }
    }
    // 다시 띄우면 이전 열쇠는 모두 401(가장 최근 것도)
    let srv = TestServer::start(d.path()).await;
    for k in &keys {
        assert_eq!(srv.with_key("GET", "/v1/state", k, None).await.status, 401);
    }
    assert_eq!(srv.hello(&"ab".repeat(32)).await.json()["face"], json!([]));
    // 새로 연 세션은 쓸 수 있다
    let fresh = srv.open_session().await;
    assert_eq!(
        srv.with_key("GET", "/v1/state", &fresh, None).await.status,
        200
    );
    srv.stop().await;
}

/// R5 H1(계약 §14.1): 옛 판이 남긴 `sessions.json`(과 쓰다 만 임시 파일)은 시작할 때 지우고 읽지 않는다.
/// 그 파일에 해시가 있던 열쇠도 401이다.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn leftover_sessions_file_is_removed_and_ignored() {
    let d = tempfile::tempdir().unwrap();
    let key = "5".repeat(64);
    let now = now_ms();
    let old = json!({
        "version": 1,
        "sessions": [{
            "sha256": sha256_hex(&key),
            "created_ms": now - 1000,
            "last_ms": now - 1000,
            "expires_ms": now - 1000 + 7 * 86_400_000_i64,
        }]
    });
    std::fs::write(d.path().join("sessions.json"), old.to_string()).unwrap();
    std::fs::write(d.path().join("sessions.tmp-4242"), old.to_string()).unwrap();
    let other = d.path().join("sessions-notes.txt");
    std::fs::write(&other, "x").unwrap();
    let srv = TestServer::start(d.path()).await;
    assert!(!d.path().join("sessions.json").exists(), "시작할 때 지운다");
    assert!(!d.path().join("sessions.tmp-4242").exists(), "임시 파일도");
    assert!(other.exists(), "상관없는 파일은 둔다");
    assert_eq!(
        srv.with_key("GET", "/v1/state", &key, None).await.status,
        401
    );
    assert_eq!(srv.hello(&"ab".repeat(32)).await.json()["face"], json!([]));
    srv.stop().await;
    assert!(!d.path().join("sessions.json").exists());
}

/// 얼굴 담당과 같은 공유 값(파이썬 hmac·hashlib으로 따로 계산): 세션 `"0123456789abcdef"×4`,
/// n `"ab"×32`, 포트 47321. 시험 쪽 계산이 계약 식과 같은지 먼저 확인한다.
#[test]
fn face_proof_helper_matches_shared_vector() {
    let session = "0123456789abcdef".repeat(4);
    let n = "ab".repeat(32);
    assert_eq!(
        sha256_hex(&session),
        "a8ae6e6ee929abea3afcfc5258c8ccd6f85273e0d4626d26c7279f3250f77c8e"
    );
    assert_eq!(
        expected_face(&session, 47321, &n),
        "607b8f06e715b5b23515fbb572a63617cf35019b687b1444f4433e27a7d368e4"
    );
    assert_eq!(
        expected_face(&session, 47322, &n),
        "fcb875c58193eea573405310509b4ded38cafc4c0296e5189bad09d3a6d83f91"
    );
}

/// R5 H1(계약 §14.1, G-036): hello의 `face`는 살아 있는 세션마다
/// `hex(HMAC-SHA256(SHA-256(세션 값), "gyeot-face-hello/v1|<실제 포트>|<n>"))`이고 순서를 섞는다.
/// 얼굴은 이 목록으로 상대가 내 세션을 아는지 확인한 뒤에만 열쇠를 보낸다.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hello_lists_face_proofs_of_live_sessions() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let port = srv.addr.port();
    let n = "ab".repeat(32);
    let face_of = |r: &Resp| -> Vec<String> {
        assert_eq!(r.status, 200, "{}", r.text());
        r.json()["face"]
            .as_array()
            .unwrap_or_else(|| panic!("{}", r.text()))
            .iter()
            .map(|x| x.as_str().unwrap().to_owned())
            .collect()
    };
    // 세션이 없으면 빈 목록(증명은 그대로)
    let r = srv.hello(&n).await;
    assert_eq!(face_of(&r), Vec::<String>::new());
    assert_eq!(
        r.json()["proof"].as_str().unwrap(),
        expected_proof(&srv.token, port, &n)
    );
    // 세션 둘이면 둘, 값은 계약 식과 같다
    let k1 = srv.open_session().await;
    let k2 = srv.open_session().await;
    let mut got = face_of(&srv.hello(&n).await);
    got.sort();
    let mut want = vec![expected_face(&k1, port, &n), expected_face(&k2, port, &n)];
    want.sort();
    assert_eq!(got, want);
    for f in &got {
        assert_eq!(f.len(), 64);
        assert!(
            f.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        for k in [&k1, &k2] {
            assert_ne!(f, k, "열쇠를 내보이지 않는다");
            assert_ne!(f, &sha256_hex(k), "해시를 내보이지 않는다");
        }
    }
    // n이 다르면 다르다(다시 쓰지 못한다)
    let m = "cd".repeat(32);
    let mut got_m = face_of(&srv.hello(&m).await);
    got_m.sort();
    let mut want_m = vec![expected_face(&k1, port, &m), expected_face(&k2, port, &m)];
    want_m.sort();
    assert_eq!(got_m, want_m);
    assert!(got_m.iter().all(|f| !got.contains(f)));
    // 토큰을 붙여도, 세션 열쇠를 붙여도 같은 목록(인증과 무관)
    let mut with_key = face_of(
        &srv.with_key("GET", &format!("/v1/hello?n={n}"), &k1, None)
            .await,
    );
    with_key.sort();
    assert_eq!(with_key, want);
    // 20개를 채우면 20개, 순서는 섞인다
    let mut keys = vec![k1.clone(), k2.clone()];
    for _ in 0..18 {
        keys.push(srv.open_session().await);
    }
    let mut want20: Vec<String> = keys.iter().map(|k| expected_face(k, port, &n)).collect();
    want20.sort();
    let mut orders = std::collections::HashSet::new();
    for _ in 0..6 {
        let got = face_of(&srv.hello(&n).await);
        assert_eq!(got.len(), 20);
        let mut sorted = got.clone();
        sorted.sort();
        assert_eq!(sorted, want20);
        orders.insert(got);
    }
    assert!(
        orders.len() > 1,
        "순서를 섞는다(20개를 6번 받아 같은 순서만 나옴)"
    );
    // 21번째가 들어오면 밀려난 세션의 증명은 빠진다
    let k21 = srv.open_session().await;
    let got = face_of(&srv.hello(&n).await);
    assert_eq!(got.len(), 20);
    assert!(got.contains(&expected_face(&k21, port, &n)));
    assert!(
        !got.contains(&expected_face(&k1, port, &n)),
        "가장 오래 안 쓴 것"
    );
    srv.stop().await;
}

/// 얼굴 폴더가 없을 때의 안내 쪽도 인증 없이, 보안 머리를 붙여 준다.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn builtin_page_without_face() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    for path in ["/", "/index.html", "/?boot=x"] {
        let r = srv.raw("GET", path, &[("Host", &srv.host())], None).await;
        assert_eq!(r.status, 200, "{path}");
        let t = r.text();
        assert!(t.contains("npm run build"), "{t}");
        assert!(t.contains("GYEOT_FACE_DIR"));
        assert!(!t.contains(&srv.token));
        assert_face_headers(&r, true, "builtin");
        assert_no_set_cookie(&r, path);
    }
    let r = srv
        .raw("GET", "/assets/x.js", &[("Host", &srv.host())], None)
        .await;
    assert_eq!(r.status, 404);
    srv.stop().await;
}

/// R3 L3·L4(계약 §13.4): Unix에서 데이터 폴더·하위 폴더는 0700, 파일은 0600으로 만든다.
/// 이미 있던 느슨한 것도 시작할 때 맞춘다.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn data_home_is_private_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let loosen = |p: &std::path::Path, m: u32| {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
    };
    // 처음 만들 때
    let srv = TestServer::start(&home).await;
    srv.post(
        "/v1/ingest/claude-code",
        hook("PreToolUse", "s1", json!({"tool_name": "Bash"})),
    )
    .await;
    srv.open_session().await;
    srv.stop().await;
    for dir in [home.clone(), home.join("events"), home.join("spool")] {
        assert_eq!(mode(&dir), 0o700, "{}", dir.display());
    }
    let logs: Vec<_> = std::fs::read_dir(home.join("events"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert!(!logs.is_empty());
    for f in logs
        .iter()
        .cloned()
        .chain(["token", "learn.json"].map(|n| home.join(n)))
    {
        assert_eq!(mode(&f), 0o600, "{}", f.display());
    }
    assert!(
        !home.join("sessions.json").exists(),
        "세션은 디스크에 없다(§14.1)"
    );
    // 이미 있던 느슨한 것: 폴더 0755, 파일 0644
    let yesterday = gyeot_core::paths::event_log_name(now_ms() - 86_400_000);
    let old_log = home.join("events").join(&yesterday);
    std::fs::write(&old_log, "").unwrap();
    std::fs::create_dir_all(home.join("open")).unwrap();
    std::fs::write(home.join("gyeotd.log"), "x\n").unwrap();
    std::fs::write(home.join("tails.json"), "{}").unwrap();
    for dir in [
        home.clone(),
        home.join("events"),
        home.join("spool"),
        home.join("open"),
    ] {
        loosen(&dir, 0o755);
    }
    let files = [
        old_log.clone(),
        home.join("token"),
        home.join("learn.json"),
        home.join("gyeotd.log"),
        home.join("tails.json"),
    ];
    for f in &files {
        loosen(f, 0o644);
    }
    // 데이터 폴더 밖을 가리키는 링크는 따라가 바꾸지 않는다
    let outside = d.path().join("outside.txt");
    std::fs::write(&outside, "x").unwrap();
    loosen(&outside, 0o644);
    std::os::unix::fs::symlink(&outside, home.join("events").join("2000-01-01.jsonl")).unwrap();
    let srv = TestServer::start(&home).await;
    for dir in [
        home.clone(),
        home.join("events"),
        home.join("spool"),
        home.join("open"),
    ] {
        assert_eq!(mode(&dir), 0o700, "시작할 때 맞춘다: {}", dir.display());
    }
    for f in &files {
        assert_eq!(mode(f), 0o600, "시작할 때 맞춘다: {}", f.display());
    }
    assert_eq!(mode(&outside), 0o644, "링크를 따라가지 않는다");
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tail_reads_new_harness_lines() {
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("fsmap.jsonl");
    std::fs::write(&log, "{\"t\":1,\"event\":\"gate\",\"result\":\"fail\"}\n").unwrap();
    let mut cfg = config(d.path());
    cfg.tails.push(gyeot_server::TailConfig {
        path: log.clone(),
        source: "fsmap-harness".into(),
        cwd: Some("C:/work/proj".into()),
    });
    let srv = TestServer::start_with(cfg).await;
    srv.post(
        "/v1/ingest/claude-code",
        hook("PreToolUse", "abc123", json!({"tool_name": "Bash"})),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    // 이미 있던 줄은 들이지 않는다
    assert!(items(&srv.state().await).is_empty());
    let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
    use std::io::Write;
    let t = now_ms() as f64 / 1000.0;
    writeln!(
        f,
        "{{\"t\":{t},\"event\":\"gate\",\"result\":\"block\",\"ms\":5}}"
    )
    .unwrap();
    drop(f);
    let mut found = false;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let st = srv.state().await;
        if let Some(i) = items(&st).into_iter().find(|i| i["kind"] == "failed") {
            assert_eq!(i["session"], "abc123", "저장소 경로로 세션을 찾는다");
            assert_eq!(i["title"], "fsmap 게이트 막힘");
            found = true;
            break;
        }
    }
    assert!(found, "tail line became an item");
    srv.stop().await;
    assert!(d.path().join("tails.json").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streams_ping_every_fifteen_seconds() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let (cid, _) = register(&srv, 1, 2, "/w").await;
    let mut face = srv.sse("/v1/stream").await;
    let auth = format!("Bearer {}", srv.token);
    let mut ch = Sse::open(
        srv.addr,
        &srv.host(),
        &format!("/v1/channel/stream?cid={cid}"),
        Some(&auth),
    )
    .await;
    assert_eq!(ch.status, 200);
    assert_eq!(face.next(S).await.unwrap().event, "snapshot");
    let (a, b) = tokio::join!(
        face.wait_for(":", Duration::from_secs(17)),
        ch.wait_for(":", Duration::from_secs(17))
    );
    assert_eq!(a.expect("face ping").data, "ping");
    assert_eq!(b.expect("channel ping").data, "ping");
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn channel_closes_ten_seconds_after_stream_drops() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    srv.post(
        "/v1/ingest/claude-code",
        envelope(
            hook("SessionStart", "abc123", json!({"source": "startup"})),
            4321,
        ),
    )
    .await;
    let (cid, session) = register(&srv, 1, 4321, "/w").await;
    assert_eq!(session, "abc123");
    let ch = srv.sse(&format!("/v1/channel/stream?cid={cid}")).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drop(ch);
    // 5초 뒤 같은 cid로 다시 붙으면 살아 있다
    tokio::time::sleep(Duration::from_secs(5)).await;
    let ch = srv.sse(&format!("/v1/channel/stream?cid={cid}")).await;
    assert_eq!(ch.status, 200);
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(srv.state().await["sessions"][0]["channel"], cid);
    // 다시 끊고 10초 넘게 두면 닫힌다
    drop(ch);
    tokio::time::sleep(Duration::from_millis(11_500)).await;
    let st = srv.state().await;
    assert_eq!(st["sessions"][0]["channel"], Value::Null);
    assert_eq!(
        srv.get(&format!("/v1/channel/stream?cid={cid}"))
            .await
            .status,
        404
    );
    srv.stop().await;
}

/// `POST /v1/boot`는 본문을 다 읽은 뒤 답한다. 본문을 남긴 채 연결을 닫으면 Windows는 RST를 보내
/// 클라이언트가 이미 받은 응답까지 잃는다(CI windows-latest에서 상태 0으로 관찰). 머리와 본문이
/// 따로 도착해도 응답을 온전히 받아야 한다.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn boot_reads_body_before_answering() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    for _ in 0..3 {
        let mut s = tokio::net::TcpStream::connect(srv.addr).await.unwrap();
        s.set_nodelay(true).unwrap();
        let head = format!(
            "POST /v1/boot HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n",
            srv.host(),
            srv.token
        );
        s.write_all(head.as_bytes()).await.unwrap();
        s.flush().await.unwrap();
        // 서버가 본문을 기다리지 않으면 이 사이에 답하고 닫는다.
        tokio::time::sleep(Duration::from_millis(150)).await;
        let mut buf = Vec::new();
        let first = tokio::time::timeout(Duration::from_millis(50), s.read_buf(&mut buf)).await;
        assert!(
            first.is_err() || buf.is_empty(),
            "본문을 받기 전에 답했다: {}",
            String::from_utf8_lossy(&buf)
        );
        let _ = s.write_all(b"{}").await;
        let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut buf)).await;
        let text = String::from_utf8_lossy(&buf);
        assert!(text.starts_with("HTTP/1.1 200"), "응답: {text}");
    }
    srv.stop().await;
}

/// 거절(401·403·421)도 본문을 다 읽은 뒤 답한다. 본문을 남긴 채 닫으면 Windows가 RST를 보내
/// 클라이언트가 거절 응답까지 잃는다(CI windows-latest에서 상태 0으로 관찰, 2026-11-26).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejections_read_body_before_answering() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let cases = [
        (
            "Authorization: Bearer nope\r\n".to_owned(),
            srv.host(),
            "401",
        ),
        (String::new(), "evil.example".to_owned(), "421"),
    ];
    for (auth, host, want) in cases {
        let mut s = tokio::net::TcpStream::connect(srv.addr).await.unwrap();
        s.set_nodelay(true).unwrap();
        let head = format!(
            "POST /v1/ingest/claude-code HTTP/1.1\r\nHost: {host}\r\n{auth}Content-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n"
        );
        s.write_all(head.as_bytes()).await.unwrap();
        s.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        let mut buf = Vec::new();
        let first = tokio::time::timeout(Duration::from_millis(50), s.read_buf(&mut buf)).await;
        assert!(
            first.is_err() || buf.is_empty(),
            "본문을 받기 전에 답했다({want}): {}",
            String::from_utf8_lossy(&buf)
        );
        let _ = s.write_all(b"{}").await;
        let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut buf)).await;
        let text = String::from_utf8_lossy(&buf);
        assert!(
            text.starts_with(&format!("HTTP/1.1 {want}")),
            "응답: {text}"
        );
    }
    srv.stop().await;
}
