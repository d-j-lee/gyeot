//! 곁 데몬(gyeotd) 찾기·띄우기·확인. 셸은 서버를 내장하지 않는다(G-002).
//!
//! 셸은 데몬에 토큰을 보내기 전에 늘 §13.1로 확인한다(G-032): `GET /v1/hello`의 증명이 내 토큰과 내가
//! 연결한 포트로 계산한 값과 같아야 하고, 토큰은 확인한 **그 연결**로만 보낸다. 확인이 안 되면 데몬이
//! 없는 것처럼 다룬다(토큰도 페이로드도 보내지 않는다). `GET /health`로는 곁 데몬인지 판단하지 않는다.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use crate::hello::{self, NOT_MINE};
use crate::http::{self, Conn};
use crate::paths;

pub const EXE_NAME: &str = if cfg!(windows) {
    "gyeotd.exe"
} else {
    "gyeotd"
};

/// 띄우기·다시 시작이 겹치지 않게 한다(트레이·감시·다시 시도가 동시에 부를 수 있다).
static LIFECYCLE: Mutex<()> = Mutex::new(());

/// 데몬이 있는지 볼 때(감시 10초마다 포함)의 한도.
const PROBE_TIMEOUT: Duration = Duration::from_millis(1200);
/// 확인 + 토큰 요청 한 번의 한도.
const AUTHED_TIMEOUT: Duration = Duration::from_secs(3);

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// 데몬에 묻다가 실패한 이유. 화면·`shell.log`에는 글자로 보인다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// §13.1 확인 실패: 이 포트의 서버가 내 곁 데몬임을 보이지 못했다. 토큰도 페이로드도 보내지 않았다.
    NotMine(String),
    /// 그 밖(닿지 않음·토큰 파일 없음·HTTP 오류·응답 모양 등).
    Other(String),
}

impl Failure {
    pub fn is_not_mine(&self) -> bool {
        matches!(self, Failure::NotMine(_))
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::NotMine(m) | Failure::Other(m) => f.write_str(m),
        }
    }
}

/// 이 포트에 지금 무엇이 있는지(토큰은 보내지 않는다).
#[derive(Debug)]
pub enum Probe {
    /// 내 곁 데몬이다(§13.1 증명이 맞다).
    Mine,
    /// 무언가 응답하지만 내 곁 데몬임을 확인하지 못했다. 이유를 담는다.
    NotMine(String),
    /// 닿지 않거나 응답하지 않는다.
    Down(io::Error),
}

fn not_mine(port: u16, why: &str) -> String {
    format!("{NOT_MINE} — 포트 {port}: {why}")
}

enum Verify {
    NotMine(String),
    Io(io::Error),
}

/// 열린 연결에서 §13.1 확인을 한다. `Ok`면 같은 연결로 토큰을 보내도 된다(`keep_alive`일 때).
fn verify(conn: &mut Conn, token: &str, port: u16, keep_alive: bool) -> Result<(), Verify> {
    let nonce = hello::new_nonce().map_err(|e| Verify::Io(io::Error::other(e)))?;
    let resp = conn
        .send("GET", &hello::path(&nonce), None, None, keep_alive)
        .map_err(|e| {
            if e.kind() == io::ErrorKind::InvalidData {
                Verify::NotMine(format!("HTTP로 답하지 않아요({e})"))
            } else {
                Verify::Io(e)
            }
        })?;
    hello::check(resp.status, &resp.body, token, port, &nonce).map_err(Verify::NotMine)
}

/// 이 포트의 서버가 내 곁 데몬인지 본다. 연결부터 한다(아무도 없으면 토큰이 없어도 `Down`: 데몬을 띄워야 한다).
pub fn probe(port: u16, data_dir: Option<&Path>) -> Probe {
    let mut conn = match Conn::open(port, PROBE_TIMEOUT) {
        Ok(c) => c,
        Err(e) => return Probe::Down(e),
    };
    let token = match paths::read_token(data_dir) {
        Ok(t) => t,
        Err(why) => {
            return Probe::NotMine(format!(
                "내 토큰을 읽지 못해 포트 {port}의 서버가 내 곁 데몬인지 확인할 수 없어요 — {why}"
            ))
        }
    };
    match verify(&mut conn, &token, port, false) {
        Ok(()) => Probe::Mine,
        Err(Verify::NotMine(why)) => Probe::NotMine(not_mine(port, &why)),
        Err(Verify::Io(e)) => Probe::Down(e),
    }
}

/// 찾는 순서: 셸 실행 파일 옆 → `<데이터>/bin`.
pub fn candidates(exe_dir: Option<&Path>, data_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Some(d) = exe_dir {
        out.push(d.join(EXE_NAME));
    }
    if let Some(d) = data_dir {
        out.push(d.join("bin").join(EXE_NAME));
    }
    out.dedup();
    out
}

pub fn find_exe(data_dir: Option<&Path>) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok();
    let exe_dir = exe.as_deref().and_then(Path::parent);
    candidates(exe_dir, data_dir)
        .into_iter()
        .find(|p| p.is_file())
}

/// 창 없이(CREATE_NO_WINDOW) 따로 산다. 셸이 꺼져도 데몬은 계속 돈다.
fn spawn(exe: &Path) -> io::Result<u32> {
    let mut cmd = Command::new(exe);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
    let child = cmd.spawn()?;
    // Child를 놓아도 프로세스는 끝나지 않는다(분리).
    Ok(child.id())
}

/// 내 곁 데몬이 확인될 때까지 기다린다. 못 하면 마지막으로 본 "내 것이 아님" 이유(있으면).
fn wait_until_mine(
    port: u16,
    data_dir: Option<&Path>,
    timeout: Duration,
) -> Result<(), Option<String>> {
    let deadline = Instant::now() + timeout;
    loop {
        let last = match probe(port, data_dir) {
            Probe::Mine => return Ok(()),
            Probe::NotMine(why) => Some(why),
            Probe::Down(_) => None,
        };
        if Instant::now() >= deadline {
            return Err(last);
        }
        thread::sleep(Duration::from_millis(250));
    }
}

/// 내 곁 데몬이 더 응답하지 않을 때까지 기다린다.
fn wait_while_mine(port: u16, data_dir: Option<&Path>, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !matches!(probe(port, data_dir), Probe::Mine) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(250));
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Started {
    AlreadyRunning,
    Spawned,
}

/// 데몬이 없으면 띄우고 확인될 때까지 기다린다. 포트에 다른 서버가 있으면 띄우지 않는다(띄워도 포트를 못 잡는다).
pub fn ensure_running(port: u16, data_dir: Option<&Path>) -> Result<Started, Failure> {
    let _guard = LIFECYCLE.lock().unwrap_or_else(|e| e.into_inner());
    ensure_running_locked(port, data_dir)
}

fn ensure_running_locked(port: u16, data_dir: Option<&Path>) -> Result<Started, Failure> {
    match probe(port, data_dir) {
        Probe::Mine => {
            crate::info!("곁 데몬을 확인했어요(포트 {port})");
            return Ok(Started::AlreadyRunning);
        }
        Probe::NotMine(why) => return Err(Failure::NotMine(why)),
        Probe::Down(e) => crate::info!("곁 데몬이 응답하지 않아요({e}). 띄웁니다"),
    }
    let exe = find_exe(data_dir).ok_or_else(|| {
        Failure::Other(format!(
            "{EXE_NAME}를 찾지 못했어요. 셸 옆이나 %LOCALAPPDATA%\\gyeot\\bin 에 두세요(install.ps1)."
        ))
    })?;
    let pid = spawn(&exe)
        .map_err(|e| Failure::Other(format!("{}를 띄우지 못했어요: {e}", exe.display())))?;
    crate::info!("곁 데몬을 띄웠어요: {} (pid {pid})", exe.display());
    match wait_until_mine(port, data_dir, Duration::from_secs(15)) {
        Ok(()) => Ok(Started::Spawned),
        Err(Some(why)) => Err(Failure::NotMine(why)),
        Err(None) => Err(Failure::Other(
            "곁 데몬이 15초 안에 응답하지 않았어요. shell.log와 데몬 로그를 확인하세요.".into(),
        )),
    }
}

/// 이 사용자의 gyeotd를 모두 끝낸다. 종료 API가 없어서 강제 종료한다.
fn stop_all() -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let taskkill = PathBuf::from(system_root)
            .join("System32")
            .join("taskkill.exe");
        let out = Command::new(taskkill)
            .args(["/F", "/T", "/IM", EXE_NAME])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status()?;
        // 128 = 그런 프로세스가 없음
        if !out.success() && out.code() != Some(128) {
            crate::warn!("taskkill 종료 코드 {:?}", out.code());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("pkill").args(["-x", "gyeotd"]).status()?;
        Ok(())
    }
}

/// 데몬을 멈추고 다시 띄운다. 다시 띄운 데몬도 처음부터 다시 확인한다.
pub fn restart(port: u16, data_dir: Option<&Path>) -> Result<Started, Failure> {
    let _guard = LIFECYCLE.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = stop_all() {
        crate::warn!("데몬을 멈추지 못했어요: {e}");
    }
    if !wait_while_mine(port, data_dir, Duration::from_secs(5)) {
        return Err(Failure::Other(
            "기존 곁 데몬이 5초 안에 멈추지 않았어요".into(),
        ));
    }
    ensure_running_locked(port, data_dir)
}

/// `POST /v1/focus` — 집중 모드. 성공하면 `until_ms`.
pub fn set_focus(
    port: u16,
    data_dir: Option<&Path>,
    on: bool,
    minutes: Option<u32>,
) -> Result<Option<i64>, Failure> {
    let resp = post_with_token(port, data_dir, "/v1/focus", &focus_body(on, minutes))?;
    match resp.status {
        200 => {
            let v: serde_json::Value = serde_json::from_slice(&resp.body).unwrap_or_default();
            Ok(v.get("until_ms").and_then(serde_json::Value::as_i64))
        }
        401 => Err(Failure::Other(TOKEN_REFUSED.into())),
        s => Err(Failure::Other(format!(
            "집중 모드 요청이 HTTP {s}로 끝났어요"
        ))),
    }
}

/// `POST /v1/boot` 응답(docs/contracts.md §12.1). 셸은 `code`만 쓴다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Boot {
    pub code: String,
    pub expires_ms: Option<i64>,
}

/// 부트 코드 모양: hex 32–128자(계약은 32바이트 hex = 64자). 주소에 넣으므로 다른 글자는 받지 않는다.
pub fn is_valid_boot_code(code: &str) -> bool {
    (32..=128).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `{"code":…, "url":…, "expires_ms":…}`에서 코드를 꺼낸다. 서버가 준 `url`로는 가지 않는다
/// (셸이 자기 포트·`?mode=overlay`로 주소를 만든다).
pub fn parse_boot(body: &[u8]) -> Option<Boot> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let code = v.get("code")?.as_str()?;
    if !is_valid_boot_code(code) {
        return None;
    }
    Some(Boot {
        code: code.to_string(),
        expires_ms: v.get("expires_ms").and_then(serde_json::Value::as_i64),
    })
}

/// 토큰 파일을 읽고, 새 연결에서 §13.1로 확인한 뒤 **같은 연결로** `Authorization: Bearer`를 실어
/// `POST`한다. 셸이 데몬에 인증해서 묻는 길은 이것뿐이다.
fn post_with_token(
    port: u16,
    data_dir: Option<&Path>,
    path: &str,
    body: &str,
) -> Result<http::Response, Failure> {
    let token = paths::read_token(data_dir).map_err(Failure::Other)?;
    let unreachable = |e: io::Error| Failure::Other(format!("곁 데몬에 닿지 않아요: {e}"));
    let mut conn = Conn::open(port, AUTHED_TIMEOUT).map_err(unreachable)?;
    match verify(&mut conn, &token, port, true) {
        Ok(()) => {}
        Err(Verify::NotMine(why)) => return Err(Failure::NotMine(not_mine(port, &why))),
        Err(Verify::Io(e)) => return Err(unreachable(e)),
    }
    if conn.is_closed() {
        // 새 연결로 보내면 확인한 서버가 받는다는 보장이 없다.
        return Err(Failure::Other(
            "곁 데몬이 확인 뒤 연결을 닫아 토큰을 보내지 않았어요".into(),
        ));
    }
    conn.send("POST", path, Some(&token), Some(body), false)
        .map_err(unreachable)
}

const TOKEN_REFUSED: &str = "토큰이 맞지 않아요(401). 데몬을 다시 시작해 보세요.";

/// `POST /v1/boot` — 얼굴 창을 열 일회용 코드(120초, 한 번만, §12.1).
pub fn request_boot(port: u16, data_dir: Option<&Path>) -> Result<Boot, Failure> {
    let resp = post_with_token(port, data_dir, "/v1/boot", "{}")?;
    match resp.status {
        200 => parse_boot(&resp.body).ok_or_else(|| {
            Failure::Other("곁 데몬의 부트 코드 응답 모양이 맞지 않아요".to_string())
        }),
        401 => Err(Failure::Other(TOKEN_REFUSED.into())),
        404 | 405 => Err(Failure::Other(format!(
            "곁 데몬이 부트 코드(/v1/boot)를 모르는 옛 판이에요(HTTP {}). 곁을 같은 판으로 다시 설치하세요(install.ps1).",
            resp.status
        ))),
        s => Err(Failure::Other(format!(
            "부트 코드 요청이 HTTP {s}로 끝났어요"
        ))),
    }
}

pub fn focus_body(on: bool, minutes: Option<u32>) -> String {
    let mut v = serde_json::json!({ "on": on });
    if let Some(m) = minutes {
        v["minutes"] = serde_json::json!(m);
    }
    v.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_order_is_beside_exe_then_data_bin() {
        let c = candidates(
            Some(Path::new("/opt/shell")),
            Some(Path::new("/data/gyeot")),
        );
        assert_eq!(
            c,
            vec![
                PathBuf::from("/opt/shell").join(EXE_NAME),
                PathBuf::from("/data/gyeot/bin").join(EXE_NAME)
            ]
        );
        // install.ps1 로 깐 경우: 셸도 <데이터>/bin 에 있으니 한 번만
        let same = candidates(
            Some(Path::new("/data/gyeot/bin")),
            Some(Path::new("/data/gyeot")),
        );
        assert_eq!(same.len(), 1);
        assert!(candidates(None, None).is_empty());
    }

    #[test]
    fn focus_body_shape() {
        let v: serde_json::Value = serde_json::from_str(&focus_body(true, Some(30))).unwrap();
        assert_eq!(v, serde_json::json!({ "on": true, "minutes": 30 }));
        let v: serde_json::Value = serde_json::from_str(&focus_body(false, None)).unwrap();
        assert_eq!(v, serde_json::json!({ "on": false }));
    }

    const CODE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn boot_body_must_carry_a_hex_code() {
        let ok = format!(
            r#"{{"code":"{CODE}","url":"http://127.0.0.1:47321/?boot={CODE}","expires_ms":1764126840123}}"#
        );
        assert_eq!(
            parse_boot(ok.as_bytes()),
            Some(Boot {
                code: CODE.into(),
                expires_ms: Some(1_764_126_840_123)
            })
        );
        let no_expiry = format!(r#"{{"code":"{CODE}"}}"#);
        assert_eq!(parse_boot(no_expiry.as_bytes()).unwrap().expires_ms, None);
        // 주소에 들어가므로 hex가 아닌 글자·너무 짧거나 긴 코드는 받지 않는다
        for bad in [
            r#"{"url":"http://127.0.0.1:47321/?boot=x"}"#.to_string(),
            r#"{"code":""}"#.to_string(),
            r#"{"code":"abc"}"#.to_string(),
            format!(r#"{{"code":"{CODE}&mode=evil"}}"#),
            format!(r#"{{"code":"{}"}}"#, "a".repeat(200)),
            format!(r#"{{"code":"{}"}}"#, CODE.replace('a', "가")),
            r#"{"code":12}"#.to_string(),
            "<html>".to_string(),
        ] {
            assert_eq!(parse_boot(bad.as_bytes()), None, "{bad}");
        }
        assert!(is_valid_boot_code(CODE));
        assert!(!is_valid_boot_code(&CODE[..31]));
    }

    #[test]
    fn boot_is_requested_with_bearer_only() {
        let token = "cd".repeat(32);
        let dir = testing::data_dir_with_token("boot-bearer", &token);
        let fake = FakeDaemon::start(Hello::Correct(token.clone()));
        let boot = request_boot(fake.port, Some(&dir)).expect("부트 코드");
        assert_eq!(boot.code, CODE);
        let port = fake.port;
        let seen = fake.finish();
        let post = &seen.last().unwrap().raw;
        assert_eq!(
            seen.last().unwrap().first_line(),
            "POST /v1/boot HTTP/1.1",
            "쿼리 없이 경로만: {post}"
        );
        assert!(
            post.contains(&format!("\r\nAuthorization: Bearer {token}\r\n")),
            "{post}"
        );
        assert!(
            post.contains(&format!("\r\nHost: 127.0.0.1:{port}\r\n")),
            "{post}"
        );
        for s in &seen {
            let lower = s.raw.to_ascii_lowercase();
            assert!(
                !lower.contains("\norigin:"),
                "Origin을 보내지 않는다: {}",
                s.raw
            );
            assert!(!lower.contains("cookie:"), "{}", s.raw);
            assert!(
                !s.raw.contains(&format!("t={token}")),
                "토큰은 URL에 싣지 않는다"
            );
        }
        // 본문은 빈 JSON 객체(서버가 JSON 추출기를 써도 받도록)
        assert!(post.ends_with("\r\n\r\n{}"), "{post}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn boot_failures_are_explained() {
        let token = "ef".repeat(32);
        let dir = testing::data_dir_with_token("boot-err", &token);
        for (status, body, want) in [
            (401, Some("{}"), "토큰"),
            (404, Some("{}"), "/v1/boot"),
            (200, Some(r#"{"code":"zz"}"#), "모양"),
        ] {
            let fake = FakeDaemon::start_with(Hello::Correct(token.clone()), status, body);
            let e = request_boot(fake.port, Some(&dir)).unwrap_err();
            fake.finish();
            assert!(!e.is_not_mine(), "{e}");
            assert!(e.to_string().contains(want), "{status}: {e}");
        }
        std::fs::remove_dir_all(&dir).ok();
        // 토큰이 없으면 데몬에 묻지도 않는다
        let empty =
            std::env::temp_dir().join(format!("gyeot-shell-boot-none-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        let e = request_boot(1, Some(&empty)).unwrap_err();
        assert!(e.to_string().contains("토큰"), "{e}");
        assert!(request_boot(1, None).is_err());
        std::fs::remove_dir_all(&empty).ok();
    }

    #[test]
    fn server_that_closes_after_hello_gets_no_token() {
        let token = "4d".repeat(32);
        let dir = testing::data_dir_with_token("boot-close", &token);
        let fake = FakeDaemon::start(Hello::CorrectThenClose(token.clone()));
        let e = request_boot(fake.port, Some(&dir)).unwrap_err();
        let seen = fake.finish();
        assert!(e.to_string().contains("토큰을 보내지 않았어요"), "{e}");
        assert!(!testing::token_sent(&seen, &token), "{seen:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn focus_follows_a_verified_hello() {
        let token = "5e".repeat(32);
        let dir = testing::data_dir_with_token("focus-ok", &token);
        let fake = FakeDaemon::start(Hello::Correct(token.clone()));
        assert_eq!(
            set_focus(fake.port, Some(&dir), true, Some(30)),
            Ok(Some(123))
        );
        let seen = fake.finish();
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert!(seen[0].first_line().starts_with("GET /v1/hello?n="));
        assert_eq!(seen[1].first_line(), "POST /v1/focus HTTP/1.1");
        assert_eq!(seen[0].conn, seen[1].conn);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn probe_tells_mine_not_mine_and_down() {
        let token = "6f".repeat(32);
        let dir = testing::data_dir_with_token("probe", &token);
        let fake = FakeDaemon::start(Hello::Correct(token.clone()));
        assert!(matches!(probe(fake.port, Some(&dir)), Probe::Mine));
        let seen = fake.finish();
        assert!(
            !testing::token_sent(&seen, &token),
            "확인에는 토큰을 싣지 않는다"
        );
        for hello in not_mine_cases(&token) {
            let fake = FakeDaemon::start(hello.clone());
            match probe(fake.port, Some(&dir)) {
                Probe::NotMine(why) => assert!(why.contains(NOT_MINE), "{hello:?}: {why}"),
                other => panic!("{hello:?}: {other:?}"),
            }
            fake.finish();
        }
        // 토큰 파일이 없으면 확인할 수 없다(무언가 듣고 있으면 내 것으로 보지 않는다)
        let fake = FakeDaemon::start(Hello::Correct(token.clone()));
        let empty =
            std::env::temp_dir().join(format!("gyeot-shell-probe-none-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        match probe(fake.port, Some(&empty)) {
            Probe::NotMine(why) => assert!(why.contains("토큰"), "{why}"),
            other => panic!("{other:?}"),
        }
        fake.finish();
        std::fs::remove_dir_all(&empty).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ensure_running_does_not_spawn_beside_a_foreign_server() {
        let token = "7a".repeat(32);
        let dir = testing::data_dir_with_token("ensure-foreign", &token);
        let fake = FakeDaemon::start(Hello::Wrong);
        // 띄우려 했다면 "gyeotd를 찾지 못했어요"가 나왔을 것이다(시험 폴더에는 데몬이 없다).
        let e = ensure_running(fake.port, Some(&dir)).unwrap_err();
        let seen = fake.finish();
        assert!(e.is_not_mine(), "{e}");
        assert!(e.to_string().contains(NOT_MINE), "{e}");
        assert!(!testing::token_sent(&seen, &token));
        std::fs::remove_dir_all(&dir).ok();
    }

    // ───── §13.1: 토큰을 보내기 전에 이 포트의 서버가 내 토큰을 아는지 확인한다 ─────

    use crate::testing::{self, FakeDaemon, Hello};

    const NOT_MINE: &str = "이 포트의 서버는 내 곁 데몬이 아니에요";

    fn not_mine_cases(token: &str) -> Vec<Hello> {
        vec![
            Hello::Wrong,
            Hello::OtherPort(token.to_string()),
            Hello::Correct("2b".repeat(32)), // 다른 토큰을 가진 곁 데몬(다른 사용자)
            Hello::Status(404),              // /v1/hello를 모르는 서버
            Hello::Status(200),              // 본문이 증명이 아님
        ]
    }

    #[test]
    fn boot_is_not_sent_to_a_server_that_does_not_know_the_token() {
        let token = "1a".repeat(32);
        for hello in not_mine_cases(&token) {
            let dir = testing::data_dir_with_token("boot-notmine", &token);
            let fake = FakeDaemon::start(hello.clone());
            let e = request_boot(fake.port, Some(&dir)).unwrap_err().to_string();
            let seen = fake.finish();
            assert!(e.contains(NOT_MINE), "{hello:?}: {e}");
            assert!(
                !testing::token_sent(&seen, &token),
                "{hello:?}: 토큰이 갔다 {seen:?}"
            );
            assert!(
                seen.iter()
                    .all(|s| s.first_line().starts_with("GET /v1/hello?n=")),
                "{hello:?}: 확인 말고는 보내지 않는다 {seen:?}"
            );
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn focus_is_not_sent_to_a_server_that_does_not_know_the_token() {
        let token = "3c".repeat(32);
        for hello in not_mine_cases(&token) {
            let dir = testing::data_dir_with_token("focus-notmine", &token);
            let fake = FakeDaemon::start(hello.clone());
            let e = set_focus(fake.port, Some(&dir), true, Some(30))
                .unwrap_err()
                .to_string();
            let seen = fake.finish();
            assert!(e.contains(NOT_MINE), "{hello:?}: {e}");
            assert!(!testing::token_sent(&seen, &token), "{hello:?}: {seen:?}");
            assert!(
                !seen.iter().any(|s| s.raw.contains("\"minutes\"")),
                "페이로드도 보내지 않는다"
            );
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn boot_follows_a_verified_hello_on_the_same_connection() {
        let token = "cd".repeat(32);
        let dir = testing::data_dir_with_token("boot-ok", &token);
        let fake = FakeDaemon::start(Hello::Correct(token.clone()));
        let boot = request_boot(fake.port, Some(&dir)).expect("부트 코드");
        assert_eq!(boot.code, CODE);
        let seen = fake.finish();
        assert_eq!(seen.len(), 2, "{seen:?}");
        let (hello, post) = (&seen[0], &seen[1]);
        let nonce = hello
            .first_line()
            .strip_prefix("GET /v1/hello?n=")
            .and_then(|r| r.strip_suffix(" HTTP/1.1"))
            .expect("확인 요청이 먼저");
        assert_eq!(nonce.len(), 64, "32바이트 난수 hex: {nonce}");
        assert!(nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert!(!hello.raw.to_ascii_lowercase().contains("authorization"));
        assert!(!hello.raw.contains(&token));
        assert_eq!(post.first_line(), "POST /v1/boot HTTP/1.1");
        assert!(post
            .raw
            .contains(&format!("\r\nAuthorization: Bearer {token}\r\n")));
        assert_eq!(
            hello.conn, post.conn,
            "확인한 그 연결로 토큰을 보낸다(사이에 서버가 바뀌지 않게)"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn probe_down_when_nothing_listens() {
        // 빈 포트를 하나 잡았다가 놓아서 아무도 듣지 않는 포트를 만든다.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert!(matches!(probe(port, None), Probe::Down(_)));
    }
}
