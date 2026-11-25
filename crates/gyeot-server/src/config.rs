//! 데몬 설정: 데이터 폴더, 포트, `config.toml`(계약 §1·§6), 얼굴 폴더.

use anyhow::{Context, Result, bail};
use gyeot_core::paths::{HomeEnv, resolve_home};
use gyeot_core::{Policy, Source};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 기본 포트.
pub const DEFAULT_PORT: u16 = 47321;

/// 따라 읽을 JSONL(`[[tail]]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TailConfig {
    /// 파일 경로.
    pub path: PathBuf,
    /// 정규화에 쓸 source(`fsmap-harness`, `aok-harness` 등).
    pub source: String,
    /// 줄에 세션이 없을 때 세션을 찾는 저장소 경로.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileConfig {
    port: Option<u16>,
    stale_minutes: Option<u32>,
    nudge_min: Option<f64>,
    interrupt_min: Option<f64>,
    approval_interrupt: Option<bool>,
    quiet_hours: Option<Vec<String>>,
    utc_offset_minutes: Option<i32>,
    retention_days: Option<u32>,
    tail: Vec<TailConfig>,
    #[serde(flatten)]
    extra: BTreeMap<String, toml::Value>,
}

/// 데몬 설정. [`ServerConfig::from_env`]가 환경 변수와 `config.toml`로 채운다.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerConfig {
    /// 포트. `0`이면 빈 포트를 고른다(시험용).
    pub port: u16,
    /// 데이터 폴더.
    pub home: PathBuf,
    /// 얼굴 정적 파일 폴더(`index.html`이 있는 곳). 없으면 안내 쪽을 보인다.
    pub face_dir: Option<PathBuf>,
    /// 멈춤 판단 분.
    pub stale_minutes: u32,
    /// 말풍선 기준 점수.
    pub nudge_min: f64,
    /// 소리 기준 점수.
    pub interrupt_min: f64,
    /// 승인·병합 대기를 소리(interrupt)까지 올릴지(계약 §13.3, 기본 false).
    pub approval_interrupt: bool,
    /// 조용한 시간(현지 시각 `HH:MM-HH:MM`).
    pub quiet_hours: Vec<String>,
    /// 현지 오프셋(분). `None`이면 시스템 현지 시간대를 따른다.
    pub utc_offset_minutes: Option<i32>,
    /// 이벤트 로그 보관 일수(최소 2).
    pub retention_days: u32,
    /// 따라 읽을 JSONL.
    pub tails: Vec<TailConfig>,
    /// 읽기는 했지만 알려 둘 것(모르는 키 등).
    pub warnings: Vec<String>,
}

/// 환경에서 데이터 폴더를 정한다(`GYEOT_HOME` → OS 기본, 계약 §1).
pub fn resolve_home_from_env() -> Result<PathBuf> {
    let env = HomeEnv {
        gyeot_home: std::env::var_os("GYEOT_HOME").map(PathBuf::from),
        windows: cfg!(windows),
        local_app_data: dirs::data_local_dir(),
        xdg_state_home: std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        home_dir: dirs::home_dir(),
    };
    resolve_home(&env).context("데이터 폴더를 정하지 못했어요. GYEOT_HOME을 지정해 주세요")
}

/// 개발·배포 위치의 얼굴 폴더: `<실행 파일 폴더>/face` → `<저장소>/face/dist`.
pub fn default_face_dir() -> Option<PathBuf> {
    let mut cands = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        cands.push(dir.join("face"));
    }
    cands.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("face")
            .join("dist"),
    );
    cands.into_iter().find(|d| d.join("index.html").is_file())
}

impl ServerConfig {
    /// 기본값(데이터 폴더만 정해 줌).
    pub fn defaults(home: PathBuf) -> ServerConfig {
        ServerConfig {
            port: DEFAULT_PORT,
            home,
            face_dir: None,
            stale_minutes: 30,
            nudge_min: 80.0,
            interrupt_min: 95.0,
            approval_interrupt: false,
            quiet_hours: Vec::new(),
            utc_offset_minutes: None,
            retention_days: 14,
            tails: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// 환경 변수(`GYEOT_HOME`·`GYEOT_PORT`·`GYEOT_FACE_DIR`)와 `config.toml`로 설정을 만든다.
    pub fn from_env() -> Result<ServerConfig> {
        let home = resolve_home_from_env()?;
        let mut cfg = ServerConfig::load(home)?;
        if let Some(p) = std::env::var_os("GYEOT_PORT") {
            let s = p.to_string_lossy();
            cfg.port = s
                .trim()
                .parse::<u16>()
                .ok()
                .filter(|p| *p != 0)
                .with_context(|| format!("GYEOT_PORT '{s}'은 1–65535 사이 숫자여야 해요"))?;
        }
        cfg.face_dir = match std::env::var_os("GYEOT_FACE_DIR").filter(|v| !v.is_empty()) {
            Some(d) => Some(PathBuf::from(d)),
            None => default_face_dir(),
        };
        Ok(cfg)
    }

    /// `<home>/config.toml`을 읽는다(없으면 기본값). 환경 변수는 보지 않는다.
    pub fn load(home: PathBuf) -> Result<ServerConfig> {
        let path = home.join("config.toml");
        match std::fs::read_to_string(&path) {
            Ok(text) => ServerConfig::parse(home, &text)
                .with_context(|| format!("설정 파일 {}을 읽지 못했어요", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ServerConfig::defaults(home)),
            Err(e) => {
                Err(e).with_context(|| format!("설정 파일 {}을 열지 못했어요", path.display()))
            }
        }
    }

    /// TOML 글을 읽는다.
    pub fn parse(home: PathBuf, text: &str) -> Result<ServerConfig> {
        let f: FileConfig = toml::from_str(text).context("TOML 형식이 맞지 않아요")?;
        let mut cfg = ServerConfig::defaults(home);
        for k in f.extra.keys() {
            cfg.warnings.push(format!("모르는 설정 '{k}'은 무시해요"));
        }
        if let Some(p) = f.port {
            if p == 0 {
                bail!("port는 1–65535 사이여야 해요");
            }
            cfg.port = p;
        }
        if let Some(v) = f.stale_minutes {
            if v == 0 || v > 24 * 60 {
                bail!("stale_minutes는 1–1440 사이여야 해요");
            }
            cfg.stale_minutes = v;
        }
        if let Some(v) = f.nudge_min {
            if !(0.0..=200.0).contains(&v) {
                bail!("nudge_min은 0–200 사이여야 해요");
            }
            cfg.nudge_min = v;
        }
        if let Some(v) = f.interrupt_min {
            if !(0.0..=200.0).contains(&v) {
                bail!("interrupt_min은 0–200 사이여야 해요");
            }
            cfg.interrupt_min = v;
        }
        if let Some(v) = f.approval_interrupt {
            cfg.approval_interrupt = v;
        }
        if let Some(q) = f.quiet_hours {
            Policy::parse_quiet_hours(&q).map_err(anyhow::Error::msg)?;
            cfg.quiet_hours = q.iter().map(|s| s.trim().to_owned()).collect();
        }
        if let Some(o) = f.utc_offset_minutes {
            if !(-14 * 60..=14 * 60).contains(&o) {
                bail!("utc_offset_minutes는 −840–840 사이여야 해요");
            }
            cfg.utc_offset_minutes = Some(o);
        }
        if let Some(d) = f.retention_days {
            if d < 2 {
                cfg.warnings
                    .push("retention_days는 재생에 이틀이 필요해서 2로 올렸어요".to_owned());
            }
            cfg.retention_days = d.clamp(2, 3650);
        }
        for t in f.tail {
            match Source::parse(&t.source) {
                Some(s) if s.is_ingestible() => {}
                _ => bail!(
                    "[[tail]] source '{}'은 쓸 수 없어요(aok-harness·fsmap-harness·claude-code·codex·codex-notify·custom)",
                    t.source
                ),
            }
            if t.path.as_os_str().is_empty() {
                bail!("[[tail]] path가 비었어요");
            }
            cfg.tails.push(t);
        }
        Ok(cfg)
    }

    /// 엔진 정책. 현지 오프셋이 설정에 없으면 시스템 현지 시간대를 쓴다.
    pub fn policy(&self) -> Policy {
        Policy {
            stale_minutes: self.stale_minutes,
            nudge_min: self.nudge_min,
            interrupt_min: self.interrupt_min,
            quiet_hours: Policy::parse_quiet_hours(&self.quiet_hours).unwrap_or_default(),
            utc_offset_minutes: self.effective_utc_offset(),
            approval_interrupt: self.approval_interrupt,
        }
    }

    /// 지금 쓰는 현지 오프셋(분).
    pub fn effective_utc_offset(&self) -> i32 {
        self.utc_offset_minutes.unwrap_or_else(local_offset_minutes)
    }

    /// `<home>/token`.
    pub fn token_path(&self) -> PathBuf {
        self.home.join("token")
    }
    /// `<home>/config.toml`.
    pub fn config_path(&self) -> PathBuf {
        self.home.join("config.toml")
    }
    /// `<home>/events`.
    pub fn events_dir(&self) -> PathBuf {
        self.home.join("events")
    }
    /// `<home>/spool`.
    pub fn spool_dir(&self) -> PathBuf {
        self.home.join("spool")
    }
    /// `<home>/learn.json`.
    pub fn learn_path(&self) -> PathBuf {
        self.home.join("learn.json")
    }
    /// `<home>/sessions.json`(얼굴 세션의 SHA-256, §12.1).
    pub fn sessions_path(&self) -> PathBuf {
        self.home.join("sessions.json")
    }
    /// `<home>/tails.json`(따라 읽기 위치).
    pub fn tail_state_path(&self) -> PathBuf {
        self.home.join("tails.json")
    }

    /// `--print-config`용: 실제로 쓰는 값을 TOML로.
    pub fn to_toml(&self) -> String {
        #[derive(Serialize)]
        struct Out<'a> {
            port: u16,
            stale_minutes: u32,
            nudge_min: f64,
            interrupt_min: f64,
            approval_interrupt: bool,
            quiet_hours: &'a [String],
            utc_offset_minutes: i32,
            retention_days: u32,
            tail: &'a [TailConfig],
        }
        let body = toml::to_string(&Out {
            port: self.port,
            stale_minutes: self.stale_minutes,
            nudge_min: self.nudge_min,
            interrupt_min: self.interrupt_min,
            approval_interrupt: self.approval_interrupt,
            quiet_hours: &self.quiet_hours,
            utc_offset_minutes: self.effective_utc_offset(),
            retention_days: self.retention_days,
            tail: &self.tails,
        })
        .unwrap_or_default();
        let mut s = String::new();
        s.push_str(&format!("# 데이터 폴더: {}\n", self.home.display()));
        s.push_str(&format!("# 설정 파일: {}\n", self.config_path().display()));
        s.push_str(&format!("# 토큰: {}\n", self.token_path().display()));
        s.push_str(&match &self.face_dir {
            Some(d) => format!("# 얼굴 폴더: {}\n", d.display()),
            None => "# 얼굴 폴더: 없음(안내 쪽을 보여 줘요)\n".to_owned(),
        });
        if self.utc_offset_minutes.is_none() {
            s.push_str("# utc_offset_minutes는 설정에 없어서 시스템 현지 시간대 값을 보였어요\n");
        }
        for w in &self.warnings {
            s.push_str(&format!("# 알림: {w}\n"));
        }
        s.push_str(&body);
        s
    }
}

/// 시스템 현지 시간대의 지금 오프셋(분).
pub fn local_offset_minutes() -> i32 {
    chrono::Local::now().offset().local_minus_utc() / 60
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_contract_example() {
        let text = r#"
port = 47321
stale_minutes = 30
nudge_min = 80
interrupt_min = 95
approval_interrupt = false      # 승인·병합 대기를 소리(interrupt)까지 올릴지(§13.3)
quiet_hours = ["22:00-08:00"]
utc_offset_minutes = 540
retention_days = 14
[[tail]]
path = "C:/work/proj/.git/fsmap-harness/log.jsonl"
source = "fsmap-harness"
cwd = "C:/work/proj"
"#;
        let c = ServerConfig::parse(PathBuf::from("/h"), text).unwrap();
        assert_eq!(c.port, 47321);
        assert_eq!(c.quiet_hours, vec!["22:00-08:00"]);
        assert_eq!(c.utc_offset_minutes, Some(540));
        assert_eq!(c.tails.len(), 1);
        assert_eq!(c.tails[0].cwd.as_deref(), Some("C:/work/proj"));
        let p = c.policy();
        assert_eq!(p.utc_offset_minutes, 540);
        assert_eq!(p.quiet_hours.len(), 1);
        assert!(c.warnings.is_empty());
        let out = c.to_toml();
        assert!(out.contains("port = 47321"), "{out}");
        assert!(out.contains("[[tail]]"), "{out}");
    }

    #[test]
    fn empty_is_defaults() {
        let c = ServerConfig::parse(PathBuf::from("/h"), "").unwrap();
        assert_eq!(c, ServerConfig::defaults(PathBuf::from("/h")));
    }

    /// 계약 §6·§13.3: 최상위 키 `approval_interrupt`(기본 false)를 엔진 정책으로 넘긴다.
    #[test]
    fn approval_interrupt_reaches_policy() {
        let c = ServerConfig::parse(PathBuf::from("/h"), "").unwrap();
        assert!(!c.approval_interrupt);
        assert!(!c.policy().approval_interrupt);
        assert!(c.to_toml().contains("approval_interrupt = false"));
        let c = ServerConfig::parse(PathBuf::from("/h"), "approval_interrupt = true").unwrap();
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert!(c.approval_interrupt);
        assert!(c.policy().approval_interrupt);
        assert!(c.to_toml().contains("approval_interrupt = true"));
        let c = ServerConfig::parse(PathBuf::from("/h"), "approval_interrupt = false").unwrap();
        assert!(!c.policy().approval_interrupt);
        assert!(ServerConfig::parse(PathBuf::from("/h"), "approval_interrupt = \"yes\"").is_err());
        assert!(ServerConfig::parse(PathBuf::from("/h"), "approval_interrupt = 1").is_err());
    }

    #[test]
    fn rejects_bad_values() {
        for bad in [
            "port = 0",
            "quiet_hours = [\"25:00-01:00\"]",
            "nudge_min = -1",
            "utc_offset_minutes = 10000",
            "stale_minutes = 0",
            "[[tail]]\npath = \"x\"\nsource = \"gyeot\"",
            "[[tail]]\npath = \"x\"\nsource = \"nope\"",
            "port = \"abc\"",
            "not toml",
        ] {
            assert!(
                ServerConfig::parse(PathBuf::from("/h"), bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn unknown_keys_warn_and_retention_clamps() {
        let c = ServerConfig::parse(PathBuf::from("/h"), "foo = 1\nretention_days = 1").unwrap();
        assert_eq!(c.retention_days, 2);
        assert_eq!(c.warnings.len(), 2);
    }

    #[test]
    fn load_missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let c = ServerConfig::load(dir.path().to_path_buf()).unwrap();
        assert_eq!(c.port, DEFAULT_PORT);
        std::fs::write(dir.path().join("config.toml"), "port = 5000").unwrap();
        assert_eq!(
            ServerConfig::load(dir.path().to_path_buf()).unwrap().port,
            5000
        );
    }
}
