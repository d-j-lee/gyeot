//! `<데이터>/shell.log`에 한 줄씩 남기는 작은 기록기(512 KiB 넘으면 `shell.log.1`로 넘긴다).
//! 릴리스 빌드는 콘솔이 없어서(windows_subsystem) 문제를 볼 곳이 이 파일뿐이다.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 512 * 1024;

static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());

pub fn init(data_dir: Option<PathBuf>) {
    let _ = PATH.set(data_dir.map(|d| d.join("shell.log")));
}

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

pub fn write(level: &str, msg: &str) {
    let line = format!("{} {level} {msg}\n", now_ms());
    if cfg!(debug_assertions) {
        eprint!("{line}");
    }
    let Some(Some(path)) = PATH.get() else {
        return;
    };
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if fs::metadata(path)
        .map(|m| m.len() > MAX_BYTES)
        .unwrap_or(false)
    {
        let _ = fs::rename(path, path.with_extension("log.1"));
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

#[macro_export]
macro_rules! info {
    ($($t:tt)*) => { $crate::logging::write("INFO", &format!($($t)*)) };
}

#[macro_export]
macro_rules! warn {
    ($($t:tt)*) => { $crate::logging::write("WARN", &format!($($t)*)) };
}
