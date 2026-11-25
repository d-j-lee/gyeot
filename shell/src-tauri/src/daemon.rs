//! 곁 데몬(gyeotd) 찾기·띄우기·상태 확인. 셸은 서버를 내장하지 않는다(G-002).

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use crate::http;

pub const EXE_NAME: &str = if cfg!(windows) {
    "gyeotd.exe"
} else {
    "gyeotd"
};

/// 띄우기·다시 시작이 겹치지 않게 한다(트레이·감시·다시 시도가 동시에 부를 수 있다).
static LIFECYCLE: Mutex<()> = Mutex::new(());

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

#[derive(Debug)]
pub enum Health {
    /// 곁 데몬이 응답한다.
    Ok { version: String },
    /// 그 포트에 무언가 있지만 곁 데몬이 아니다.
    Foreign(String),
    /// 닿지 않는다.
    Down(io::Error),
}

/// `/health` 응답 본문이 곁 데몬의 것인지 본다. 버전을 돌려준다.
pub fn parse_health(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    if v.get("ok")?.as_bool()? {
        Some(
            v.get("version")
                .and_then(|x| x.as_str())
                .unwrap_or("?")
                .to_string(),
        )
    } else {
        None
    }
}

pub fn health(port: u16) -> Health {
    match http::request(
        port,
        "GET",
        "/health",
        None,
        None,
        Duration::from_millis(1200),
    ) {
        Ok(resp) if resp.status == 200 => match parse_health(&resp.body_text()) {
            Some(version) => Health::Ok { version },
            None => Health::Foreign("`/health`가 곁 데몬의 응답이 아니에요".into()),
        },
        Ok(resp) => Health::Foreign(format!("`/health`가 HTTP {}를 돌려줬어요", resp.status)),
        Err(e) => Health::Down(e),
    }
}

pub fn is_healthy(port: u16) -> bool {
    matches!(health(port), Health::Ok { .. })
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

fn wait_for(port: u16, healthy: bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if is_healthy(port) == healthy {
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

/// 데몬이 없으면 띄우고 응답할 때까지 기다린다.
pub fn ensure_running(port: u16, data_dir: Option<&Path>) -> Result<Started, String> {
    let _guard = LIFECYCLE.lock().unwrap_or_else(|e| e.into_inner());
    ensure_running_locked(port, data_dir)
}

fn ensure_running_locked(port: u16, data_dir: Option<&Path>) -> Result<Started, String> {
    match health(port) {
        Health::Ok { version } => {
            crate::info!("곁 데몬이 이미 떠 있어요(v{version}, 포트 {port})");
            return Ok(Started::AlreadyRunning);
        }
        Health::Foreign(why) => {
            return Err(format!("포트 {port}를 다른 프로그램이 쓰고 있어요: {why}"));
        }
        Health::Down(e) => crate::info!("곁 데몬이 응답하지 않아요({e}). 띄웁니다"),
    }
    let exe = find_exe(data_dir).ok_or_else(|| {
        format!("{EXE_NAME}를 찾지 못했어요. 셸 옆이나 %LOCALAPPDATA%\\gyeot\\bin 에 두세요(install.ps1).")
    })?;
    let pid = spawn(&exe).map_err(|e| format!("{}를 띄우지 못했어요: {e}", exe.display()))?;
    crate::info!("곁 데몬을 띄웠어요: {} (pid {pid})", exe.display());
    if wait_for(port, true, Duration::from_secs(15)) {
        Ok(Started::Spawned)
    } else {
        Err("곁 데몬이 15초 안에 응답하지 않았어요. shell.log와 데몬 로그를 확인하세요.".into())
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

/// 데몬을 멈추고 다시 띄운다.
pub fn restart(port: u16, data_dir: Option<&Path>) -> Result<Started, String> {
    let _guard = LIFECYCLE.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = stop_all() {
        crate::warn!("데몬을 멈추지 못했어요: {e}");
    }
    if !wait_for(port, false, Duration::from_secs(5)) {
        return Err("기존 곁 데몬이 5초 안에 멈추지 않았어요".into());
    }
    ensure_running_locked(port, data_dir)
}

/// `POST /v1/focus` — 집중 모드. 성공하면 `until_ms`.
pub fn set_focus(
    port: u16,
    data_dir: Option<&Path>,
    on: bool,
    minutes: Option<u32>,
) -> Result<Option<i64>, String> {
    let resp = post_with_token(port, data_dir, "/v1/focus", &focus_body(on, minutes))?;
    match resp.status {
        200 => {
            let v: serde_json::Value = serde_json::from_slice(&resp.body).unwrap_or_default();
            Ok(v.get("until_ms").and_then(serde_json::Value::as_i64))
        }
        401 => Err(TOKEN_REFUSED.into()),
        s => Err(format!("집중 모드 요청이 HTTP {s}로 끝났어요")),
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

/// 토큰 파일을 읽어 `Authorization: Bearer`로 `POST`한다. 셸이 데몬에 인증해서 묻는 길은 이것뿐이다.
fn post_with_token(
    port: u16,
    data_dir: Option<&Path>,
    path: &str,
    body: &str,
) -> Result<http::Response, String> {
    let token = crate::paths::read_token(data_dir)?;
    http::request(
        port,
        "POST",
        path,
        Some(&token),
        Some(body),
        Duration::from_secs(3),
    )
    .map_err(|e| format!("곁 데몬에 닿지 않아요: {e}"))
}

const TOKEN_REFUSED: &str = "토큰이 맞지 않아요(401). 데몬을 다시 시작해 보세요.";

/// `POST /v1/boot` — 얼굴 창을 열 일회용 코드(120초, 한 번만, §12.1).
pub fn request_boot(port: u16, data_dir: Option<&Path>) -> Result<Boot, String> {
    let resp = post_with_token(port, data_dir, "/v1/boot", "{}")?;
    match resp.status {
        200 => parse_boot(&resp.body)
            .ok_or_else(|| "곁 데몬의 부트 코드 응답 모양이 맞지 않아요".to_string()),
        401 => Err(TOKEN_REFUSED.into()),
        404 | 405 => Err(format!(
            "곁 데몬이 부트 코드(/v1/boot)를 모르는 옛 판이에요(HTTP {}). 곁을 같은 판으로 다시 설치하세요(install.ps1).",
            resp.status
        )),
        s => Err(format!("부트 코드 요청이 HTTP {s}로 끝났어요")),
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
    fn health_body_must_be_gyeot() {
        assert_eq!(
            parse_health(r#"{"ok":true,"version":"0.1.0"}"#),
            Some("0.1.0".into())
        );
        assert_eq!(parse_health(r#"{"ok":true}"#), Some("?".into()));
        assert_eq!(parse_health(r#"{"ok":false}"#), None);
        assert_eq!(parse_health(r#"{"status":"up"}"#), None);
        assert_eq!(parse_health("<html>"), None);
    }

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

    /// 가짜 데몬: 요청 하나를 받아 머리를 돌려주고 `reply`로 답한다.
    fn fake_daemon(reply: &'static str) -> (u16, thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut got = Vec::new();
            let mut b = [0u8; 4096];
            loop {
                let n = s.read(&mut b).unwrap();
                got.extend_from_slice(&b[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let len = text[..head_end]
                        .lines()
                        .find_map(|l| l.strip_prefix("Content-Length: "))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if got.len() >= head_end + 4 + len || n == 0 {
                        break;
                    }
                }
            }
            s.write_all(reply.as_bytes()).unwrap();
            String::from_utf8(got).unwrap()
        });
        (port, server)
    }

    fn temp_data_dir(tag: &str, token: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gyeot-shell-boot-{tag}-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        if let Some(t) = token {
            std::fs::write(dir.join("token"), format!("{t}\n")).unwrap();
        }
        dir
    }

    #[test]
    fn boot_is_requested_with_bearer_only() {
        const REPLY: &str = concat!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 126\r\n\r\n",
            r#"{"code":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","url":"http://evil.example/?boot=x","expires_ms":1}"#
        );
        let token = "cd".repeat(32);
        let dir = temp_data_dir("ok", Some(&token));
        let (port, server) = fake_daemon(REPLY);
        let boot = request_boot(port, Some(&dir)).expect("부트 코드");
        assert_eq!(boot.code, CODE);
        let seen = server.join().unwrap();
        let first_line = seen.lines().next().unwrap();
        assert_eq!(
            first_line, "POST /v1/boot HTTP/1.1",
            "쿼리 없이 경로만: {seen}"
        );
        assert!(
            seen.contains(&format!("\r\nAuthorization: Bearer {token}\r\n")),
            "{seen}"
        );
        assert!(
            seen.contains(&format!("\r\nHost: 127.0.0.1:{port}\r\n")),
            "{seen}"
        );
        let lower = seen.to_ascii_lowercase();
        assert!(
            !lower.contains("\norigin:"),
            "Origin을 보내지 않는다: {seen}"
        );
        assert!(!lower.contains("cookie:"), "{seen}");
        assert!(
            !seen.contains(&format!("t={token}")),
            "토큰은 URL에 싣지 않는다"
        );
        // 본문은 빈 JSON 객체(서버가 JSON 추출기를 써도 받도록)
        assert!(seen.ends_with("\r\n\r\n{}"), "{seen}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn boot_failures_are_explained() {
        let token = "ef".repeat(32);
        let dir = temp_data_dir("err", Some(&token));
        let (port, server) = fake_daemon("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n");
        let e = request_boot(port, Some(&dir)).unwrap_err();
        assert!(e.contains("토큰"), "{e}");
        server.join().unwrap();
        let (port, server) = fake_daemon("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        let e = request_boot(port, Some(&dir)).unwrap_err();
        assert!(e.contains("/v1/boot"), "{e}");
        server.join().unwrap();
        let (port, server) =
            fake_daemon("HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\n{\"code\":\"zz\"}");
        let e = request_boot(port, Some(&dir)).unwrap_err();
        assert!(e.contains("모양"), "{e}");
        server.join().unwrap();
        std::fs::remove_dir_all(&dir).ok();
        // 토큰이 없으면 데몬에 묻지도 않는다
        let empty = temp_data_dir("none", None);
        let e = request_boot(1, Some(&empty)).unwrap_err();
        assert!(e.contains("토큰"), "{e}");
        assert!(request_boot(1, None).is_err());
        std::fs::remove_dir_all(&empty).ok();
    }

    #[test]
    fn health_down_when_nothing_listens() {
        // 빈 포트를 하나 잡았다가 놓아서 아무도 듣지 않는 포트를 만든다.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert!(matches!(health(port), Health::Down(_)));
        assert!(!is_healthy(port));
    }
}
