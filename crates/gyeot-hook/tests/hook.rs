//! 만든 `gyeot-hook` 실행 파일을 띄워 본다: 데몬 없음(스풀), 진짜 데몬(전달·스풀 비우기).

use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn hook_cmd(home: &Path, url: &str) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_gyeot-hook"));
    c.env("GYEOT_HOME", home)
        .env("GYEOT_URL", url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}

fn run_hook(home: &Path, url: &str, args: &[&str], stdin: &[u8]) -> (Output, Duration) {
    let start = Instant::now();
    let mut child = hook_cmd(home, url).args(args).spawn().expect("spawn hook");
    {
        let mut si = child.stdin.take().unwrap();
        let _ = si.write_all(stdin);
    }
    let out = child.wait_with_output().unwrap();
    (out, start.elapsed())
}

fn spool_files(home: &Path) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(home.join("spool"))
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

fn notification(session: &str) -> Vec<u8> {
    json!({
        "session_id": session,
        "transcript_path": "/t.jsonl",
        "cwd": "/work/proj",
        "hook_event_name": "Notification",
        "notification_type": "permission_prompt",
        "message": "Claude needs your permission to use Bash"
    })
    .to_string()
    .into_bytes()
}

#[test]
fn without_daemon_it_spools_silently() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("token"), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}", free_port());
    let (out, took) = run_hook(d.path(), &url, &["claude-code"], &notification("s1"));
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty(), "stdout must stay empty");
    assert!(took < Duration::from_secs(3), "took {took:?}");
    let files = spool_files(d.path());
    assert_eq!(files.len(), 1, "{files:?}");
    let name = files[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.ends_with(".json"), "{name}");
    let v: Value = serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
    assert_eq!(v["source"], "claude-code");
    assert_eq!(v["payload"]["session_id"], "s1");
    assert!(v["received_ms"].as_i64().unwrap() > 0);
    assert!(v["pid_hint"].as_u64().is_some());
}

#[test]
fn bad_input_and_odd_setups_still_exit_zero() {
    let d = tempfile::tempdir().unwrap();
    let url = format!("http://127.0.0.1:{}", free_port());
    // 쓰레기 입력: 아무것도 남기지 않는다
    let (out, _) = run_hook(d.path(), &url, &["claude-code"], b"\xff\xfe not json");
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(spool_files(d.path()).is_empty());
    // 인자 없음, 이상한 source
    for args in [&[][..], &["../../etc"][..]] {
        let (out, _) = run_hook(d.path(), &url, args, &notification("x"));
        assert_eq!(out.status.code(), Some(0));
        assert!(out.stdout.is_empty());
    }
    assert!(spool_files(d.path()).is_empty());
    // 토큰이 없으면 스풀에만
    let (out, _) = run_hook(d.path(), &url, &["codex"], &notification("x"));
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(spool_files(d.path()).len(), 1);
    // 바깥 주소는 쓰지 않는다(토큰을 보내지 않고 스풀에만)
    std::fs::write(d.path().join("token"), "b".repeat(64)).unwrap();
    let (out, took) = run_hook(
        d.path(),
        "http://example.com:80",
        &["codex"],
        &notification("y"),
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(took < Duration::from_secs(3));
    assert_eq!(spool_files(d.path()).len(), 2);
    // codex-notify인데 인자가 없으면 그냥 끝
    let (out, _) = run_hook(d.path(), &url, &["codex-notify"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(spool_files(d.path()).len(), 2);
}

const SECRET: &str = "sk-ant-api03-SECRETSECRETSECRET123";

/// R1 M3·L6(계약 §12.5): 스풀에는 줄인 페이로드만, 권한은 파일 0600·폴더 0700.
#[test]
fn spooled_payload_is_sanitized_and_private() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("token"), "a".repeat(64)).unwrap();
    // 이미 있던 느슨한 스풀 폴더도 좁힌다
    std::fs::create_dir_all(d.path().join("spool")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            d.path().join("spool"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let url = format!("http://127.0.0.1:{}", free_port());
    let prompt = format!("비밀 프롬프트 {SECRET}");
    let input = json!({
        "session_id": "s1",
        "transcript_path": "/home/me/.claude/projects/p/s1.jsonl",
        "cwd": "/work/proj",
        "hook_event_name": "UserPromptSubmit",
        "permission_mode": "default",
        "prompt": prompt,
    });
    let (out, _) = run_hook(
        d.path(),
        &url,
        &["claude-code"],
        input.to_string().as_bytes(),
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    let write = json!({
        "session_id": "s1",
        "transcript_path": "/t.jsonl",
        "cwd": "/work/proj",
        "hook_event_name": "PermissionRequest",
        "tool_name": "Write",
        "tool_input": {"file_path": "/work/proj/.env", "content": format!("KEY={SECRET}\n비밀 내용")},
    });
    run_hook(
        d.path(),
        &url,
        &["claude-code"],
        write.to_string().as_bytes(),
    );
    let files = spool_files(d.path());
    assert_eq!(files.len(), 2, "{files:?}");
    let mut payloads = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        for bad in [
            "비밀",
            SECRET,
            "transcript_path",
            "permission_mode",
            "content",
        ] {
            assert!(!text.contains(bad), "{bad} 남음: {text}");
        }
        let v: Value = serde_json::from_str(&text).unwrap();
        payloads.push(v["payload"].clone());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(f).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{}", f.display());
        }
    }
    assert!(
        payloads
            .iter()
            .any(|p| p["prompt"] == "x".repeat(prompt.chars().count())),
        "{payloads:?}"
    );
    assert!(
        payloads
            .iter()
            .any(|p| p["tool_input"] == json!({"file_path": "/work/proj/.env"})),
        "{payloads:?}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(d.path().join("spool"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
    }
}

/// 데몬에 보내는 본문도 줄인 것이다(가짜 데몬이 받은 본문을 본다).
#[test]
fn sent_payload_is_sanitized() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("token"), "a".repeat(64)).unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut buf = Vec::new();
        let mut tmp = [0u8; 65536];
        loop {
            let n = s.read(&mut tmp).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            let text = String::from_utf8_lossy(&buf).into_owned();
            if let Some(at) = text.find("\r\n\r\n") {
                let len = text[..at]
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case("content-length")
                            .then(|| v.trim().parse::<usize>().ok())?
                    })
                    .unwrap_or(0);
                if buf.len() >= at + 4 + len {
                    let _ = s.write_all(
                        b"HTTP/1.1 202 Accepted\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                    );
                    return String::from_utf8_lossy(&buf[at + 4..at + 4 + len]).into_owned();
                }
            }
        }
        String::new()
    });
    let input = json!({
        "session_id": "s1",
        "transcript_path": "/t.jsonl",
        "cwd": "/work/proj",
        "hook_event_name": "Stop",
        "last_assistant_message": format!("끝났어요. 키는 {SECRET} 입니다."),
        "stop_hook_active": false,
    });
    let url = format!("http://127.0.0.1:{port}");
    let (out, _) = run_hook(
        d.path(),
        &url,
        &["claude-code"],
        input.to_string().as_bytes(),
    );
    assert_eq!(out.status.code(), Some(0));
    let body = server.join().unwrap();
    let v: Value = serde_json::from_str(&body).unwrap_or_else(|e| panic!("{e}: {body}"));
    assert_eq!(
        v["payload"],
        json!({
            "session_id": "s1",
            "cwd": "/work/proj",
            "hook_event_name": "Stop",
            "last_assistant_message": "끝났어요. 키는 [가림] 입니다.",
        })
    );
    assert!(spool_files(d.path()).is_empty());
}

/// 1 MiB가 넘는 stdin도 끝까지 읽어 버린다(호스트가 EPIPE를 받지 않는다).
#[test]
fn oversized_stdin_is_drained_without_epipe() {
    let d = tempfile::tempdir().unwrap();
    let url = format!("http://127.0.0.1:{}", free_port());
    let big = format!(
        r#"{{"session_id":"big","cwd":"/w","hook_event_name":"Stop","last_assistant_message":"끝","tool_response":"{}"}}"#,
        "y".repeat(3 * 1024 * 1024)
    );
    let start = Instant::now();
    let mut child = hook_cmd(d.path(), &url).arg("claude-code").spawn().unwrap();
    let mut si = child.stdin.take().unwrap();
    let wrote = si.write_all(big.as_bytes());
    drop(si);
    let out = child.wait_with_output().unwrap();
    assert!(wrote.is_ok(), "호스트 쪽 쓰기가 실패함: {wrote:?}");
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(start.elapsed() < Duration::from_secs(3));
    // 잘린 입력에서도 쓰는 필드는 건진다
    let files = spool_files(d.path());
    assert_eq!(files.len(), 1, "{files:?}");
    let v: Value = serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
    assert_eq!(v["payload"]["hook_event_name"], "Stop");
    assert_eq!(v["payload"]["session_id"], "big");
    assert!(v["payload"].get("tool_response").is_none());
}

fn get_state(port: u16, token: &str) -> Value {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        s,
        "GET /v1/state HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("");
    serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}: {text}"))
}

#[test]
fn with_daemon_it_delivers_and_drains_spool() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().to_path_buf();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let mut cfg = gyeot_server::ServerConfig::load(home.clone()).unwrap();
    cfg.port = 0;
    cfg.face_dir = None;
    let server = rt.block_on(gyeot_server::bind(cfg)).unwrap();
    let port = server.local_addr().port();
    let token = server.token().to_owned();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let handle = rt.spawn(server.run(async move {
        let _ = stop_rx.await;
    }));
    let url = format!("http://127.0.0.1:{port}");

    // 데몬이 뜬 뒤에 남은 스풀(전에 못 보낸 것): 심이 성공한 뒤 이어서 보낸다
    let spooled = json!({
        "source": "codex-notify",
        "payload": {"type": "agent-turn-complete", "thread-id": "old-thread", "last-assistant-message": "끝"},
        "pid_hint": 1,
        "received_ms": 1_764_126_720_000_i64
    });
    std::fs::write(
        home.join("spool").join("1764126720000-1-0.json"),
        spooled.to_string(),
    )
    .unwrap();

    let (out, took) = run_hook(&home, &url, &["claude-code"], &notification("live"));
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(took < Duration::from_secs(3), "took {took:?}");
    assert!(spool_files(&home).is_empty(), "{:?}", spool_files(&home));

    let st = get_state(port, &token);
    let sessions: Vec<String> = st["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["session"].as_str().unwrap().to_owned())
        .collect();
    assert!(sessions.contains(&"live".to_owned()), "{st}");
    assert!(sessions.contains(&"old-thread".to_owned()), "{st}");
    let live = st["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["session"] == "live")
        .unwrap()
        .clone();
    assert_eq!(live["kind"], "approval");

    // codex-notify: 마지막 인자 JSON
    let arg = json!({"type": "approval-requested", "thread-id": "cx", "command": ["git", "push"]})
        .to_string();
    let (out, _) = run_hook(&home, &url, &["codex-notify", &arg], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    let st = get_state(port, &token);
    let cx = st["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["session"] == "cx")
        .expect("codex item")
        .clone();
    assert_eq!(cx["kind"], "approval");
    assert_eq!(cx["body"], "git push");
    assert_eq!(cx["agent"], "codex");

    // 잘못된 토큰이면 스풀에 남긴다(데몬이 다음에 비운다)
    std::fs::write(home.join("token"), "c".repeat(64)).unwrap();
    let (out, _) = run_hook(&home, &url, &["claude-code"], &notification("later"));
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(spool_files(&home).len(), 1);

    let _ = stop_tx.send(());
    rt.block_on(handle).unwrap().unwrap();
}
