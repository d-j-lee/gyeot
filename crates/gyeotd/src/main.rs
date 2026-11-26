//! 곁 데몬: `gyeotd`(서버), `--open [--print]`, `--init`, `--print-config`, `--version`.

use anyhow::{Context, Result, bail};
use gyeot_server::ServerConfig;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;
/// `--open`이 데몬의 답을 기다리는 시간.
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);
/// 시작할 때 보이는 얼굴 여는 방법.
/// 데몬을 다시 띄운 뒤의 안내(계약 §14.1: 세션 열쇠는 데몬 메모리에만 있다). 셸은 저절로 다시 연다.
macro_rules! reopen_note {
    () => {
        "데몬을 다시 띄우면 이전 열쇠는 쓸 수 없어요. 브라우저로 연 얼굴은 gyeotd --open으로 다시 여세요"
    };
}
const OPEN_HINT: &str = concat!("얼굴 열기: gyeotd --open (", reopen_note!(), ")");
/// hello가 맞지 않을 때(계약 §13.1).
const NOT_MY_DAEMON: &str =
    "이 포트의 서버는 내 곁 데몬이 아니에요(다른 사용자나 프로그램일 수 있어요)";
/// 열기 쪽을 남겨 두는 시간. 넘으면 다음 `--open`이 지운다(계약 §12.1).
const OPEN_PAGE_MAX_AGE: Duration = Duration::from_secs(5 * 60);

const HELP: &str = concat!(
    "곁 데몬

사용:
  gyeotd                 서버를 돌린다(Ctrl+C로 멈춤)
  gyeotd --open          실행 중인 데몬이 내 데몬인지 확인하고 일회용 열기 코드를 받아 기본 브라우저로 얼굴을 연다
                         (주소는 프로그램 인자가 아니라 <데이터>/open/의 나만 읽는 파일로 넘긴다)
  gyeotd --open --print  여는 대신 주소만 표준 출력에 보인다(120초 안에 한 번 쓸 수 있다)
                         ",
    reopen_note!(),
    " (트레이의 셸은 저절로 다시 열어요)
  gyeotd --init          데이터 폴더와 토큰을 만들고 경로를 보인다
  gyeotd --print-config  실제로 쓰는 설정을 보인다
  gyeotd --version       판을 보인다
  gyeotd --help          이 도움말

환경 변수: GYEOT_HOME(데이터 폴더), GYEOT_PORT(포트), GYEOT_FACE_DIR(얼굴 폴더), RUST_LOG(로그 수준)"
);

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

/// 기본 브라우저로 열기 쪽 **파일**을 여는 명령(계약 §12.1). 인자에 주소·코드를 싣지 않는다: Linux·macOS에서는
/// 다른 사용자가 `ps`·`/proc/*/cmdline`으로 인자를 읽을 수 있다(R2 N1, G-033).
fn opener(page: &Path) -> (&'static str, Vec<OsString>) {
    let p = page.as_os_str().to_owned();
    if cfg!(windows) {
        ("rundll32", vec!["url.dll,FileProtocolHandler".into(), p])
    } else if cfg!(target_os = "macos") {
        ("open", vec![p])
    } else {
        ("xdg-open", vec![p])
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

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(OPEN_TIMEOUT))
        .proxy(None)
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into()
}

/// 그 포트의 서버가 내 토큰을 아는지 확인한다(계약 §13.1, G-032). 토큰은 보내지 않는다.
///
/// 32바이트 난수 `n`으로 `GET /v1/hello?n=`을 부르고, 내 토큰과 **내가 연결한 포트**로 계산한 증명과
/// 상수 시간으로 비교한다. 연결이 안 되면 "닿지 않아요", 답이 다르거나 이상하면 [`NOT_MY_DAEMON`].
fn verify_daemon(agent: &ureq::Agent, port: u16, token: &str) -> Result<()> {
    let base = format!("http://127.0.0.1:{port}");
    let n = gyeot_server::store::new_token()?;
    let mut resp = agent
        .get(format!("{base}/v1/hello?n={n}"))
        .call()
        .map_err(|e| {
            anyhow::anyhow!("곁 데몬에 닿지 않아요({base}). 먼저 gyeotd를 띄우세요 — {e}")
        })?;
    let status = resp.status().as_u16();
    let body = resp
        .body_mut()
        .with_config()
        .limit(4096)
        .read_to_string()
        .unwrap_or_default();
    let proof = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .filter(|v| status == 200 && v["v"] == 1)
        .and_then(|v| v["proof"].as_str().map(str::to_owned));
    let expected = gyeot_server::auth::hello_proof(token, port, &n);
    match proof {
        Some(p) if gyeot_server::http::ct_eq(p.as_bytes(), expected.as_bytes()) => Ok(()),
        _ => bail!("{NOT_MY_DAEMON} — {base}"),
    }
}

/// 실행 중인 데몬을 확인한 뒤(§13.1) `POST /v1/boot` 해서 일회용 열기 주소를 받는다.
fn boot_url(cfg: &ServerConfig) -> Result<String> {
    let token = read_token(&cfg.token_path())?;
    let base = format!("http://127.0.0.1:{}", cfg.port);
    let agent = agent();
    verify_daemon(&agent, cfg.port, &token)?;
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
        401 => bail!("토큰이 맞지 않아요({base}). 데몬을 다시 띄웠다면 다시 해 보세요"),
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

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// 열기 쪽: 그 주소로 바로 넘어가고(meta refresh), 안 되면 누를 링크. 스크립트는 없다.
fn open_page_html(url: &str) -> String {
    let u = html_escape(url);
    format!(
        r#"<!doctype html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="referrer" content="no-referrer">
<meta http-equiv="refresh" content="0;url={u}">
<title>곁 열기</title>
</head>
<body>
<p>곁의 얼굴로 넘어가요. 넘어가지 않으면 <a href="{u}">여기</a>를 누르세요(120초 안에 한 번 쓸 수 있어요).</p>
</body>
</html>
"#
    )
}

/// `<dir>/open-<난수>.html`에 열기 쪽을 쓴다. 폴더 0700, 파일 0600(Unix), 이미 있는 이름은 쓰지 않는다.
fn write_open_page(dir: &Path, url: &str) -> Result<PathBuf> {
    gyeot_server::store::create_private_dir(dir)?;
    let path = dir.join(format!("open-{}.html", gyeot_server::store::random_hex(16)));
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(&path)
        .with_context(|| format!("열기 파일 {}을 만들지 못했어요", path.display()))?;
    f.write_all(open_page_html(url).as_bytes())
        .with_context(|| format!("열기 파일 {}을 쓰지 못했어요", path.display()))?;
    Ok(path)
}

/// 5분 넘은 `open-*.html`을 지운다. 지운 수. 다른 이름·폴더·링크가 아닌 것만 본다.
fn cleanup_open_pages(dir: &Path, now: SystemTime) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut n = 0;
    for e in rd.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("open-") && name.ends_with(".html")) {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(e.path()) else {
            continue;
        };
        let old = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age > OPEN_PAGE_MAX_AGE);
        if meta.file_type().is_file() && old && std::fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }
    n
}

fn open_face(print: bool) -> Result<()> {
    let cfg = ServerConfig::from_env()?;
    let url = boot_url(&cfg)?;
    let dir = cfg.home.join("open");
    cleanup_open_pages(&dir, SystemTime::now());
    if print {
        println!("{url}");
        return Ok(());
    }
    let page = write_open_page(&dir, &url)?;
    let (prog, args) = opener(&page);
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
            "브라우저를 열지 못했어요({prog}: {s}). 120초 안에 이 파일을 브라우저로 여세요: {}",
            page.display()
        ),
        Err(e) => bail!(
            "브라우저를 열지 못했어요({prog}: {e}). 120초 안에 이 파일을 브라우저로 여세요: {}",
            page.display()
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

/// `<데이터>/gyeotd.log`를 연다. Unix에서는 데이터 폴더 0700, 로그 0600(이미 있던 것도, 계약 §13.4).
fn open_log(home: &Path) -> Option<std::fs::File> {
    gyeot_server::store::create_private_dir(home).ok()?;
    let path = home.join("gyeotd.log");
    let too_big = std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_MAX_BYTES);
    let mut opts = OpenOptions::new();
    opts.create(true);
    if too_big {
        opts.write(true).truncate(true);
    } else {
        opts.append(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let f = opts.open(&path).ok()?;
    gyeot_server::store::secure_home(home);
    Some(f)
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

    /// R2 N1(G-033): 브라우저를 여는 명령의 인자는 열기 파일 경로뿐이다(주소·코드를 싣지 않는다).
    #[test]
    fn opener_gets_only_the_page_path() {
        let page = Path::new("/h/open/open-0123456789abcdef.html");
        let (prog, args) = opener(page);
        let args: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let p = page.display().to_string();
        if cfg!(windows) {
            assert_eq!(prog, "rundll32");
            assert_eq!(args, vec!["url.dll,FileProtocolHandler".to_owned(), p]);
        } else if cfg!(target_os = "macos") {
            assert_eq!((prog, args), ("open", vec![p]));
        } else {
            assert_eq!((prog, args), ("xdg-open", vec![p]));
        }
    }

    const URL: &str = "http://127.0.0.1:47321/?boot=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn open_page_redirects_to_the_url_with_a_link() {
        let html = open_page_html(URL);
        assert!(html.starts_with("<!doctype html>"), "{html}");
        assert!(html.contains(r#"<html lang="ko">"#));
        assert!(
            html.contains(&format!(
                r#"<meta http-equiv="refresh" content="0;url={URL}">"#
            )),
            "{html}"
        );
        assert!(html.contains(&format!(r#"<a href="{URL}">"#)), "{html}");
        assert!(html.contains(r#"<meta name="referrer" content="no-referrer">"#));
        assert!(!html.contains("<script"), "스크립트 없이 넘어간다");
        // 혹시 모를 특수 문자는 HTML로 가린다
        let odd = open_page_html("http://x/?a=1&b=\"<>'");
        assert!(odd.contains("a=1&amp;b=&quot;&lt;&gt;&#39;"), "{odd}");
        assert!(!odd.contains("b=\"<"), "{odd}");
    }

    #[test]
    fn open_page_file_is_private_and_unique() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("open");
        let a = write_open_page(&dir, URL).unwrap();
        let b = write_open_page(&dir, URL).unwrap();
        assert_ne!(a, b, "이름은 실행마다 다르다");
        for p in [&a, &b] {
            assert_eq!(p.parent(), Some(dir.as_path()));
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let mid = name
                .strip_prefix("open-")
                .and_then(|s| s.strip_suffix(".html"))
                .unwrap_or_else(|| panic!("{name}"));
            assert_eq!(mid.len(), 32, "{name}");
            assert!(mid.bytes().all(|c| c.is_ascii_hexdigit()));
            assert_eq!(std::fs::read_to_string(p).unwrap(), open_page_html(URL));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&dir), 0o700);
            assert_eq!(mode(&a), 0o600);
            // 이미 있던 느슨한 폴더도 좁힌다
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            let c = write_open_page(&dir, URL).unwrap();
            assert_eq!(mode(&dir), 0o700);
            assert_eq!(mode(&c), 0o600);
        }
    }

    #[test]
    fn old_open_pages_are_removed_after_five_minutes() {
        use std::time::SystemTime;
        let d = tempfile::tempdir().unwrap();
        let dir = d.path();
        let now = SystemTime::now();
        let make = |name: &str, age_s: u64| {
            let p = dir.join(name);
            std::fs::write(&p, "x").unwrap();
            let f = std::fs::File::options().write(true).open(&p).unwrap();
            f.set_modified(now - Duration::from_secs(age_s)).unwrap();
            p
        };
        let old = make("open-aaaa.html", 5 * 60 + 1);
        let fresh = make("open-bbbb.html", 5 * 60 - 5);
        let other = make("notes.html", 3600);
        let other2 = make("open-cccc.txt", 3600);
        std::fs::create_dir(dir.join("open-dddd.html")).unwrap();
        assert_eq!(cleanup_open_pages(dir, now), 1);
        assert!(!old.exists());
        assert!(fresh.exists() && other.exists() && other2.exists());
        assert!(
            dir.join("open-dddd.html").is_dir(),
            "폴더는 건드리지 않는다"
        );
        assert_eq!(cleanup_open_pages(dir, now + Duration::from_secs(10)), 1);
        assert!(!fresh.exists());
        // 폴더가 없어도 괜찮다
        assert_eq!(cleanup_open_pages(&dir.join("none"), now), 0);
    }

    /// R3 L3(계약 §13.4): 로그는 서버가 묶이기 전에 열리므로 여기서부터 0600이다(데이터 폴더 0700).
    #[cfg(unix)]
    #[test]
    fn log_file_is_private_from_the_start() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        drop(open_log(&home).unwrap());
        assert_eq!(mode(&home), 0o700);
        assert_eq!(mode(&home.join("gyeotd.log")), 0o600);
        // 이미 있던 느슨한 로그도
        let p = home.join("gyeotd.log");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        drop(open_log(&home).unwrap());
        assert_eq!(mode(&p), 0o600);
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
