//! 만든 `gyeot-hook` 실행 파일을 띄워 본다: 데몬 없음(스풀), 가짜 데몬(데몬 확인 §13.1), 진짜 데몬(전달·스풀 비우기).

use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ───────────────────── 가짜 데몬(계약 §13.1) ─────────────────────

/// 가짜 데몬이 `GET /v1/hello`에 어떻게 답하나.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hello {
    /// 같은 토큰·자기 포트로 계산한 proof(곁 데몬).
    Good,
    /// 다른 토큰으로 계산한 proof(다른 사용자의 데몬).
    WrongToken,
    /// 다른 포트로 계산한 proof(다른 포트의 진짜 데몬으로 중계하는 서버).
    OtherPort,
    /// `404`(곁이 아닌 프로그램, 또는 확인을 모르는 옛 서버).
    NotFound,
    /// `200`이지만 JSON이 아님.
    Garbage,
    /// 연결만 받고 답하지 않음.
    Stall,
}

/// 가짜 데몬이 받은 요청 하나.
#[derive(Debug, Clone)]
struct Seen {
    /// 요청줄(`GET /v1/hello?n=… HTTP/1.1`).
    line: String,
    /// `Authorization` 머리가 있었나.
    auth: bool,
    body: String,
    /// 몇 번째로 받은 연결인가(같은 연결을 다시 썼는지 본다).
    conn: usize,
}

struct FakeDaemon {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl FakeDaemon {
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

/// 계약 §13.1의 proof(시험 쪽에서 따로 계산한다).
fn hello_proof(token: &str, port: u16, n: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes()).unwrap();
    mac.update(format!("gyeot-hello/v1|{port}|{n}").as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 요청 하나를 읽는다: (요청줄, 머리 줄들, 본문).
fn read_request(s: &mut TcpStream) -> Option<(String, Vec<String>, Vec<u8>)> {
    s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 65536];
    loop {
        let n = s.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&buf[..at]).into_owned();
        let mut lines = head.split("\r\n").map(str::to_owned);
        let line = lines.next().unwrap_or_default();
        let headers: Vec<String> = lines.collect();
        let len = headers
            .iter()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse::<usize>().ok())?
            })
            .unwrap_or(0);
        while buf.len() < at + 4 + len {
            let n = s.read(&mut tmp).ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&tmp[..n]);
        }
        return Some((line, headers, buf[at + 4..at + 4 + len].to_vec()));
    }
}

/// 답한다. 연결은 닫지 않는다(keep-alive, 진짜 데몬과 같이).
fn respond(s: &mut TcpStream, status: &str, body: &str) {
    let _ = write!(
        s,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
}

/// 진짜 `TcpListener`로 만든 작은 가짜 데몬. 연결마다 실을 하나 띄우고, 한 연결에서 요청을 여러 개 받는다
/// (keep-alive). 시험이 끝나면 프로세스와 함께 사라진다.
fn fake_daemon(token: &str, mode: Hello) -> FakeDaemon {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (token, log) = (token.to_owned(), seen.clone());
    std::thread::spawn(move || {
        for (conn, s) in l.incoming().enumerate() {
            let Ok(mut s) = s else { continue };
            let (token, log) = (token.clone(), log.clone());
            std::thread::spawn(move || {
                while let Some((line, headers, body)) = read_request(&mut s) {
                    let auth = headers.iter().any(|h| {
                        h.split_once(':')
                            .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case("authorization"))
                    });
                    log.lock().unwrap().push(Seen {
                        line: line.clone(),
                        auth,
                        body: String::from_utf8_lossy(&body).into_owned(),
                        conn,
                    });
                    if let Some(rest) = line.strip_prefix("GET /v1/hello?n=") {
                        let n = rest.split(' ').next().unwrap_or("");
                        let ok = |p: String| json!({"v": 1, "proof": p}).to_string();
                        match mode {
                            Hello::Good => {
                                respond(&mut s, "200 OK", &ok(hello_proof(&token, port, n)))
                            }
                            Hello::WrongToken => respond(
                                &mut s,
                                "200 OK",
                                &ok(hello_proof(&"f".repeat(64), port, n)),
                            ),
                            Hello::OtherPort => respond(
                                &mut s,
                                "200 OK",
                                &ok(hello_proof(&token, port.wrapping_add(1), n)),
                            ),
                            Hello::NotFound => respond(&mut s, "404 Not Found", "{}"),
                            Hello::Garbage => respond(&mut s, "200 OK", "hello"),
                            Hello::Stall => {
                                std::thread::sleep(Duration::from_secs(3));
                                return;
                            }
                        }
                    } else if line.starts_with("POST /v1/ingest/") {
                        respond(&mut s, "202 Accepted", r#"{"accepted":1}"#);
                    } else {
                        respond(&mut s, "404 Not Found", "{}");
                    }
                }
            });
        }
    });
    FakeDaemon { port, seen }
}

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
    let token = "a".repeat(64);
    std::fs::write(d.path().join("token"), &token).unwrap();
    let fake = fake_daemon(&token, Hello::Good);
    let input = json!({
        "session_id": "s1",
        "transcript_path": "/t.jsonl",
        "cwd": "/work/proj",
        "hook_event_name": "Stop",
        "last_assistant_message": format!("끝났어요. 키는 {SECRET} 입니다."),
        "stop_hook_active": false,
    });
    let (out, _) = run_hook(
        d.path(),
        &fake.url(),
        &["claude-code"],
        input.to_string().as_bytes(),
    );
    assert_eq!(out.status.code(), Some(0));
    let seen = fake.seen();
    let ingest: Vec<&Seen> = seen
        .iter()
        .filter(|r| r.line.starts_with("POST /v1/ingest/claude-code "))
        .collect();
    assert_eq!(ingest.len(), 1, "{seen:?}");
    let body = &ingest[0].body;
    let v: Value = serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}: {body}"));
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

/// R3 M2(계약 §13.1, G-032): 그 포트의 서버가 같은 토큰을 안다고 증명하지 못하면 토큰도 페이로드도
/// 보내지 않고 스풀에 남긴다(다른 사용자의 서버·다른 포트로 중계하는 서버·곁이 아닌 프로그램).
#[test]
fn unproven_server_gets_no_token_and_payload_is_spooled() {
    let token = "a".repeat(64);
    for mode in [
        Hello::WrongToken,
        Hello::OtherPort,
        Hello::NotFound,
        Hello::Garbage,
    ] {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("token"), &token).unwrap();
        let fake = fake_daemon(&token, mode);
        let (out, took) = run_hook(d.path(), &fake.url(), &["claude-code"], &notification("s1"));
        assert_eq!(out.status.code(), Some(0), "{mode:?}");
        assert!(out.stdout.is_empty(), "{mode:?}");
        assert!(took < Duration::from_secs(3), "{mode:?} took {took:?}");
        let seen = fake.seen();
        assert!(
            seen.iter().any(|r| r.line.starts_with("GET /v1/hello?n=")),
            "{mode:?}: 확인 요청이 없다 {seen:?}"
        );
        assert!(
            seen.iter().all(|r| !r.auth),
            "{mode:?}: Authorization 머리가 나갔다 {seen:?}"
        );
        assert!(
            seen.iter().all(|r| !r.line.starts_with("POST")),
            "{mode:?}: ingest가 나갔다 {seen:?}"
        );
        let files = spool_files(d.path());
        assert_eq!(files.len(), 1, "{mode:?}: {files:?}");
        let v: Value = serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
        assert_eq!(v["payload"]["session_id"], "s1", "{mode:?}");
        assert_eq!(v["source"], "claude-code", "{mode:?}");
    }
}

/// 맞는 proof를 주면 보낸다. 확인 요청에는 인증 머리가 없고, n은 32바이트 난수(hex 64자)다.
/// 성공한 뒤에는 남은 스풀도 이어서 보낸다.
#[test]
fn proven_server_gets_payload_and_spool_is_drained() {
    let d = tempfile::tempdir().unwrap();
    let token = "a".repeat(64);
    std::fs::write(d.path().join("token"), &token).unwrap();
    std::fs::create_dir_all(d.path().join("spool")).unwrap();
    let old = json!({"source": "codex-notify", "payload": {"type": "agent-turn-complete", "thread-id": "old"}, "pid_hint": 1, "received_ms": 1});
    std::fs::write(d.path().join("spool").join("1-1-0.json"), old.to_string()).unwrap();
    let fake = fake_daemon(&token, Hello::Good);
    let (out, took) = run_hook(d.path(), &fake.url(), &["claude-code"], &notification("s1"));
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(took < Duration::from_secs(3), "took {took:?}");
    let seen = fake.seen();
    let hellos: Vec<&Seen> = seen
        .iter()
        .filter(|r| r.line.starts_with("GET /v1/hello?n="))
        .collect();
    assert_eq!(hellos.len(), 1, "프로세스마다 한 번 {seen:?}");
    assert!(!hellos[0].auth, "확인 요청에는 토큰을 싣지 않는다");
    let n = hellos[0].line["GET /v1/hello?n=".len()..]
        .split(' ')
        .next()
        .unwrap();
    assert_eq!(n.len(), 64, "{n}");
    assert!(
        n.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{n}"
    );
    let posts: Vec<&Seen> = seen.iter().filter(|r| r.line.starts_with("POST")).collect();
    assert_eq!(posts.len(), 2, "{seen:?}");
    assert!(posts.iter().all(|r| r.auth));
    assert!(posts[0].line.starts_with("POST /v1/ingest/claude-code "));
    assert!(posts[1].line.starts_with("POST /v1/ingest/codex-notify "));
    // R5 L3: 확인한 연결로 보낸다(확인·전송·스풀 비우기가 한 연결)
    assert!(
        posts.iter().all(|r| r.conn == hellos[0].conn),
        "다른 연결로 보냈다: {seen:?}"
    );
    assert!(
        spool_files(d.path()).is_empty(),
        "{:?}",
        spool_files(d.path())
    );
    // 다음 프로세스는 다시 확인한다(n도 새로)
    run_hook(d.path(), &fake.url(), &["claude-code"], &notification("s2"));
    let hellos: Vec<String> = fake
        .seen()
        .iter()
        .filter(|r| r.line.starts_with("GET /v1/hello?n="))
        .map(|r| r.line.clone())
        .collect();
    assert_eq!(hellos.len(), 2);
    assert_ne!(hellos[0], hellos[1]);
}

/// R3 회귀 N4(계약 §14.2): 1 MiB가 넘는 `PostToolBatch`. 심은 최상위 필드만 건지고(중첩된
/// `tool_calls[0].tool_name`을 최상위로 잡지 않는다) `gyeot_partial`을 붙인다. 잘린 뒤쪽의 `agent_id`는 잃는다.
/// 보내든 스풀에 두든(큰 입력을 읽느라 통신 시간이 모자랄 수 있다) 줄인 페이로드는 같다.
#[test]
fn oversized_batch_is_salvaged_from_top_level_only() {
    let d = tempfile::tempdir().unwrap();
    let token = "a".repeat(64);
    std::fs::write(d.path().join("token"), &token).unwrap();
    let fake = fake_daemon(&token, Hello::Good);
    let big = "y".repeat(1_200_000);
    let input = format!(
        r#"{{"session_id":"big","cwd":"/w","hook_event_name":"PostToolBatch","tool_calls":[{{"tool_name":"Bash","tool_use_id":"t1","tool_input":{{"command":"make"}},"tool_response":"{big}"}}],"agent_id":"a9"}}"#
    );
    let (out, took) = run_hook(d.path(), &fake.url(), &["claude-code"], input.as_bytes());
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(took < Duration::from_secs(3), "took {took:?}");
    let sent: Vec<Value> = fake
        .seen()
        .iter()
        .filter(|r| r.line.starts_with("POST /v1/ingest/claude-code "))
        .map(|r| serde_json::from_str::<Value>(&r.body).unwrap()["payload"].clone())
        .collect();
    let spooled: Vec<Value> = spool_files(d.path())
        .iter()
        .map(|f| {
            serde_json::from_slice::<Value>(&std::fs::read(f).unwrap()).unwrap()["payload"].clone()
        })
        .collect();
    let all: Vec<Value> = sent.into_iter().chain(spooled).collect();
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(
        all[0],
        json!({"session_id": "big", "cwd": "/w", "hook_event_name": "PostToolBatch", "gyeot_partial": true})
    );
}

/// 확인에 답하지 않는 서버: 기존 400ms 통신 한도 안에서 끝내고 스풀에 남긴다(토큰은 나가지 않는다).
#[test]
fn stalled_hello_keeps_the_net_budget() {
    let d = tempfile::tempdir().unwrap();
    let token = "a".repeat(64);
    std::fs::write(d.path().join("token"), &token).unwrap();
    let fake = fake_daemon(&token, Hello::Stall);
    let (out, took) = run_hook(d.path(), &fake.url(), &["claude-code"], &notification("s1"));
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    // 400ms 통신 한도 + 프로세스 띄우기·stdin·스풀 쓰기
    assert!(took < Duration::from_millis(1200), "took {took:?}");
    assert!(
        took >= Duration::from_millis(350),
        "확인을 기다리긴 했다: {took:?}"
    );
    let seen = fake.seen();
    assert!(seen.iter().all(|r| !r.auth), "{seen:?}");
    assert_eq!(spool_files(d.path()).len(), 1);
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
