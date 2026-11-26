//! 시험용 가짜 데몬. 한 연결에서 요청을 차례로 읽고(keep-alive) 답하며, 받은 요청을 연결 번호와 함께
//! 남긴다. `/v1/hello`는 모드에 따라 맞는 증명·틀린 증명·다른 포트용 증명·HTTP 오류를 준다(§13.1).

use std::io::{ErrorKind, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use hmac::{Hmac, Mac};
use sha2::Sha256;

/// `/v1/hello`에 어떻게 답할지.
#[derive(Debug, Clone)]
pub enum Hello {
    /// 이 토큰과 실제 포트로 계산한 맞는 증명.
    Correct(String),
    /// 아무 값(토큰을 모르는 서버).
    Wrong,
    /// 이 토큰으로, 하지만 다른 포트(+1)로 계산한 증명(다른 포트로 중계하는 서버).
    OtherPort(String),
    /// 이 상태 코드로 답한다(옛 판·다른 프로그램).
    Status(u16),
    /// 확인 응답 뒤 연결을 닫는다(맞는 증명).
    CorrectThenClose(String),
}

#[derive(Debug, Clone)]
pub struct Seen {
    pub conn: usize,
    pub raw: String,
}

impl Seen {
    pub fn first_line(&self) -> &str {
        self.raw.lines().next().unwrap_or("")
    }
}

pub struct FakeDaemon {
    pub port: u16,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<Vec<Seen>>>,
}

/// 증명 계산(서버 쪽 흉내). 셸 코드와 따로 둔다.
pub fn proof_hex(token: &str, port: u16, nonce: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(token.as_bytes()).unwrap();
    mac.update(format!("gyeot-hello/v1|{port}|{nonce}").as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn reply(status: u16, body: &str, close: bool) -> String {
    format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: {}\r\n\r\n{body}",
        body.len(),
        if close { "close" } else { "keep-alive" }
    )
}

const BOOT_CODE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

impl FakeDaemon {
    pub fn start(hello: Hello) -> Self {
        Self::start_with(hello, 200, None)
    }

    /// `POST /v1/boot`에 이 상태 코드(와 본문)로 답한다. 본문이 없으면 맞는 부트 응답.
    pub fn start_with(hello: Hello, boot_status: u16, boot_body: Option<&str>) -> Self {
        let boot_body = boot_body.map(str::to_string);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = thread::spawn(move || {
            let mut seen = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut conn = 0usize;
            while !flag.load(SeqCst) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((s, _)) => {
                        conn += 1;
                        serve(
                            s,
                            conn,
                            port,
                            &hello,
                            (boot_status, boot_body.as_deref()),
                            &flag,
                            &mut seen,
                        );
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => break,
                }
            }
            seen
        });
        FakeDaemon {
            port,
            stop,
            handle: Some(handle),
        }
    }

    /// 멈추고 받은 요청을 모두 돌려준다.
    pub fn finish(mut self) -> Vec<Seen> {
        self.stop.store(true, SeqCst);
        self.handle.take().unwrap().join().unwrap()
    }
}

impl Drop for FakeDaemon {
    fn drop(&mut self) {
        self.stop.store(true, SeqCst);
    }
}

fn serve(
    mut s: std::net::TcpStream,
    conn: usize,
    port: u16,
    hello: &Hello,
    boot: (u16, Option<&str>),
    stop: &AtomicBool,
    seen: &mut Vec<Seen>,
) {
    s.set_nonblocking(false).unwrap();
    s.set_read_timeout(Some(Duration::from_millis(800)))
        .unwrap();
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        // 요청 하나(머리 + Content-Length 본문)를 모은다.
        let req = loop {
            let text = String::from_utf8_lossy(&buf).to_string();
            if let Some(head_end) = text.find("\r\n\r\n") {
                let len = text[..head_end]
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().to_string())
                    })
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0);
                if buf.len() >= head_end + 4 + len {
                    let raw: Vec<u8> = buf.drain(..head_end + 4 + len).collect();
                    break String::from_utf8_lossy(&raw).to_string();
                }
            }
            match s.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        };
        let first = req.lines().next().unwrap_or("").to_string();
        let client_close = req.to_ascii_lowercase().contains("\r\nconnection: close");
        seen.push(Seen {
            conn,
            raw: req.clone(),
        });
        let (status, body, mut close) = if let Some(rest) = first.strip_prefix("GET /v1/hello?n=") {
            let nonce = rest.split(' ').next().unwrap_or("");
            match hello {
                Hello::Correct(t) => (
                    200,
                    format!(r#"{{"v":1,"proof":"{}"}}"#, proof_hex(t, port, nonce)),
                    false,
                ),
                Hello::CorrectThenClose(t) => (
                    200,
                    format!(r#"{{"v":1,"proof":"{}"}}"#, proof_hex(t, port, nonce)),
                    true,
                ),
                Hello::Wrong => (
                    200,
                    format!(r#"{{"v":1,"proof":"{}"}}"#, "7".repeat(64)),
                    false,
                ),
                Hello::OtherPort(t) => (
                    200,
                    format!(
                        r#"{{"v":1,"proof":"{}"}}"#,
                        proof_hex(t, port.wrapping_add(1), nonce)
                    ),
                    false,
                ),
                Hello::Status(code) => (*code, "{}".to_string(), false),
            }
        } else if first.starts_with("POST /v1/boot ") {
            let body = boot.1.map(str::to_string).unwrap_or_else(|| {
                format!(
                    r#"{{"code":"{BOOT_CODE}","url":"http://evil.example/?boot=x","expires_ms":1}}"#
                )
            });
            (boot.0, body, false)
        } else if first.starts_with("GET /v1/stream ") {
            // SSE 흉내: 머리와 ping 하나를 보내고, 멈출 때까지 연결을 붙잡는다. 멈추면 닫는다(데몬이 끝남).
            let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nTransfer-Encoding: chunked\r\n\r\n8\r\n: ping\n\n\r\n";
            if s.write_all(head.as_bytes()).is_err() {
                return;
            }
            while !stop.load(SeqCst) {
                thread::sleep(Duration::from_millis(10));
            }
            return;
        } else if first.starts_with("POST /v1/focus ") {
            (200, r#"{"ok":true,"until_ms":123}"#.to_string(), false)
        } else {
            (404, "{}".to_string(), false)
        };
        close |= client_close;
        if s.write_all(reply(status, &body, close).as_bytes()).is_err() || close {
            return;
        }
    }
}

/// 이 요청들 가운데 토큰이 실린 것이 있는지(머리·주소·본문 어디든).
pub fn token_sent(seen: &[Seen], token: &str) -> bool {
    seen.iter().any(|s| s.raw.contains(token))
}

/// 시험용 데이터 폴더(토큰 파일 포함).
pub fn data_dir_with_token(tag: &str, token: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gyeot-shell-{tag}-{}-{:?}",
        std::process::id(),
        thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("token"), format!("{token}\n")).unwrap();
    dir
}
