//! 엔진을 감싸는 허브: 이벤트 적용 → 로그 → SSE 방송을 한 잠금 안에서 차례로 한다.
//! 그리고 channel 연결(메시지 대기열·스트림 세대)을 관리한다.

use crate::store::LogSink;
use gyeot_core::{Effect, Engine, Event, EventBody, Source};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Notify, broadcast};

/// SSE 메시지 하나(이벤트 이름 + JSON 글).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseMsg {
    /// 이벤트 이름.
    pub event: &'static str,
    /// `data` JSON.
    pub data: String,
}

/// 엔진·로그·방송.
#[derive(Debug)]
pub struct Hub {
    /// 엔진.
    pub engine: Engine,
    seq: u64,
    log: Option<LogSink>,
    tx: broadcast::Sender<Arc<SseMsg>>,
    /// 마지막으로 저장한 학습 기록 판.
    pub learn_saved_rev: u64,
}

impl Hub {
    /// 새 허브. `log`가 없으면 기록하지 않는다(재생 중).
    pub fn new(engine: Engine, log: Option<LogSink>, tx: broadcast::Sender<Arc<SseMsg>>) -> Hub {
        Hub {
            engine,
            seq: 0,
            log,
            tx,
            learn_saved_rev: 0,
        }
    }

    /// 기록 손잡이를 단다.
    pub fn set_log(&mut self, log: LogSink) {
        self.log = Some(log);
    }

    /// 방송 구독(스냅샷을 만들기 전에 불러야 빠짐이 없다).
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<SseMsg>> {
        self.tx.subscribe()
    }

    fn stamp(&mut self, ev: &mut Event) {
        ev.id = format!("{}-{}", ev.ts_ms, self.seq);
        self.seq += 1;
    }

    /// 이벤트를 적용하고 기록하고 방송한다.
    pub fn ingest(&mut self, mut ev: Event, now_ms: i64) {
        self.stamp(&mut ev);
        let fx = self.engine.apply(&ev);
        if let Some(log) = &self.log {
            log.append(&ev, now_ms);
        }
        self.broadcast(fx, now_ms);
    }

    /// 엔진이 이미 적용한 이벤트(`act` 결과)를 기록하고 효과를 방송한다.
    pub fn record(&mut self, mut ev: Event, fx: Vec<Effect>, now_ms: i64) {
        self.stamp(&mut ev);
        if let Some(log) = &self.log {
            log.append(&ev, now_ms);
        }
        self.broadcast(fx, now_ms);
    }

    /// 시간을 흘린다.
    pub fn tick(&mut self, now_ms: i64) {
        let fx = self.engine.tick(now_ms);
        self.broadcast(fx, now_ms);
    }

    /// 스냅샷 JSON.
    pub fn snapshot_json(&mut self, now_ms: i64) -> String {
        let fx = self.engine.tick(now_ms);
        self.broadcast(fx, now_ms);
        serde_json::to_string(&self.engine.snapshot(now_ms)).unwrap_or_else(|_| "{}".into())
    }

    /// 아무 SSE 메시지나 얼굴에 보낸다(`reply` 등).
    pub fn send(&self, event: &'static str, data: &Value) {
        let _ = self.tx.send(Arc::new(SseMsg {
            event,
            data: data.to_string(),
        }));
    }

    fn broadcast(&mut self, fx: Vec<Effect>, now_ms: i64) {
        for e in fx {
            let msg = match e {
                Effect::ItemUpsert(item) => SseMsg {
                    event: "item",
                    data: serde_json::to_string(&item).unwrap_or_default(),
                },
                Effect::ItemRemoved { id } => SseMsg {
                    event: "item_removed",
                    data: json!({ "id": id }).to_string(),
                },
                Effect::Presence(p) => SseMsg {
                    event: "presence",
                    data: serde_json::to_string(&p).unwrap_or_default(),
                },
                Effect::Deliver(d) => SseMsg {
                    event: "deliver",
                    data: serde_json::to_string(&d).unwrap_or_default(),
                },
                Effect::Proposal(p) => SseMsg {
                    event: "proposal",
                    data: serde_json::to_string(&p).unwrap_or_default(),
                },
                Effect::Sessions(s) => SseMsg {
                    event: "sessions",
                    data: serde_json::to_string(&s).unwrap_or_default(),
                },
                Effect::Resync => SseMsg {
                    event: "snapshot",
                    data: serde_json::to_string(&self.engine.snapshot(now_ms)).unwrap_or_default(),
                },
            };
            let _ = self.tx.send(Arc::new(msg));
        }
    }
}

/// 곁이 만드는 이벤트(source `gyeot`).
pub fn gyeot_event(
    ts_ms: i64,
    session: Option<String>,
    cwd: Option<String>,
    body: EventBody,
) -> Event {
    Event {
        id: String::new(),
        ts_ms,
        source: Source::Gyeot,
        agent: None,
        agent_id: None,
        session,
        cwd,
        pid_hint: None,
        raw_event: None,
        body,
    }
}

// ───────────────────────── channel 연결 ─────────────────────────

/// channel 대기열에 쌓아 두는 최대 메시지 수.
const QUEUE_MAX: usize = 200;
/// channel마다 기억하는 발급한 `chat_id` 수(계약 §12.4).
pub const ISSUED_MAX: usize = 256;
/// 발급한 `chat_id`로 답할 수 있는 시간(계약 §12.4).
pub const ISSUED_TTL_MS: i64 = 24 * 3_600_000;

/// 데몬이 한 channel에 발급한 `chat_id`(say의 `m<n>`, 결정의 `d…`)와 발급 시각.
#[derive(Debug, Default)]
pub struct IssuedIds {
    ids: VecDeque<(String, i64)>,
}

impl IssuedIds {
    /// 발급을 기억한다. 같은 id면 시각만 새로 한다. 넘치면 가장 오래된 것부터 잊는다.
    pub fn issue(&mut self, chat_id: &str, now_ms: i64) {
        self.ids.retain(|(id, _)| id != chat_id);
        self.ids.push_back((chat_id.to_owned(), now_ms));
        while self.ids.len() > ISSUED_MAX {
            self.ids.pop_front();
        }
    }

    /// 24시간 안에 발급한 id인가.
    pub fn contains(&self, chat_id: &str, now_ms: i64) -> bool {
        self.ids
            .iter()
            .any(|(id, at)| id == chat_id && now_ms - *at <= ISSUED_TTL_MS)
    }
}

/// channel 하나의 연결 상태.
#[derive(Debug)]
pub struct ChannelConn {
    /// 보낼 메시지(JSON 글).
    pub queue: VecDeque<String>,
    /// 새 메시지·세대 변화 알림.
    pub notify: Arc<Notify>,
    /// 스트림 세대. 새 스트림이 붙거나 닫히면 늘어난다.
    pub generation: u64,
    /// 지금 붙어 있는 스트림이 있는가.
    pub connected: bool,
    /// 스트림이 마지막으로 끊긴 때.
    pub disconnected_at: Option<Instant>,
    /// 등록한 때.
    pub registered_at: Instant,
    /// 한 번이라도 스트림이 붙었는가.
    pub ever_connected: bool,
    /// 이 channel에 발급한 `chat_id`.
    pub issued: IssuedIds,
}

impl ChannelConn {
    fn new() -> ChannelConn {
        ChannelConn {
            queue: VecDeque::new(),
            notify: Arc::new(Notify::new()),
            generation: 0,
            connected: false,
            disconnected_at: None,
            registered_at: Instant::now(),
            ever_connected: false,
            issued: IssuedIds::default(),
        }
    }
}

/// 그 스트림은 더 보낼 곳이 없다(새 스트림이 붙었거나 channel이 닫혔다).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gone;

/// 모든 channel 연결.
#[derive(Debug, Default)]
pub struct Channels {
    map: HashMap<String, ChannelConn>,
}

impl Channels {
    /// 새로 등록.
    pub fn register(&mut self, cid: &str) {
        self.map.insert(cid.to_owned(), ChannelConn::new());
    }

    /// 지운다(스트림은 다음 확인에서 끝난다).
    pub fn remove(&mut self, cid: &str) {
        if let Some(c) = self.map.remove(cid) {
            c.notify.notify_waiters();
        }
    }

    /// 있는가.
    pub fn contains(&self, cid: &str) -> bool {
        self.map.contains_key(cid)
    }

    /// 메시지를 넣는다. 없는 channel이면 `false`.
    pub fn push(&mut self, cid: &str, msg: String) -> bool {
        let Some(c) = self.map.get_mut(cid) else {
            return false;
        };
        if c.queue.len() >= QUEUE_MAX {
            c.queue.pop_front();
        }
        c.queue.push_back(msg);
        c.notify.notify_waiters();
        true
    }

    /// 이 channel에 `chat_id`를 발급했다고 기억한다. 없는 channel이면 `false`.
    pub fn issue(&mut self, cid: &str, chat_id: &str, now_ms: i64) -> bool {
        match self.map.get_mut(cid) {
            Some(c) => {
                c.issued.issue(chat_id, now_ms);
                true
            }
            None => false,
        }
    }

    /// 이 channel에 24시간 안에 발급한 `chat_id`인가.
    pub fn was_issued(&self, cid: &str, chat_id: &str, now_ms: i64) -> bool {
        self.map
            .get(cid)
            .is_some_and(|c| c.issued.contains(chat_id, now_ms))
    }

    /// 새 스트림이 붙는다: 세대를 올리고 (세대, 알림)을 돌려준다.
    pub fn attach(&mut self, cid: &str) -> Option<(u64, Arc<Notify>)> {
        let c = self.map.get_mut(cid)?;
        c.generation += 1;
        c.connected = true;
        c.ever_connected = true;
        c.disconnected_at = None;
        c.notify.notify_waiters();
        Some((c.generation, c.notify.clone()))
    }

    /// 그 세대의 스트림이 보낼 다음 메시지. 세대가 바뀌었거나 channel이 없으면 `Err(Gone)`.
    pub fn next_for(&mut self, cid: &str, generation: u64) -> Result<Option<String>, Gone> {
        let c = self.map.get_mut(cid).ok_or(Gone)?;
        if c.generation != generation {
            return Err(Gone);
        }
        Ok(c.queue.pop_front())
    }

    /// 보내지 못한 메시지를 대기열 앞에 되돌린다.
    pub fn unshift(&mut self, cid: &str, msg: String) {
        if let Some(c) = self.map.get_mut(cid) {
            c.queue.push_front(msg);
        }
    }

    /// 그 세대의 스트림이 끊겼다.
    pub fn detach(&mut self, cid: &str, generation: u64) {
        if let Some(c) = self.map.get_mut(cid)
            && c.generation == generation
        {
            c.connected = false;
            c.disconnected_at = Some(Instant::now());
        }
    }

    /// 닫아야 할 channel: 끊긴 지 `grace`가 지났거나, 등록 뒤 `never_grace` 동안 한 번도 붙지 않은 것.
    pub fn expired(
        &self,
        grace: std::time::Duration,
        never_grace: std::time::Duration,
    ) -> Vec<String> {
        let mut v: Vec<String> = self
            .map
            .iter()
            .filter(|(_, c)| {
                !c.connected
                    && match c.disconnected_at {
                        Some(t) => t.elapsed() >= grace,
                        None => !c.ever_connected && c.registered_at.elapsed() >= never_grace,
                    }
            })
            .map(|(k, _)| k.clone())
            .collect();
        v.sort();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn channel_queue_and_generations() {
        let mut c = Channels::default();
        assert!(!c.push("x", "m".into()));
        c.register("x");
        assert!(c.push("x", "a".into()));
        let (g1, _) = c.attach("x").unwrap();
        assert_eq!(c.next_for("x", g1), Ok(Some("a".into())));
        assert_eq!(c.next_for("x", g1), Ok(None));
        let (g2, _) = c.attach("x").unwrap();
        assert!(c.next_for("x", g1).is_err());
        c.push("x", "b".into());
        c.unshift("x", "z".into());
        assert_eq!(c.next_for("x", g2), Ok(Some("z".into())));
        // 옛 세대의 끊김은 무시한다
        c.detach("x", g1);
        assert!(c.expired(Duration::ZERO, Duration::ZERO).is_empty());
        c.detach("x", g2);
        assert_eq!(
            c.expired(Duration::ZERO, Duration::from_secs(999)),
            vec!["x".to_owned()]
        );
        assert!(
            c.expired(Duration::from_secs(999), Duration::from_secs(999))
                .is_empty()
        );
        c.register("y");
        assert_eq!(
            c.expired(Duration::from_secs(999), Duration::ZERO),
            vec!["y".to_owned()]
        );
        c.remove("x");
        assert!(!c.contains("x"));
        assert!(c.next_for("x", g2).is_err());
    }

    #[test]
    fn issued_ids_expire_and_are_bounded() {
        let t = 1_764_126_720_000;
        let mut ids = IssuedIds::default();
        ids.issue("m1", t);
        assert!(ids.contains("m1", t));
        assert!(ids.contains("m1", t + ISSUED_TTL_MS));
        assert!(!ids.contains("m1", t + ISSUED_TTL_MS + 1));
        assert!(!ids.contains("m2", t));
        // 다시 발급하면 시각이 새로 된다
        ids.issue("m1", t + ISSUED_TTL_MS);
        assert!(ids.contains("m1", t + ISSUED_TTL_MS + 1));
        for i in 0..ISSUED_MAX {
            ids.issue(&format!("x{i}"), t + ISSUED_TTL_MS);
        }
        assert!(
            !ids.contains("m1", t + ISSUED_TTL_MS),
            "가장 오래된 것부터 잊는다"
        );
        assert!(ids.contains("x0", t + ISSUED_TTL_MS));
        assert!(ids.contains(&format!("x{}", ISSUED_MAX - 1), t + ISSUED_TTL_MS));

        let mut c = Channels::default();
        assert!(!c.issue("ch", "m1", t));
        c.register("ch");
        assert!(c.issue("ch", "m1", t));
        assert!(c.was_issued("ch", "m1", t));
        assert!(!c.was_issued("other", "m1", t));
        c.remove("ch");
        assert!(!c.was_issued("ch", "m1", t));
    }

    #[test]
    fn queue_is_bounded() {
        let mut c = Channels::default();
        c.register("x");
        for i in 0..(QUEUE_MAX + 5) {
            c.push("x", i.to_string());
        }
        let (g, _) = c.attach("x").unwrap();
        assert_eq!(c.next_for("x", g), Ok(Some("5".into())));
    }
}
