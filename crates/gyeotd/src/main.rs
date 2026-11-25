//! 곁 데몬: `gyeotd`(서버), `--open [--print]`, `--init`, `--print-config`, `--version`.

use anyhow::{Context, Result, bail};
use gyeot_server::ServerConfig;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;
/// `--open`이 데몬의 답을 기다리는 시간.
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);
/// 시작할 때 보이는 얼굴 여는 방법.
const OPEN_HINT: &str = "얼굴 열기: gyeotd --open";

const HELP: &str = "곁 데몬

사용:
  gyeotd                 서버를 돌린다(Ctrl+C로 멈춤)
  gyeotd --open          실행 중인 데몬에서 일회용 열기 코드를 받아 기본 브라우저로 얼굴을 연다
  gyeotd --open --print  여는 대신 주소만 보인다(120초 안에 한 번 쓸 수 있다)
  gyeotd --init          데이터 폴더와 토큰을 만들고 경로를 보인다
  gyeotd --print-config  실제로 쓰는 설정을 보인다
  gyeotd --version       판을 보인다
  gyeotd --help          이 도움말

환경 변수: GYEOT_HOME(데이터 폴더), GYEOT_PORT(포트), GYEOT_FACE_DIR(얼굴 폴더), RUST_LOG(로그 수준)";

#[derive(Debug, PartialEq, Eq)]
enum Cmd {
    Run,
    Open { print: bool },
    Init,
    PrintConfig,
    Version,
    Help,
}

fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Cmd, String> {
    let mut cmd = Cmd::Run;
    let mut print = false;
    for a in args {
        let next = match a.as_str() {
            "--open" => Cmd::Open { print: false },
            "--print" => {
                print = true;
                continue;
            }
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
    match cmd {
        Cmd::Open { .. } => Ok(Cmd::Open { print }),
        _ if print => Err("--print는 --open과 함께 써요".to_owned()),
        other => Ok(other),
    }
}

/// 기본 브라우저로 주소를 여는 명령(계약 §12.1).
fn opener(url: &str) -> (&'static str, Vec<String>) {
    if cfg!(windows) {
        (
            "rundll32",
            vec!["url.dll,FileProtocolHandler".to_owned(), url.to_owned()],
        )
    } else if cfg!(target_os = "macos") {
        ("open", vec![url.to_owned()])
    } else {
        ("xdg-open", vec![url.to_owned()])
    }
}

fn read_token(path: &Path) -> Result<String> {
    let s = std::fs::read_to_string(path).with_context(|| {
        format!(
            "토큰 파일 {}을 읽지 못했어요. 곁 데몬(gyeotd)을 먼저 한 번 띄우세요",
            path.display()
        )
    })?;
    let t = s.trim();
    if t.len() != 64 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!(
            "토큰 파일 {}의 모양이 맞지 않아요(64자 hex)",
            path.display()
        );
    }
    Ok(t.to_owned())
}

/// 실행 중인 데몬에 `POST /v1/boot` 해서 일회용 열기 주소를 받는다.
fn boot_url(cfg: &ServerConfig) -> Result<String> {
    let token = read_token(&cfg.token_path())?;
    let base = format!("http://127.0.0.1:{}", cfg.port);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(OPEN_TIMEOUT))
        .proxy(None)
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into();
    let mut resp = agent
        .post(format!("{base}/v1/boot"))
        .header("Authorization", &format!("Bearer {token}"))
        .send_empty()
        .map_err(|e| {
            anyhow::anyhow!("곁 데몬에 닿지 않아요({base}). 먼저 gyeotd를 띄우세요 — {e}")
        })?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    match status {
        200 => {}
        401 => bail!(
            "토큰이 맞지 않아요. 그 포트({base})에는 다른 데이터 폴더(GYEOT_HOME)의 데몬이 떠 있을 수 있어요"
        ),
        s => bail!("데몬이 {s}로 답했어요({base}/v1/boot)"),
    }
    let v: serde_json::Value = serde_json::from_str(&body).context("데몬의 답을 읽지 못했어요")?;
    let url = v["url"].as_str().unwrap_or_default();
    // 데몬이 준 주소가 이 루프백 주소의 부트 주소인지 확인한다(엉뚱한 곳을 열지 않는다).
    let ok = url
        .strip_prefix(&format!("{base}/?boot="))
        .is_some_and(|c| c.len() == 64 && c.bytes().all(|b| b.is_ascii_hexdigit()));
    if !ok {
        bail!("데몬이 준 주소가 이상해요: {url}");
    }
    Ok(url.to_owned())
}

fn open_face(print: bool) -> Result<()> {
    let cfg = ServerConfig::from_env()?;
    let url = boot_url(&cfg)?;
    if print {
        println!("{url}");
        return Ok(());
    }
    let (prog, args) = opener(&url);
    let status = std::process::Command::new(prog)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => {
            println!("곁의 얼굴을 기본 브라우저로 열었어요");
            Ok(())
        }
        Ok(s) => bail!(
            "브라우저를 열지 못했어요({prog}: {s}). 120초 안에 이 주소를 브라우저에 붙여 넣으세요: {url}"
        ),
        Err(e) => bail!(
            "브라우저를 열지 못했어요({prog}: {e}). 120초 안에 이 주소를 브라우저에 붙여 넣으세요: {url}"
        ),
    }
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
        Cmd::Open { print } => open_face(print),
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
            rt.block_on(async {
                let server = gyeot_server::bind(cfg).await?;
                tracing::info!("{OPEN_HINT}");
                server.run(shutdown_signal()).await
            })
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
        assert_eq!(p(&["--open"]), Ok(Cmd::Open { print: false }));
        assert_eq!(p(&["--open", "--print"]), Ok(Cmd::Open { print: true }));
        assert_eq!(p(&["--print", "--open"]), Ok(Cmd::Open { print: true }));
        assert!(p(&["--print"]).is_err());
        assert!(p(&["--init", "--print"]).is_err());
        assert!(p(&["--open", "--init"]).is_err());
    }

    #[test]
    fn opener_per_os() {
        let url = "http://127.0.0.1:47321/?boot=ab";
        let (prog, args) = opener(url);
        if cfg!(windows) {
            assert_eq!(prog, "rundll32");
            assert_eq!(args, vec!["url.dll,FileProtocolHandler", url]);
        } else if cfg!(target_os = "macos") {
            assert_eq!((prog, args), ("open", vec![url.to_owned()]));
        } else {
            assert_eq!((prog, args), ("xdg-open", vec![url.to_owned()]));
        }
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
