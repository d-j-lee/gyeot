//! 디스크: 데이터 폴더·토큰, 이벤트 로그(쓰기 스레드·재생), 보관 정리, `learn.json`, 스풀.

use anyhow::{Context, Result};
use gyeot_core::paths::{event_log_name, parse_event_log_name};
use gyeot_core::{Event, Learn, Source};
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

const DAY_MS: i64 = 86_400_000;
/// 스풀 파일 하나의 최대 크기.
pub const SPOOL_MAX_BYTES: u64 = 2 * 1024 * 1024;
/// 다른 쪽이 집어 간 채로 이만큼 지난 스풀 파일은 다시 집는다(그쪽이 죽었다고 본다).
const STALE_CLAIM: Duration = Duration::from_secs(300);

/// 데이터 폴더와 하위 폴더를 만든다.
pub fn ensure_dirs(home: &Path) -> Result<()> {
    for d in [home.to_path_buf(), home.join("events"), home.join("spool")] {
        fs::create_dir_all(&d)
            .with_context(|| format!("폴더 {}를 만들지 못했어요", d.display()))?;
    }
    Ok(())
}

fn is_valid_token(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// 32바이트 난수의 소문자 hex 64자.
pub fn new_token() -> Result<String> {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).map_err(|e| anyhow::anyhow!("난수를 얻지 못했어요: {e}"))?;
    Ok(hex(&buf))
}

/// 소문자 hex.
pub fn hex(bytes: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(D[usize::from(b >> 4)] as char);
        s.push(D[usize::from(b & 15)] as char);
    }
    s
}

/// 무작위 hex 문자열(id용).
pub fn random_hex(n_bytes: usize) -> String {
    let mut buf = vec![0u8; n_bytes];
    if getrandom::fill(&mut buf).is_err() {
        // 난수원이 없을 때만: 시각으로 대신한다(충돌해도 보안 문제는 아니다).
        let t = now_ms().to_le_bytes();
        for (i, b) in buf.iter_mut().enumerate() {
            *b = t[i % t.len()] ^ (i as u8);
        }
    }
    hex(&buf)
}

/// 토큰을 읽고, 없거나 망가졌으면 새로 만든다(Unix 권한 0600).
pub fn ensure_token(path: &Path) -> Result<String> {
    if let Ok(s) = fs::read_to_string(path) {
        let t = s.trim();
        if is_valid_token(t) {
            restrict_permissions(path);
            return Ok(t.to_owned());
        }
        tracing::warn!("토큰 파일 {}이 망가져서 새로 만들어요", path.display());
    }
    let token = new_token()?;
    write_private(path, token.as_bytes())?;
    Ok(token)
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

/// 임시 파일에 쓰고 이름을 바꾼다(Unix 권한 0600).
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut opts = OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts
            .open(&tmp)
            .with_context(|| format!("{}를 쓰지 못했어요", tmp.display()))?;
        f.write_all(bytes)?;
        f.sync_all().ok();
    }
    fs::rename(&tmp, path).with_context(|| format!("{}로 옮기지 못했어요", path.display()))?;
    restrict_permissions(path);
    Ok(())
}

/// 지금 시각(UTC 밀리초).
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

// ───────────────────────── 이벤트 로그 ─────────────────────────

enum LogCmd {
    Append { file: String, line: String },
    Flush(mpsc::SyncSender<()>),
}

/// 이벤트 로그 쓰기 스레드에 보내는 손잡이. 보낸 순서대로 덧붙인다.
#[derive(Debug, Clone)]
pub struct LogSink {
    tx: mpsc::Sender<LogCmd>,
}

impl std::fmt::Debug for LogCmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogCmd::Append { file, .. } => write!(f, "Append({file})"),
            LogCmd::Flush(_) => f.write_str("Flush"),
        }
    }
}

impl LogSink {
    /// 이벤트를 `now_ms`의 UTC 날짜 파일에 덧붙이도록 보낸다(쓴 순서 = 적용 순서).
    pub fn append(&self, ev: &Event, now_ms: i64) {
        match serde_json::to_string(ev) {
            Ok(line) => {
                let _ = self.tx.send(LogCmd::Append {
                    file: event_log_name(now_ms),
                    line,
                });
            }
            Err(e) => tracing::error!("이벤트를 직렬화하지 못했어요: {e}"),
        }
    }

    /// 지금까지 보낸 것을 다 쓸 때까지 기다린다(블로킹).
    pub fn flush_blocking(&self) {
        let (tx, rx) = mpsc::sync_channel(1);
        if self.tx.send(LogCmd::Flush(tx)).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(10));
        }
    }
}

/// 쓰기 스레드를 띄운다. 모든 `LogSink`가 사라지면 남은 것을 쓰고 끝난다.
pub fn spawn_log_writer(dir: PathBuf) -> Result<(LogSink, std::thread::JoinHandle<()>)> {
    fs::create_dir_all(&dir)?;
    let (tx, rx) = mpsc::channel::<LogCmd>();
    let handle = std::thread::Builder::new()
        .name("gyeot-log".into())
        .spawn(move || writer_loop(&dir, &rx))?;
    Ok((LogSink { tx }, handle))
}

fn writer_loop(dir: &Path, rx: &mpsc::Receiver<LogCmd>) {
    let mut current: Option<(String, File)> = None;
    while let Ok(first) = rx.recv() {
        let mut cmd = Some(first);
        let mut flushes = Vec::new();
        while let Some(c) = cmd.take() {
            match c {
                LogCmd::Append { file, line } => {
                    if current.as_ref().map(|(n, _)| n != &file).unwrap_or(true) {
                        if let Some((_, f)) = current.as_mut() {
                            let _ = f.flush();
                        }
                        current = OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(dir.join(&file))
                            .map_err(|e| tracing::error!("로그 파일 {file}을 열지 못했어요: {e}"))
                            .ok()
                            .map(|f| (file, f));
                    }
                    if let Some((_, f)) = current.as_mut() {
                        let mut buf = line.into_bytes();
                        buf.push(b'\n');
                        if let Err(e) = f.write_all(&buf) {
                            tracing::error!("이벤트 로그를 쓰지 못했어요: {e}");
                        }
                    }
                }
                LogCmd::Flush(done) => flushes.push(done),
            }
            cmd = rx.try_recv().ok();
        }
        if let Some((_, f)) = current.as_mut() {
            let _ = f.flush();
            if !flushes.is_empty() {
                let _ = f.sync_data();
            }
        }
        for d in flushes {
            let _ = d.send(());
        }
    }
    if let Some((_, mut f)) = current {
        let _ = f.flush();
        let _ = f.sync_data();
    }
}

/// 재생할 이벤트: `since_ms` 이후 날짜 파일들을 날짜순으로 읽고, `since_ms` 이상인 첫 이벤트부터 끝까지.
/// 읽지 못한 줄 수도 돌려준다.
pub fn read_replay(dir: &Path, since_ms: i64) -> (Vec<Event>, usize) {
    let mut files: Vec<(i64, PathBuf)> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                parse_event_log_name(&name).map(|d| (d, e.path()))
            })
            .filter(|(d, _)| *d + DAY_MS > since_ms)
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    let mut events = Vec::new();
    let mut bad = 0usize;
    for (_, path) in files {
        let Ok(f) = File::open(&path) else {
            continue;
        };
        for line in BufReader::new(f).lines() {
            let Ok(line) = line else {
                bad += 1;
                break;
            };
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Event>(&line) {
                Ok(ev) => events.push(ev),
                Err(_) => bad += 1,
            }
        }
    }
    let start = events
        .iter()
        .position(|e| e.ts_ms >= since_ms)
        .unwrap_or(events.len());
    events.drain(..start);
    (events, bad)
}

/// 보관 기간이 지난 이벤트 로그를 지운다. 지운 파일 수.
pub fn cleanup_retention(dir: &Path, now_ms: i64, retention_days: u32) -> usize {
    let today = now_ms - now_ms.rem_euclid(DAY_MS);
    let cutoff = today - i64::from(retention_days) * DAY_MS;
    let mut n = 0;
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(d) = parse_event_log_name(&name)
                && d < cutoff
                && fs::remove_file(e.path()).is_ok()
            {
                n += 1;
            }
        }
    }
    n
}

// ───────────────────────── learn.json ─────────────────────────

/// `learn.json`을 읽는다. 없으면 빈 기록, 망가졌으면 옆으로 치우고 빈 기록.
pub fn load_learn(path: &Path) -> Learn {
    match fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str::<Learn>(&s) {
            Ok(l) => l,
            Err(e) => {
                let aside = path.with_extension(format!("json.bad-{}", now_ms()));
                tracing::warn!(
                    "learn.json을 읽지 못해 {}로 옮기고 새로 시작해요: {e}",
                    aside.display()
                );
                let _ = fs::rename(path, aside);
                Learn::default()
            }
        },
        Err(_) => Learn::default(),
    }
}

/// `learn.json`을 쓴다(원자적).
pub fn save_learn(path: &Path, learn: &Learn) -> Result<()> {
    let s = serde_json::to_vec_pretty(learn)?;
    write_private(path, &s)
}

// ───────────────────────── 스풀 ─────────────────────────

/// 스풀에서 꺼낸 훅 요청 하나.
#[derive(Debug, Clone, PartialEq)]
pub struct SpoolEntry {
    /// source.
    pub source: Source,
    /// 원본 페이로드.
    pub payload: Value,
    /// 훅을 띄운 PID.
    pub pid_hint: Option<u32>,
    /// 심이 받은 시각.
    pub received_ms: Option<i64>,
}

/// 스풀 봉투(`{"source","payload","pid_hint","received_ms"}`)를 읽는다.
pub fn parse_spool(bytes: &[u8]) -> Option<SpoolEntry> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    let o = v.as_object()?;
    let source = Source::parse(o.get("source")?.as_str()?).filter(|s| s.is_ingestible())?;
    Some(SpoolEntry {
        source,
        payload: o.get("payload").cloned().unwrap_or(Value::Null),
        pid_hint: o
            .get("pid_hint")
            .and_then(Value::as_u64)
            .and_then(|p| u32::try_from(p).ok()),
        received_ms: o.get("received_ms").and_then(Value::as_i64),
    })
}

fn claimable(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.ends_with(".json") {
        return true;
    }
    // 다른 쪽이 집어 간 채 오래된 것.
    if name.contains(".json.claim-") || name.ends_with(".json.draining") {
        return fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > STALE_CLAIM);
    }
    false
}

/// 스풀을 비운다: 이름 순(= 시각 순)으로 집어 읽고 지운다. 읽지 못한 파일도 지운다.
pub fn drain_spool(dir: &Path, max: usize) -> Vec<SpoolEntry> {
    let mut paths: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| claimable(p))
            .collect(),
        Err(_) => return Vec::new(),
    };
    paths.sort();
    let mut out = Vec::new();
    for p in paths.into_iter().take(max) {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let base = name.split(".json").next().unwrap_or(&name).to_owned();
        let claimed = dir.join(format!("{base}.json.draining"));
        if fs::rename(&p, &claimed).is_err() {
            continue; // 심이 먼저 집어 갔다.
        }
        let mut bytes = Vec::new();
        let ok = File::open(&claimed)
            .and_then(|f| f.take(SPOOL_MAX_BYTES + 1).read_to_end(&mut bytes))
            .is_ok();
        let _ = fs::remove_file(&claimed);
        if !ok || bytes.len() as u64 > SPOOL_MAX_BYTES {
            tracing::warn!("스풀 파일 {name}을 읽지 못해 버렸어요");
            continue;
        }
        match parse_spool(&bytes) {
            Some(e) => out.push(e),
            None => tracing::warn!("스풀 파일 {name}의 모양이 맞지 않아 버렸어요"),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyeot_core::EventBody;

    fn ev(ts: i64) -> Event {
        Event {
            id: format!("{ts}-0"),
            ts_ms: ts,
            source: Source::ClaudeCode,
            agent: None,
            session: Some("s".into()),
            cwd: None,
            pid_hint: None,
            raw_event: None,
            body: EventBody::Activity { tool: None },
        }
    }

    #[test]
    fn token_is_created_once_and_private() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("token");
        let t = ensure_token(&p).unwrap();
        assert!(is_valid_token(&t));
        assert_eq!(ensure_token(&p).unwrap(), t);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        fs::write(&p, "garbage").unwrap();
        let t2 = ensure_token(&p).unwrap();
        assert_ne!(t2, t);
        assert!(is_valid_token(&t2));
    }

    #[test]
    fn log_writer_and_replay_window() {
        let d = tempfile::tempdir().unwrap();
        let (sink, h) = spawn_log_writer(d.path().to_path_buf()).unwrap();
        let day = 1_764_115_200_000; // 2025-11-26 00:00 UTC
        sink.append(&ev(day - 3 * DAY_MS), day - 3 * DAY_MS);
        sink.append(&ev(day - DAY_MS - 10), day - DAY_MS - 10);
        sink.append(&ev(day - DAY_MS + 10), day - DAY_MS + 10);
        // 늦게 온(스풀) 이벤트: 적은 날은 오늘이지만 시각은 옛날
        sink.append(&ev(day - 5 * DAY_MS), day + 5);
        sink.append(&ev(day + 100), day + 100);
        sink.flush_blocking();
        drop(sink);
        h.join().unwrap();
        fs::write(d.path().join("junk.txt"), "x").unwrap();
        let mut f = OpenOptions::new()
            .append(true)
            .open(d.path().join("2025-11-26.jsonl"))
            .unwrap();
        writeln!(f, "not json").unwrap();
        let (evs, bad) = read_replay(d.path(), day - DAY_MS);
        assert_eq!(bad, 1);
        let ts: Vec<i64> = evs.iter().map(|e| e.ts_ms).collect();
        // 기준 이후 첫 이벤트부터 끝까지, 쓴 순서대로(늦게 온 것도 그 자리에)
        assert_eq!(ts, vec![day - DAY_MS + 10, day - 5 * DAY_MS, day + 100]);
        assert_eq!(cleanup_retention(d.path(), day, 2), 1);
        assert!(!d.path().join("2025-11-23.jsonl").exists());
        assert!(d.path().join("2025-11-24.jsonl").exists());
    }

    #[test]
    fn spool_drain_orders_and_cleans() {
        let d = tempfile::tempdir().unwrap();
        let w = |name: &str, body: &str| fs::write(d.path().join(name), body).unwrap();
        w(
            "0000000000002-1-0.json",
            r#"{"source":"claude-code","payload":{"a":2},"pid_hint":5,"received_ms":2}"#,
        );
        w(
            "0000000000001-1-0.json",
            r#"{"source":"codex-notify","payload":{"a":1},"received_ms":1}"#,
        );
        w("0000000000003-1-0.json", "broken");
        w(
            "0000000000004-1-0.json",
            r#"{"source":"gyeot","payload":{}}"#,
        );
        w(
            "0000000000005-1-0.json.claim-77",
            r#"{"source":"codex","payload":{}}"#,
        );
        w(
            "0000000000006-1-0.json.tmp",
            r#"{"source":"codex","payload":{}}"#,
        );
        let got = drain_spool(d.path(), 50);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].source, Source::CodexNotify);
        assert_eq!(got[0].pid_hint, None);
        assert_eq!(got[1].source, Source::ClaudeCode);
        assert_eq!(got[1].pid_hint, Some(5));
        assert_eq!(got[1].received_ms, Some(2));
        let left: Vec<String> = fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        // 심이 막 집어 간 것과 쓰는 중인 임시 파일은 건드리지 않는다
        let mut left = left;
        left.sort();
        assert_eq!(
            left,
            vec![
                "0000000000005-1-0.json.claim-77",
                "0000000000006-1-0.json.tmp"
            ]
        );
    }

    #[test]
    fn learn_roundtrip_and_corruption() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("learn.json");
        assert_eq!(load_learn(&p), Learn::default());
        let mut l = Learn::default();
        l.quiet_kinds.insert(gyeot_core::ItemKind::Unread);
        save_learn(&p, &l).unwrap();
        assert_eq!(load_learn(&p), l);
        fs::write(&p, "{broken").unwrap();
        assert_eq!(load_learn(&p), Learn::default());
        assert!(!p.exists());
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 1);
    }

    #[test]
    fn hex_and_random() {
        assert_eq!(hex(&[0, 15, 255]), "000fff");
        let a = random_hex(8);
        assert_eq!(a.len(), 16);
        assert_ne!(a, random_hex(8));
    }
}
