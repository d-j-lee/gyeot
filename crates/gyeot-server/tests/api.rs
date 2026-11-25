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

/// R1 H1·M1·L3(계약 §7·§12.1): 얼굴 쪽에 토큰을 싣지 않고, 쿠키 없이는 안내 쪽(401)을 보인다.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn index_needs_session_and_never_contains_token() {
    let d = tempfile::tempdir().unwrap();
    let face = face_dir_with_token_meta();
    std::fs::write(d.path().join("secret.txt"), "nope").unwrap();
    let mut cfg = config(d.path());
    cfg.face_dir = Some(face.path().to_path_buf());
    let srv = TestServer::start_with(cfg).await;
    let host = srv.host();
    // 쿠키 없음 → 한국어 안내 쪽(401), 토큰 없음
    for path in ["/", "/index.html", "/%69ndex.html", "/?mode=overlay"] {
        let r = srv.raw("GET", path, &[("Host", &host)], None).await;
        let t = r.text();
        assert!(!t.contains(&srv.token), "{path}");
        assert!(!t.contains("얼굴</body>"), "{path}: 얼굴이 새면 안 됨");
        if path == "/%69ndex.html" {
            assert_eq!(r.status, 404, "{path}");
            continue;
        }
        assert_eq!(r.status, 401, "{path}");
        assert!(r.header("content-type").unwrap().starts_with("text/html"));
        assert!(t.contains("곁 열기"), "{t}");
        assert!(t.contains("gyeotd --open"), "{t}");
        assert_face_headers(&r, true, path);
    }
    // 틀린·없는 쿠키도 안내 쪽
    let r = srv.with_cookie("GET", "/", &"0".repeat(64)).await;
    assert_eq!(r.status, 401);
    let r = srv.with_cookie("GET", "/", "garbage").await;
    assert_eq!(r.status, 401);
    // 쿠키가 있으면 얼굴(토큰을 채우지 않는다)
    let cookie = srv.open_session().await;
    for path in ["/", "/index.html"] {
        let r = srv.with_cookie("GET", path, &cookie).await;
        assert_eq!(r.status, 200, "{path}");
        let t = r.text();
        assert!(t.contains("얼굴</body>"), "{t}");
        assert!(t.contains(r#"<meta name="gyeot-token" content="">"#), "{t}");
        assert!(!t.contains(&srv.token));
        assert_face_headers(&r, true, path);
    }
    // 정적 파일은 쿠키 없이도(비밀 없음), 보안 머리를 붙인다
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

/// 부트 코드: Bearer로만 받고, 한 번만 쓰며, 쓰면 세션 쿠키를 달고 boot만 뺀 주소로 303.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn boot_code_opens_a_cookie_session_once() {
    let d = tempfile::tempdir().unwrap();
    let face = face_dir_with_token_meta();
    let mut cfg = config(d.path());
    cfg.face_dir = Some(face.path().to_path_buf());
    let srv = TestServer::start_with(cfg).await;
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

    // 다른 쿼리는 남기고 boot만 뺀다
    let r = srv
        .raw(
            "GET",
            &format!("/?mode=overlay&boot={code}&x=1"),
            &[("Host", &host)],
            None,
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.text());
    assert_eq!(r.header("location"), Some("/?mode=overlay&x=1"));
    let set = r.header("set-cookie").unwrap().to_owned();
    let cookie = cookie_of(&r).unwrap();
    assert_eq!(cookie.len(), 64);
    assert!(cookie.bytes().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(
        set,
        format!("gyeot_session={cookie}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000")
    );
    assert_ne!(cookie, code);
    assert!(!r.text().contains(&srv.token));
    assert_eq!(r.header("cache-control"), Some("no-store"));
    assert_eq!(r.header("referrer-policy"), Some("no-referrer"));

    // 한 번만 쓴다
    let r = srv
        .raw("GET", &format!("/?boot={code}"), &[("Host", &host)], None)
        .await;
    assert_eq!(r.status, 401);
    assert!(r.header("set-cookie").is_none());
    let t = r.text();
    assert!(t.contains("gyeotd --open") && t.contains("곁 열기"), "{t}");
    assert_face_headers(&r, true, "used boot");
    // 틀린 코드·빈 코드
    for bad in [
        "/?boot=nope",
        "/?boot=",
        &format!("/?boot={}", "a".repeat(64)),
    ] {
        let r = srv.raw("GET", bad, &[("Host", &host)], None).await;
        assert_eq!(r.status, 401, "{bad}");
        assert!(r.header("set-cookie").is_none());
    }
    // 다른 경로에서도 같고, 경로는 그대로 둔다
    let code2 = srv.boot().await["code"].as_str().unwrap().to_owned();
    let r = srv
        .raw(
            "GET",
            &format!("/index.html?boot={code2}"),
            &[("Host", &host)],
            None,
        )
        .await;
    assert_eq!(r.status, 303);
    assert_eq!(r.header("location"), Some("/index.html"));
    // 프로토콜 상대 주소로 보내지 않는다
    let code3 = srv.boot().await["code"].as_str().unwrap().to_owned();
    let r = srv
        .raw(
            "GET",
            &format!("//evil.example/?boot={code3}"),
            &[("Host", &host)],
            None,
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.text());
    assert_eq!(r.header("location"), Some("/"));

    // 쿠키로 API·SSE를 쓴다
    let r = srv.with_cookie("GET", "/v1/state", &cookie).await;
    assert_eq!(r.status, 200, "{}", r.text());
    let c = format!("gyeot_session={cookie}");
    let mut s = Sse::open(srv.addr, &host, "/v1/stream", None).await;
    assert_eq!(s.status, 401, "쿠키·토큰 없는 SSE");
    drop(s);
    let mixed = format!("other=1; {c}; x=y");
    let mut face = Sse::open_with(srv.addr, &host, "/v1/stream", &[("Cookie", &mixed)]).await;
    assert_eq!(face.status, 200, "쿠키로 여는 얼굴 SSE");
    assert_eq!(face.next(S).await.unwrap().event, "snapshot");
    // 쿠키로는 부트 코드를 만들 수 없다
    let r = srv.with_cookie("POST", "/v1/boot", &cookie).await;
    assert_eq!(r.status, 401);
    let r = srv
        .raw(
            "POST",
            "/v1/boot",
            &[
                ("Host", &host),
                ("Cookie", &c),
                ("Authorization", "Bearer nope"),
            ],
            None,
        )
        .await;
    assert_eq!(r.status, 401);
    // 틀린 쿠키로는 API를 쓸 수 없다
    let r = srv.with_cookie("GET", "/v1/state", &"f".repeat(64)).await;
    assert_eq!(r.status, 401);
    s = srv.sse("/v1/stream").await;
    assert_eq!(s.status, 200);
    srv.stop().await;
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
    let host = srv.host();
    let r = srv
        .raw(
            "GET",
            &format!("/?boot={}", codes[0]),
            &[("Host", &host)],
            None,
        )
        .await;
    assert_eq!(r.status, 401, "가장 오래된 코드는 버렸다");
    for c in &codes[1..] {
        let r = srv
            .raw("GET", &format!("/?boot={c}"), &[("Host", &host)], None)
            .await;
        assert_eq!(r.status, 303);
    }
    srv.stop().await;
}

/// 세션은 SHA-256만 0600 파일에 남기고, 데몬을 다시 띄워도 이어진다. 최대 20개.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_are_hashed_on_disk_and_survive_restart() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let mut cookies = Vec::new();
    for _ in 0..21 {
        cookies.push(srv.open_session().await);
    }
    // 가장 오래 안 쓴 첫 세션이 밀려났다
    assert_eq!(
        srv.with_cookie("GET", "/v1/state", &cookies[0])
            .await
            .status,
        401
    );
    for c in &cookies[1..] {
        assert_eq!(srv.with_cookie("GET", "/v1/state", c).await.status, 200);
    }
    srv.stop().await;
    let path = d.path().join("sessions.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    let list = v["sessions"].as_array().unwrap();
    assert_eq!(list.len(), 20);
    for c in &cookies {
        assert!(!text.contains(c.as_str()), "세션 값이 그대로 남으면 안 됨");
    }
    for e in list {
        let h = e["sha256"].as_str().unwrap();
        assert_eq!(h.len(), 64);
        let created = e["created_ms"].as_i64().unwrap();
        assert_eq!(e["expires_ms"].as_i64().unwrap(), created + 30 * 86_400_000);
        assert!(e["last_ms"].as_i64().unwrap() >= created);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    let srv = TestServer::start(d.path()).await;
    assert_eq!(
        srv.with_cookie("GET", "/v1/state", &cookies[20])
            .await
            .status,
        200
    );
    assert_eq!(
        srv.with_cookie("GET", "/v1/state", &cookies[0])
            .await
            .status,
        401
    );
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn builtin_page_without_face() {
    let d = tempfile::tempdir().unwrap();
    let srv = TestServer::start(d.path()).await;
    let r = srv.raw("GET", "/", &[("Host", &srv.host())], None).await;
    assert_eq!(r.status, 401);
    let cookie = srv.open_session().await;
    let r = srv.with_cookie("GET", "/", &cookie).await;
    assert_eq!(r.status, 200);
    let t = r.text();
    assert!(t.contains("npm run build"), "{t}");
    assert!(t.contains("GYEOT_FACE_DIR"));
    assert!(!t.contains(&srv.token));
    assert_face_headers(&r, true, "builtin");
    let r = srv
        .raw("GET", "/assets/x.js", &[("Host", &srv.host())], None)
        .await;
    assert_eq!(r.status, 404);
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
