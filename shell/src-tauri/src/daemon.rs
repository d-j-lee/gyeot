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
    let token = crate::paths::read_token(data_dir)?;
    let body = focus_body(on, minutes);
    let resp = http::request(
        port,
        "POST",
        "/v1/focus",
        Some(&token),
        Some(&body),
        Duration::from_secs(3),
    )
    .map_err(|e| format!("곁 데몬에 닿지 않아요: {e}"))?;
    match resp.status {
        200 => {
            let v: serde_json::Value = serde_json::from_slice(&resp.body).unwrap_or_default();
            Ok(v.get("until_ms").and_then(serde_json::Value::as_i64))
        }
        401 => Err("토큰이 맞지 않아요(401). 데몬을 다시 시작해 보세요.".into()),
        s => Err(format!("집중 모드 요청이 HTTP {s}로 끝났어요")),
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
