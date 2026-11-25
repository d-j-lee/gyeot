//! 투명 오버레이 창: 만들기, 오른쪽 아래 고정, 선택적 클릭 통과, 데몬 연결.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use tauri::ipc::CapabilityBuilder;
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, State, Url, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::geometry::{self, HitRect, WorkArea};
use crate::{daemon, paths, platform};

pub const MAIN: &str = "main";
/// 접힌 창(논리 px).
pub const COLLAPSED: (f64, f64) = (360.0, 520.0);
/// 패널을 펼친 창(논리 px).
pub const EXPANDED: (f64, f64) = (420.0, 680.0);

/// 마우스 위치를 보는 주기(약 30 Hz).
const POINTER_PERIOD: Duration = Duration::from_millis(33);
/// 창 위치·배율을 다시 읽는 주기(틱 수). 이벤트가 오면 바로 읽는다.
const GEOM_EVERY: u64 = 30;
/// 작업 영역 변화(작업 표시줄 이동·해상도 변경)를 확인하는 주기(틱 수, 약 3초).
const ANCHOR_EVERY: u64 = 90;
/// 데몬 감시 주기.
const WATCH_PERIOD: Duration = Duration::from_secs(10);

pub struct AppConfig {
    pub port: u16,
    pub data_dir: Option<PathBuf>,
}

impl AppConfig {
    pub fn face_url(&self) -> Url {
        face_url(self.port)
    }
}

pub fn face_url(port: u16) -> Url {
    Url::parse(&format!("http://127.0.0.1:{port}/?mode=overlay")).expect("얼굴 주소")
}

#[derive(Debug, Clone, Copy)]
struct Geom {
    origin: (i32, i32),
    scale: f64,
}

#[derive(Default)]
pub struct OverlayState {
    hit: Mutex<Option<HitRect>>,
    geom: Mutex<Option<Geom>>,
    geom_dirty: AtomicBool,
    reanchor: AtomicBool,
    expanded: AtomicBool,
    /// 마지막으로 창에 적용한 클릭 통과 상태.
    ignoring: AtomicBool,
    /// 창이 보이는지(우리가 보이고 숨긴 기록).
    shown: AtomicBool,
    /// 사람이 숨겼으면 페이지가 다시 읽혀도 저절로 보이지 않는다.
    user_hidden: AtomicBool,
    on_face: AtomicBool,
    connecting: AtomicBool,
    quitting: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl OverlayState {
    pub fn set_quitting(&self) {
        self.quitting.store(true, SeqCst);
    }

    pub fn is_quitting(&self) -> bool {
        self.quitting.load(SeqCst)
    }

    #[cfg(test)]
    pub fn hit_rect(&self) -> Option<HitRect> {
        *lock(&self.hit)
    }

    #[cfg(test)]
    pub fn is_expanded(&self) -> bool {
        self.expanded.load(SeqCst)
    }
}

// ───────────────────────── 페이지 구분 ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// 데몬이 서빙하는 얼굴
    Face,
    /// 셸에 든 대체 화면(깨우는 중)
    Loading,
    /// 셸에 든 대체 화면(연결 실패)
    Error,
    Other,
}

pub fn is_daemon_url(url: &Url, port: u16) -> bool {
    url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"))
        && url.port_or_known_default() == Some(port)
        && url.username().is_empty()
        && url.password().is_none()
}

pub fn is_app_url(url: &Url) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" => url.host_str() == Some("tauri.localhost"),
        _ => false,
    }
}

pub fn classify(url: &Url, port: u16) -> Page {
    if is_daemon_url(url, port) {
        Page::Face
    } else if is_app_url(url) {
        if url.query_pairs().any(|(k, v)| k == "state" && v == "error") {
            Page::Error
        } else {
            Page::Loading
        }
    } else {
        Page::Other
    }
}

/// 셸에 든 대체 화면 주소. Tauri 2 기본(https 스킴 끔) 그대로: Windows는 http://tauri.localhost.
fn app_page(error: Option<&str>) -> Url {
    let base = if cfg!(windows) {
        "http://tauri.localhost/index.html"
    } else {
        "tauri://localhost/index.html"
    };
    let mut url = Url::parse(base).expect("대체 화면 주소");
    if let Some(reason) = error {
        url.query_pairs_mut()
            .append_pair("state", "error")
            .append_pair("reason", reason);
    }
    url
}

fn allow_navigation(url: &Url, port: u16) -> bool {
    if classify(url, port) != Page::Other || url.as_str() == "about:blank" {
        return true;
    }
    open_external(url);
    false
}

fn open_external(url: &Url) {
    if platform::is_safe_external(url) {
        if !platform::open_in_browser(url.as_str()) {
            crate::warn!("브라우저로 열지 못했어요: {url}");
        }
    } else {
        crate::warn!("허락하지 않은 주소로 가려는 것을 막았어요: {url}");
    }
}

// ───────────────────────── 창 ─────────────────────────

pub fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let port = app.state::<AppConfig>().port;
    let win = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("곁")
        .inner_size(COLLAPSED.0, COLLAPSED.1)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(false)
        .disable_drag_drop_handler()
        .on_navigation(move |url| allow_navigation(url, port))
        .on_new_window(|url, _features| {
            open_external(&url);
            NewWindowResponse::Deny
        })
        .on_page_load(move |win, payload| on_page_load(&win, payload.url(), payload.event(), port))
        .build()?;

    let st = app.state::<OverlayState>();
    // 처음에는 모두 통과. 얼굴이 클릭 영역을 알려 주면 그 안에서만 받는다.
    win.set_ignore_cursor_events(true)?;
    st.ignoring.store(true, SeqCst);

    let handle = app.clone();
    win.on_window_event(move |event| {
        let st = handle.state::<OverlayState>();
        match event {
            WindowEvent::CloseRequested { api, .. } => {
                // Alt+F4 등으로 닫혀도 트레이가 남도록 숨기기만 한다.
                api.prevent_close();
                hide(&handle);
            }
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => st.geom_dirty.store(true, SeqCst),
            WindowEvent::ScaleFactorChanged { .. } => {
                st.geom_dirty.store(true, SeqCst);
                st.reanchor.store(true, SeqCst);
            }
            _ => {}
        }
    });

    if let Err(e) = anchor(&win, false) {
        crate::warn!("처음 자리 잡기 실패: {e}");
    }
    refresh_geom(&win, &st);
    Ok(win)
}

fn on_page_load(win: &WebviewWindow, url: &Url, event: PageLoadEvent, port: u16) {
    let st = win.state::<OverlayState>();
    match event {
        PageLoadEvent::Started => {
            // 새 문서: 이전 페이지의 클릭 영역은 더 맞지 않는다. 펼침도 접힌 상태로 돌린다.
            *lock(&st.hit) = None;
            if st.expanded.swap(false, SeqCst) {
                st.reanchor.store(true, SeqCst);
            }
        }
        PageLoadEvent::Finished => {
            let page = classify(url, port);
            st.on_face.store(page == Page::Face, SeqCst);
            st.geom_dirty.store(true, SeqCst);
            crate::info!("페이지 읽음: {page:?}");
            if matches!(page, Page::Face | Page::Error) && !st.user_hidden.load(SeqCst) {
                show_window(win, &st);
            }
        }
    }
}

fn show_window(win: &WebviewWindow, st: &OverlayState) {
    match win.show() {
        Ok(()) => {
            st.shown.store(true, SeqCst);
            st.geom_dirty.store(true, SeqCst);
            st.reanchor.store(true, SeqCst);
        }
        Err(e) => crate::warn!("창을 보이지 못했어요: {e}"),
    }
}

pub fn show(app: &AppHandle) {
    let st = app.state::<OverlayState>();
    st.user_hidden.store(false, SeqCst);
    if let Some(win) = app.get_webview_window(MAIN) {
        show_window(&win, &st);
    }
}

pub fn hide(app: &AppHandle) {
    let st = app.state::<OverlayState>();
    st.user_hidden.store(true, SeqCst);
    if let Some(win) = app.get_webview_window(MAIN) {
        match win.hide() {
            Ok(()) => st.shown.store(false, SeqCst),
            Err(e) => crate::warn!("창을 숨기지 못했어요: {e}"),
        }
    }
}

pub fn toggle(app: &AppHandle) {
    if app.state::<OverlayState>().shown.load(SeqCst) {
        hide(app);
    } else {
        show(app);
    }
}

/// 주 모니터 작업 영역의 오른쪽 아래에 붙인다(펼침이면 더 크게).
pub fn anchor(win: &WebviewWindow, expanded: bool) -> tauri::Result<()> {
    let monitor = match win.primary_monitor()? {
        Some(m) => Some(m),
        None => win.current_monitor()?,
    };
    let Some(monitor) = monitor else {
        return Ok(());
    };
    let wa = monitor.work_area();
    let wa = WorkArea {
        x: wa.position.x,
        y: wa.position.y,
        width: wa.size.width,
        height: wa.size.height,
    };
    let logical = if expanded { EXPANDED } else { COLLAPSED };
    let size = geometry::fit_size(geometry::physical_size(logical, monitor.scale_factor()), wa);

    let inner = win.inner_size()?;
    let outer = win.outer_size()?;
    // 테두리 없는 창은 보통 0이지만, 있으면 그만큼 더해서 바깥 테두리가 작업 영역 안에 들게 한다.
    let border = (
        outer.width.saturating_sub(inner.width),
        outer.height.saturating_sub(inner.height),
    );
    let pos = geometry::bottom_right(wa, (size.0 + border.0, size.1 + border.1));
    let cur = (inner.width, inner.height);
    let cur_pos = win.outer_position()?;
    if cur == size && (cur_pos.x, cur_pos.y) == pos {
        return Ok(());
    }
    let set_size = || win.set_size(PhysicalSize::new(size.0, size.1));
    let set_pos = || win.set_position(PhysicalPosition::new(pos.0, pos.1));
    if geometry::move_before_resize(cur, size) {
        set_pos()?;
        set_size()?;
    } else {
        set_size()?;
        set_pos()?;
    }
    Ok(())
}

fn refresh_geom(win: &WebviewWindow, st: &OverlayState) {
    if let (Ok(p), Ok(scale)) = (win.inner_position(), win.scale_factor()) {
        *lock(&st.geom) = Some(Geom {
            origin: (p.x, p.y),
            scale,
        });
    }
}

// ───────────────────────── 얼굴이 부르는 명령 ─────────────────────────

/// 클릭을 받을 영역(CSS px, 웹뷰 기준). 넓이가 0이면 어디도 받지 않는다.
#[tauri::command]
pub fn set_hit_rect<R: Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, OverlayState>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) {
    if window.label() != MAIN {
        return;
    }
    *lock(&state.hit) = HitRect::sanitize(x, y, width, height);
}

/// 패널을 펼쳤는지. 창 크기를 바꾸되 오른쪽 아래 모서리는 그대로 둔다.
#[tauri::command]
pub fn set_expanded<R: Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, OverlayState>,
    expanded: bool,
) {
    if window.label() != MAIN {
        return;
    }
    if state.expanded.swap(expanded, SeqCst) != expanded {
        state.reanchor.store(true, SeqCst);
    }
}

/// 대체 화면의 "다시 시도"(셸에 든 페이지만 부를 수 있다. capabilities/overlay-local.json).
#[tauri::command]
pub fn retry_connect(app: AppHandle, restart: Option<bool>) {
    connect_async(app, restart.unwrap_or(false));
}

// ───────────────────────── 클릭 통과 ─────────────────────────

/// 마우스가 클릭 영역 안이면 클릭을 받고, 밖이면 통과시킨다.
pub fn decide_ignore(inside: bool, currently_ignoring: bool, button_down: bool) -> bool {
    if inside {
        return false;
    }
    // 받는 중에 단추를 누른 채 밖으로 끌면(선택·끌기) 놓을 때까지 받는다.
    if !currently_ignoring && button_down {
        return false;
    }
    true
}

pub fn start_pointer_thread(app: AppHandle) {
    let spawned = thread::Builder::new()
        .name("gyeot-pointer".into())
        .spawn(move || pointer_loop(&app));
    if let Err(e) = spawned {
        crate::warn!("클릭 통과 스레드를 띄우지 못했어요: {e}");
    }
}

fn pointer_loop(app: &AppHandle) {
    let mut tick: u64 = 0;
    loop {
        thread::sleep(POINTER_PERIOD);
        tick = tick.wrapping_add(1);
        let st = app.state::<OverlayState>();
        if st.quitting.load(SeqCst) {
            return;
        }
        let Some(win) = app.get_webview_window(MAIN) else {
            continue;
        };
        if st.reanchor.swap(false, SeqCst) || tick % ANCHOR_EVERY == 0 {
            if let Err(e) = anchor(&win, st.expanded.load(SeqCst)) {
                crate::warn!("자리 잡기 실패: {e}");
            }
            st.geom_dirty.store(true, SeqCst);
        }
        if st.geom_dirty.swap(false, SeqCst) || tick % GEOM_EVERY == 0 {
            refresh_geom(&win, &st);
        }
        if !st.shown.load(SeqCst) {
            continue;
        }
        let cursor =
            platform::cursor_position().or_else(|| win.cursor_position().ok().map(|p| (p.x, p.y)));
        let Some((cx, cy)) = cursor else {
            continue;
        };
        let inside = match (*lock(&st.hit), *lock(&st.geom)) {
            (Some(hit), Some(g)) => hit.to_screen(g.origin, g.scale).contains(cx, cy),
            _ => false,
        };
        let currently = st.ignoring.load(SeqCst);
        let want = decide_ignore(inside, currently, platform::mouse_button_down());
        if want != currently {
            match win.set_ignore_cursor_events(want) {
                Ok(()) => st.ignoring.store(want, SeqCst),
                Err(e) => crate::warn!("클릭 통과 바꾸기 실패: {e}"),
            }
        }
    }
}

// ───────────────────────── 데몬 연결 ─────────────────────────

/// 데몬을 확인하거나 띄운 뒤 얼굴을 연다. 실패하면 대체 화면에 이유를 보인다.
pub fn connect_async(app: AppHandle, restart: bool) {
    if app.state::<OverlayState>().connecting.swap(true, SeqCst) {
        return;
    }
    let worker = app.clone();
    let spawned = thread::Builder::new()
        .name("gyeot-connect".into())
        .spawn(move || {
            connect(&worker, restart);
            worker
                .state::<OverlayState>()
                .connecting
                .store(false, SeqCst);
        });
    if let Err(e) = spawned {
        crate::warn!("연결 스레드를 띄우지 못했어요: {e}");
        app.state::<OverlayState>().connecting.store(false, SeqCst);
    }
}

fn connect(app: &AppHandle, restart: bool) -> bool {
    let cfg = app.state::<AppConfig>();
    let result = if restart {
        daemon::restart(cfg.port, cfg.data_dir.as_deref())
    } else {
        daemon::ensure_running(cfg.port, cfg.data_dir.as_deref())
    };
    let Some(win) = app.get_webview_window(MAIN) else {
        return false;
    };
    let (target, ok) = match result {
        Ok(how) => {
            crate::info!("곁 데몬 준비됨({how:?}), 얼굴을 엽니다");
            (cfg.face_url(), true)
        }
        Err(reason) => {
            crate::warn!("곁 데몬 연결 실패: {reason}");
            (app_page(Some(&reason)), false)
        }
    };
    if let Err(e) = win.navigate(target) {
        crate::warn!("페이지를 열지 못했어요: {e}");
    }
    ok
}

/// 데몬이 죽으면 다시 띄우고(지수 대기), 다른 경로로 살아나면(작업 스케줄러 등) 얼굴을 연다.
pub fn start_watchdog(app: AppHandle) {
    let spawned = thread::Builder::new()
        .name("gyeot-watchdog".into())
        .spawn(move || {
            let mut failures: u32 = 0;
            let mut next_attempt = Instant::now();
            loop {
                thread::sleep(WATCH_PERIOD);
                let st = app.state::<OverlayState>();
                if st.quitting.load(SeqCst) {
                    return;
                }
                if st.connecting.load(SeqCst) {
                    continue;
                }
                let port = app.state::<AppConfig>().port;
                if daemon::is_healthy(port) {
                    failures = 0;
                    if !st.on_face.load(SeqCst) {
                        crate::info!("곁 데몬이 응답해요. 얼굴을 엽니다");
                        connect_async(app.clone(), false);
                    }
                    continue;
                }
                failures = failures.saturating_add(1);
                if failures >= 2 && Instant::now() >= next_attempt {
                    let backoff = backoff_after(failures);
                    next_attempt = Instant::now() + backoff;
                    crate::warn!("곁 데몬이 응답하지 않아요({failures}번째). 다시 띄워 봅니다(다음 시도까지 {}초)", backoff.as_secs());
                    connect_async(app.clone(), false);
                }
            }
        });
    if let Err(e) = spawned {
        crate::warn!("감시 스레드를 띄우지 못했어요: {e}");
    }
}

/// 연속 실패 횟수에 따른 다음 재시작 시도까지의 간격(20초부터 두 배씩, 최대 10분).
pub fn backoff_after(failures: u32) -> Duration {
    let exp = failures.saturating_sub(2).min(5);
    Duration::from_secs((20u64 << exp).min(600))
}

/// 설정으로 포트를 바꿨으면 그 포트의 얼굴에도 같은 두 명령만 허락한다.
pub fn allow_custom_port(app: &AppHandle, port: u16) -> tauri::Result<()> {
    if port == paths::DEFAULT_PORT {
        return Ok(());
    }
    let cap = CapabilityBuilder::new("overlay-remote-custom-port")
        .local(false)
        .remote(format!("http://127.0.0.1:{port}/*"))
        .remote(format!("http://localhost:{port}/*"))
        .window(MAIN)
        .permission("allow-set-hit-rect")
        .permission("allow-set-expanded");
    app.add_capability(cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn face_url_is_overlay_mode() {
        assert_eq!(
            face_url(47321).as_str(),
            "http://127.0.0.1:47321/?mode=overlay"
        );
        assert_eq!(classify(&face_url(47321), 47321), Page::Face);
    }

    #[test]
    fn classify_pages() {
        assert_eq!(
            classify(&u("http://localhost:47321/assets/x.js"), 47321),
            Page::Face
        );
        assert_eq!(classify(&u("http://127.0.0.1:47322/"), 47321), Page::Other);
        assert_eq!(classify(&u("https://127.0.0.1:47321/"), 47321), Page::Other);
        assert_eq!(
            classify(&u("http://evil@127.0.0.1:47321/"), 47321),
            Page::Other
        );
        assert_eq!(
            classify(&u("http://tauri.localhost/index.html"), 47321),
            Page::Loading
        );
        assert_eq!(
            classify(&u("tauri://localhost/index.html"), 47321),
            Page::Loading
        );
        assert_eq!(classify(&app_page(Some("이유")), 47321), Page::Error);
        assert_eq!(classify(&u("https://claude.ai/"), 47321), Page::Other);
        assert_eq!(
            classify(&u("http://tauri.localhost.evil.com/"), 47321),
            Page::Other
        );
    }

    #[test]
    fn error_page_carries_reason() {
        let url = app_page(Some("포트 47321를 다른 프로그램이 쓰고 있어요 & ?"));
        let reason: String = url
            .query_pairs()
            .find(|(k, _)| k == "reason")
            .map(|(_, v)| v.into_owned())
            .unwrap();
        assert_eq!(reason, "포트 47321를 다른 프로그램이 쓰고 있어요 & ?");
        assert!(is_app_url(&url));
    }

    #[test]
    fn click_through_decision() {
        // 안이면 받는다
        assert!(!decide_ignore(true, true, false));
        assert!(!decide_ignore(true, false, true));
        // 밖이면 통과
        assert!(decide_ignore(false, false, false));
        assert!(decide_ignore(false, true, false));
        // 받는 중에 단추를 누른 채 나가면 놓을 때까지 받는다
        assert!(!decide_ignore(false, false, true));
        // 통과 중에 단추를 누른 채 밖에 있으면 계속 통과
        assert!(decide_ignore(false, true, true));
    }

    #[test]
    fn watchdog_backoff_grows_and_caps() {
        assert_eq!(backoff_after(2), Duration::from_secs(20));
        assert_eq!(backoff_after(3), Duration::from_secs(40));
        assert_eq!(backoff_after(5), Duration::from_secs(160));
        assert_eq!(backoff_after(7), Duration::from_secs(600));
        assert_eq!(backoff_after(100), Duration::from_secs(600));
    }
}
