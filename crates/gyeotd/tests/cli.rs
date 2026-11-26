//! 실행 파일 시험: `--version`, `--init`, `--print-config`, `--open [--print]`.

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

/// 계약 §14.1: 세션은 데몬 메모리에만 있어 데몬을 다시 띄우면 브라우저 얼굴은 `gyeotd --open`을 다시 해야 한다.
const REOPEN: &str = "데몬을 다시 띄우면 이전 열쇠는 쓸 수 없어요. 브라우저로 연 얼굴은 gyeotd --open으로 다시 여세요";

#[test]
fn help_says_to_reopen_after_restart() {
    let out = bin().arg("--help").output().unwrap();
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("gyeotd --open"), "{s}");
    assert!(s.contains(REOPEN), "{s}");
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
    http_body(port, request_line, extra, "")
}

/// 본문을 실은 날 HTTP/1.1. (상태, 머리+본문).
fn http_body(port: u16, request_line: &str, extra: &str, body: &str) -> (u16, String) {
    use std::io::{Read, Write};
    let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
        return (0, String::new());
    };
    s.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let len = if body.is_empty() && !request_line.starts_with("POST") {
        String::new()
    } else {
        format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        )
    };
    let req = format!(
        "{request_line}Host: 127.0.0.1:{port}\r\n{extra}{len}Connection: close\r\n\r\n{body}"
    );
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

/// 부트 코드를 `POST /v1/session`으로 바꾼다. (상태, 머리+본문).
fn redeem(port: u16, code: &str) -> (u16, String) {
    http_body(
        port,
        "POST /v1/session HTTP/1.1\r\n",
        "",
        &format!("{{\"code\":\"{code}\"}}"),
    )
}

/// 주소에서 64자 부트 코드를 꺼낸다.
#[cfg(target_os = "linux")]
fn code_in(text: &str) -> String {
    let at = text.find("?boot=").unwrap_or_else(|| panic!("{text}")) + 6;
    let code: String = text[at..]
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect();
    assert_eq!(code.len(), 64, "{text}");
    code
}

const NOT_MINE: &str = "이 포트의 서버는 내 곁 데몬이 아니에요(다른 사용자나 프로그램일 수 있어요)";

/// R1·R3 H1(계약 §12.1): `gyeotd --open --print`는 데몬을 확인하고 부트 주소를 표준 출력에만 보인다.
/// 그 코드는 `POST /v1/session`으로 한 번 세션 열쇠가 된다. 열기 파일은 만들지 않는다.
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
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains(code),
        "코드는 표준 출력에만"
    );
    // 서버는 `?boot=`를 읽지 않는다: 그 주소로 열어도 쪽만 받고, 쿠키는 없다
    let (status, text) = http(daemon.port, &format!("GET /?boot={code} HTTP/1.1\r\n"), "");
    assert_eq!(status, 200, "{text}");
    assert!(!text.to_ascii_lowercase().contains("set-cookie"), "{text}");
    // 코드는 세션 열쇠로 한 번 바뀐다
    let (status, text) = redeem(daemon.port, code);
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("\"session\""), "{text}");
    assert!(!text.to_ascii_lowercase().contains("set-cookie"), "{text}");
    let (status, _) = redeem(daemon.port, code);
    assert_eq!(status, 401);
    // --print는 열기 파일을 만들지 않는다
    let pages = std::fs::read_dir(d.path().join("open"))
        .map(|rd| rd.count())
        .unwrap_or(0);
    assert_eq!(pages, 0);
    // 시작할 때 여는 방법과, 다시 띄우면 브라우저 얼굴을 다시 열어야 한다는 것을 알려 준다(§14.1)
    let log = std::fs::read_to_string(d.path().join("gyeotd.log")).unwrap();
    assert!(log.contains("얼굴 열기: gyeotd --open"), "{log}");
    assert!(log.contains(REOPEN), "{log}");
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

/// 다른 데이터 폴더(다른 토큰)의 데몬이면 hello가 맞지 않아 토큰을 보내지 않고 멈춘다(§13.1).
#[test]
fn open_with_wrong_token_says_not_my_daemon() {
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
    assert!(err.contains(NOT_MINE), "{err}");
    assert!(out.stdout.is_empty());
}

/// 계약 §13.1의 증명(시험 쪽에서 따로 계산한다).
fn proof(token: &str, port: u16, n: &str) -> String {
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(token.as_bytes()).unwrap();
    mac.update(format!("gyeot-hello/v1|{port}|{n}").as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 가짜 서버: 받은 요청을 모두 적고, `/v1/hello`에는 `hello(내 포트, n)`이 정한 (상태, 본문)으로,
/// 그 밖에는 그럴듯한 `/v1/boot` 답으로 답한다. 확인을 건너뛰는 클라이언트는 여기에 토큰을 보내고 성공한다.
type HelloFn = dyn Fn(u16, &str) -> (u16, String) + Send + Sync;

fn fake_server(hello: Box<HelloFn>) -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            let head_end = loop {
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break Some(i + 4);
                }
                match s.read(&mut tmp) {
                    Ok(n) if n > 0 => buf.extend_from_slice(&tmp[..n]),
                    _ => break None,
                }
            };
            let Some(head_end) = head_end else { continue };
            let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
            let len = head
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            while buf.len() < head_end + len {
                match s.read(&mut tmp) {
                    Ok(n) if n > 0 => buf.extend_from_slice(&tmp[..n]),
                    _ => break,
                }
            }
            let text = String::from_utf8_lossy(&buf).into_owned();
            log.lock().unwrap().push(text.clone());
            let target = text.split_whitespace().nth(1).unwrap_or("").to_owned();
            let (status, body) = if let Some(q) = target.strip_prefix("/v1/hello?n=") {
                hello(port, q)
            } else {
                let code = "c".repeat(64);
                (
                    200,
                    format!(
                        "{{\"code\":\"{code}\",\"url\":\"http://127.0.0.1:{port}/?boot={code}\",\"expires_ms\":0}}"
                    ),
                )
            };
            let resp = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    (port, seen)
}

/// R3 M2(계약 §13.1, G-032): 그 포트의 서버가 내 토큰을 아는지 hello로 확인하기 전에는 토큰을 보내지 않는다.
/// 증명이 틀리거나, 다른 포트용이거나(중계), hello를 모르거나, 답이 이상하면 exit 1.
#[test]
fn open_checks_hello_before_sending_the_token() {
    let d = tempfile::tempdir().unwrap();
    let token = "9".repeat(64);
    std::fs::write(d.path().join("token"), &token).unwrap();
    let t1 = token.clone();
    let t2 = token.clone();
    let cases: Vec<(&str, Box<HelloFn>)> = vec![
        (
            "틀린 증명",
            Box::new(|_, _| (200, format!("{{\"v\":1,\"proof\":\"{}\"}}", "0".repeat(64)))),
        ),
        (
            "다른 포트에서 중계한 증명",
            Box::new(move |port, n| {
                (
                    200,
                    format!(
                        "{{\"v\":1,\"proof\":\"{}\"}}",
                        proof(&t1, port.wrapping_add(1), n)
                    ),
                )
            }),
        ),
        (
            "hello를 모르는 서버",
            Box::new(|_, _| (401, "{\"ok\":false,\"error\":\"x\"}".to_owned())),
        ),
        ("이상한 답", Box::new(|_, _| (200, "<html>".to_owned()))),
        (
            "판이 다른 답",
            Box::new(move |port, n| {
                (
                    200,
                    format!("{{\"v\":2,\"proof\":\"{}\"}}", proof(&t2, port, n)),
                )
            }),
        ),
    ];
    for (what, hello) in cases {
        let (port, seen) = fake_server(hello);
        for args in [&["--open", "--print"][..], &["--open"][..]] {
            let out = bin()
                .args(args)
                .env("GYEOT_HOME", d.path())
                .env("GYEOT_PORT", port.to_string())
                .env("PATH", "")
                .output()
                .unwrap();
            let err = String::from_utf8_lossy(&out.stderr);
            assert_eq!(out.status.code(), Some(1), "{what} {args:?}: {err}");
            assert!(err.contains(NOT_MINE), "{what}: {err}");
            assert!(out.stdout.is_empty(), "{what}");
        }
        let seen = seen.lock().unwrap();
        assert!(!seen.is_empty(), "{what}: hello를 보냈어야 함");
        for req in seen.iter() {
            assert!(
                req.starts_with("GET /v1/hello?n="),
                "{what}: hello 말고는 보내지 않는다: {req}"
            );
            assert!(!req.contains(&token), "{what}: 토큰이 샜다: {req}");
            assert!(
                !req.to_ascii_lowercase().contains("authorization"),
                "{what}: {req}"
            );
            let n = req["GET /v1/hello?n=".len()..]
                .split_whitespace()
                .next()
                .unwrap();
            assert_eq!(n.len(), 64, "32바이트 난수: {req}");
            assert!(n.bytes().all(|b| b.is_ascii_hexdigit()));
        }
        let ns: std::collections::HashSet<&str> = seen
            .iter()
            .map(|r| r.split_whitespace().nth(1).unwrap())
            .collect();
        assert_eq!(ns.len(), seen.len(), "{what}: n은 실행마다 다르다");
    }
    // 대조: 증명이 맞으면 그때 토큰으로 boot한다(가짜 서버가 시험 장치로 맞다는 확인)
    let t3 = token.clone();
    let (port, seen) = fake_server(Box::new(move |port, n| {
        (
            200,
            format!("{{\"v\":1,\"proof\":\"{}\"}}", proof(&t3, port, n)),
        )
    }));
    let out = bin()
        .args(["--open", "--print"])
        .env("GYEOT_HOME", d.path())
        .env("GYEOT_PORT", port.to_string())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(seen[0].starts_with("GET /v1/hello?n="));
    assert!(seen[1].starts_with("POST /v1/boot "), "{}", seen[1]);
    assert!(seen[1].contains(&format!("Bearer {token}")), "{}", seen[1]);
}

/// R2 N1(계약 §12.1 끝, G-033): `--open`은 코드가 든 주소를 프로그램 인자에 싣지 않는다.
/// `<데이터>/open/open-<난수>.html`(폴더 0700·파일 0600)을 쓰고 그 경로로 브라우저를 연다. 5분 넘은 쪽은 지운다.
#[cfg(target_os = "linux")]
#[test]
fn open_hands_the_browser_a_private_file_not_the_code() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let daemon = Daemon::start(d.path());
    let tools = tempfile::tempdir().unwrap();
    let script = tools.path().join("xdg-open");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$GYEOT_TEST_ARGS\"\nexit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let args_file = tools.path().join("args.txt");
    // 이미 있던 열기 폴더(느슨한 권한)와 오래된·최근 쪽, 상관없는 파일
    let open_dir = d.path().join("open");
    std::fs::create_dir_all(&open_dir).unwrap();
    std::fs::set_permissions(&open_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let age = |p: &std::path::Path, secs: u64| {
        let f = std::fs::File::options().write(true).open(p).unwrap();
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs))
            .unwrap();
    };
    let old = open_dir.join("open-0123456789abcdef.html");
    let recent = open_dir.join("open-fedcba9876543210.html");
    let other = open_dir.join("notes.txt");
    for (p, secs) in [(&old, 6 * 60), (&recent, 60), (&other, 6 * 60)] {
        std::fs::write(p, "x").unwrap();
        age(p, secs);
    }
    let path = format!(
        "{}:{}",
        tools.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = bin()
        .arg("--open")
        .env("GYEOT_HOME", d.path())
        .env("GYEOT_PORT", daemon.port.to_string())
        .env("PATH", path)
        .env("GYEOT_TEST_ARGS", &args_file)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("boot="), "{stdout}");
    let args = std::fs::read_to_string(&args_file).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert_eq!(args.len(), 1, "인자는 파일 경로 하나: {args:?}");
    let page = std::path::PathBuf::from(args[0]);
    assert_eq!(page.parent(), Some(open_dir.as_path()), "{args:?}");
    let name = page.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("open-") && name.ends_with(".html"),
        "{name}"
    );
    assert!(!args[0].contains("boot"), "{args:?}");
    let html = std::fs::read_to_string(&page).unwrap();
    let code = code_in(&html);
    assert!(!args[0].contains(&code), "인자에 코드가 없다");
    assert!(!stdout.contains(&code) && !stderr.contains(&code));
    let url = format!("http://127.0.0.1:{}/?boot={code}", daemon.port);
    assert!(
        html.contains(&format!(
            r#"<meta http-equiv="refresh" content="0;url={url}">"#
        )),
        "{html}"
    );
    assert!(html.contains(&format!(r#"<a href="{url}">"#)), "{html}");
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&page), 0o600);
    assert_eq!(mode(&open_dir), 0o700);
    // 5분 넘은 open-*.html만 지운다
    assert!(!old.exists(), "오래된 쪽은 지운다");
    assert!(recent.exists(), "최근 쪽은 둔다");
    assert!(other.exists(), "상관없는 파일은 둔다");
    // 브라우저가 읽을 코드는 아직 살아 있다
    let (status, text) = redeem(daemon.port, &code);
    assert_eq!(status, 200, "{text}");
}

/// R3 L3(계약 §13.4): Unix에서 데몬은 데이터 폴더 0700, `gyeotd.log` 0600으로 만들고 이미 있던 것도 맞춘다.
#[cfg(unix)]
#[test]
fn daemon_log_and_home_are_private_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("fresh");
    let daemon = Daemon::start(&home);
    assert_eq!(mode(&home), 0o700);
    assert_eq!(mode(&home.join("gyeotd.log")), 0o600);
    drop(daemon);
    let loose = d.path().join("loose");
    std::fs::create_dir_all(&loose).unwrap();
    std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(loose.join("gyeotd.log"), "old\n").unwrap();
    std::fs::set_permissions(
        loose.join("gyeotd.log"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let daemon = Daemon::start(&loose);
    assert_eq!(mode(&loose), 0o700);
    assert_eq!(mode(&loose.join("gyeotd.log")), 0o600);
    drop(daemon);
}
