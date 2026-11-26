//! 곁 훅 심(`docs/contracts.md` §9).
//!
//! 지키는 것: stdout에 아무것도 쓰지 않는다. 언제나 exit 0. 패닉하지 않는다(해도 삼킨다).
//! 데몬과 주고받는 시간은 모두 합쳐 400ms 안이다(데몬 확인 포함).
//! 토큰과 페이로드는 그 포트의 서버가 같은 토큰을 안다고 증명한 뒤에만 보낸다(§13.1).

use gyeot_core::Source;
use gyeot_core::paths::{HomeEnv, resolve_home, spool_file_name};
use gyeot_core::sanitize_payload;
use hmac::{Hmac, Mac};
use serde_json::{Map, Value, json};
use sha2::Sha256;
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
/// 데몬 확인 응답(`{"v":1,"proof":"<64 hex>"}`)으로 읽는 최대 바이트.
const HELLO_BODY_MAX: u64 = 4096;

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
/// `localhost`는 `127.0.0.1`로 바꾼다. `::1`로 풀리면 그 주소의 같은 포트를 잡은 서버가 진짜 데몬에
/// hello를 중계해 확인을 통과할 수 있다(데몬은 127.0.0.1에만 묶인다, 계약 §13.1).
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
    (name_ok && port_ok && no_path).then(|| match port {
        Some(p) => format!("http://127.0.0.1:{p}"),
        None => "http://127.0.0.1".to_owned(),
    })
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
const KEEP: [&str; 29] = [
    "session_id",
    "cwd",
    "hook_event_name",
    "agent_id",
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
/// 건진 것에는 `"gyeot_partial": true`를 붙인다(정규화가 그 activity로 승인 요청을 지우지 않게, §14.2).
fn salvage(bytes: &[u8]) -> Option<Value> {
    let text = String::from_utf8_lossy(bytes);
    let mut m = top_level_strings(&text);
    if !(m.contains_key("hook_event_name") || m.contains_key("type")) {
        return None;
    }
    m.insert("gyeot_partial".into(), Value::Bool(true));
    Some(Value::Object(m))
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while b.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

/// `b[i]`가 여는 따옴표인 JSON 문자열을 읽는다: (해석한 값, 닫는 따옴표 다음 자리). 닫히지 않았으면 `None`.
fn read_string(text: &str, i: usize) -> Option<(String, usize)> {
    let b = text.as_bytes();
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => {
                // 따옴표는 ASCII라 UTF-8 글자 경계다.
                let v = serde_json::from_str::<String>(&text[i..=j]).ok()?;
                return Some((v, j + 1));
            }
            _ => j += 1,
        }
    }
    None
}

/// `b[i]`가 여는 `{`·`[`인 값을 건너뛴다(따옴표 안의 괄호는 세지 않는다). 닫히지 않았으면 `None`.
fn skip_nested(b: &[u8], i: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut j = i;
    let mut in_str = false;
    while j < b.len() {
        let c = b[j];
        if in_str {
            match c {
                b'\\' => j += 1,
                b'"' => in_str = false,
                _ => {}
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'{' | b'[' => depth += 1,
                b'}' | b']' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(j + 1);
                    }
                }
                _ => {}
            }
        }
        j += 1;
    }
    None
}

/// 최상위 객체의 문자열 필드 가운데 [`KEEP`]에 든 것(처음 나온 것). 중첩된 객체·배열은 깊이를 세어 건너뛰므로
/// `tool_calls[0].tool_name` 같은 안쪽 필드를 최상위로 잡지 않는다(R3 회귀 N4). 잘린 곳에서 멈춘다.
fn top_level_strings(text: &str) -> Map<String, Value> {
    let b = text.as_bytes();
    let mut out = Map::new();
    let mut i = skip_ws(b, 0);
    if b.get(i) != Some(&b'{') {
        return out;
    }
    i += 1;
    loop {
        i = skip_ws(b, i);
        match b.get(i) {
            Some(b',') => {
                i += 1;
                continue;
            }
            Some(b'"') => {}
            _ => return out,
        }
        let Some((key, j)) = read_string(text, i) else {
            return out;
        };
        i = skip_ws(b, j);
        if b.get(i) != Some(&b':') {
            return out;
        }
        i = skip_ws(b, i + 1);
        match b.get(i) {
            Some(b'"') => {
                let Some((v, j)) = read_string(text, i) else {
                    return out;
                };
                if KEEP.contains(&key.as_str()) && !out.contains_key(&key) {
                    out.insert(key, Value::String(v));
                }
                i = j;
            }
            Some(b'{' | b'[') => match skip_nested(b, i) {
                Some(j) => i = j,
                None => return out,
            },
            Some(_) => {
                // 숫자·true·false·null
                while b
                    .get(i)
                    .is_some_and(|c| !matches!(c, b',' | b'}') && !c.is_ascii_whitespace())
                {
                    i += 1;
                }
            }
            None => return out,
        }
    }
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

/// 남은 시간 `left`에서 (요청 전체, 연결 단계) 한도. 연결 단계는 [`CONNECT_BUDGET`]까지만.
fn budget(left: Duration) -> (Duration, Duration) {
    (left, left.min(CONNECT_BUDGET))
}

/// 프로세스에 하나 두는 HTTP 손잡이. 확인·전송·스풀 비우기가 같은 연결을 다시 쓴다(keep-alive, R5 L3).
/// 시간 한도는 요청마다 남은 시간으로 건다([`timed`]).
fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .proxy(None)
        .http_status_as_error(false)
        .max_redirects(0)
        .max_idle_connections(1)
        .max_idle_connections_per_host(1)
        .build()
        .into()
}

/// 요청에 남은 시간 `left` 안의 한도를 건다.
fn timed<B>(req: ureq::RequestBuilder<B>, left: Duration) -> ureq::RequestBuilder<B> {
    let (global, connect) = budget(left);
    req.config()
        .timeout_global(Some(global))
        .timeout_connect(Some(connect))
        .build()
}

/// 데몬 확인(§13.1)을 통과한 연결 대상. 토큰을 보내는 함수는 이것만 받는다(확인 없이 보낼 길이 없다).
struct Verified<'a> {
    ctx: &'a Ctx,
    base: &'a str,
    token: &'a str,
    /// 확인한 연결이 들어 있는 손잡이(같은 연결로 보낸다).
    agent: ureq::Agent,
}

/// URL(`http://<호스트>[:<포트>]`, [`daemon_url`]이 거른 꼴)의 포트. 없으면 80.
fn port_of(base: &str) -> Option<u16> {
    let host = base.strip_prefix("http://")?;
    match host.rsplit_once(':') {
        Some((_, p)) => p.parse().ok(),
        None => Some(80),
    }
}

/// 32바이트 난수의 소문자 hex 64자.
fn nonce() -> Option<String> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).ok()?;
    Some(b.iter().map(|x| format!("{x:02x}")).collect())
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// `proof == hex(HMAC-SHA256(key=토큰 UTF-8, msg="gyeot-hello/v1|<포트>|<n>"))`인가(상수 시간 비교).
fn proof_ok(token: &str, port: u16, n: &str, proof: &str) -> bool {
    let Some(tag) = decode_hex(proof).filter(|t| t.len() == 32) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(token.as_bytes()) else {
        return false;
    };
    mac.update(format!("gyeot-hello/v1|{port}|{n}").as_bytes());
    mac.verify_slice(&tag).is_ok()
}

/// 그 포트의 서버가 같은 토큰을 아는지 확인한다(`GET /v1/hello?n=`, 인증 머리 없음). 포트는 심이 연결한 포트다.
/// 다르거나 오류·시간 초과면 `None`(곁 데몬으로 보지 않는다). 남은 통신 시간 안에서만 기다린다.
fn hello(ctx: &Ctx) -> Option<Verified<'_>> {
    let (Some(base), Some(token)) = (ctx.base.as_deref(), ctx.token.as_deref()) else {
        return None;
    };
    let port = port_of(base)?;
    let n = nonce()?;
    let left = ctx.remaining()?;
    let agent = http_agent();
    let mut resp = timed(agent.get(&format!("{base}/v1/hello?n={n}")), left)
        .call()
        .ok()?;
    if resp.status() != 200 {
        return None;
    }
    let text = resp
        .body_mut()
        .with_config()
        .limit(HELLO_BODY_MAX)
        .read_to_string()
        .ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    if v.get("v").and_then(Value::as_u64) != Some(1) {
        return None;
    }
    let proof = v.get("proof").and_then(Value::as_str)?;
    proof_ok(token, port, &n, proof).then_some(Verified {
        ctx,
        base,
        token,
        agent,
    })
}

fn post(v: &Verified<'_>, source: &str, body: &[u8]) -> bool {
    let Some(left) = v.ctx.remaining() else {
        return false;
    };
    let url = format!("{}/v1/ingest/{source}", v.base);
    let req = v
        .agent
        .post(&url)
        .header("Authorization", &format!("Bearer {}", v.token))
        .header("Content-Type", "application/json");
    match timed(req, left).send(body) {
        Ok(mut resp) => {
            let ok = resp.status().is_success();
            // 본문을 끝까지 읽어야 연결이 손잡이로 돌아가 다음 요청이 같은 연결을 쓴다.
            let _ = resp
                .body_mut()
                .with_config()
                .limit(HELLO_BODY_MAX)
                .read_to_vec();
            ok
        }
        Err(_) => false,
    }
}

fn deliver(ctx: &Ctx, source: &str, envelope: &Value) {
    let Ok(body) = serde_json::to_vec(envelope) else {
        return;
    };
    // 확인과 전송을 합쳐 [`NET_BUDGET`] 안. 확인이 안 되면 토큰도 페이로드도 보내지 않고 스풀에만 둔다.
    match hello(ctx) {
        Some(v) if post(&v, source, &body) => drain(&v),
        _ => {
            spool(&ctx.spool, source, envelope);
        }
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

/// 스풀에 남은 것을 이름 순으로 최대 50개 보낸다(같은 확인 결과로). 실패하면 멈춘다.
fn drain(v: &Verified<'_>) {
    let ctx = v.ctx;
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
        if post(v, &source, &body) {
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
            Some("http://127.0.0.1:5000")
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
        // §15.3: PostToolUse는 tool_input의 미리보기 필드 하나만 남긴다(본문·응답은 버린다)
        assert_eq!(
            p,
            json!({"hook_event_name": "PostToolUse", "session_id": "s1", "cwd": "/w", "tool_name": "Read", "tool_input": {"file_path": "/w/a.rs"}})
        );
        assert!(serde_json::to_vec(&p).unwrap().len() < 1000);
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
            r#"{"session_id":"abc","transcript_path":"/t","cwd":"C:\\work\\proj","hook_event_name":"Stop","agent_id":"a1","last_assistant_message":"끝\n할까요?","tool_response":""#,
        );
        s.push_str(&"y".repeat(5000));
        let p = payload_from_bytes(s.as_bytes(), true).unwrap();
        assert_eq!(p["session_id"], "abc");
        assert_eq!(p["agent_id"], "a1");
        assert_eq!(p["cwd"], "C:\\work\\proj");
        assert_eq!(p["hook_event_name"], "Stop");
        assert_eq!(p["last_assistant_message"], "끝\n할까요?");
        let clean = sanitize_payload(Source::ClaudeCode, &p);
        assert!(clean.get("transcript_path").is_none());
        assert_eq!(clean["last_assistant_message"], "끝\n할까요?");
        // 이름이 없으면 버린다
        assert_eq!(payload_from_bytes(br#"{"session_id":"abc"#, true), None);
        // 건진 것에는 잘렸다는 표시를 붙인다(§14.2). 줄여도 남는다
        assert_eq!(p["gyeot_partial"], true);
        assert_eq!(clean["gyeot_partial"], true);
    }

    /// R3 회귀 N4(계약 §14.2): 건지기는 최상위 필드만 본다(중괄호·대괄호·따옴표 깊이를 센다).
    #[test]
    fn salvage_reads_top_level_fields_only() {
        // 중첩된 tool_calls[0].tool_name을 최상위처럼 잡지 않는다. agent_id가 잘린 뒤쪽에 있으면 없다
        let mut s = String::from(
            r#"{"session_id":"s1","cwd":"/w","hook_event_name":"PostToolBatch","tool_calls":[{"tool_name":"Bash","tool_input":{"command":"echo \"}{[\" x"},"tool_response":"#,
        );
        s.push_str(&"\"y".repeat(10));
        s.push_str(&"z".repeat(5000));
        let p = payload_from_bytes(s.as_bytes(), true).unwrap();
        assert_eq!(p["hook_event_name"], "PostToolBatch");
        assert_eq!(p["session_id"], "s1");
        assert!(p.get("tool_name").is_none(), "{p}");
        assert!(p.get("agent_id").is_none(), "{p}");
        assert_eq!(p["gyeot_partial"], true);
        // 최상위의 것은 중첩된 같은 이름보다 뒤에 있어도 잡는다
        let s = r#"{"tool_input":{"hook_event_name":"Fake","session_id":"inner","agent_id":"inner","x":[1,{"cwd":"in"}]},"session_id":"outer","agent_id":"a7","hook_event_name":"PermissionRequest","tool_name":"Bash","tool_response":"aaaa"#;
        let p = payload_from_bytes(s.as_bytes(), true).unwrap();
        assert_eq!(p["hook_event_name"], "PermissionRequest");
        assert_eq!(p["session_id"], "outer");
        assert_eq!(p["agent_id"], "a7");
        assert_eq!(p["tool_name"], "Bash");
        assert!(p.get("cwd").is_none(), "{p}");
        // 숫자·참거짓·null 값, 공백, 이스케이프를 건너뛴다
        let s = r#"{ "n" : 12.5e3 , "b":true,"z":null, "message" : "a\u0041\n\"q\"" ,"hook_event_name":"Notification","notification_type":"permission_prompt","tool_input":{"#;
        let p = payload_from_bytes(s.as_bytes(), true).unwrap();
        assert_eq!(p["message"], "aA\n\"q\"");
        assert_eq!(p["notification_type"], "permission_prompt");
        // 최상위가 객체가 아니면 버린다
        assert_eq!(
            payload_from_bytes(br#"[{"hook_event_name":"Stop"#, true),
            None
        );
        // 닫히지 않은 문자열 값은 건지지 않는다
        let p = payload_from_bytes(
            br#"{"hook_event_name":"Stop","last_assistant_message":"abc"#,
            true,
        )
        .unwrap();
        assert!(p.get("last_assistant_message").is_none(), "{p}");
    }

    /// 연결 단계는 100ms(남은 시간이 더 짧으면 그만큼)까지만 기다린다.
    #[test]
    fn connect_phase_has_its_own_budget() {
        assert_eq!(CONNECT_BUDGET, Duration::from_millis(100));
        assert_eq!(budget(NET_BUDGET), (NET_BUDGET, CONNECT_BUDGET));
        let ms30 = Duration::from_millis(30);
        assert_eq!(budget(ms30), (ms30, ms30));
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

    const TOK: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// 계약 §13.1의 식. 기대값은 파이썬 `hmac`으로 따로 계산했다.
    #[test]
    fn hello_proof_follows_contract_formula() {
        let n = format!("{}{}", "00".repeat(16), "ff".repeat(16));
        let p47321 = "c4c3f5b005a77115e7607b3924e1dbc59a84de16446cfad6d7fcc17f20999a86";
        let p47322 = "52cd99f84ef74af1d3cdeb8ae16914b4c8ba06e4a512b6706586a6d640068101";
        assert!(proof_ok(TOK, 47321, &n, p47321));
        assert!(proof_ok(TOK, 47321, &n, &p47321.to_uppercase()));
        assert!(proof_ok(TOK, 47322, &n, p47322));
        // 다른 포트용 proof(중계), 다른 토큰, 다른 n
        assert!(!proof_ok(TOK, 47321, &n, p47322));
        assert!(!proof_ok(&"a".repeat(64), 47321, &n, p47321));
        assert!(!proof_ok(TOK, 47321, &"11".repeat(32), p47321));
        // 꼴이 틀린 것
        let bad_char = p47321.replacen('c', "g", 1);
        let long = format!("{p47321}00");
        for bad in ["", "zz", &p47321[..62], &long, &bad_char, &p47321[..63]] {
            assert!(!proof_ok(TOK, 47321, &n, bad), "{bad}");
        }
    }

    #[test]
    fn nonce_and_port() {
        let a = nonce().unwrap();
        let b = nonce().unwrap();
        assert_eq!(a.len(), 64);
        assert!(
            a.bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
            "{a}"
        );
        assert_ne!(a, b);
        assert_eq!(port_of("http://127.0.0.1:47321"), Some(47321));
        assert_eq!(port_of("http://localhost:5000"), Some(5000));
        assert_eq!(port_of("http://127.0.0.1"), Some(80));
        assert_eq!(port_of("https://127.0.0.1:1"), None);
    }

    /// 연결만 받고 아무 말도 하지 않는 서버(받은 연결은 붙잡아 둔다). 받은 요청줄을 차례로 돌려준다.
    fn silent_server() -> (u16, mpsc::Receiver<String>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for mut s in l.incoming().flatten() {
                let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                let _ = tx.send(text.lines().next().unwrap_or("").to_owned());
                held.push(s);
            }
        });
        (port, rx)
    }

    /// 확인과 전송을 합쳐 400ms 통신 한도 안(답하지 않는 서버). 보내지 않고 스풀에 남긴다.
    #[test]
    fn hello_and_send_share_the_net_budget() {
        let (port, rx) = silent_server();
        let d = tempfile::tempdir().unwrap();
        let start = Instant::now();
        let ctx = Ctx {
            base: Some(format!("http://127.0.0.1:{port}")),
            token: Some(TOK.into()),
            spool: d.path().join("spool"),
            deadline: start + NET_BUDGET,
        };
        deliver(
            &ctx,
            "claude-code",
            &json!({"payload": {"session_id": "s"}, "pid_hint": null, "received_ms": 1}),
        );
        let took = start.elapsed();
        assert!(took < NET_BUDGET + Duration::from_millis(100), "{took:?}");
        assert!(took >= NET_BUDGET - Duration::from_millis(50), "{took:?}");
        assert_eq!(fs::read_dir(d.path().join("spool")).unwrap().count(), 1);
        let first = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(first.starts_with("GET /v1/hello?n="), "{first}");
        // 확인에서 시간을 다 썼으므로 토큰을 싣는 요청은 없다
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }
}
