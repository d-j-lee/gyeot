//! # gyeot-server
//!
//! 곁 데몬의 로컬 HTTP·SSE 서버(`docs/contracts.md` §7·§8).
//!
//! ```no_run
//! # async fn run() -> anyhow::Result<()> {
//! let cfg = gyeot_server::ServerConfig::from_env()?;
//! gyeot_server::serve(cfg, async { let _ = tokio::signal::ctrl_c().await; }).await
//! # }
//! ```

pub mod auth;
pub mod config;
pub mod face;
pub mod http;
pub mod hub;
pub mod store;
pub mod tail;

pub use config::{DEFAULT_PORT, ServerConfig, TailConfig, default_face_dir, resolve_home_from_env};

use anyhow::{Context, Result};
use gyeot_core::{Engine, EventBody, normalize};
use http::Shared;
use hub::{Hub, gyeot_event};
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use store::{LogSink, now_ms};
use tokio::sync::{broadcast, watch};

/// 다시 뜰 때 재생하는 기간.
pub const REPLAY_MS: i64 = 48 * 3_600_000;
/// 엔진 tick 간격.
pub const TICK_EVERY: Duration = Duration::from_secs(1);
/// 스풀 비우기 간격.
pub const SPOOL_EVERY: Duration = Duration::from_secs(30);
/// 하네스 로그 따라 읽기 간격.
pub const TAIL_EVERY: Duration = Duration::from_secs(2);
/// channel 스트림이 끊긴 뒤 닫기까지 기다리는 시간(§10.1).
pub const CHANNEL_GRACE: Duration = Duration::from_secs(10);
/// 등록한 뒤 스트림이 한 번도 붙지 않으면 닫기까지 기다리는 시간.
pub const CHANNEL_NEVER_GRACE: Duration = Duration::from_secs(60);
const LEARN_SAVE_EVERY: Duration = Duration::from_secs(5);
const RETENTION_EVERY: Duration = Duration::from_secs(3600);
const OFFSET_EVERY: Duration = Duration::from_secs(60);
const SPOOL_BATCH: usize = 5000;

/// 데이터 폴더를 만들고 토큰을 준비한다(`gyeotd --init`). 토큰을 돌려준다.
pub fn init_home(cfg: &ServerConfig) -> Result<String> {
    store::ensure_dirs(&cfg.home)?;
    store::ensure_token(&cfg.token_path())
}

/// 묶인 서버. [`Server::run`]으로 돌린다.
#[derive(Debug)]
pub struct Server {
    listener: tokio::net::TcpListener,
    addr: SocketAddr,
    cfg: ServerConfig,
    shared: Arc<Shared>,
    shutdown_tx: watch::Sender<bool>,
    log: LogSink,
    log_thread: std::thread::JoinHandle<()>,
}

/// 포트에 묶고 상태를 되살린다(learn.json → 최근 48시간 재생 → 남은 channel 닫기 → 스풀 비우기 → tick).
pub async fn bind(cfg: ServerConfig) -> Result<Server> {
    store::ensure_dirs(&cfg.home)?;
    let token = store::ensure_token(&cfg.token_path())?;
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, cfg.port))
        .await
        .with_context(|| {
            format!(
                "127.0.0.1:{}에 묶지 못했어요. 곁 데몬이 이미 돌고 있거나 다른 프로그램이 그 포트를 쓰고 있어요",
                cfg.port
            )
        })?;
    let addr = listener.local_addr()?;
    let (log, log_thread) = store::spawn_log_writer(cfg.events_dir())?;
    let (tx, _) = broadcast::channel(1024);

    let cfg2 = cfg.clone();
    let log2 = log.clone();
    let tx2 = tx.clone();
    let hub = tokio::task::spawn_blocking(move || restore(&cfg2, log2, tx2))
        .await
        .context("상태를 되살리지 못했어요")??;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let sessions_path = cfg.sessions_path();
    let sessions =
        tokio::task::spawn_blocking(move || auth::Sessions::load(&sessions_path, now_ms()))
            .await
            .context("세션을 읽지 못했어요")?;
    let shared = Arc::new(Shared::new(
        addr.port(),
        token,
        cfg.face_dir.clone(),
        hub,
        sessions,
        shutdown_rx,
    ));
    Ok(Server {
        listener,
        addr,
        cfg,
        shared,
        shutdown_tx,
        log,
        log_thread,
    })
}

fn restore(
    cfg: &ServerConfig,
    log: LogSink,
    tx: broadcast::Sender<Arc<hub::SseMsg>>,
) -> Result<Hub> {
    let now = now_ms();
    let removed = store::cleanup_retention(&cfg.events_dir(), now, cfg.retention_days);
    if removed > 0 {
        tracing::info!("보관 기간이 지난 이벤트 로그 {removed}개를 지웠어요");
    }
    let learn = store::load_learn(&cfg.learn_path());
    let mut engine = Engine::with_learn(cfg.policy(), learn);
    let (events, bad) = store::read_replay(&cfg.events_dir(), now - REPLAY_MS);
    if bad > 0 {
        tracing::warn!("이벤트 로그에서 읽지 못한 줄 {bad}개를 건너뛰었어요");
    }
    let n = events.len();
    for ev in &events {
        engine.apply(ev);
    }
    tracing::info!("이벤트 {n}개를 재생했어요");
    let mut hub = Hub::new(engine, Some(log), tx);
    // 재생으로 되살아난 channel은 이미 끊긴 연결이다. 다시 등록하게 닫는다.
    for ch in hub.engine.channels() {
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
    drain_spool_into(cfg, &mut hub, now);
    hub.tick(now);
    Ok(hub)
}

fn drain_spool_into(cfg: &ServerConfig, hub: &mut Hub, now: i64) -> usize {
    let entries = store::drain_spool(&cfg.spool_dir(), SPOOL_BATCH);
    let mut n = 0;
    for e in entries {
        let (payload, pid, recv) = http::unwrap_envelope(
            serde_json::json!({
                "payload": e.payload,
                "pid_hint": e.pid_hint,
                "received_ms": e.received_ms.unwrap_or(now),
            }),
            now,
        );
        for ev in normalize(e.source, &payload, recv, pid) {
            hub.ingest(ev, now);
            n += 1;
        }
    }
    if n > 0 {
        tracing::info!("스풀에서 이벤트 {n}개를 받았어요");
    }
    n
}

impl Server {
    /// 실제로 묶인 주소.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// 토큰.
    pub fn token(&self) -> &str {
        &self.shared.token
    }

    /// `http://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }

    /// `shutdown`이 끝날 때까지 돌고, 끝나면 로그·학습 기록·따라 읽기 위치를 저장한다.
    pub async fn run<F>(self, shutdown: F) -> Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let Server {
            listener,
            addr,
            cfg,
            shared,
            shutdown_tx,
            log,
            log_thread,
        } = self;
        tracing::info!("곁 데몬이 http://{addr} 에서 기다려요");
        let cfg = Arc::new(cfg);
        let mut tasks = vec![
            tokio::spawn(tick_loop(shared.clone())),
            tokio::spawn(spool_loop(shared.clone(), cfg.clone())),
            tokio::spawn(tail_loop(shared.clone(), cfg.clone())),
            tokio::spawn(learn_loop(shared.clone(), cfg.clone())),
            tokio::spawn(retention_loop(shared.clone(), cfg.clone())),
            tokio::spawn(channel_sweep_loop(shared.clone())),
        ];
        if cfg.utc_offset_minutes.is_none() {
            tasks.push(tokio::spawn(offset_loop(shared.clone())));
        }

        let app = http::router(shared.clone());
        let stop_tx = shutdown_tx.clone();
        let mut stop_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            shutdown.await;
            let _ = stop_tx.send(true);
        });
        let served = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = stop_rx.wait_for(|v| *v).await;
            })
            .await;
        let _ = shutdown_tx.send(true);
        for t in tasks {
            let _ = t.await;
        }
        save_learn(&shared, &cfg).await;
        {
            let sh = shared.clone();
            let _ = tokio::task::spawn_blocking(move || sh.sessions().flush()).await;
        }
        drop(shared);
        let log2 = log.clone();
        let _ = tokio::task::spawn_blocking(move || log2.flush_blocking()).await;
        drop(log);
        let _ = tokio::task::spawn_blocking(move || log_thread.join()).await;
        tracing::info!("곁 데몬을 멈췄어요");
        served.context("HTTP 서버가 멈췄어요")
    }
}

/// 묶고 `shutdown`이 끝날 때까지 돈다.
pub async fn serve<F>(cfg: ServerConfig, shutdown: F) -> Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    bind(cfg).await?.run(shutdown).await
}

macro_rules! every {
    ($shared:expr, $period:expr, $body:block) => {{
        let mut stop = $shared.shutdown.clone();
        let mut iv = tokio::time::interval($period);
        iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        iv.tick().await;
        loop {
            tokio::select! {
                _ = iv.tick() => $body,
                r = stop.changed() => {
                    if r.is_err() || *stop.borrow() {
                        break;
                    }
                }
            }
        }
    }};
}

async fn tick_loop(shared: Arc<Shared>) {
    every!(shared, TICK_EVERY, {
        shared.hub.lock().await.tick(now_ms());
    });
}

async fn spool_loop(shared: Arc<Shared>, cfg: Arc<ServerConfig>) {
    every!(shared, SPOOL_EVERY, {
        let dir = cfg.spool_dir();
        let entries = tokio::task::spawn_blocking(move || store::drain_spool(&dir, SPOOL_BATCH))
            .await
            .unwrap_or_default();
        if !entries.is_empty() {
            let now = now_ms();
            let mut hub = shared.hub.lock().await;
            for e in entries {
                let recv = e.received_ms.unwrap_or(now);
                let (payload, pid, recv) = http::unwrap_envelope(
                    serde_json::json!({"payload": e.payload, "pid_hint": e.pid_hint, "received_ms": recv}),
                    now,
                );
                for ev in normalize(e.source, &payload, recv, pid) {
                    hub.ingest(ev, now);
                }
            }
        }
    });
}

fn load_tail_state(path: &PathBuf) -> tail::TailState {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

async fn tail_loop(shared: Arc<Shared>, cfg: Arc<ServerConfig>) {
    if cfg.tails.is_empty() {
        return;
    }
    let path = cfg.tail_state_path();
    let tails = cfg.tails.clone();
    let mut st = {
        let (p, t) = (path.clone(), tails.clone());
        tokio::task::spawn_blocking(move || {
            let mut s = load_tail_state(&p);
            tail::init_state(&t, &mut s);
            if let Ok(b) = serde_json::to_vec(&s) {
                let _ = store::write_private(&p, &b);
            }
            s
        })
        .await
        .unwrap_or_default()
    };
    every!(shared, TAIL_EVERY, {
        let (t, s) = (tails.clone(), st.clone());
        let res = tokio::task::spawn_blocking(move || {
            let mut s = s;
            let (lines, changed) = tail::poll(&t, &mut s);
            (lines, changed, s)
        })
        .await;
        if let Ok((lines, changed, s)) = res {
            if !lines.is_empty() {
                let now = now_ms();
                let mut hub = shared.hub.lock().await;
                for l in lines {
                    for ev in normalize(l.source, &l.value, now, None) {
                        hub.ingest(ev, now);
                    }
                }
            }
            if changed {
                let p = path.clone();
                let snapshot = s.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    if let Ok(b) = serde_json::to_vec(&snapshot) {
                        let _ = store::write_private(&p, &b);
                    }
                })
                .await;
            }
            st = s;
        }
    });
}

async fn save_learn(shared: &Shared, cfg: &ServerConfig) {
    let snapshot = {
        let mut hub = shared.hub.lock().await;
        let rev = hub.engine.learn_revision();
        if rev == hub.learn_saved_rev && cfg.learn_path().exists() {
            return;
        }
        hub.learn_saved_rev = rev;
        hub.engine.learn().clone()
    };
    let path = cfg.learn_path();
    let r = tokio::task::spawn_blocking(move || store::save_learn(&path, &snapshot)).await;
    if let Ok(Err(e)) = r {
        tracing::error!("learn.json을 저장하지 못했어요: {e:#}");
    }
}

async fn learn_loop(shared: Arc<Shared>, cfg: Arc<ServerConfig>) {
    every!(shared, LEARN_SAVE_EVERY, {
        save_learn(&shared, &cfg).await;
    });
}

async fn retention_loop(shared: Arc<Shared>, cfg: Arc<ServerConfig>) {
    every!(shared, RETENTION_EVERY, {
        let dir = cfg.events_dir();
        let days = cfg.retention_days;
        let _ = tokio::task::spawn_blocking(move || store::cleanup_retention(&dir, now_ms(), days))
            .await;
    });
}

async fn offset_loop(shared: Arc<Shared>) {
    every!(shared, OFFSET_EVERY, {
        let off = config::local_offset_minutes();
        let mut hub = shared.hub.lock().await;
        if hub.engine.policy().utc_offset_minutes != off {
            tracing::info!("현지 시간대 오프셋이 {off}분으로 바뀌었어요");
            hub.engine.set_utc_offset_minutes(off);
        }
    });
}

async fn channel_sweep_loop(shared: Arc<Shared>) {
    every!(shared, Duration::from_secs(1), {
        let expired = shared
            .channels
            .lock()
            .map(|c| c.expired(CHANNEL_GRACE, CHANNEL_NEVER_GRACE))
            .unwrap_or_default();
        for cid in expired {
            tracing::info!("channel {cid}의 스트림이 돌아오지 않아 닫았어요");
            shared.close_channel(&cid).await;
        }
    });
}
