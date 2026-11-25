//! 곁 훅 심(`docs/contracts.md` §9).
//!
//! 지키는 것: stdout에 아무것도 쓰지 않는다. 언제나 exit 0. 패닉하지 않는다(해도 삼킨다).
//! 데몬과 주고받는 시간은 모두 합쳐 400ms 안이다.

use gyeot_core::Source;
use gyeot_core::paths::{HomeEnv, resolve_home, spool_file_name};
use gyeot_core::sanitize_payload;
use serde_json::{Map, Value, json};
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

mod ppid;

/// 읽는 입력의 최대 크기.
const INPUT_MAX: usize = 1024 * 1024;
/// 데몬과 주고받는 데 쓰는 전체 시간.
const NET_BUDGET: Duration = Duration::from_millis(400);
/// 연결 단계에 쓰는 최대 시간. 살아 있는 루프백 연결은 1ms 안에 끝난다. Windows는 닫힌 루프백 포트로의
/// 연결이 곧바로 실패하지 않아서(재시도) 이것이 없으면 데몬이 없을 때 [`NET_BUDGET`]을 다 쓴다.
const CONNECT_BUDGET: Duration = Duration::from_millis(100);
/// stdin이 닫히지 않을 때 기다리는 최대 시간(호스트가 잘못됐을 때만 걸린다).
const STDIN_DEADLINE: Duration = Duration::from_millis(1000);
/// 성공한 뒤 이어서 보내는 스풀 파일 수.
const DRAIN_MAX: usize = 50;
/// 스풀에 쌓아 두는 최대 파일 수(데몬이 오래 없을 때 디스크를 지킨다).
const SPOOL_CAP: usize = 10_000;
const DEFAULT_URL: &str = "http://127.0.0.1:47321";

fn main() {
    // 패닉 메시지도 내지 않는다.
    std::panic::set_hook(Box::new(|_| {}));
    let _ = std::panic::catch_unwind(|| run(std::env::args_os().skip(1).collect()));
    std::process::exit(0);
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// source 이름은 URL 경로에 들어가므로 `[a-z0-9-]{1,32}`만 받는다.
fn valid_source(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn run(args: Vec<OsString>) {
    let started = Instant::now();
    let Some(source) = args.first().and_then(|a| a.to_str()).map(str::to_owned) else {
        return;
    };
    if !valid_source(&source) {
        return;
    }
    // 데몬이 받지 않는 source는 보낼 것도 스풀에 둘 것도 없다.
    let Some(src) = Source::parse(&source).filter(|s| s.is_ingestible()) else {
        return;
    };
    // 넘친 stdin은 이 값이 사라질 때(함수 끝) 마감 시각까지 끝까지 읽어 버린다.
    let _stdin;
    let raw = if src == Source::CodexNotify {
        match args.get(1..).and_then(<[OsString]>::last) {
            Some(a) => payload_from_arg(&a.to_string_lossy()),
            None => return,
        }
    } else {
        let input = read_stdin(INPUT_MAX, started + STDIN_DEADLINE);
        let parsed = match &input.head {
            Some((bytes, truncated)) if !bytes.is_empty() => payload_from_bytes(bytes, *truncated),
            _ => None,
        };
        _stdin = input;
        match parsed {
            Some(p) => p,
            None => return,
        }
    };
    // 보내기 전·스풀에 쓰기 전에 줄인다(계약 §12.5).
    let payload = sanitize_payload(src, &raw);
    let envelope = json!({
        "payload": payload,
        "pid_hint": ppid::agent_pid(),
        "received_ms": now_ms(),
    });
    let Some(home) = home_dir() else {
        return;
    };
    let ctx = Ctx {
        base: daemon_url(std::env::var("GYEOT_URL").ok().as_deref()),
        token: read_token(&home.join("token")),
        spool: home.join("spool"),
        deadline: Instant::now() + NET_BUDGET,
    };
    deliver(&ctx, &source, &envelope);
}

fn home_dir() -> Option<PathBuf> {
    resolve_home(&HomeEnv {
        gyeot_home: std::env::var_os("GYEOT_HOME").map(PathBuf::from),
        windows: cfg!(windows),
        local_app_data: dirs::data_local_dir(),
        xdg_state_home: std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        home_dir: dirs::home_dir(),
    })
}

/// `GYEOT_URL`: `http://127.0.0.1`·`http://localhost`만 받는다(토큰을 보내므로). 아니면 `None`.
fn daemon_url(env: Option<&str>) -> Option<String> {
    let raw = env
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_URL);
    let url = raw.trim_end_matches('/');
    let rest = url.strip_prefix("http://")?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let (name, port) = match host.rsplit_once(':') {
        Some((n, p)) => (n, Some(p)),
        None => (host, None),
    };
    let name_ok = name == "127.0.0.1" || name.eq_ignore_ascii_case("localhost");
    let port_ok = port.is_none_or(|p| p.parse::<u16>().is_ok_and(|p| p != 0));
    let no_path = rest.len() == host.len();
    (name_ok && port_ok && no_path).then(|| url.to_owned())
}

fn read_token(path: &Path) -> Option<String> {
    let s = fs::read_to_string(path).ok()?;
    let t = s.trim();
    (t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit())).then(|| t.to_owned())
}

/// 읽은 stdin. 사라질 때 나머지를 다 읽어 버릴 때까지(또는 마감 시각까지) 기다린다.
struct StdinInput {
    /// (앞 `max`바이트, 넘쳤는가). 마감 시각 안에 읽지 못했으면 `None`.
    head: Option<(Vec<u8>, bool)>,
    drained: Option<mpsc::Receiver<()>>,
    deadline: Instant,
}

impl Drop for StdinInput {
    fn drop(&mut self) {
        if let Some(rx) = self.drained.take() {
            let left = self.deadline.saturating_duration_since(Instant::now());
            let _ = rx.recv_timeout(left);
        }
    }
}

/// stdin을 `max`바이트까지 읽는다. 넘치면 나머지는 EOF까지 읽어 버린다(호스트가 EPIPE를 받지 않게).
fn read_stdin(max: usize, deadline: Instant) -> StdinInput {
    let (tx, rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("stdin".into())
        .spawn(move || {
            let stdin = std::io::stdin();
            let mut lock = stdin.lock();
            let mut buf = Vec::new();
            let r = lock.by_ref().take(max as u64 + 1).read_to_end(&mut buf);
            let truncated = buf.len() > max;
            buf.truncate(max);
            let _ = tx.send(r.ok().map(|_| (buf, truncated)));
            if truncated {
                let _ = std::io::copy(&mut lock, &mut std::io::sink());
            }
            let _ = done_tx.send(());
        });
    if spawned.is_err() {
        return StdinInput {
            head: None,
            drained: None,
            deadline,
        };
    }
    let left = deadline.saturating_duration_since(Instant::now());
    StdinInput {
        head: rx.recv_timeout(left).ok().flatten(),
        drained: Some(done_rx),
        deadline,
    }
}

fn payload_from_arg(s: &str) -> Value {
    if s.len() > INPUT_MAX {
        return Value::Null;
    }
    serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_owned()))
}

/// stdin 바이트 → 원본 페이로드. 잘렸으면 최상위 문자열 필드를 건진다(줄이기는 그 뒤 [`sanitize_payload`]).
fn payload_from_bytes(bytes: &[u8], truncated: bool) -> Option<Value> {
    if !truncated && let Ok(v) = serde_json::from_slice::<Value>(bytes) {
        return Some(v);
    }
    salvage(bytes)
}

/// 잘린 입력에서 건지는 최상위 문자열 필드(정규화가 읽는 것).
const KEEP: [&str; 28] = [
    "session_id",
    "cwd",
    "hook_event_name",
    "permission_mode",
    "notification_type",
    "message",
    "title",
    "tool_name",
    "source",
    "reason",
    "model",
    "session_title",
    "last_assistant_message",
    "error_type",
    "error",
    "task_subject",
    "subject",
    "trigger",
    "matcher",
    "agent_type",
    "type",
    "thread-id",
    "turn-id",
    "last-assistant-message",
    "prompt",
    "tool_input",
    "command",
    "result",
];
/// 잘린 JSON에서 최상위 문자열 필드를 건진다. `hook_event_name`이나 `type`이 없으면 버린다.
fn salvage(bytes: &[u8]) -> Option<Value> {
    let text = String::from_utf8_lossy(bytes);
    let mut m = Map::new();
    for k in KEEP {
        if let Some(v) = find_string_field(&text, k) {
            m.insert(k.to_owned(), Value::String(v));
        }
    }
    (m.contains_key("hook_event_name") || m.contains_key("type")).then_some(Value::Object(m))
}

/// `"key": "value"`의 값을 찾는다(처음 나온 것, JSON 이스케이프 해석).
fn find_string_field(text: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let mut from = 0;
    while let Some(i) = text[from..].find(&pat) {
        let after = from + i + pat.len();
        let rest = text[after..].trim_start();
        if let Some(rest) = rest.strip_prefix(':') {
            let rest = rest.trim_start();
            if let Some(body) = rest.strip_prefix('"') {
                // 닫는 따옴표까지 잘라 JSON 문자열로 해석한다.
                let mut esc = false;
                for (j, c) in body.char_indices() {
                    if esc {
                        esc = false;
                    } else if c == '\\' {
                        esc = true;
                    } else if c == '"' {
                        let lit = format!("\"{}\"", &body[..j]);
                        return serde_json::from_str(&lit).ok();
                    }
                }
                return None;
            }
        }
        from = after;
    }
    None
}

struct Ctx {
    base: Option<String>,
    token: Option<String>,
    spool: PathBuf,
    deadline: Instant,
}

impl Ctx {
    fn remaining(&self) -> Option<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
    }
}

/// 남은 시간 `left` 안에서만 기다리는 HTTP 손잡이. 연결 단계는 [`CONNECT_BUDGET`]까지만.
fn agent_for(left: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(left))
        .timeout_connect(Some(left.min(CONNECT_BUDGET)))
        .proxy(None)
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into()
}

fn post(ctx: &Ctx, source: &str, body: &[u8]) -> bool {
    let (Some(base), Some(token), Some(left)) = (&ctx.base, &ctx.token, ctx.remaining()) else {
        return false;
    };
    let agent = agent_for(left);
    let url = format!("{base}/v1/ingest/{source}");
    match agent
        .post(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .send(body)
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

fn deliver(ctx: &Ctx, source: &str, envelope: &Value) {
    let Ok(body) = serde_json::to_vec(envelope) else {
        return;
    };
    if post(ctx, source, &body) {
        drain(ctx);
    } else {
        spool(&ctx.spool, source, envelope);
    }
}

/// 스풀 폴더를 만들고 사용자 전용으로 좁힌다(Unix 0700. Windows는 `%LOCALAPPDATA%` 권한을 이어받는다).
fn ensure_private_dir(dir: &Path) -> Option<()> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        b.mode(0o700);
        b.create(dir).ok()?;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    b.create(dir).ok()?;
    Some(())
}

/// 스풀 파일로 쓴다(임시 파일 → 이름 바꾸기, Unix 0600). 스풀이 가득 찼으면 버린다.
fn spool(dir: &Path, source: &str, envelope: &Value) -> Option<PathBuf> {
    ensure_private_dir(dir)?;
    if fs::read_dir(dir).ok()?.take(SPOOL_CAP).count() >= SPOOL_CAP {
        return None;
    }
    let mut entry = envelope.as_object().cloned().unwrap_or_default();
    entry.insert("source".into(), Value::String(source.to_owned()));
    let bytes = serde_json::to_vec(&Value::Object(entry)).ok()?;
    let ts = now_ms();
    let pid = std::process::id();
    for n in 0..1000u32 {
        let name = spool_file_name(ts, pid, n);
        let path = dir.join(&name);
        if path.exists() {
            continue;
        }
        let tmp = dir.join(format!("{name}.tmp"));
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).ok()?;
        std::io::Write::write_all(&mut f, &bytes).ok()?;
        drop(f);
        if fs::rename(&tmp, &path).is_ok() {
            return Some(path);
        }
        let _ = fs::remove_file(&tmp);
        return None;
    }
    None
}

/// 스풀에 남은 것을 이름 순으로 최대 50개 보낸다. 실패하면 멈춘다.
fn drain(ctx: &Ctx) {
    let Ok(rd) = fs::read_dir(&ctx.spool) else {
        return;
    };
    let mut files: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    for path in files.into_iter().take(DRAIN_MAX) {
        if ctx.remaining().is_none() {
            return;
        }
        let claimed = PathBuf::from(format!("{}.claim-{}", path.display(), std::process::id()));
        if fs::rename(&path, &claimed).is_err() {
            continue; // 데몬이나 다른 심이 먼저 집었다.
        }
        let parsed = fs::read(&claimed)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let Some(Value::Object(mut entry)) = parsed else {
            let _ = fs::remove_file(&claimed);
            continue;
        };
        let source = entry
            .remove("source")
            .and_then(|s| s.as_str().map(str::to_owned))
            .filter(|s| valid_source(s));
        let Some(source) = source else {
            let _ = fs::remove_file(&claimed);
            continue;
        };
        let body = serde_json::to_vec(&Value::Object(entry)).unwrap_or_default();
        if post(ctx, &source, &body) {
            let _ = fs::remove_file(&claimed);
        } else {
            let _ = fs::rename(&claimed, &path);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources() {
        assert!(valid_source("claude-code"));
        assert!(valid_source("codex-notify"));
        assert!(!valid_source(""));
        assert!(!valid_source("../x"));
        assert!(!valid_source("Claude"));
        assert!(!valid_source(&"a".repeat(33)));
    }

    #[test]
    fn urls() {
        assert_eq!(daemon_url(None).as_deref(), Some("http://127.0.0.1:47321"));
        assert_eq!(
            daemon_url(Some("")).as_deref(),
            Some("http://127.0.0.1:47321")
        );
        assert_eq!(
            daemon_url(Some("http://localhost:5000/")).as_deref(),
            Some("http://localhost:5000")
        );
        assert_eq!(
            daemon_url(Some("http://127.0.0.1")).as_deref(),
            Some("http://127.0.0.1")
        );
        for bad in [
            "https://127.0.0.1:1",
            "http://evil.com:47321",
            "http://127.0.0.1.evil.com",
            "http://127.0.0.1:0",
            "http://127.0.0.1:99999",
            "http://127.0.0.1:1/x",
            "ftp://127.0.0.1",
        ] {
            assert_eq!(daemon_url(Some(bad)), None, "{bad}");
        }
    }

    #[test]
    fn tokens() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("token");
        assert_eq!(read_token(&p), None);
        fs::write(&p, format!("{}\n", "a".repeat(64))).unwrap();
        assert_eq!(read_token(&p), Some("a".repeat(64)));
        fs::write(&p, "short").unwrap();
        assert_eq!(read_token(&p), None);
    }

    #[test]
    fn arg_payloads() {
        assert_eq!(payload_from_arg(r#"{"type":"x"}"#), json!({"type": "x"}));
        assert_eq!(payload_from_arg("not json"), json!("not json"));
    }

    #[test]
    fn small_payload_passes_through() {
        let v = json!({"hook_event_name": "Stop", "session_id": "s", "x": [1, 2]});
        let b = serde_json::to_vec(&v).unwrap();
        assert_eq!(payload_from_bytes(&b, false), Some(v));
        assert_eq!(payload_from_bytes(b"not json", false), None);
    }

    #[test]
    fn big_payload_is_sanitized() {
        let big = "x".repeat(INPUT_MAX);
        let v = json!({
            "hook_event_name": "PostToolUse",
            "session_id": "s1",
            "cwd": "/w",
            "tool_name": "Read",
            "tool_input": {"file_path": "/w/a.rs", "content": big.clone(), "n": 3, "arr": [1]},
            "tool_response": {"content": big.clone()},
        });
        let b = serde_json::to_vec(&v).unwrap();
        let raw = payload_from_bytes(&b, false).unwrap();
        let p = sanitize_payload(Source::ClaudeCode, &raw);
        assert_eq!(
            p,
            json!({"hook_event_name": "PostToolUse", "session_id": "s1", "cwd": "/w", "tool_name": "Read"})
        );
        let v = json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "s1",
            "tool_name": "Write",
            "tool_input": {"file_path": "/w/a.rs", "content": big},
        });
        let raw = payload_from_bytes(&serde_json::to_vec(&v).unwrap(), false).unwrap();
        let p = sanitize_payload(Source::ClaudeCode, &raw);
        assert_eq!(p["tool_input"], json!({"file_path": "/w/a.rs"}));
        assert!(serde_json::to_vec(&p).unwrap().len() < 1000);
    }

    #[test]
    fn truncated_payload_is_salvaged() {
        let mut s = String::from(
            r#"{"session_id":"abc","transcript_path":"/t","cwd":"C:\\work\\proj","hook_event_name":"Stop","last_assistant_message":"끝\n할까요?","tool_response":""#,
        );
        s.push_str(&"y".repeat(5000));
        let p = payload_from_bytes(s.as_bytes(), true).unwrap();
        assert_eq!(p["session_id"], "abc");
        assert_eq!(p["cwd"], "C:\\work\\proj");
        assert_eq!(p["hook_event_name"], "Stop");
        assert_eq!(p["last_assistant_message"], "끝\n할까요?");
        let clean = sanitize_payload(Source::ClaudeCode, &p);
        assert!(clean.get("transcript_path").is_none());
        assert_eq!(clean["last_assistant_message"], "끝\n할까요?");
        // 이름이 없으면 버린다
        assert_eq!(payload_from_bytes(br#"{"session_id":"abc"#, true), None);
    }

    /// 연결 단계는 100ms(남은 시간이 더 짧으면 그만큼)까지만 기다린다.
    #[test]
    fn connect_phase_has_its_own_budget() {
        assert_eq!(CONNECT_BUDGET, Duration::from_millis(100));
        let t = agent_for(NET_BUDGET).config().timeouts();
        assert_eq!(t.connect, Some(CONNECT_BUDGET));
        assert_eq!(t.global, Some(NET_BUDGET));
        let t = agent_for(Duration::from_millis(30)).config().timeouts();
        assert_eq!(t.connect, Some(Duration::from_millis(30)));
        assert_eq!(t.global, Some(Duration::from_millis(30)));
    }

    #[test]
    fn find_fields() {
        let t = r#"{"a":"x\"y","b" : "z","c":1}"#;
        assert_eq!(find_string_field(t, "a").as_deref(), Some("x\"y"));
        assert_eq!(find_string_field(t, "b").as_deref(), Some("z"));
        assert_eq!(find_string_field(t, "c"), None);
        assert_eq!(find_string_field(t, "d"), None);
    }

    #[test]
    fn spool_files_are_named_and_complete() {
        let d = tempfile::tempdir().unwrap();
        let env = json!({"payload": {"a": 1}, "pid_hint": 3, "received_ms": 5});
        let p1 = spool(d.path(), "claude-code", &env).unwrap();
        let p2 = spool(d.path(), "codex", &env).unwrap();
        assert_ne!(p1, p2);
        let name = p1.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.ends_with(".json"));
        assert_eq!(name.split('-').count(), 3);
        let v: Value = serde_json::from_slice(&fs::read(&p1).unwrap()).unwrap();
        assert_eq!(v["source"], "claude-code");
        assert_eq!(v["payload"], json!({"a": 1}));
        let left: Vec<_> = fs::read_dir(d.path()).unwrap().collect();
        assert_eq!(left.len(), 2, "임시 파일이 남지 않는다");
    }

    #[test]
    fn deliver_without_daemon_spools() {
        let d = tempfile::tempdir().unwrap();
        let ctx = Ctx {
            base: Some("http://127.0.0.1:9".into()),
            token: None,
            spool: d.path().join("spool"),
            deadline: Instant::now() + NET_BUDGET,
        };
        deliver(
            &ctx,
            "claude-code",
            &json!({"payload": {}, "pid_hint": null, "received_ms": 1}),
        );
        assert_eq!(fs::read_dir(d.path().join("spool")).unwrap().count(), 1);
    }

    #[test]
    fn parent_pid_is_known() {
        let p = ppid::agent_pid();
        assert!(p.is_some());
        assert_ne!(p, Some(std::process::id()));
    }
}
