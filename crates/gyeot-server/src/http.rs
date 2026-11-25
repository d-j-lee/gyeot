//! 라우터·보안 검사·처리기(계약 §7)와 SSE(§8).

use crate::auth::{self, BootCodes, Sessions};
use crate::face;
use crate::hub::{Channels, Hub, gyeot_event};
use crate::store::{now_ms, random_hex};
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as SseEvent, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures_util::Stream;
use gyeot_core::text::{mask, truncate_chars};
use gyeot_core::{ActError, Action, EventBody, ItemState, Source, normalize};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::{Mutex, broadcast, mpsc, watch};

/// 본문 최대 크기(1 MiB).
pub const BODY_LIMIT: usize = 1024 * 1024;
/// SSE ping 간격.
pub const PING_EVERY: Duration = Duration::from_secs(15);
/// 사람에게 보이는 channel 없음 안내(계약 §7).
pub const NO_CHANNEL: &str =
    "channel 없음 — 세션을 --dangerously-load-development-channels server:gyeot 로 시작하세요";
/// `/v1/say` 글 최대 길이(§10.1).
pub const SAY_MAX: usize = 32_000;
/// `/v1/channel/reply` 글 최대 길이(§10.1).
pub const REPLY_MAX: usize = 16_000;
/// 결정 질문 최대 길이(§10.1).
pub const QUESTION_MAX: usize = 300;
/// 결정 선택지 최대 길이(§10.1).
pub const OPTION_MAX: usize = 60;
/// 발급하지 않은 `chat_id`로 답했을 때(계약 §12.4).
pub const NOT_ISSUED: &str = "발급하지 않은 chat_id";
/// 세션 열쇠로 토큰 전용 경로를 부를 때(계약 §7, G-031).
pub const KEY_NOT_ALLOWED: &str = "이 열쇠로는 쓸 수 없어요";
/// 토큰도 살아 있는 세션 열쇠도 아닐 때(계약 §7).
pub const UNAUTHORIZED: &str = "토큰이나 세션 열쇠가 없거나 맞지 않아요";
/// 부트 코드가 틀렸거나 지났거나 이미 썼을 때(계약 §12.1).
pub const BAD_BOOT_CODE: &str = "열기 코드가 틀렸거나, 120초가 지났거나, 이미 쓴 코드예요. 트레이의 곁 열기나 gyeotd --open으로 다시 여세요";
const RECEIVED_MAX_AGE_MS: i64 = 30 * 86_400_000;

/// 처리기들이 함께 쓰는 상태.
#[derive(Debug)]
pub struct Shared {
    /// 실제로 묶인 포트(Host·Origin 검사용).
    pub port: u16,
    /// 토큰.
    pub token: String,
    /// 얼굴 폴더.
    pub face_dir: Option<PathBuf>,
    /// 엔진 허브.
    pub hub: Mutex<Hub>,
    /// channel 연결.
    pub channels: StdMutex<Channels>,
    /// 결정 id → 그 결정을 연 channel.
    pub decisions: StdMutex<HashMap<String, String>>,
    /// 열린 부트 코드(§12.1).
    pub boot: StdMutex<BootCodes>,
    /// 얼굴 세션(§12.1).
    pub sessions: StdMutex<Sessions>,
    /// 끝내기 신호.
    pub shutdown: watch::Receiver<bool>,
    chat_seq: AtomicU64,
}

impl Shared {
    /// 새 공유 상태.
    pub fn new(
        port: u16,
        token: String,
        face_dir: Option<PathBuf>,
        hub: Hub,
        sessions: Sessions,
        shutdown: watch::Receiver<bool>,
    ) -> Shared {
        Shared {
            port,
            token,
            face_dir,
            hub: Mutex::new(hub),
            channels: StdMutex::new(Channels::default()),
            decisions: StdMutex::new(HashMap::new()),
            boot: StdMutex::new(BootCodes::default()),
            sessions: StdMutex::new(sessions),
            shutdown,
            chat_seq: AtomicU64::new(1),
        }
    }

    fn boot_codes(&self) -> std::sync::MutexGuard<'_, BootCodes> {
        self.boot.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 세션 목록(끌 때 저장하려고 공개).
    pub fn sessions(&self) -> std::sync::MutexGuard<'_, Sessions> {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// `Authorization: Bearer <값>`이 누구인가(계약 §7). 토큰(상수 시간 비교)을 먼저 보고, 아니면
    /// 세션 열쇠(해시 대조, 맞으면 마지막 사용 시각 갱신). 쿠키·쿼리는 보지 않는다.
    fn caller(&self, headers: &HeaderMap) -> Option<Caller> {
        let v = bearer_token(headers)?;
        if ct_eq(v.as_bytes(), self.token.as_bytes()) {
            return Some(Caller::Token);
        }
        self.sessions()
            .validate(v, now_ms())
            .then_some(Caller::Session)
    }

    fn channels(&self) -> std::sync::MutexGuard<'_, Channels> {
        self.channels.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn decisions(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.decisions.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn new_chat_id(&self) -> String {
        format!("m{}", self.chat_seq.fetch_add(1, Ordering::Relaxed))
    }

    /// channel을 닫는다(`channel_close` 이벤트를 기록하고 연결을 지운다).
    pub async fn close_channel(&self, cid: &str) {
        let mut hub = self.hub.lock().await;
        if let Some(ch) = hub.engine.channel(cid) {
            let now = now_ms();
            let ev = gyeot_event(
                now,
                ch.session.clone(),
                Some(ch.cwd.clone()),
                EventBody::ChannelClose {
                    cid: ch.cid,
                    pid: ch.pid,
                    ppid: ch.ppid,
                    cwd: ch.cwd,
                },
            );
            hub.ingest(ev, now);
        }
        self.channels().remove(cid);
        self.decisions().retain(|_, c| c != cid);
    }
}

type St = Arc<Shared>;

/// 인증한 쪽.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Caller {
    /// 토큰: 모든 `/v1/*`.
    Token,
    /// 얼굴의 세션 열쇠: [`session_may_use`] 경로만.
    Session,
}

fn json_resp(status: StatusCode, v: Value) -> Response {
    (status, axum::Json(v)).into_response()
}

fn err(status: StatusCode, msg: &str) -> Response {
    json_resp(status, json!({ "ok": false, "error": msg }))
}

fn ok() -> Response {
    json_resp(StatusCode::OK, json!({ "ok": true }))
}

/// 본문을 읽지 못함(400).
struct BadBody(String);

impl IntoResponse for BadBody {
    fn into_response(self) -> Response {
        err(StatusCode::BAD_REQUEST, &self.0)
    }
}

fn parse_body<T: DeserializeOwned>(body: &Bytes) -> Result<T, BadBody> {
    serde_json::from_slice(body).map_err(|e| BadBody(format!("본문 JSON을 읽지 못했어요: {e}")))
}

// ───────────────────────── 보안 검사 ─────────────────────────

fn host_ok(host: Option<&str>, port: u16) -> bool {
    let Some(h) = host else {
        return false;
    };
    let h = h.trim().to_ascii_lowercase();
    h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}")
}

/// 검사할 Host: `Host` 머리가 있으면 그것만 본다(읽을 수 없으면 `None` → 421). 없을 때만 요청줄의 주소.
fn request_host<'a>(headers: &'a HeaderMap, uri: &'a Uri) -> Option<&'a str> {
    match headers.get(header::HOST) {
        Some(v) => v.to_str().ok(),
        None => uri.authority().map(|a| a.as_str()),
    }
}

/// 같은 출처만(계약 §7·§12.2). Tauri 출처도 받지 않는다.
fn origin_ok(origin: &str, port: u16) -> bool {
    origin == format!("http://127.0.0.1:{port}") || origin == format!("http://localhost:{port}")
}

/// 상수 시간 비교(길이는 숨기지 않는다. 토큰 길이는 공개된 64자다).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for (x, y) in a.iter().zip(b) {
        d |= x ^ y;
    }
    std::hint::black_box(d) == 0
}

/// `Authorization: Bearer <토큰>`의 토큰. 쿼리 토큰(`?t=`)은 받지 않는다(계약 §7).
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let v = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, tok) = v.trim().split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| tok.trim())
}

/// 인증 없이 받는 `/v1` 경로(계약 §7 표의 "없어도 됨"). 본문·쿼리 검사는 처리기가 한다.
fn is_public_v1(path: &str) -> bool {
    path == "/v1/hello" || path == "/v1/session"
}

/// 세션 열쇠로 쓸 수 있는 경로(계약 §7 표의 "세션: 예", G-031). 허용 목록이다: 여기 없는 `/v1/*`
/// (`/v1/boot`·`/v1/ingest/*`·`/v1/channel/*`·모르는 경로·인코딩한 경로)는 토큰만 쓴다.
pub fn session_may_use(path: &str) -> bool {
    matches!(
        path,
        "/v1/state" | "/v1/stream" | "/v1/focus" | "/v1/presence/listening" | "/v1/say"
    ) || [
        "/v1/items/",
        "/v1/decisions/",
        "/v1/proposals/",
        "/v1/adjustments/",
    ]
    .iter()
    .any(|p| path.starts_with(p))
}

/// Host(421) → Origin(403) → 인증(401) → 열쇠의 권한(403) 순으로 검사한다. CORS 헤더는 내지 않는다.
///
/// 인증은 `Authorization: Bearer` 하나다. 쿠키는 어떤 경로에서도 보지 않고, `Set-Cookie`도 내지 않는다
/// (G-030). 얼굴 쪽(`/`·`/assets/*`)과 `/health`·`/v1/hello`·`/v1/session`은 인증 없이 받는다.
async fn guard(State(st): State<St>, req: Request, next: Next) -> Response {
    match rejection(&st, &req) {
        None => next.run(req).await,
        Some(resp) => {
            // 거절도 본문을 다 읽은 뒤 답한다. 남긴 채 닫으면 Windows가 RST를 보내 클라이언트가
            // 거절 응답까지 잃는다(CI windows-latest, 2026-11-26). 한도를 넘으면 거기서 그친다.
            let _ = axum::body::to_bytes(req.into_body(), BODY_LIMIT).await;
            resp
        }
    }
}

fn rejection(st: &St, req: &Request) -> Option<Response> {
    if !host_ok(request_host(req.headers(), req.uri()), st.port) {
        return Some(err(StatusCode::MISDIRECTED_REQUEST, "Host가 맞지 않아요"));
    }
    if let Some(v) = req.headers().get(header::ORIGIN) {
        match v.to_str() {
            Ok(o) if origin_ok(o, st.port) => {}
            _ => return Some(err(StatusCode::FORBIDDEN, "허용하지 않는 Origin이에요")),
        }
    }
    let path = req.uri().path();
    if (path == "/v1" || path.starts_with("/v1/")) && !is_public_v1(path) {
        match st.caller(req.headers()) {
            Some(Caller::Token) => {}
            Some(Caller::Session) if session_may_use(path) => {}
            Some(Caller::Session) => return Some(err(StatusCode::FORBIDDEN, KEY_NOT_ALLOWED)),
            None => return Some(err(StatusCode::UNAUTHORIZED, UNAUTHORIZED)),
        }
    }
    None
}

/// 라우터.
pub fn router(st: St) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/v1/hello", get(hello))
        .route("/v1/boot", post(boot))
        .route("/v1/session", post(session))
        .route("/v1/ingest/{source}", post(ingest))
        .route("/v1/state", get(state))
        .route("/v1/stream", get(stream))
        .route("/v1/items/{id}/action", post(item_action))
        .route("/v1/focus", post(focus))
        .route("/v1/presence/listening", post(listening))
        .route("/v1/say", post(say))
        .route("/v1/decisions/{id}", post(decision_choice))
        .route("/v1/proposals/{id}", post(proposal))
        .route("/v1/adjustments/{kind}", post(adjustment))
        .route("/v1/channel/register", post(ch_register))
        .route("/v1/channel/stream", get(ch_stream))
        .route("/v1/channel/reply", post(ch_reply))
        .route("/v1/channel/decision", post(ch_decision))
        .route("/v1/channel/unregister", post(ch_unregister))
        .fallback(fallback)
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(middleware::from_fn_with_state(st.clone(), guard))
        .with_state(st)
}

// ───────────────────────── 처리기 ─────────────────────────

async fn health() -> Response {
    json_resp(
        StatusCode::OK,
        json!({ "ok": true, "version": gyeot_core::VERSION }),
    )
}

/// `GET /`·`/index.html`: 인증 없이 얼굴 `index.html` 그대로(비밀이 없다, 계약 §7). `?boot=`는 서버가
/// 읽지 않는다(얼굴 스크립트가 `POST /v1/session`으로 바꾼다, §12.1).
async fn index(State(st): State<St>) -> Response {
    face::index_response(st.face_dir.as_deref()).await
}

fn no_store(mut r: Response) -> Response {
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

/// `GET /v1/hello?n=<16–128자 hex>`(인증 없음, 계약 §13.1): 토큰과 실제로 묶인 포트·`n`으로 만든 증명.
/// 토큰을 가진 쪽이 토큰을 보내기 전에 이 서버가 곁 데몬인지 확인한다.
async fn hello(State(st): State<St>, uri: Uri) -> Response {
    let n = uri
        .query()
        .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("n=")))
        .filter(|n| auth::valid_nonce(n));
    match n {
        Some(n) => no_store(json_resp(
            StatusCode::OK,
            json!({ "v": 1, "proof": auth::hello_proof(&st.token, st.port, n) }),
        )),
        None => err(StatusCode::BAD_REQUEST, "n은 16–128자 hex여야 해요"),
    }
}

#[derive(Deserialize)]
struct CodeBody {
    code: String,
}

/// `POST /v1/session {"code"}`(인증 머리 없음, 계약 §12.1): 일회용 부트 코드를 세션 열쇠로 바꾼다.
/// 틀렸거나 지났거나 이미 쓴 코드는 `401`. 열쇠는 응답 본문으로만 준다(쿠키 없음).
async fn session(State(st): State<St>, body: Bytes) -> Response {
    let b: CodeBody = match parse_body(&body) {
        Ok(b) => b,
        Err(r) => return r.into_response(),
    };
    let now = now_ms();
    let redeemed = st.boot_codes().redeem(&b.code, now);
    if !redeemed {
        return err(StatusCode::UNAUTHORIZED, BAD_BOOT_CODE);
    }
    let created = st.sessions().create(now);
    match created {
        Ok((id, expires_ms)) => no_store(json_resp(
            StatusCode::OK,
            json!({ "session": id, "expires_ms": expires_ms }),
        )),
        Err(e) => {
            tracing::error!("세션을 만들지 못했어요: {e:#}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "세션을 만들지 못했어요")
        }
    }
}

/// `POST /v1/boot`(토큰만, G-031): 120초 동안 한 번 쓸 수 있는 얼굴 열기 코드.
///
/// 본문은 쓰지 않지만 끝까지 읽은 뒤 답한다(`_body`). 읽지 않은 본문을 남긴 채 닫으면 Windows가
/// RST를 보내 클라이언트가 응답을 잃는다(CI windows-latest, 2026-11-26).
async fn boot(State(st): State<St>, _body: axum::body::Bytes) -> Response {
    let now = now_ms();
    let issued = st.boot_codes().issue(now);
    match issued {
        Ok((code, expires_ms)) => {
            let url = format!("http://127.0.0.1:{}/?boot={code}", st.port);
            no_store(json_resp(
                StatusCode::OK,
                json!({ "code": code, "url": url, "expires_ms": expires_ms }),
            ))
        }
        Err(e) => {
            tracing::error!("부트 코드를 만들지 못했어요: {e:#}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "코드를 만들지 못했어요")
        }
    }
}

async fn fallback(State(st): State<St>, method: Method, uri: Uri) -> Response {
    let path = uri.path();
    if path == "/v1" || path.starts_with("/v1/") {
        return err(StatusCode::NOT_FOUND, "없는 API예요");
    }
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    face::static_response(st.face_dir.as_deref(), path).await
}

/// 심의 봉투 `{"payload", "pid_hint", "received_ms"}`를 벗긴다. 봉투가 아니면 본문 전체가 페이로드다.
pub fn unwrap_envelope(v: Value, now: i64) -> (Value, Option<u32>, i64) {
    let is_env = v.as_object().is_some_and(|o| {
        o.contains_key("payload") && (o.contains_key("received_ms") || o.contains_key("pid_hint"))
    });
    if !is_env {
        return (v, None, now);
    }
    let Value::Object(mut o) = v else {
        return (Value::Null, None, now);
    };
    let pid = o
        .get("pid_hint")
        .and_then(Value::as_u64)
        .and_then(|p| u32::try_from(p).ok());
    let recv = o
        .get("received_ms")
        .and_then(Value::as_i64)
        .map_or(now, |r| r.clamp(now - RECEIVED_MAX_AGE_MS, now));
    let payload = o.remove("payload").unwrap_or(Value::Null);
    (payload, pid, recv)
}

async fn ingest(State(st): State<St>, Path(source): Path<String>, body: Bytes) -> Response {
    let Some(src) = Source::parse(&source).filter(|s| s.is_ingestible()) else {
        return err(StatusCode::BAD_REQUEST, "알 수 없는 source예요");
    };
    let v: Value = match parse_body(&body) {
        Ok(v) => v,
        Err(r) => return r.into_response(),
    };
    let now = now_ms();
    let (payload, pid, recv) = unwrap_envelope(v, now);
    let events = normalize(src, &payload, recv, pid);
    let n = events.len();
    let mut hub = st.hub.lock().await;
    for ev in events {
        hub.ingest(ev, now);
    }
    json_resp(StatusCode::ACCEPTED, json!({ "accepted": n }))
}

async fn state(State(st): State<St>) -> Response {
    let s = st.hub.lock().await.snapshot_json(now_ms());
    let mut r = (StatusCode::OK, s).into_response();
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

fn act_error(e: &ActError) -> Response {
    let status = match e {
        ActError::UnknownItem | ActError::UnknownProposal | ActError::UnknownDecision => {
            StatusCode::NOT_FOUND
        }
        _ => StatusCode::BAD_REQUEST,
    };
    err(status, &e.to_string())
}

#[derive(Deserialize)]
struct ActionBody {
    action: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    minutes: Option<i64>,
}

async fn item_action(State(st): State<St>, Path(id): Path<String>, body: Bytes) -> Response {
    let b: ActionBody = match parse_body(&body) {
        Ok(b) => b,
        Err(r) => return r.into_response(),
    };
    let Some(action) = Action::parse(&b.action).filter(|a| a.is_item_action()) else {
        return err(
            StatusCode::BAD_REQUEST,
            "action은 open·done·dismiss·snooze 가운데 하나예요",
        );
    };
    let now = now_ms();
    let mut hub = st.hub.lock().await;
    match hub
        .engine
        .act(&id, action, b.reason.as_deref(), b.minutes, now)
    {
        Ok((ev, fx)) => {
            hub.record(ev, fx, now);
            ok()
        }
        Err(e) => act_error(&e),
    }
}

#[derive(Deserialize)]
struct FocusBody {
    on: bool,
    #[serde(default)]
    minutes: Option<i64>,
}

async fn focus(State(st): State<St>, body: Bytes) -> Response {
    let b: FocusBody = match parse_body(&body) {
        Ok(b) => b,
        Err(r) => return r.into_response(),
    };
    let now = now_ms();
    let mut hub = st.hub.lock().await;
    match hub.engine.set_focus(b.on, b.minutes, now) {
        Ok((ev, fx)) => {
            hub.record(ev, fx, now);
            let until = hub.engine.presence().focus_mode_until_ms;
            json_resp(StatusCode::OK, json!({ "ok": true, "until_ms": until }))
        }
        Err(e) => act_error(&e),
    }
}

#[derive(Deserialize)]
struct OnBody {
    on: bool,
}

async fn listening(State(st): State<St>, body: Bytes) -> Response {
    let b: OnBody = match parse_body(&body) {
        Ok(b) => b,
        Err(r) => return r.into_response(),
    };
    let now = now_ms();
    let mut hub = st.hub.lock().await;
    let (ev, fx) = hub.engine.set_listening(b.on, now);
    hub.record(ev, fx, now);
    ok()
}

/// 넘치면 자르고 자른 것을 밝힌다.
fn limit_text(s: &str, max: usize) -> String {
    const MARK: &str = "…(잘렸어요)";
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let keep = max.saturating_sub(MARK.chars().count());
    let mut out: String = s.chars().take(keep).collect();
    out.push_str(MARK);
    out
}

#[derive(Deserialize)]
struct SayBody {
    text: String,
    #[serde(default)]
    session: Option<String>,
}

async fn say(State(st): State<St>, body: Bytes) -> Response {
    let b: SayBody = match parse_body(&body) {
        Ok(b) => b,
        Err(r) => return r.into_response(),
    };
    if b.text.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "보낼 글이 비었어요");
    }
    let text = limit_text(&b.text, SAY_MAX);
    let target = {
        let hub = st.hub.lock().await;
        match b.session.as_deref().filter(|s| !s.is_empty()) {
            Some(s) if !hub.engine.has_session(s) => Err("그 세션을 찾지 못했어요"),
            Some(s) => hub
                .engine
                .channel_of_session(s)
                .map(|c| (s.to_owned(), c))
                .ok_or(NO_CHANNEL),
            None => hub.engine.latest_channel_session().ok_or(NO_CHANNEL),
        }
    };
    match target {
        Ok((session, cid)) => {
            let chat_id = st.new_chat_id();
            let msg = json!({ "chat_id": chat_id, "text": text, "kind": "say" }).to_string();
            let pushed = {
                let mut ch = st.channels();
                ch.push(&cid, msg) && ch.issue(&cid, &chat_id, now_ms())
            };
            if pushed {
                json_resp(
                    StatusCode::OK,
                    json!({ "delivered_to": session, "reason": "channel에 넘겼어요", "chat_id": chat_id }),
                )
            } else {
                json_resp(
                    StatusCode::OK,
                    json!({ "delivered_to": null, "reason": NO_CHANNEL }),
                )
            }
        }
        Err(reason) => json_resp(
            StatusCode::OK,
            json!({ "delivered_to": null, "reason": reason }),
        ),
    }
}

#[derive(Deserialize)]
struct ChoiceBody {
    choice: String,
}

async fn decision_choice(State(st): State<St>, Path(id): Path<String>, body: Bytes) -> Response {
    let b: ChoiceBody = match parse_body(&body) {
        Ok(b) => b,
        Err(r) => return r.into_response(),
    };
    let now = now_ms();
    let mut hub = st.hub.lock().await;
    let Some(info) = hub.engine.decision(&id) else {
        return err(StatusCode::NOT_FOUND, "그런 결정이 없어요");
    };
    if info.state == ItemState::Done {
        return json_resp(
            StatusCode::CONFLICT,
            json!({ "delivered_to": null, "reason": "이미 답한 결정이에요" }),
        );
    }
    if !info.options.iter().any(|o| o == &b.choice) {
        return err(StatusCode::BAD_REQUEST, "선택지에 없는 답이에요");
    }
    let msg = json!({ "chat_id": id, "text": format!("[선택] {}", b.choice), "kind": "choice" })
        .to_string();
    let via_session = hub.engine.channel_of_session(&info.session);
    let opener = st.decisions().get(&id).cloned();
    let delivered = {
        let mut ch = st.channels();
        [via_session, opener]
            .into_iter()
            .flatten()
            .any(|cid| ch.push(&cid, msg.clone()) && ch.issue(&cid, &id, now))
    };
    if !delivered {
        return json_resp(
            StatusCode::OK,
            json!({ "delivered_to": null, "reason": NO_CHANNEL }),
        );
    }
    if let Ok((ev, fx)) = hub.engine.decide(&id, now) {
        hub.record(ev, fx, now);
    }
    st.decisions().remove(&id);
    json_resp(StatusCode::OK, json!({ "delivered_to": info.session }))
}

#[derive(Deserialize)]
struct AcceptBody {
    accept: bool,
}

async fn proposal(State(st): State<St>, Path(id): Path<String>, body: Bytes) -> Response {
    let b: AcceptBody = match parse_body(&body) {
        Ok(b) => b,
        Err(r) => return r.into_response(),
    };
    let now = now_ms();
    let mut hub = st.hub.lock().await;
    match hub.engine.respond_proposal(&id, b.accept, now) {
        Ok((ev, fx)) => {
            hub.record(ev, fx, now);
            ok()
        }
        Err(e) => act_error(&e),
    }
}

#[derive(Deserialize)]
struct QuietBody {
    quiet: bool,
}

/// 받아들인 조정 되돌리기(계약 §5 "설정 화면에서 언제든 되돌린다"). 켜기는 제안으로만 한다.
async fn adjustment(State(st): State<St>, Path(kind): Path<String>, body: Bytes) -> Response {
    let b: QuietBody = match parse_body(&body) {
        Ok(b) => b,
        Err(r) => return r.into_response(),
    };
    if b.quiet {
        return err(
            StatusCode::BAD_REQUEST,
            "조용히 알리기는 제안을 받아들일 때만 켜져요",
        );
    }
    let now = now_ms();
    let mut hub = st.hub.lock().await;
    match hub.engine.act(&kind, Action::Unquiet, None, None, now) {
        Ok((ev, fx)) => {
            hub.record(ev, fx, now);
            ok()
        }
        Err(_) => err(StatusCode::NOT_FOUND, "그런 종류가 없어요"),
    }
}

// ───────────────────────── channel ─────────────────────────

#[derive(Deserialize)]
struct RegisterBody {
    pid: u32,
    ppid: u32,
    cwd: String,
}

async fn ch_register(State(st): State<St>, body: Bytes) -> Response {
    let b: RegisterBody = match parse_body(&body) {
        Ok(b) => b,
        Err(r) => return r.into_response(),
    };
    let cwd = truncate_chars(b.cwd.trim(), 4096);
    let cid = format!("ch_{}", random_hex(8));
    let now = now_ms();
    let mut hub = st.hub.lock().await;
    let ev = gyeot_event(
        now,
        None,
        Some(cwd.clone()),
        EventBody::ChannelOpen {
            cid: cid.clone(),
            pid: b.pid,
            ppid: b.ppid,
            cwd,
        },
    );
    hub.ingest(ev, now);
    let session = hub.engine.channel(&cid).and_then(|c| c.session);
    st.channels().register(&cid);
    json_resp(StatusCode::OK, json!({ "cid": cid, "session": session }))
}

#[derive(Deserialize)]
struct CidBody {
    cid: String,
}

async fn ch_unregister(State(st): State<St>, body: Bytes) -> Response {
    let b: CidBody = match parse_body(&body) {
        Ok(b) => b,
        Err(r) => return r.into_response(),
    };
    st.close_channel(&b.cid).await;
    ok()
}

/// `chat_id` 꼴: `[A-Za-z0-9_-]{1,64}`(§10.1).
pub fn valid_chat_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn known_channel(st: &Shared, hub: &Hub, cid: &str) -> bool {
    st.channels().contains(cid) && hub.engine.channel(cid).is_some()
}

#[derive(Deserialize)]
struct ReplyBody {
    cid: String,
    chat_id: String,
    text: String,
}

async fn ch_reply(State(st): State<St>, body: Bytes) -> Response {
    let b: ReplyBody = match parse_body(&body) {
        Ok(b) => b,
        Err(r) => return r.into_response(),
    };
    let hub = st.hub.lock().await;
    if !known_channel(&st, &hub, &b.cid) {
        return err(StatusCode::NOT_FOUND, "모르는 channel이에요");
    }
    if !valid_chat_id(&b.chat_id) {
        return err(
            StatusCode::BAD_REQUEST,
            "chat_id는 [A-Za-z0-9_-] 1–64자여야 해요",
        );
    }
    if b.text.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "답 글이 비었어요");
    }
    if b.text.chars().count() > REPLY_MAX {
        return err(StatusCode::BAD_REQUEST, "답 글은 16,000자까지예요");
    }
    // 그 channel에 데몬이 발급한 chat_id만(계약 §12.4).
    if !st.channels().was_issued(&b.cid, &b.chat_id, now_ms()) {
        return err(StatusCode::BAD_REQUEST, NOT_ISSUED);
    }
    let session = hub.engine.channel(&b.cid).and_then(|c| c.session);
    let project = session
        .as_deref()
        .and_then(|s| hub.engine.session_project(s));
    hub.send(
        "reply",
        &json!({
            "session": session,
            "project": project,
            "chat_id": b.chat_id,
            "text": mask(&b.text),
            "at_ms": now_ms(),
        }),
    );
    ok()
}

#[derive(Deserialize)]
struct DecisionBody {
    cid: String,
    question: String,
    options: Vec<String>,
}

async fn ch_decision(State(st): State<St>, body: Bytes) -> Response {
    let b: DecisionBody = match parse_body(&body) {
        Ok(b) => b,
        Err(r) => return r.into_response(),
    };
    let now = now_ms();
    let mut hub = st.hub.lock().await;
    if !known_channel(&st, &hub, &b.cid) {
        return err(StatusCode::NOT_FOUND, "모르는 channel이에요");
    }
    let question = b.question.trim();
    if question.is_empty() || question.chars().count() > QUESTION_MAX {
        return err(StatusCode::BAD_REQUEST, "질문은 1–300자여야 해요");
    }
    if !(2..=4).contains(&b.options.len()) {
        return err(StatusCode::BAD_REQUEST, "선택지는 2–4개여야 해요");
    }
    let options: Vec<String> = b.options.iter().map(|o| o.trim().to_owned()).collect();
    if options
        .iter()
        .any(|o| o.is_empty() || o.chars().count() > OPTION_MAX)
    {
        return err(StatusCode::BAD_REQUEST, "선택지는 각각 1–60자여야 해요");
    }
    let mut uniq = options.clone();
    uniq.sort();
    uniq.dedup();
    if uniq.len() != options.len() {
        return err(StatusCode::BAD_REQUEST, "선택지가 겹쳐요");
    }
    let Some(ch) = hub.engine.channel(&b.cid) else {
        return err(StatusCode::NOT_FOUND, "모르는 channel이에요");
    };
    let decision_id = format!("d{}", random_hex(6));
    let ev = gyeot_event(
        now,
        ch.session.clone(),
        Some(ch.cwd.clone()),
        EventBody::DecisionOpen {
            decision_id: decision_id.clone(),
            question: mask(question),
            options: options.iter().map(|o| mask(o)).collect(),
        },
    );
    hub.ingest(ev, now);
    st.decisions().insert(decision_id.clone(), b.cid.clone());
    st.channels().issue(&b.cid, &decision_id, now);
    json_resp(StatusCode::OK, json!({ "decision_id": decision_id }))
}

// ───────────────────────── SSE ─────────────────────────

type SseItem = Result<SseEvent, Infallible>;

fn sse_from(rx: mpsc::Receiver<SseItem>) -> Sse<impl Stream<Item = SseItem>> {
    let mut rx = rx;
    Sse::new(futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx)))
}

fn ping_timer() -> tokio::time::Interval {
    let mut iv = tokio::time::interval_at(tokio::time::Instant::now() + PING_EVERY, PING_EVERY);
    iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    iv
}

fn ev_msg(event: &str, data: &str) -> SseItem {
    Ok(SseEvent::default().event(event).data(data))
}

async fn stream(State(st): State<St>) -> Response {
    let (mut rx, snap) = {
        let mut hub = st.hub.lock().await;
        let rx = hub.subscribe();
        (rx, hub.snapshot_json(now_ms()))
    };
    let (tx, out) = mpsc::channel::<SseItem>(256);
    let mut shutdown = st.shutdown.clone();
    let st2 = st.clone();
    tokio::spawn(async move {
        if tx.send(ev_msg("snapshot", &snap)).await.is_err() {
            return;
        }
        let mut ping = ping_timer();
        loop {
            tokio::select! {
                m = rx.recv() => {
                    let item = match m {
                        Ok(m) => ev_msg(m.event, &m.data),
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            let s = st2.hub.lock().await.snapshot_json(now_ms());
                            ev_msg("snapshot", &s)
                        }
                        Err(broadcast::error::RecvError::Closed) => return,
                    };
                    if tx.send(item).await.is_err() {
                        return;
                    }
                }
                _ = ping.tick() => {
                    if tx.send(Ok(SseEvent::default().comment("ping"))).await.is_err() {
                        return;
                    }
                }
                () = tx.closed() => return,
                _ = shutdown.changed() => return,
            }
        }
    });
    sse_from(out).into_response()
}

#[derive(Deserialize)]
struct StreamQuery {
    cid: String,
}

async fn ch_stream(State(st): State<St>, Query(q): Query<StreamQuery>) -> Response {
    let attached = {
        let hub = st.hub.lock().await;
        if hub.engine.channel(&q.cid).is_none() {
            None
        } else {
            st.channels().attach(&q.cid)
        }
    };
    let Some((generation, notify)) = attached else {
        return err(StatusCode::NOT_FOUND, "모르는 channel이에요");
    };
    let (tx, out) = mpsc::channel::<SseItem>(64);
    let mut shutdown = st.shutdown.clone();
    let cid = q.cid.clone();
    tokio::spawn(async move {
        let mut ping = ping_timer();
        loop {
            let notified = notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            // 쌓인 메시지를 먼저 보낸다.
            loop {
                let next = st.channels().next_for(&cid, generation);
                match next {
                    Err(_) => return,
                    Ok(None) => break,
                    Ok(Some(m)) => {
                        if tx.send(ev_msg("message", &m)).await.is_err() {
                            let mut ch = st.channels();
                            ch.unshift(&cid, m);
                            ch.detach(&cid, generation);
                            return;
                        }
                    }
                }
            }
            tokio::select! {
                () = &mut notified => {}
                _ = ping.tick() => {
                    if tx.send(Ok(SseEvent::default().comment("ping"))).await.is_err() {
                        st.channels().detach(&cid, generation);
                        return;
                    }
                }
                () = tx.closed() => {
                    st.channels().detach(&cid, generation);
                    return;
                }
                _ = shutdown.changed() => return,
            }
        }
    });
    sse_from(out).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_and_origin_rules() {
        assert!(host_ok(Some("127.0.0.1:47321"), 47321));
        assert!(host_ok(Some("LOCALHOST:47321"), 47321));
        assert!(!host_ok(Some("127.0.0.1"), 47321));
        assert!(!host_ok(Some("evil.com:47321"), 47321));
        assert!(!host_ok(Some("127.0.0.1:1"), 47321));
        assert!(!host_ok(None, 47321));
        assert!(origin_ok("http://127.0.0.1:47321", 47321));
        assert!(origin_ok("http://localhost:47321", 47321));
        assert!(!origin_ok("tauri://localhost", 47321));
        assert!(!origin_ok("http://tauri.localhost", 47321));
        assert!(!origin_ok("https://tauri.localhost", 47321));
        assert!(!origin_ok("https://127.0.0.1:47321", 47321));
        assert!(!origin_ok("null", 47321));
        assert!(!origin_ok("http://evil.com", 47321));
    }

    #[test]
    fn host_header_is_not_bypassed() {
        let uri: Uri = "http://127.0.0.1:47321/health".parse().unwrap();
        let mut h = HeaderMap::new();
        // 머리가 없을 때만 요청줄의 주소
        assert_eq!(request_host(&h, &uri), Some("127.0.0.1:47321"));
        h.insert(
            header::HOST,
            HeaderValue::from_bytes(b"\xff127.0.0.1:47321").unwrap(),
        );
        assert_eq!(request_host(&h, &uri), None);
        h.insert(header::HOST, HeaderValue::from_static("evil.example"));
        assert_eq!(request_host(&h, &uri), Some("evil.example"));
    }

    /// G-031: 세션 열쇠는 계약 §7 표의 "세션: 예" 경로만(허용 목록).
    #[test]
    fn session_route_allowlist() {
        for ok in [
            "/v1/state",
            "/v1/stream",
            "/v1/items/i1/action",
            "/v1/focus",
            "/v1/presence/listening",
            "/v1/say",
            "/v1/decisions/d1",
            "/v1/proposals/p1",
            "/v1/adjustments/unread",
        ] {
            assert!(session_may_use(ok), "{ok}");
        }
        for no in [
            "/v1/boot",
            "/v1/ingest/claude-code",
            "/v1/channel/register",
            "/v1/channel/stream",
            "/v1/channel/reply",
            "/v1/channel/decision",
            "/v1/channel/unregister",
            "/v1",
            "/v1/",
            "/v1/nope",
            "/v1//boot",
            "/v1/%62oot",
            "/v1/state/x",
            "/v1/sayx",
            "/v1/items",
        ] {
            assert!(!session_may_use(no), "{no}");
        }
        assert!(is_public_v1("/v1/hello") && is_public_v1("/v1/session"));
        assert!(!is_public_v1("/v1/hello/") && !is_public_v1("/v1/sessions"));
    }

    #[test]
    fn constant_time_compare() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn token_sources() {
        let mut h = HeaderMap::new();
        h.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("bearer  xyz "),
        );
        assert_eq!(bearer_token(&h), Some("xyz"));
        h.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic xyz"));
        assert_eq!(bearer_token(&h), None);
        assert_eq!(bearer_token(&HeaderMap::new()), None);
    }

    #[test]
    fn envelope_unwrapping() {
        let now = 1_000_000_000_000;
        let (p, pid, r) = unwrap_envelope(
            json!({"payload": {"x": 1}, "pid_hint": 42, "received_ms": now - 5}),
            now,
        );
        assert_eq!(p, json!({"x": 1}));
        assert_eq!(pid, Some(42));
        assert_eq!(r, now - 5);
        // 미래·아주 옛날 시각은 묶는다
        assert_eq!(
            unwrap_envelope(json!({"payload": 1, "received_ms": now + 99}), now).2,
            now
        );
        assert_eq!(
            unwrap_envelope(json!({"payload": 1, "received_ms": 0}), now).2,
            now - RECEIVED_MAX_AGE_MS
        );
        // 봉투가 아니면 그대로
        let raw = json!({"hook_event_name": "Stop", "payload": "x"});
        assert_eq!(unwrap_envelope(raw.clone(), now), (raw, None, now));
    }

    #[test]
    fn chat_ids_and_limits() {
        assert!(valid_chat_id("m17"));
        assert!(valid_chat_id("d_ab-12"));
        assert!(valid_chat_id(&"a".repeat(64)));
        assert!(!valid_chat_id(&"a".repeat(65)));
        assert!(!valid_chat_id(""));
        assert!(!valid_chat_id("a b"));
        assert!(!valid_chat_id("한"));
        assert_eq!(limit_text("abc", 3), "abc");
        let long = "가".repeat(40);
        let cut = limit_text(&long, 20);
        assert_eq!(cut.chars().count(), 20);
        assert!(cut.ends_with("…(잘렸어요)"));
    }
}
