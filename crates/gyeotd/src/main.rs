//! 곁 데몬: `gyeotd`(서버), `--init`, `--print-config`, `--version`.

use anyhow::Result;
use gyeot_server::ServerConfig;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;

const HELP: &str = "곁 데몬

사용:
  gyeotd                 서버를 돌린다(Ctrl+C로 멈춤)
  gyeotd --init          데이터 폴더와 토큰을 만들고 경로를 보인다
  gyeotd --print-config  실제로 쓰는 설정을 보인다
  gyeotd --version       판을 보인다
  gyeotd --help          이 도움말

환경 변수: GYEOT_HOME(데이터 폴더), GYEOT_PORT(포트), GYEOT_FACE_DIR(얼굴 폴더), RUST_LOG(로그 수준)";

#[derive(Debug, PartialEq, Eq)]
enum Cmd {
    Run,
    Init,
    PrintConfig,
    Version,
    Help,
}

fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Cmd, String> {
    let mut cmd = Cmd::Run;
    for a in args {
        let next = match a.as_str() {
            "--init" => Cmd::Init,
            "--print-config" => Cmd::PrintConfig,
            "--version" | "-V" => Cmd::Version,
            "--help" | "-h" => Cmd::Help,
            other => return Err(format!("모르는 인자 '{other}'예요. --help를 보세요")),
        };
        if cmd != Cmd::Run && cmd != next {
            return Err("한 번에 하나만 쓸 수 있어요".to_owned());
        }
        cmd = next;
    }
    Ok(cmd)
}

/// 표준 오류와 로그 파일에 함께 쓴다.
#[derive(Clone)]
struct Tee {
    file: Option<Arc<Mutex<std::fs::File>>>,
}

struct TeeWriter {
    file: Option<Arc<Mutex<std::fs::File>>>,
}

impl Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if let Some(f) = &self.file
            && let Ok(mut f) = f.lock()
        {
            let _ = f.write_all(buf);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Tee {
    type Writer = TeeWriter;
    fn make_writer(&'a self) -> TeeWriter {
        TeeWriter {
            file: self.file.clone(),
        }
    }
}

fn open_log(home: &Path) -> Option<std::fs::File> {
    std::fs::create_dir_all(home).ok()?;
    let path = home.join("gyeotd.log");
    let too_big = std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_MAX_BYTES);
    let mut opts = OpenOptions::new();
    opts.create(true);
    if too_big {
        opts.write(true).truncate(true);
    } else {
        opts.append(true);
    }
    opts.open(path).ok()
}

fn init_logging(home: Option<&Path>) {
    let file = home.and_then(open_log).map(|f| Arc::new(Mutex::new(f)));
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(false)
        .with_writer(Tee { file })
        .try_init();
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    tracing::info!("멈추라는 신호를 받았어요");
}

fn run(cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Version => {
            println!("gyeotd {VERSION}");
            Ok(())
        }
        Cmd::Help => {
            println!("{HELP}");
            Ok(())
        }
        Cmd::PrintConfig => {
            let cfg = ServerConfig::from_env()?;
            print!("{}", cfg.to_toml());
            Ok(())
        }
        Cmd::Init => {
            let cfg = ServerConfig::from_env()?;
            gyeot_server::init_home(&cfg)?;
            println!("데이터 폴더: {}", cfg.home.display());
            println!("토큰: {}", cfg.token_path().display());
            println!("설정 파일: {}", cfg.config_path().display());
            println!("이벤트 로그: {}", cfg.events_dir().display());
            println!("스풀: {}", cfg.spool_dir().display());
            println!("주소: http://127.0.0.1:{}", cfg.port);
            Ok(())
        }
        Cmd::Run => {
            let cfg = ServerConfig::from_env()?;
            init_logging(Some(&cfg.home));
            for w in &cfg.warnings {
                tracing::warn!("설정: {w}");
            }
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            rt.block_on(gyeot_server::serve(cfg, shutdown_signal()))
        }
    }
}

fn main() -> std::process::ExitCode {
    let cmd = match parse_args(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    };
    match run(cmd) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("곁 데몬 오류: {e:#}");
            tracing::error!("{e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Result<Cmd, String> {
        parse_args(a.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn args() {
        assert_eq!(p(&[]), Ok(Cmd::Run));
        assert_eq!(p(&["--init"]), Ok(Cmd::Init));
        assert_eq!(p(&["--print-config"]), Ok(Cmd::PrintConfig));
        assert_eq!(p(&["--version"]), Ok(Cmd::Version));
        assert_eq!(p(&["-V"]), Ok(Cmd::Version));
        assert_eq!(p(&["--help"]), Ok(Cmd::Help));
        assert!(p(&["--bogus"]).is_err());
        assert!(p(&["--init", "--version"]).is_err());
        assert_eq!(p(&["--init", "--init"]), Ok(Cmd::Init));
    }

    #[test]
    fn log_file_is_truncated_when_too_big() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("gyeotd.log");
        std::fs::write(&path, vec![b'x'; (LOG_MAX_BYTES + 1) as usize]).unwrap();
        drop(open_log(d.path()).unwrap());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        std::fs::write(&path, b"abc").unwrap();
        let mut f = open_log(d.path()).unwrap();
        f.write_all(b"d").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"abcd");
    }
}
