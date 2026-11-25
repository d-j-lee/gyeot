//! 127.0.0.1 데몬 전용의 아주 작은 HTTP/1.1 클라이언트.
//!
//! 셸이 데몬에 묻는 것은 `GET /health`, `POST /v1/boot`, `POST /v1/focus` 셋뿐이라, TLS·프록시·
//! 리다이렉트가 필요 없다. 외부 HTTP 라이브러리 대신 표준 라이브러리만 쓴다.
//!
//! - `Host`는 데몬의 DNS 리바인딩 검사(§7)를 통과하도록 늘 `127.0.0.1:<port>`로 보낸다.
//! - 토큰은 `Authorization: Bearer`로만 보낸다. 경로에 쿼리를 받지 않는다(URL에 비밀을 싣지 않는다).
//! - `Origin` 머리는 보내지 않는다. 데몬은 Tauri 출처를 `403`으로 막는다(§12.2).

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

const MAX_RESPONSE: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// 요청 하나를 보내고 응답 전체를 받는다. `timeout`은 연결부터 응답 끝까지의 전체 시간이다.
pub fn request(
    port: u16,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    json_body: Option<&str>,
    timeout: Duration,
) -> io::Result<Response> {
    let raw_request = build_request(port, method, path, bearer, json_body)?;
    let deadline = Instant::now() + timeout;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_nodelay(true).ok();
    stream.set_write_timeout(Some(remaining(deadline)?))?;
    stream.write_all(raw_request.as_bytes())?;

    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 8192];
    loop {
        if let Some(resp) = try_parse(&buf, false)? {
            return Ok(resp);
        }
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        let n = match stream.read(&mut chunk) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if n == 0 {
            return try_parse(&buf, true)?.ok_or_else(|| {
                io::Error::new(io::ErrorKind::UnexpectedEof, "응답이 중간에 끊겼어요")
            });
        }
        if buf.len() + n > MAX_RESPONSE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "응답이 너무 커요",
            ));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "데몬 응답 시간이 지났어요",
        ))
    } else {
        Ok(left)
    }
}

fn has_ctl(s: &str) -> bool {
    s.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0)
}

pub fn build_request(
    port: u16,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    json_body: Option<&str>,
) -> io::Result<String> {
    let bad = |what: &str| io::Error::new(io::ErrorKind::InvalidInput, format!("잘못된 {what}"));
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(bad("메서드"));
    }
    // 셸의 요청에는 쿼리가 없다. `?t=<토큰>` 같은 꼴이 끼어들 길을 막는다.
    if !path.starts_with('/') || has_ctl(path) || path.contains([' ', '?', '#']) {
        return Err(bad("경로"));
    }
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUser-Agent: gyeot-shell/{}\r\nAccept: application/json\r\nConnection: close\r\n",
        env!("CARGO_PKG_VERSION")
    );
    if let Some(token) = bearer {
        if has_ctl(token) || token.contains(' ') {
            return Err(bad("토큰"));
        }
        req.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    match json_body {
        Some(body) => {
            req.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            ));
            req.push_str(body);
        }
        None if method == "POST" => req.push_str("Content-Length: 0\r\n\r\n"),
        None => req.push_str("\r\n"),
    }
    Ok(req)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// 받은 바이트로 응답이 끝났으면 `Some`. `eof`이면 남은 것을 본문으로 본다.
pub fn try_parse(buf: &[u8], eof: bool) -> io::Result<Option<Response>> {
    let invalid = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    let Some(head_end) = find(buf, b"\r\n\r\n") else {
        return if eof {
            Err(invalid("HTTP 머리가 끝나지 않았어요"))
        } else {
            Ok(None)
        };
    };
    let head = std::str::from_utf8(&buf[..head_end])
        .map_err(|_| invalid("HTTP 머리가 UTF-8이 아니에요"))?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.splitn(3, ' ');
    let proto = parts.next().unwrap_or("");
    if !proto.starts_with("HTTP/1.") {
        return Err(invalid("HTTP 응답이 아니에요"));
    }
    let status = parts
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| invalid("상태 코드를 읽지 못했어요"))?;

    let mut content_length: Option<usize> = None;
    let mut chunked = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        if name == "content-length" {
            content_length = Some(
                value
                    .parse()
                    .map_err(|_| invalid("Content-Length가 이상해요"))?,
            );
        } else if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        }
    }

    let rest = &buf[head_end + 4..];
    // 본문이 없는 상태 코드
    if status / 100 == 1 || status == 204 || status == 304 {
        return Ok(Some(Response {
            status,
            body: Vec::new(),
        }));
    }
    if chunked {
        return match decode_chunked(rest)? {
            Some(body) => Ok(Some(Response { status, body })),
            None if eof => Err(invalid("chunked 본문이 끝나지 않았어요")),
            None => Ok(None),
        };
    }
    if let Some(len) = content_length {
        if rest.len() >= len {
            return Ok(Some(Response {
                status,
                body: rest[..len].to_vec(),
            }));
        }
        return if eof {
            Err(invalid("본문이 Content-Length보다 짧아요"))
        } else {
            Ok(None)
        };
    }
    if eof {
        Ok(Some(Response {
            status,
            body: rest.to_vec(),
        }))
    } else {
        Ok(None)
    }
}

/// 끝 청크(0)까지 다 왔으면 `Some(본문)`.
fn decode_chunked(mut data: &[u8]) -> io::Result<Option<Vec<u8>>> {
    let invalid = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    let mut body = Vec::new();
    loop {
        let Some(line_end) = find(data, b"\r\n") else {
            return Ok(None);
        };
        let size_line = std::str::from_utf8(&data[..line_end]).map_err(|_| invalid("청크 크기"))?;
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16).map_err(|_| invalid("청크 크기"))?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(Some(body));
        }
        if data.len() < size + 2 {
            return Ok(None);
        }
        body.extend_from_slice(&data[..size]);
        if &data[size..size + 2] != b"\r\n" {
            return Err(invalid("청크 끝이 CRLF가 아니에요"));
        }
        data = &data[size + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn request_has_host_and_auth() {
        let r = build_request(
            47321,
            "POST",
            "/v1/focus",
            Some("abc"),
            Some("{\"on\":true}"),
        )
        .unwrap();
        assert!(r.starts_with("POST /v1/focus HTTP/1.1\r\nHost: 127.0.0.1:47321\r\n"));
        assert!(r.contains("Authorization: Bearer abc\r\n"));
        assert!(r.contains("Content-Length: 11\r\n\r\n{\"on\":true}"));
        let g = build_request(47321, "GET", "/health", None, None).unwrap();
        assert!(g.ends_with("Connection: close\r\n\r\n"));
        assert!(!g.contains("Authorization"));
    }

    #[test]
    fn request_never_carries_query_token_or_origin() {
        // 토큰은 Authorization 머리로만 보낸다. 셸 요청에는 쿼리(`?t=` 등)가 아예 없다(§7).
        for path in [
            "/v1/focus?t=abc",
            "/v1/boot?t=",
            "/?mode=overlay",
            "/health#x",
        ] {
            assert!(
                build_request(47321, "POST", path, Some("abc"), None).is_err(),
                "{path}"
            );
        }
        // Origin 머리는 보내지 않는다(데몬은 Tauri 출처를 403으로 막는다, §12.2).
        for (method, path, bearer) in [
            ("GET", "/health", None),
            ("POST", "/v1/boot", Some("abc")),
            ("POST", "/v1/focus", Some("abc")),
        ] {
            let r = build_request(47321, method, path, bearer, Some("{}")).unwrap();
            assert!(!r.to_ascii_lowercase().contains("\r\norigin:"), "{r}");
            assert!(
                r.starts_with(&format!("{method} {path} HTTP/1.1\r\n")),
                "{r}"
            );
        }
    }

    #[test]
    fn request_rejects_header_injection() {
        assert!(build_request(1, "GET", "/a\r\nX: y", None, None).is_err());
        assert!(build_request(1, "GET", "health", None, None).is_err());
        assert!(build_request(1, "get", "/", None, None).is_err());
        assert!(build_request(1, "GET", "/", Some("t\r\nX: y"), None).is_err());
    }

    #[test]
    fn parses_content_length_and_waits_for_more() {
        let full = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\ncontent-length: 11\r\n\r\n{\"ok\":true}";
        let resp = try_parse(full, false).unwrap().unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body_text(), "{\"ok\":true}");
        assert_eq!(try_parse(&full[..full.len() - 3], false).unwrap(), None);
        assert!(try_parse(&full[..full.len() - 3], true).is_err());
        assert_eq!(try_parse(b"HTTP/1.1 200 OK\r\n", false).unwrap(), None);
    }

    #[test]
    fn parses_chunked() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"ok\r\n7;x=y\r\n\":true}\r\n0\r\n\r\n";
        let resp = try_parse(raw, false).unwrap().unwrap();
        assert_eq!(resp.body_text(), "{\"ok\":true}");
        assert_eq!(try_parse(&raw[..raw.len() - 5], false).unwrap(), None);
    }

    #[test]
    fn parses_until_eof_and_no_body_codes() {
        let raw = b"HTTP/1.0 401 Unauthorized\r\nX: y\r\n\r\nnope";
        assert_eq!(try_parse(raw, false).unwrap(), None);
        assert_eq!(try_parse(raw, true).unwrap().unwrap().status, 401);
        let raw = b"HTTP/1.1 204 No Content\r\n\r\n";
        assert_eq!(
            try_parse(raw, false).unwrap().unwrap().body,
            Vec::<u8>::new()
        );
        assert!(try_parse(b"SSH-2.0-OpenSSH\r\n\r\n", true).is_err());
    }

    #[test]
    fn round_trip_against_local_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut got = Vec::new();
            let mut b = [0u8; 1024];
            while find(&got, b"\r\n\r\n").is_none() {
                let n = s.read(&mut b).unwrap();
                got.extend_from_slice(&b[..n]);
            }
            // 연결을 닫지 않고 Content-Length로 끝을 알린다(keep-alive 서버 흉내).
            s.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 29\r\n\r\n{\"ok\":true,\"version\":\"0.1.0\"}",
            )
            .unwrap();
            thread::sleep(Duration::from_millis(300));
            String::from_utf8(got).unwrap()
        });
        let started = Instant::now();
        let resp = request(port, "GET", "/health", None, None, Duration::from_secs(2)).unwrap();
        // 서버가 연결을 닫기(300ms) 전에 Content-Length만으로 끝을 알아야 한다.
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(resp.status, 200);
        assert!(resp.body_text().contains("\"ok\":true"));
        let seen = server.join().unwrap();
        assert!(seen.contains(&format!("Host: 127.0.0.1:{port}\r\n")));
        assert!(!seen.to_ascii_lowercase().contains("\norigin:"), "{seen}");
    }

    #[test]
    fn times_out_on_silent_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (_s, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(600));
        });
        let started = Instant::now();
        let err = request(
            port,
            "GET",
            "/health",
            None,
            None,
            Duration::from_millis(200),
        )
        .unwrap_err();
        assert!(
            matches!(
                err.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ),
            "{err:?}"
        );
        assert!(started.elapsed() < Duration::from_millis(550));
        server.join().unwrap();
    }
}
