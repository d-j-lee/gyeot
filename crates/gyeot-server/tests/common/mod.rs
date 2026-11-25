//! 시험용: 임시 데이터 폴더와 빈 포트로 서버를 띄우고, 날 HTTP/1.1로 말을 건다(Host·Origin을 마음대로 넣으려고).

#![allow(dead_code)]

use gyeot_server::ServerConfig;
use serde_json::Value;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

pub struct TestServer {
    pub addr: SocketAddr,
    pub token: String,
    pub home: PathBuf,
    stop: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<anyhow::Result<()>>>,
}

pub fn config(home: &Path) -> ServerConfig {
    let mut cfg = ServerConfig::load(home.to_path_buf()).expect("config");
    cfg.port = 0;
    cfg.face_dir = None;
    cfg.utc_offset_minutes = Some(0);
    cfg
}

impl TestServer {
    pub async fn start(home: &Path) -> TestServer {
        TestServer::start_with(config(home)).await
    }

    pub async fn start_with(cfg: ServerConfig) -> TestServer {
        let home = cfg.home.clone();
        let server = gyeot_server::bind(cfg).await.expect("bind");
        let addr = server.local_addr();
        let token = server.token().to_owned();
        let (tx, rx) = oneshot::channel::<()>();
        let handle = tokio::spawn(server.run(async move {
            let _ = rx.await;
        }));
        TestServer {
            addr,
            token,
            home,
            stop: Some(tx),
            handle: Some(handle),
        }
    }

    pub async fn stop(mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.handle.take() {
            tokio::time::timeout(Duration::from_secs(20), h)
                .await
                .expect("server stops in time")
                .expect("join")
                .expect("server result");
        }
    }

    pub fn host(&self) -> String {
        format!("127.0.0.1:{}", self.addr.port())
    }

    /// 토큰과 올바른 Host를 붙인 요청.
    pub async fn req(&self, method: &str, path: &str, body: Option<Value>) -> Resp {
        let auth = format!("Bearer {}", self.token);
        let body = body.map(|b| b.to_string());
        self.raw(
            method,
            path,
            &[("Host", &self.host()), ("Authorization", &auth)],
            body.as_deref().map(str::as_bytes),
        )
        .await
    }

    pub async fn get(&self, path: &str) -> Resp {
        self.req("GET", path, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Resp {
        self.req("POST", path, Some(body)).await
    }

    pub async fn state(&self) -> Value {
        let r = self.get("/v1/state").await;
        assert_eq!(r.status, 200, "{}", r.text());
        r.json()
    }

    /// 요청 바이트를 그대로 보낸다(읽을 수 없는 머리 등).
    pub async fn raw_bytes(&self, req: &[u8]) -> Resp {
        let mut s = TcpStream::connect(self.addr).await.expect("connect");
        s.write_all(req).await.unwrap();
        let mut buf = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(10), s.read_to_end(&mut buf)).await;
        Resp::parse(&buf)
    }

    pub async fn raw(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Resp {
        let mut s = TcpStream::connect(self.addr).await.expect("connect");
        let mut req = format!("{method} {path} HTTP/1.1\r\n");
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        let body = body.unwrap_or(b"");
        if !body.is_empty() || method == "POST" {
            req.push_str("Content-Type: application/json\r\n");
            req.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        req.push_str("Connection: close\r\n\r\n");
        s.write_all(req.as_bytes()).await.unwrap();
        // 본문을 다 못 보내도(413 등으로 서버가 먼저 끊어도) 응답은 읽는다.
        let _ = s.write_all(body).await;
        let mut buf = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(10), s.read_to_end(&mut buf)).await;
        Resp::parse(&buf)
    }

    /// SSE 스트림을 연다(토큰은 Authorization 머리로).
    pub async fn sse(&self, path: &str) -> Sse {
        let auth = format!("Bearer {}", self.token);
        Sse::open(self.addr, &self.host(), path, Some(&auth)).await
    }

    /// `POST /v1/boot`(Bearer)로 받은 응답 JSON.
    pub async fn boot(&self) -> Value {
        let r = self.post("/v1/boot", serde_json::json!({})).await;
        assert_eq!(r.status, 200, "{}", r.text());
        r.json()
    }

    /// 쿠키만 붙인 요청(토큰 없음).
    pub async fn with_cookie(&self, method: &str, path: &str, cookie: &str) -> Resp {
        let c = format!("gyeot_session={cookie}");
        self.raw(
            method,
            path,
            &[("Host", &self.host()), ("Cookie", &c)],
            None,
        )
        .await
    }

    /// 부트 코드로 세션을 열고 쿠키 값을 돌려준다.
    pub async fn open_session(&self) -> String {
        let code = self.boot().await["code"].as_str().unwrap().to_owned();
        let r = self
            .raw(
                "GET",
                &format!("/?boot={code}"),
                &[("Host", &self.host())],
                None,
            )
            .await;
        assert_eq!(r.status, 303, "{}", r.text());
        cookie_of(&r).expect("Set-Cookie")
    }
}

#[derive(Debug)]
pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

type Head = (u16, Vec<(String, String)>, usize);

fn parse_head(buf: &[u8]) -> Option<Head> {
    let end = find(buf, b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&buf[..end]);
    let mut lines = head.split("\r\n");
    let status: u16 = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    Some((status, headers, end + 4))
}

/// chunked 본문을 풀 수 있는 만큼 푼다. (풀린 바이트, 쓴 바이트 수).
fn dechunk(buf: &[u8]) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(nl) = find(&buf[i..], b"\r\n") {
        let size_s = String::from_utf8_lossy(&buf[i..i + nl]);
        let size_s = size_s.split(';').next().unwrap_or("").trim().to_owned();
        let Ok(size) = usize::from_str_radix(&size_s, 16) else {
            break;
        };
        let start = i + nl + 2;
        if size == 0 {
            i = start;
            break;
        }
        if buf.len() < start + size + 2 {
            break;
        }
        out.extend_from_slice(&buf[start..start + size]);
        i = start + size + 2;
    }
    (out, i)
}

impl Resp {
    fn parse(buf: &[u8]) -> Resp {
        let Some((status, headers, body_at)) = parse_head(buf) else {
            return Resp {
                status: 0,
                headers: vec![],
                body: buf.to_vec(),
            };
        };
        let raw = &buf[body_at..];
        let chunked = headers
            .iter()
            .any(|(k, v)| k == "transfer-encoding" && v.contains("chunked"));
        let body = if chunked {
            dechunk(raw).0
        } else {
            raw.to_vec()
        };
        Resp {
            status,
            headers,
            body,
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        let n = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == n)
            .map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| panic!("json: {e}: {}", self.text()))
    }
}

/// SSE 이벤트 하나. 주석(`: ping`)은 이름이 `:`다.
#[derive(Debug, Clone, PartialEq)]
pub struct SseEv {
    pub event: String,
    pub data: String,
}

impl SseEv {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.data).unwrap()
    }
}

pub struct Sse {
    pub status: u16,
    stream: TcpStream,
    raw: Vec<u8>,
    text: Vec<u8>,
}

impl Sse {
    pub async fn open(addr: SocketAddr, host: &str, path: &str, auth: Option<&str>) -> Sse {
        match auth {
            Some(a) => Sse::open_with(addr, host, path, &[("Authorization", a)]).await,
            None => Sse::open_with(addr, host, path, &[]).await,
        }
    }

    pub async fn open_with(
        addr: SocketAddr,
        host: &str,
        path: &str,
        headers: &[(&str, &str)],
    ) -> Sse {
        let mut s = TcpStream::connect(addr).await.expect("connect");
        let mut req =
            format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nAccept: text/event-stream\r\n");
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        req.push_str("\r\n");
        s.write_all(req.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        let mut tmp = [0u8; 8192];
        let (status, body_at) = loop {
            if let Some((st, _, at)) = parse_head(&raw) {
                break (st, at);
            }
            let n = tokio::time::timeout(Duration::from_secs(10), s.read(&mut tmp))
                .await
                .expect("sse head")
                .unwrap();
            assert!(n > 0, "connection closed before head");
            raw.extend_from_slice(&tmp[..n]);
        };
        raw.drain(..body_at);
        Sse {
            status,
            stream: s,
            raw,
            text: Vec::new(),
        }
    }

    fn pop_event(&mut self) -> Option<SseEv> {
        let (more, used) = dechunk(&self.raw);
        self.text.extend_from_slice(&more);
        self.raw.drain(..used);
        let end = find(&self.text, b"\n\n")?;
        let block = String::from_utf8_lossy(&self.text[..end]).into_owned();
        self.text.drain(..end + 2);
        let mut ev = SseEv {
            event: "message".into(),
            data: String::new(),
        };
        for line in block.lines() {
            if let Some(c) = line.strip_prefix(':') {
                return Some(SseEv {
                    event: ":".into(),
                    data: c.trim().to_owned(),
                });
            } else if let Some(v) = line.strip_prefix("event:") {
                ev.event = v.trim().to_owned();
            } else if let Some(v) = line.strip_prefix("data:") {
                if !ev.data.is_empty() {
                    ev.data.push('\n');
                }
                ev.data.push_str(v.strip_prefix(' ').unwrap_or(v));
            }
        }
        Some(ev)
    }

    /// 다음 이벤트(주석 포함). 기한 안에 없거나 끊기면 `None`.
    pub async fn next(&mut self, within: Duration) -> Option<SseEv> {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            if let Some(e) = self.pop_event() {
                return Some(e);
            }
            let mut tmp = [0u8; 8192];
            let r = tokio::time::timeout_at(deadline, self.stream.read(&mut tmp)).await;
            match r {
                Ok(Ok(n)) if n > 0 => self.raw.extend_from_slice(&tmp[..n]),
                _ => return None,
            }
        }
    }

    /// 이름이 맞는 다음 이벤트를 기다린다(다른 것은 건너뛴다).
    pub async fn wait_for(&mut self, name: &str, within: Duration) -> Option<SseEv> {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let e = self.next(left).await?;
            if e.event == name {
                return Some(e);
            }
        }
    }

    /// 조건에 맞는 이벤트를 기다린다.
    pub async fn wait_until(
        &mut self,
        within: Duration,
        mut pred: impl FnMut(&SseEv) -> bool,
    ) -> Option<SseEv> {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let e = self.next(left).await?;
            if pred(&e) {
                return Some(e);
            }
        }
    }
}

/// `Set-Cookie: gyeot_session=<값>; …`의 값.
pub fn cookie_of(r: &Resp) -> Option<String> {
    let v = r.header("set-cookie")?;
    let first = v.split(';').next()?.trim();
    first.strip_prefix("gyeot_session=").map(str::to_owned)
}

pub fn hook(name: &str, session: &str, extra: Value) -> Value {
    let mut v = serde_json::json!({
        "session_id": session,
        "transcript_path": "/t.jsonl",
        "cwd": "C:\\work\\proj",
        "hook_event_name": name,
    });
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        for (k, x) in e {
            o.insert(k.clone(), x.clone());
        }
    }
    v
}

pub fn envelope(payload: Value, pid: u32) -> Value {
    serde_json::json!({ "payload": payload, "pid_hint": pid, "received_ms": now_ms() })
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

pub fn items(state: &Value) -> Vec<Value> {
    state["items"].as_array().cloned().unwrap_or_default()
}

pub fn item_kinds(state: &Value) -> Vec<String> {
    items(state)
        .iter()
        .map(|i| i["kind"].as_str().unwrap_or("").to_owned())
        .collect()
}

pub const S: Duration = Duration::from_secs(5);
