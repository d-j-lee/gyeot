//! 실행 파일 시험: `--version`, `--init`, `--print-config`.

use std::process::Command;

fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_gyeotd"));
    c.env_remove("GYEOT_PORT").env_remove("GYEOT_FACE_DIR");
    c
}

#[test]
fn version() {
    let out = bin().arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("gyeotd {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn init_creates_home_and_token() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let out = bin()
        .arg("--init")
        .env("GYEOT_HOME", &home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("데이터 폴더:"), "{stdout}");
    assert!(stdout.contains(&home.display().to_string()), "{stdout}");
    let token = std::fs::read_to_string(home.join("token")).unwrap();
    assert_eq!(token.trim().len(), 64);
    assert!(home.join("events").is_dir());
    assert!(home.join("spool").is_dir());
    // 두 번 해도 토큰은 그대로
    bin()
        .arg("--init")
        .env("GYEOT_HOME", &home)
        .output()
        .unwrap();
    assert_eq!(std::fs::read_to_string(home.join("token")).unwrap(), token);
}

#[test]
fn print_config_shows_effective_values() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("config.toml"),
        "port = 5555\nquiet_hours = [\"23:00-07:00\"]\nutc_offset_minutes = 540\n",
    )
    .unwrap();
    let out = bin()
        .arg("--print-config")
        .env("GYEOT_HOME", d.path())
        .env("GYEOT_PORT", "6000")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("port = 6000"), "{s}");
    assert!(s.contains("23:00-07:00"), "{s}");
    assert!(s.contains("utc_offset_minutes = 540"), "{s}");
}

#[test]
fn bad_config_fails_with_message() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("config.toml"),
        "quiet_hours = [\"99:00-01:00\"]",
    )
    .unwrap();
    let out = bin()
        .arg("--print-config")
        .env("GYEOT_HOME", d.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("곁 데몬 오류"));
}

#[test]
fn unknown_argument_exits_2() {
    let out = bin().arg("--nope").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

/// 데몬을 띄우고 `/health`가 답할 때까지 기다린다. 끝나면 죽인다.
struct Daemon {
    child: std::process::Child,
    port: u16,
}

impl Daemon {
    fn start(home: &std::path::Path) -> Daemon {
        let port = free_port();
        let child = bin()
            .env("GYEOT_HOME", home)
            .env("GYEOT_PORT", port.to_string())
            .env("GYEOT_FACE_DIR", "")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            if http(port, "GET /health HTTP/1.1\r\n", "").0 == 200 {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "데몬이 뜨지 않음");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Daemon { child, port }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 날 HTTP/1.1. (상태, 머리+본문).
fn http(port: u16, request_line: &str, extra: &str) -> (u16, String) {
    use std::io::{Read, Write};
    let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
        return (0, String::new());
    };
    s.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let req = format!("{request_line}Host: 127.0.0.1:{port}\r\n{extra}Connection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() {
        return (0, String::new());
    }
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (status, text)
}

/// R1(계약 §12.1): `gyeotd --open --print`는 실행 중인 데몬에서 부트 주소를 받아 보인다.
#[test]
fn open_print_gets_a_boot_url_from_the_running_daemon() {
    let d = tempfile::tempdir().unwrap();
    let daemon = Daemon::start(d.path());
    let out = bin()
        .args(["--open", "--print"])
        .env("GYEOT_HOME", d.path())
        .env("GYEOT_PORT", daemon.port.to_string())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let url = stdout.trim();
    let prefix = format!("http://127.0.0.1:{}/?boot=", daemon.port);
    let code = url.strip_prefix(&prefix).unwrap_or_else(|| panic!("{url}"));
    assert_eq!(code.len(), 64, "{url}");
    assert!(code.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    // 그 주소로 열면 세션 쿠키를 받는다(한 번만)
    let (status, text) = http(daemon.port, &format!("GET /?boot={code} HTTP/1.1\r\n"), "");
    assert_eq!(status, 303, "{text}");
    assert!(
        text.to_ascii_lowercase()
            .contains("set-cookie: gyeot_session="),
        "{text}"
    );
    let (status, _) = http(daemon.port, &format!("GET /?boot={code} HTTP/1.1\r\n"), "");
    assert_eq!(status, 401);
    // 시작할 때 여는 방법을 알려 준다
    let log = std::fs::read_to_string(d.path().join("gyeotd.log")).unwrap();
    assert!(log.contains("얼굴 열기: gyeotd --open"), "{log}");
}

#[test]
fn open_without_daemon_or_token_explains_in_korean() {
    let d = tempfile::tempdir().unwrap();
    let port = free_port();
    // 토큰이 없으면
    let out = bin()
        .args(["--open", "--print"])
        .env("GYEOT_HOME", d.path())
        .env("GYEOT_PORT", port.to_string())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("토큰"), "{err}");
    assert!(out.stdout.is_empty());
    // 데몬이 없으면
    std::fs::write(d.path().join("token"), "a".repeat(64)).unwrap();
    let out = bin()
        .args(["--open"])
        .env("GYEOT_HOME", d.path())
        .env("GYEOT_PORT", port.to_string())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("곁 데몬에 닿지 않아요"), "{err}");
    assert!(err.contains(&format!("127.0.0.1:{port}")), "{err}");
    assert!(out.stdout.is_empty());
}

#[test]
fn open_with_wrong_token_says_so() {
    let d = tempfile::tempdir().unwrap();
    let daemon = Daemon::start(d.path());
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("token"), "b".repeat(64)).unwrap();
    let out = bin()
        .args(["--open", "--print"])
        .env("GYEOT_HOME", other.path())
        .env("GYEOT_PORT", daemon.port.to_string())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("토큰이 맞지 않아요"), "{err}");
}
