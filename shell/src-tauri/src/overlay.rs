//! 투명 오버레이 창: 만들기, 오른쪽 아래 고정, 선택적 클릭 통과, 데몬 연결.
//!
//! 얼굴 열기(docs/contracts.md §12.1): 데몬이 건강해지면 토큰으로 `POST /v1/boot` 해서 일회용 코드를
//! 받고 `http://127.0.0.1:<port>/?mode=overlay&boot=<code>`를 연다. 데몬은 `303` + 세션 쿠키로
//! `boot`를 뺀 주소로 보내고, 얼굴은 그 뒤 같은 출처 쿠키로 API를 부른다. 페이지에는 토큰이 없다.

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
/// 얼굴이 `renew_session`을 부를 수 있는 간격(401 고리에 빠져도 데몬을 두드리지 않게).
pub const RENEW_EVERY: Duration = Duration::from_secs(30);
/// `renew_session` 오류의 머리. ACL 거절과 구분된다.
pub const RENEW_ERR: &str = "곁 열쇠를 새로 받지 못했어요";

pub struct AppConfig {
    pub port: u16,
    pub data_dir: Option<PathBuf>,
}

/// 부트 코드를 실은 얼굴 주소. 데몬이 코드를 받으면 `boot`만 뺀 `/?mode=overlay`로 보낸다.
pub fn face_boot_url(port: u16, code: &str) -> Url {
    let mut url = Url::parse(&format!("http://127.0.0.1:{port}/")).expect("얼굴 주소");
    url.query_pairs_mut()
        .append_pair("mode", "overlay")
        .append_pair("boot", code);
    url
}

/// 토큰으로 새 부트 코드를 받아 얼굴 주소를 만든다(서버가 준 `url`은 쓰지 않는다).
pub fn boot_url(port: u16, data_dir: Option<&std::path::Path>) -> Result<Url, String> {
    daemon::request_boot(port, data_dir).map(|boot| face_boot_url(port, &boot.code))
}

#[derive(Debug, Clone, Copy)]
struct Geom {
    origin: (i32, i32),
    scale: f64,
}

/// `renew_session`을 30초에 한 번으로 묶는다.
#[derive(Default)]
pub struct RenewGate {
    last: Mutex<Option<Instant>>,
}

impl RenewGate {
    /// 지난번 허락에서 30초가 지났으면 지금 시각을 남기고 `Ok`. 아니면 남은 시간.
    pub fn try_acquire(&self, now: Instant) -> Result<(), Duration> {
        let mut last = lock(&self.last);
        if let Some(prev) = *last {
            let since = now.saturating_duration_since(prev);
            if since < RENEW_EVERY {
                return Err(RENEW_EVERY - since);
            }
        }
        *last = Some(now);
        Ok(())
    }
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
    /// 데몬이 부트 코드를 거절했거나 얼굴의 `renew_session`이 실패했다.
    auth_failed: AtomicBool,
    /// 지금 페이지가 셸 명령(`set_hit_rect`·`set_expanded`)을 불렀다 = 쿠키로 붙은 얼굴이 떴다.
    /// 데몬의 안내 쪽(401)은 부르지 않는다. 새 페이지를 읽기 시작하면 지운다.
    face_alive: AtomicBool,
    renew: RenewGate,
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

    #[cfg(test)]
    pub fn face_alive(&self) -> bool {
        self.face_alive.load(SeqCst)
    }

    #[cfg(test)]
    pub fn auth_failed(&self) -> bool {
        self.auth_failed.load(SeqCst)
    }
}

// ───────────────────────── 페이지 구분 ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// 데몬이 서빙하는 얼굴
    Face,
    /// 데몬 페이지인데 주소에 `boot`가 남아 있다(코드를 거절당함)
    Unauthorized,
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
        if url.query_pairs().any(|(k, _)| k == "boot") {
            Page::Unauthorized
        } else {
            Page::Face
        }
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
        PageLoadEvent::Started => page_started(&st),
        PageLoadEvent::Finished => {
            let page = classify(url, port);
            note_page(&st, page);
            st.geom_dirty.store(true, SeqCst);
            // 주소에는 부트 코드가 있을 수 있어 남기지 않는다.
            crate::info!("페이지 읽음: {page:?}");
            if page == Page::Unauthorized {
                crate::warn!("곁 데몬이 부트 코드를 받지 않았어요(안내 쪽). 트레이의 \"곁 열기\"로 다시 열 수 있어요");
            }
            if matches!(page, Page::Face | Page::Unauthorized | Page::Error)
                && !st.user_hidden.load(SeqCst)
            {
                show_window(win, &st);
            }
        }
    }
}

/// 새 문서를 읽기 시작했다: 이전 페이지의 클릭 영역·펼침·"얼굴이 떴다"는 더 맞지 않는다.
fn page_started(st: &OverlayState) {
    *lock(&st.hit) = None;
    st.face_alive.store(false, SeqCst);
    if st.expanded.swap(false, SeqCst) {
        st.reanchor.store(true, SeqCst);
    }
}

/// 다 읽은 페이지로 상태를 맞춘다. 거절된 부트(데몬의 안내 쪽)도 데몬 페이지로 쳐서 감시가 10초마다
/// 다시 열지 않게 하고(고리 방지), "곁 열기"·얼굴의 `renew_session`이 새 코드로 다시 연다.
fn note_page(st: &OverlayState, page: Page) {
    st.on_face
        .store(matches!(page, Page::Face | Page::Unauthorized), SeqCst);
    st.auth_failed.store(page == Page::Unauthorized, SeqCst);
}

/// "곁 열기"가 새 부트 코드로 다시 열어야 하는지: 얼굴이 아니거나, 코드를 거절당했거나 갱신에
/// 실패했거나, 데몬 페이지인데 셸 명령을 부른 적이 없을 때(boot 없는 주소의 안내 쪽일 수 있다).
pub fn needs_reconnect(st: &OverlayState) -> bool {
    !st.on_face.load(SeqCst) || st.auth_failed.load(SeqCst) || !st.face_alive.load(SeqCst)
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

/// 트레이 "곁 열기"·두 번째 실행: 창을 보이고 앞으로 가져온다. 얼굴이 아니거나 부트 코드를
/// 거절당했으면 새 코드로 다시 연다(데몬이 없으면 띄운다).
pub fn open(app: &AppHandle) {
    show(app);
    if let Some(win) = app.get_webview_window(MAIN) {
        if let Err(e) = win.set_focus() {
            crate::warn!("창을 앞으로 가져오지 못했어요: {e}");
        }
    }
    if needs_reconnect(&app.state::<OverlayState>()) {
        crate::info!("곁 열기: 새 부트 코드로 얼굴을 다시 엽니다");
        connect_async(app.clone(), false);
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
    state.face_alive.store(true, SeqCst);
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
    state.face_alive.store(true, SeqCst);
    if state.expanded.swap(expanded, SeqCst) != expanded {
        state.reanchor.store(true, SeqCst);
    }
}

/// 얼굴이 `401`을 받았을 때(세션 쿠키 만료·삭제) 부른다. 새 부트 코드를 받아 같은 창을 다시 연다.
/// 코드와 주소는 페이지에 돌려주지 않는다. 30초에 한 번까지(그보다 잦으면 오류 글).
/// 데몬 얼굴(capabilities/overlay-remote.json)만 부를 수 있다.
#[tauri::command]
pub async fn renew_session<R: Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, OverlayState>,
    cfg: State<'_, AppConfig>,
) -> Result<(), String> {
    if window.label() != MAIN {
        return Err(format!("{RENEW_ERR}: 곁 창이 아니에요"));
    }
    // 얼굴이 401을 받았다. 여기서 다시 열지 못하면 트레이의 "곁 열기"가 새 코드로 연다.
    state.auth_failed.store(true, SeqCst);
    if let Err(wait) = state.renew.try_acquire(Instant::now()) {
        let secs = wait.as_millis().div_ceil(1000);
        return Err(format!(
            "{RENEW_ERR}: {}초에 한 번만 다시 열 수 있어요. {secs}초 뒤에 다시 시도하세요",
            RENEW_EVERY.as_secs()
        ));
    }
    let port = cfg.port;
    let data_dir = cfg.data_dir.clone();
    let url = tauri::async_runtime::spawn_blocking(move || boot_url(port, data_dir.as_deref()))
        .await
        .map_err(|e| format!("{RENEW_ERR}: {e}"))?
        .map_err(|reason| {
            crate::warn!("얼굴이 청한 세션 갱신 실패: {reason}");
            format!("{RENEW_ERR}: {reason}")
        })?;
    crate::info!("얼굴이 세션을 새로 청해서 새 부트 코드로 다시 엽니다");
    window
        .navigate(url)
        .map_err(|e| format!("{RENEW_ERR}: {e}"))
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
    let opened = result.and_then(|how| {
        crate::info!("곁 데몬 준비됨({how:?}), 부트 코드를 받아 얼굴을 엽니다");
        boot_url(cfg.port, cfg.data_dir.as_deref())
    });
    let (target, ok) = match opened {
        Ok(url) => (url, true),
        Err(reason) => {
            crate::warn!("곁 얼굴을 열지 못했어요: {reason}");
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
            // 데몬은 건강한데 얼굴을 못 연 채(부트 코드를 못 받음 등) 남았을 때 다시 여는 횟수.
            let mut reopens: u32 = 0;
            let mut next_reopen = Instant::now();
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
                    if st.on_face.load(SeqCst) {
                        reopens = 0;
                    } else if Instant::now() >= next_reopen {
                        // 처음은 바로, 그 뒤로도 못 열면 20초부터 두 배씩(최대 10분) 기다린다.
                        reopens = reopens.saturating_add(1);
                        next_reopen = Instant::now() + backoff_after(reopens.saturating_add(1));
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

/// 설정으로 포트를 바꿨으면 그 포트의 얼굴에도 같은 세 명령만 허락한다(capabilities/overlay-remote.json과 같게).
pub fn allow_custom_port<R: Runtime>(app: &AppHandle<R>, port: u16) -> tauri::Result<()> {
    if port == paths::DEFAULT_PORT {
        return Ok(());
    }
    let cap = CapabilityBuilder::new("overlay-remote-custom-port")
        .local(false)
        .remote(format!("http://127.0.0.1:{port}/*"))
        .remote(format!("http://localhost:{port}/*"))
        .window(MAIN)
        .permission("allow-set-hit-rect")
        .permission("allow-set-expanded")
        .permission("allow-renew-session");
    app.add_capability(cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    const CODE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn boot_url_is_overlay_mode_with_code() {
        assert_eq!(
            face_boot_url(47321, CODE).as_str(),
            format!("http://127.0.0.1:47321/?mode=overlay&boot={CODE}")
        );
        assert_eq!(face_boot_url(48000, CODE).port(), Some(48000));
    }

    #[test]
    fn page_with_boot_left_in_address_is_unauthorized() {
        // 맞는 코드면 데몬이 303으로 boot를 뺀 주소로 보낸다. 다 읽은 뒤에도 남아 있으면 거절된 것.
        assert_eq!(
            classify(&face_boot_url(47321, CODE), 47321),
            Page::Unauthorized
        );
        assert_eq!(
            classify(&u("http://localhost:47321/?boot="), 47321),
            Page::Unauthorized
        );
        assert_eq!(
            classify(&u("http://127.0.0.1:47321/?mode=overlay"), 47321),
            Page::Face
        );
        assert_eq!(
            classify(&u("http://127.0.0.1:47321/?mode=overlay&reboot=1"), 47321),
            Page::Face
        );
        assert_eq!(
            classify(&u("http://127.0.0.1:47322/?boot=x"), 47321),
            Page::Other
        );
    }

    #[test]
    fn open_reconnects_unless_a_live_face_is_showing() {
        let st = OverlayState::default();
        assert!(needs_reconnect(&st), "아직 얼굴이 아니다(깨우는 중)");
        page_started(&st);
        note_page(&st, Page::Face);
        // 데몬 페이지지만 셸 명령을 부른 적이 없다 = 안내 쪽(401)일 수 있다(boot 없는 주소에서도).
        assert!(needs_reconnect(&st), "명령을 부르지 않은 데몬 페이지");
        st.face_alive.store(true, SeqCst); // 얼굴이 set_hit_rect를 불렀다
        assert!(!needs_reconnect(&st));
        st.auth_failed.store(true, SeqCst); // 얼굴의 renew_session이 실패했다
        assert!(
            needs_reconnect(&st),
            "갱신에 실패한 얼굴은 곁 열기로 다시 연다"
        );
        page_started(&st);
        assert!(!st.face_alive.load(SeqCst), "새 페이지는 다시 알려야 한다");
        note_page(&st, Page::Face);
        st.face_alive.store(true, SeqCst);
        assert!(!needs_reconnect(&st), "다시 연 얼굴이 뜨면 풀린다");
    }

    #[test]
    fn rejected_boot_reopens_on_request_but_not_in_a_loop() {
        let st = OverlayState::default();
        assert!(needs_reconnect(&st), "아직 얼굴이 아니다(깨우는 중)");
        note_page(&st, Page::Face);
        st.face_alive.store(true, SeqCst);
        assert!(!needs_reconnect(&st));
        page_started(&st);
        note_page(&st, Page::Unauthorized);
        assert!(needs_reconnect(&st), "곁 열기는 새 코드로 다시 연다");
        assert!(
            st.on_face.load(SeqCst),
            "감시는 거절된 안내 쪽을 10초마다 다시 열지 않는다"
        );
        note_page(&st, Page::Face);
        st.face_alive.store(true, SeqCst);
        assert!(!needs_reconnect(&st));
        assert!(!st.auth_failed.load(SeqCst));
        note_page(&st, Page::Error);
        assert!(needs_reconnect(&st));
    }

    #[test]
    fn renew_gate_allows_once_per_30s() {
        assert_eq!(RENEW_EVERY, Duration::from_secs(30));
        let gate = RenewGate::default();
        let t0 = Instant::now();
        assert!(gate.try_acquire(t0).is_ok());
        assert!(gate.try_acquire(t0 + Duration::from_millis(100)).is_err());
        assert_eq!(
            gate.try_acquire(t0 + Duration::from_secs(29)),
            Err(Duration::from_secs(1))
        );
        // 거절된 시도는 시각을 늦추지 않는다
        assert!(gate.try_acquire(t0 + RENEW_EVERY).is_ok());
        assert!(gate
            .try_acquire(t0 + RENEW_EVERY + Duration::from_secs(1))
            .is_err());
    }

    #[test]
    fn bundled_fallback_never_talks_to_the_daemon() {
        // 셸에 든 대체 화면은 IPC만 쓴다. 데몬 API를 부르지 않는다(§12.2).
        const PAGES: [(&str, &str); 2] = [
            ("index.html", include_str!("../../dist/index.html")),
            ("fallback.js", include_str!("../../dist/fallback.js")),
        ];
        for (name, text) in PAGES {
            for needle in [
                "127.0.0.1",
                "localhost:",
                "47321",
                "/v1/",
                "fetch(",
                "XMLHttpRequest",
                "EventSource",
                "WebSocket",
                "sendBeacon",
            ] {
                assert!(!text.contains(needle), "{name}에 {needle:?}가 있어요");
            }
        }
        // 셸 페이지의 CSP도 IPC 밖으로의 연결을 막는다.
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let csp = conf["app"]["security"]["csp"].as_str().unwrap();
        let connect = csp
            .split(';')
            .map(str::trim)
            .find(|d| d.starts_with("connect-src"))
            .unwrap();
        assert_eq!(connect, "connect-src ipc: http://ipc.localhost");
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
