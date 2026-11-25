//! 127.0.0.1 데몬 전용의 아주 작은 HTTP/1.1 클라이언트.
//!
//! 셸이 데몬에 묻는 것은 `GET /v1/hello`(데몬 확인), `POST /v1/boot`, `POST /v1/focus` 셋뿐이라,
//! TLS·프록시·리다이렉트가 필요 없다. 외부 HTTP 라이브러리 대신 표준 라이브러리만 쓴다.
//!
//! - `Host`는 데몬의 DNS 리바인딩 검사(§7)를 통과하도록 늘 `127.0.0.1:<port>`로 보낸다.
//! - 토큰은 `Authorization: Bearer`로만 보낸다. 쿼리는 토큰이 없는 `/v1/hello?n=<hex>` 하나만 받는다
//!   (URL에 비밀을 싣지 않는다).
//! - `Origin` 머리는 보내지 않는다. 데몬은 Tauri 출처를 `403`으로 막는다(§12.2).
//! - [`Conn`]은 한 TCP 연결로 요청을 차례로 보낸다. 셸은 데몬을 확인한 **그 연결**로 토큰을 보낸다
//!   (§13.1: 확인과 전송 사이에 포트의 주인이 바뀌지 않게).

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

const MAX_RESPONSE: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    /// 서버가 이 응답 뒤에 연결을 닫는다(`Connection: close`, HTTP/1.0 기본, 본문이 연결 끝까지).
    pub close: bool,
}

impl Response {
    #[cfg(test)]
    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// 데몬과의 TCP 연결 하나. `deadline`은 연결부터 마지막 응답까지의 전체 시간이다.
pub struct Conn {
    stream: TcpStream,
    port: u16,
    deadline: Instant,
    /// 앞 응답 뒤에 이미 받아 둔 바이트.
    buf: Vec<u8>,
    closed: bool,
}

impl Conn {
    pub fn open(port: u16, timeout: Duration) -> io::Result<Conn> {
        let deadline = Instant::now() + timeout;
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let stream = TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_nodelay(true).ok();
        Ok(Conn {
            stream,
            port,
            deadline,
            buf: Vec::with_capacity(1024),
            closed: false,
        })
    }

    /// 서버가 닫았거나 마지막 요청으로 보냈다(더 보낼 수 없다).
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 요청 하나를 보내고 응답 전체를 받는다. `keep_alive`면 다음 요청을 같은 연결로 보낼 수 있다.
    pub fn send(
        &mut self,
        method: &str,
        target: &str,
        bearer: Option<&str>,
        json_body: Option<&str>,
        keep_alive: bool,
    ) -> io::Result<Response> {
        if self.closed {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "곁 데몬이 연결을 닫았어요",
            ));
        }
        let raw_request = build_request(self.port, method, target, bearer, json_body, keep_alive)?;
        self.stream
            .set_write_timeout(Some(remaining(self.deadline)?))?;
        self.stream.write_all(raw_request.as_bytes())?;

        let mut chunk = [0u8; 8192];
        loop {
            if let Some((resp, used)) = try_parse(&self.buf, false)? {
                self.buf.drain(..used);
                self.closed |= resp.close || !keep_alive;
                return Ok(resp);
            }
            self.stream
                .set_read_timeout(Some(remaining(self.deadline)?))?;
            let n = match self.stream.read(&mut chunk) {
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if n == 0 {
                self.closed = true;
                let parsed = try_parse(&self.buf, true)?;
                self.buf.clear();
                return parsed.map(|(r, _)| r).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::UnexpectedEof, "응답이 중간에 끊겼어요")
                });
            }
            if self.buf.len() + n > MAX_RESPONSE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "응답이 너무 커요",
                ));
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }
}

/// 요청 하나를 새 연결로 보내고 응답 전체를 받는다. `timeout`은 연결부터 응답 끝까지의 전체 시간이다.
#[cfg(test)]
pub fn request(
    port: u16,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    json_body: Option<&str>,
    timeout: Duration,
) -> io::Result<Response> {
    Conn::open(port, timeout)?.send(method, path, bearer, json_body, false)
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

/// 쿼리가 붙은 요청은 데몬 확인(`/v1/hello?n=<hex 16–128자>`) 하나뿐이고, 거기에는 토큰을 싣지 않는다.
fn is_hello_target(target: &str) -> bool {
    target
        .strip_prefix("/v1/hello?n=")
        .is_some_and(|n| (16..=128).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn build_request(
    port: u16,
    method: &str,
    target: &str,
    bearer: Option<&str>,
    json_body: Option<&str>,
    keep_alive: bool,
) -> io::Result<String> {
    let bad = |what: &str| io::Error::new(io::ErrorKind::InvalidInput, format!("잘못된 {what}"));
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(bad("메서드"));
    }
    // 셸의 토큰 요청에는 쿼리가 없다. `?t=<토큰>` 같은 꼴이 끼어들 길을 막는다.
    if !target.starts_with('/') || has_ctl(target) || target.contains([' ', '#']) {
        return Err(bad("경로"));
    }
    if target.contains('?') && (bearer.is_some() || !is_hello_target(target)) {
        return Err(bad("경로"));
    }
    let connection = if keep_alive { "keep-alive" } else { "close" };
    let mut req = format!(
        "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUser-Agent: gyeot-shell/{}\r\nAccept: application/json\r\nConnection: {connection}\r\n",
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

/// 받은 바이트로 응답 하나가 끝났으면 `Some((응답, 쓴 바이트 수))`. `eof`이면 남은 것을 본문으로 본다.
pub fn try_parse(buf: &[u8], eof: bool) -> io::Result<Option<(Response, usize)>> {
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
    let mut connection = String::new();
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
        } else if name == "connection" {
            connection = value.to_ascii_lowercase();
        }
    }
    let close =
        connection.contains("close") || (proto == "HTTP/1.0" && !connection.contains("keep-alive"));

    let start = head_end + 4;
    let rest = &buf[start..];
    // 본문이 없는 상태 코드
    if status / 100 == 1 || status == 204 || status == 304 {
        return Ok(Some((
            Response {
                status,
                body: Vec::new(),
                close,
            },
            start,
        )));
    }
    if chunked {
        return match decode_chunked(rest)? {
            Some((body, used)) => Ok(Some((
                Response {
                    status,
                    body,
                    close,
                },
                start + used,
            ))),
            None if eof => Err(invalid("chunked 본문이 끝나지 않았어요")),
            None => Ok(None),
        };
    }
    if let Some(len) = content_length {
        if rest.len() >= len {
            return Ok(Some((
                Response {
                    status,
                    body: rest[..len].to_vec(),
                    close,
                },
                start + len,
            )));
        }
        return if eof {
            Err(invalid("본문이 Content-Length보다 짧아요"))
        } else {
            Ok(None)
        };
    }
    // 길이를 알리지 않은 본문은 연결이 끝나야 끝난다.
    if eof {
        Ok(Some((
            Response {
                status,
                body: rest.to_vec(),
                close: true,
            },
            buf.len(),
        )))
    } else {
        Ok(None)
    }
}

/// 끝 청크(0)와 꼬리 머리까지 다 왔으면 `Some((본문, 쓴 바이트 수))`.
fn decode_chunked(data: &[u8]) -> io::Result<Option<(Vec<u8>, usize)>> {
    let invalid = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    let mut body = Vec::new();
    let mut pos = 0usize;
    loop {
        let Some(line_end) = find(&data[pos..], b"\r\n") else {
            return Ok(None);
        };
        let size_line =
            std::str::from_utf8(&data[pos..pos + line_end]).map_err(|_| invalid("청크 크기"))?;
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16).map_err(|_| invalid("청크 크기"))?;
        pos += line_end + 2;
        if size == 0 {
            // 꼬리 머리(trailer)를 빈 줄까지 건너뛴다.
            loop {
                let Some(end) = find(&data[pos..], b"\r\n") else {
                    return Ok(None);
                };
                pos += end + 2;
                if end == 0 {
                    return Ok(Some((body, pos)));
                }
            }
        }
        if data.len() < pos + size + 2 {
            return Ok(None);
        }
        body.extend_from_slice(&data[pos..pos + size]);
        if &data[pos + size..pos + size + 2] != b"\r\n" {
            return Err(invalid("청크 끝이 CRLF가 아니에요"));
        }
        pos += size + 2;
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
            false,
        )
        .unwrap();
        assert!(r.starts_with("POST /v1/focus HTTP/1.1\r\nHost: 127.0.0.1:47321\r\n"));
        assert!(r.contains("Authorization: Bearer abc\r\n"));
        assert!(r.contains("Connection: close\r\n"));
        assert!(r.contains("Content-Length: 11\r\n\r\n{\"on\":true}"));
        let g = build_request(
            47321,
            "GET",
            "/v1/hello?n=abcd0123abcd0123",
            None,
            None,
            true,
        )
        .unwrap();
        assert!(g.ends_with("Connection: keep-alive\r\n\r\n"), "{g}");
        assert!(!g.contains("Authorization"));
    }

    #[test]
    fn request_never_carries_query_token_or_origin() {
        // 토큰은 Authorization 머리로만 보낸다. 셸의 토큰 요청에는 쿼리(`?t=` 등)가 아예 없다(§7).
        let n = "ab".repeat(32);
        for (path, bearer) in [
            ("/v1/focus?t=abc".to_string(), Some("abc")),
            ("/v1/boot?t=".to_string(), Some("abc")),
            ("/?mode=overlay".to_string(), Some("abc")),
            ("/health#x".to_string(), Some("abc")),
            // 확인 요청에도 토큰은 싣지 않는다
            (format!("/v1/hello?n={n}"), Some("abc")),
            // 쿼리는 확인 요청의 hex 난수 하나뿐
            ("/v1/hello?t=abc".to_string(), None),
            (format!("/v1/hello?n={n}&t=abc"), None),
            ("/v1/hello?n=xyz".to_string(), None),
            ("/v1/hello?n=abc".to_string(), None),
            (format!("/v1/focus?n={n}"), None),
        ] {
            assert!(
                build_request(47321, "POST", &path, bearer, None, false).is_err(),
                "{path}"
            );
        }
        assert!(build_request(47321, "GET", &format!("/v1/hello?n={n}"), None, None, true).is_ok());
        // Origin 머리는 보내지 않는다(데몬은 Tauri 출처를 403으로 막는다, §12.2).
        let hello = format!("/v1/hello?n={n}");
        for (method, path, bearer) in [
            ("GET", hello.as_str(), None),
            ("POST", "/v1/boot", Some("abc")),
            ("POST", "/v1/focus", Some("abc")),
        ] {
            let r = build_request(47321, method, path, bearer, Some("{}"), false).unwrap();
            assert!(!r.to_ascii_lowercase().contains("\r\norigin:"), "{r}");
            assert!(
                r.starts_with(&format!("{method} {path} HTTP/1.1\r\n")),
                "{r}"
            );
        }
    }

    #[test]
    fn request_rejects_header_injection() {
        assert!(build_request(1, "GET", "/a\r\nX: y", None, None, false).is_err());
        assert!(build_request(1, "GET", "health", None, None, false).is_err());
        assert!(build_request(1, "get", "/", None, None, false).is_err());
        assert!(build_request(1, "GET", "/", Some("t\r\nX: y"), None, false).is_err());
    }

    #[test]
    fn parses_content_length_and_waits_for_more() {
        let full = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\ncontent-length: 11\r\n\r\n{\"ok\":true}";
        let (resp, used) = try_parse(full, false).unwrap().unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body_text(), "{\"ok\":true}");
        assert_eq!(used, full.len());
        assert!(!resp.close);
        assert_eq!(try_parse(&full[..full.len() - 3], false).unwrap(), None);
        assert!(try_parse(&full[..full.len() - 3], true).is_err());
        assert_eq!(try_parse(b"HTTP/1.1 200 OK\r\n", false).unwrap(), None);
    }

    #[test]
    fn parse_stops_at_the_end_of_one_response() {
        // keep-alive: 다음 응답의 바이트는 남겨 둔다.
        let two = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}HTTP/1.1 204 No Content\r\n\r\n";
        let (first, used) = try_parse(two, false).unwrap().unwrap();
        assert_eq!(first.body_text(), "{}");
        let (second, used2) = try_parse(&two[used..], false).unwrap().unwrap();
        assert_eq!(second.status, 204);
        assert_eq!(used + used2, two.len());
        let close = b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
        assert!(try_parse(close, false).unwrap().unwrap().0.close);
        let old = b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n";
        assert!(try_parse(old, false).unwrap().unwrap().0.close);
        let old_keep = b"HTTP/1.0 200 OK\r\nConnection: Keep-Alive\r\nContent-Length: 0\r\n\r\n";
        assert!(!try_parse(old_keep, false).unwrap().unwrap().0.close);
    }

    #[test]
    fn parses_chunked() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"ok\r\n7;x=y\r\n\":true}\r\n0\r\n\r\n";
        let (resp, used) = try_parse(raw, false).unwrap().unwrap();
        assert_eq!(resp.body_text(), "{\"ok\":true}");
        assert_eq!(used, raw.len(), "끝 청크 뒤 빈 줄까지 쓴다");
        assert_eq!(try_parse(&raw[..raw.len() - 5], false).unwrap(), None);
        assert_eq!(try_parse(&raw[..raw.len() - 2], false).unwrap(), None);
        let trailer =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\nX-T: 1\r\n\r\n";
        let (resp, used) = try_parse(trailer, false).unwrap().unwrap();
        assert_eq!((resp.body_text().as_str(), used), ("{}", trailer.len()));
    }

    #[test]
    fn parses_until_eof_and_no_body_codes() {
        let raw = b"HTTP/1.0 401 Unauthorized\r\nX: y\r\n\r\nnope";
        assert_eq!(try_parse(raw, false).unwrap(), None);
        let (resp, _) = try_parse(raw, true).unwrap().unwrap();
        assert_eq!(resp.status, 401);
        assert!(resp.close);
        let raw = b"HTTP/1.1 204 No Content\r\n\r\n";
        assert_eq!(
            try_parse(raw, false).unwrap().unwrap().0.body,
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
    fn one_connection_carries_two_requests() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut got = Vec::new();
            let mut b = [0u8; 1024];
            // 두 요청을 한 연결에서 받는다. 첫 답에 다음 답의 앞부분을 붙여 보낸다.
            while got.windows(4).filter(|w| *w == b"\r\n\r\n").count() < 1 {
                let n = s.read(&mut b).unwrap();
                got.extend_from_slice(&b[..n]);
            }
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\naHTTP/1.1 200 OK\r\n")
                .unwrap();
            while got.windows(4).filter(|w| *w == b"\r\n\r\n").count() < 2 {
                let n = s.read(&mut b).unwrap();
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&b[..n]);
            }
            s.write_all(b"Content-Length: 1\r\n\r\nb").unwrap();
            String::from_utf8(got).unwrap()
        });
        let mut conn = Conn::open(port, Duration::from_secs(2)).unwrap();
        let n = "ab".repeat(32);
        let a = conn
            .send("GET", &format!("/v1/hello?n={n}"), None, None, true)
            .unwrap();
        assert_eq!(a.body_text(), "a");
        let b = conn
            .send("POST", "/v1/boot", Some("tok"), Some("{}"), false)
            .unwrap();
        assert_eq!(b.body_text(), "b");
        let seen = server.join().unwrap();
        assert!(seen.starts_with(&format!("GET /v1/hello?n={n} HTTP/1.1\r\n")));
        assert!(
            seen.contains("\r\n\r\nPOST /v1/boot HTTP/1.1\r\n"),
            "{seen}"
        );
        // 연결을 닫은 뒤에는 더 보내지 않는다
        assert!(conn.send("POST", "/v1/boot", None, None, false).is_err());
    }

    #[test]
    fn closed_after_response_means_no_second_request() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut b = [0u8; 1024];
            let _ = s.read(&mut b).unwrap();
            s.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        let mut conn = Conn::open(port, Duration::from_secs(2)).unwrap();
        let n = "cd".repeat(32);
        conn.send("GET", &format!("/v1/hello?n={n}"), None, None, true)
            .unwrap();
        let e = conn
            .send("POST", "/v1/boot", Some("tok"), None, false)
            .unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::ConnectionAborted);
        server.join().unwrap();
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
