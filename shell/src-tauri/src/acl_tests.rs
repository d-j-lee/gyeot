//! 실제 capabilities(ACL)를 가짜 런타임으로 확인한다.
//!
//! 지키는 것: 데몬이 서빙하는 얼굴(원격 출처)은 `set_hit_rect`·`set_expanded`·`renew_session`
//! 세 명령만 부를 수 있고, 다른 포트·다른 출처·다른 창·Tauri 코어 API·`retry_connect`는 거절된다.
//! 셸에 든 대체 화면은 `renew_session`을 받지 않는다.

use serde_json::json;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::overlay::{self, AppConfig, OverlayState, RENEW_ERR};
use crate::testing::{self, FakeDaemon, Hello};

/// 대체 화면용 명령의 가짜(실제는 Wry 전용). ACL은 명령 이름과 출처로만 판정한다.
#[tauri::command]
fn retry_connect(restart: Option<bool>) -> bool {
    restart.unwrap_or(false)
}

fn app() -> App<MockRuntime> {
    // 데이터 폴더가 없으니 renew_session은 토큰을 읽는 데서 멈춘다(네트워크에 나가지 않는다).
    app_with(47321, None)
}

/// 셸 설정(데몬 포트·데이터 폴더)을 준 가짜 앱. ACL은 페이지 출처로 판정하고, 명령은 이 설정으로 데몬에 묻는다.
fn app_with(port: u16, data_dir: Option<std::path::PathBuf>) -> App<MockRuntime> {
    mock_builder()
        .manage(OverlayState::default())
        .manage(AppConfig { port, data_dir })
        .invoke_handler(tauri::generate_handler![
            overlay::set_hit_rect,
            overlay::set_expanded,
            overlay::renew_session,
            retry_connect
        ])
        .build(tauri::generate_context!())
        .expect("가짜 앱")
}

fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .build()
        .expect("가짜 창")
}

/// 명령 하나를 부른다. 실패는 글자로(ACL 거절이든 명령이 낸 오류든).
fn call(
    win: &WebviewWindow<MockRuntime>,
    origin: &str,
    cmd: &str,
    body: serde_json::Value,
) -> Result<(), String> {
    let url: Url = origin.parse().unwrap();
    get_ipc_response(
        win,
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url,
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|_| ())
    .map_err(|v| {
        v.as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| v.to_string())
    })
}

fn invoke(
    win: &WebviewWindow<MockRuntime>,
    origin: &str,
    cmd: &str,
    body: serde_json::Value,
) -> bool {
    call(win, origin, cmd, body).is_ok()
}

/// renew_session이 ACL을 지나 명령까지 닿았는지(명령의 오류는 늘 `RENEW_ERR`로 시작한다).
fn renew_reached(win: &WebviewWindow<MockRuntime>, origin: &str) -> Result<String, String> {
    match call(win, origin, "renew_session", json!({})) {
        Ok(()) => Ok(String::new()),
        Err(e) if e.starts_with(RENEW_ERR) => Ok(e),
        Err(e) => Err(e),
    }
}

const FACE: &str = "http://127.0.0.1:47321/?mode=overlay";

fn local_page() -> &'static str {
    if cfg!(windows) {
        "http://tauri.localhost/index.html"
    } else {
        "tauri://localhost/index.html"
    }
}

fn rect() -> serde_json::Value {
    json!({ "x": 10, "y": 20, "width": 96, "height": 120 })
}

#[test]
fn face_may_set_hit_rect_and_expanded() {
    let app = app();
    let win = window(&app, overlay::MAIN);
    assert!(!app.state::<OverlayState>().face_alive());
    assert!(invoke(&win, FACE, "set_hit_rect", rect()));
    assert!(
        app.state::<OverlayState>().face_alive(),
        "셸 명령을 부른 얼굴은 살아 있다"
    );
    let hit = app
        .state::<OverlayState>()
        .hit_rect()
        .expect("영역이 저장돼야 한다");
    assert_eq!(
        (hit.x, hit.y, hit.width, hit.height),
        (10.0, 20.0, 96.0, 120.0)
    );
    assert!(invoke(
        &win,
        FACE,
        "set_expanded",
        json!({ "expanded": true })
    ));
    assert!(app.state::<OverlayState>().is_expanded());
    assert!(invoke(
        &win,
        "http://localhost:47321/",
        "set_hit_rect",
        rect()
    ));
}

#[test]
fn face_may_renew_session_once_per_30s() {
    let app = app();
    let win = window(&app, overlay::MAIN);
    assert!(!app.state::<OverlayState>().auth_failed());
    let first = renew_reached(&win, FACE).expect("얼굴은 renew_session을 부를 수 있다");
    // 가짜 앱에는 데이터 폴더가 없어서 토큰을 읽는 데서 멈춘다 = ACL과 30초 제한을 지나 부트 요청까지 갔다.
    assert!(first.contains("데이터 폴더"), "{first}");
    assert!(
        app.state::<OverlayState>().auth_failed(),
        "갱신 실패는 곁 열기가 다시 열도록 남긴다"
    );
    let second = renew_reached(&win, "http://localhost:47321/").expect("localhost도 같은 얼굴");
    assert!(second.contains("30초"), "두 번째는 30초 제한: {second}");
}

#[test]
fn renew_session_is_refused_outside_the_face() {
    let app = app();
    let win = window(&app, overlay::MAIN);
    for origin in [
        "http://127.0.0.1:47322/",
        "https://127.0.0.1:47321/",
        "http://192.168.0.10:47321/",
        "https://example.com/",
        "http://127.0.0.1.nip.io:47321/",
        local_page(),
    ] {
        assert!(renew_reached(&win, origin).is_err(), "{origin}");
    }
    let other = window(&app, "other");
    assert!(renew_reached(&other, FACE).is_err(), "다른 창");
    assert!(
        !app.state::<OverlayState>().auth_failed(),
        "거절된 호출은 상태를 바꾸지 않는다"
    );
    // 거절된 호출은 30초 제한을 쓰지 않는다
    let first = renew_reached(&win, FACE).unwrap();
    assert!(first.contains("데이터 폴더"), "{first}");
}

#[test]
fn custom_port_gets_the_same_three_commands_only() {
    let app = app();
    overlay::allow_custom_port(app.handle(), 48000).expect("포트 권한");
    let win = window(&app, overlay::MAIN);
    let face = "http://127.0.0.1:48000/?mode=overlay";
    assert!(invoke(&win, face, "set_hit_rect", rect()));
    assert!(invoke(
        &win,
        "http://localhost:48000/",
        "set_expanded",
        json!({ "expanded": true })
    ));
    assert!(renew_reached(&win, face).is_ok());
    assert!(!invoke(&win, face, "retry_connect", json!({})));
    assert!(!invoke(
        &win,
        face,
        "plugin:window|hide",
        json!({ "label": "main" })
    ));
    assert!(!invoke(
        &win,
        "http://127.0.0.1:48001/",
        "set_hit_rect",
        rect()
    ));
    // 기본 포트의 얼굴도 그대로
    assert!(invoke(&win, FACE, "set_hit_rect", rect()));
}

#[test]
fn face_may_not_call_anything_else() {
    let app = app();
    let win = window(&app, overlay::MAIN);
    assert!(!invoke(&win, FACE, "retry_connect", json!({})));
    assert!(!invoke(
        &win,
        FACE,
        "plugin:window|hide",
        json!({ "label": "main" })
    ));
    assert!(!invoke(
        &win,
        FACE,
        "plugin:event|listen",
        json!({ "event": "x", "target": { "kind": "Any" }, "handler": 0 })
    ));
    assert!(!invoke(
        &win,
        FACE,
        "plugin:webview|set_webview_zoom",
        json!({ "value": 2.0 })
    ));
}

#[test]
fn other_origins_are_refused() {
    let app = app();
    let win = window(&app, overlay::MAIN);
    for origin in [
        "http://127.0.0.1:47322/",
        "https://127.0.0.1:47321/",
        "http://192.168.0.10:47321/",
        "https://example.com/",
        "http://127.0.0.1.nip.io:47321/",
    ] {
        assert!(!invoke(&win, origin, "set_hit_rect", rect()), "{origin}");
    }
    assert!(app.state::<OverlayState>().hit_rect().is_none());
    assert!(!app.state::<OverlayState>().face_alive());
}

#[test]
fn other_windows_are_refused() {
    let app = app();
    let other = window(&app, "other");
    assert!(!invoke(&other, FACE, "set_hit_rect", rect()));
}

#[test]
fn bundled_fallback_page_may_retry() {
    let app = app();
    let win = window(&app, overlay::MAIN);
    let local = local_page();
    assert!(invoke(
        &win,
        local,
        "retry_connect",
        json!({ "restart": true })
    ));
    assert!(invoke(&win, local, "set_hit_rect", rect()));
    // 대체 화면은 데몬 세션을 다루지 않는다
    assert!(renew_reached(&win, local).is_err());
    // 대체 화면에도 코어 API는 주지 않는다
    assert!(!invoke(
        &win,
        local,
        "plugin:window|hide",
        json!({ "label": "main" })
    ));
}

#[test]
fn empty_rect_clears_hit_area() {
    let app = app();
    let win = window(&app, overlay::MAIN);
    assert!(invoke(&win, FACE, "set_hit_rect", rect()));
    assert!(invoke(
        &win,
        FACE,
        "set_hit_rect",
        json!({ "x": 0, "y": 0, "width": 0, "height": 0 })
    ));
    assert!(app.state::<OverlayState>().hit_rect().is_none());
}

#[test]
fn renew_session_sends_no_token_to_a_server_that_is_not_mine() {
    let token = "8b".repeat(32);
    let dir = testing::data_dir_with_token("acl-renew-notmine", &token);
    for hello in [
        Hello::Wrong,
        Hello::OtherPort(token.clone()),
        Hello::Status(404),
    ] {
        let fake = FakeDaemon::start(hello.clone());
        let app = app_with(fake.port, Some(dir.clone()));
        let win = window(&app, overlay::MAIN);
        let e = renew_reached(&win, FACE).expect("얼굴은 부를 수 있다");
        let seen = fake.finish();
        assert!(
            e.contains("이 포트의 서버는 내 곁 데몬이 아니에요"),
            "{hello:?}: {e}"
        );
        assert!(!testing::token_sent(&seen, &token), "{hello:?}: {seen:?}");
        assert!(app.state::<OverlayState>().auth_failed());
        // 그 포트의 페이지를 닫고 셸의 대체 화면에 이유를 보인다.
        let now = win.url().unwrap();
        assert_eq!(
            overlay::classify(&now, 47321),
            overlay::Page::Error,
            "{now}"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn renewed_face_cannot_loop_through_renew_session() {
    let token = "9c".repeat(32);
    let dir = testing::data_dir_with_token("acl-renew-chain", &token);
    let fake = FakeDaemon::start(Hello::Correct(token.clone()));
    let app = app_with(fake.port, Some(dir.clone()));
    let win = window(&app, overlay::MAIN);
    // 확인한 데몬에서 새 코드를 받아 같은 창을 다시 연다.
    assert_eq!(renew_reached(&win, FACE), Ok(String::new()));
    // 다시 연 얼굴이 또 열쇠를 받지 못했다고 불러도 저절로 다시 열지 않는다.
    let again = renew_reached(&win, FACE).unwrap();
    assert!(again.contains("곁 열기"), "{again}");
    let seen = fake.finish();
    let boots = seen
        .iter()
        .filter(|s| s.first_line().starts_with("POST /v1/boot "))
        .count();
    assert_eq!(boots, 1, "부트 코드는 한 번만 받았다: {seen:?}");
    assert!(
        app.state::<OverlayState>().auth_failed(),
        "곁 열기가 새 코드로 연다"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn face_from_a_stopped_daemon_does_not_reopen_through_renew_session() {
    // 데몬이 멈춘 것을 셸이 봤다(붙여 둔 연결이 끊김). 그 사이 옛 얼굴이 새 데몬에서 401을 받고 부른 renew_session은
    // 창을 열지 않는다: 새 데몬이 뜨면 감시가 한 번 연다(두 번 열지 않게, 고리 제한에도 걸리지 않게).
    let token = "ad".repeat(32);
    let dir = testing::data_dir_with_token("acl-renew-stale", &token);
    let fake = FakeDaemon::start(Hello::Correct(token.clone()));
    let app = app_with(fake.port, Some(dir.clone()));
    let win = window(&app, overlay::MAIN);
    let before = win.url().unwrap();
    app.state::<OverlayState>().daemon_gone();
    let e = renew_reached(&win, FACE).unwrap();
    assert!(e.contains("다시 시작돼서"), "{e}");
    let seen = fake.finish();
    assert!(seen.is_empty(), "데몬에 묻지도 않는다: {seen:?}");
    assert_eq!(win.url().unwrap(), before, "창을 옮기지 않는다");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn held_stream_ending_closes_the_face_at_once() {
    // 붙여 둔 연결(확인한 /v1/stream)이 끊기면(데몬이 끝남) 10초 감시를 기다리지 않고 바로 얼굴을 닫는다(§14.1).
    let token = "be".repeat(32);
    let dir = testing::data_dir_with_token("acl-liveness", &token);
    let fake = FakeDaemon::start(Hello::Correct(token.clone()));
    let app = app_with(fake.port, Some(dir.clone()));
    let win = window(&app, overlay::MAIN);
    win.navigate(FACE.parse().unwrap()).unwrap();
    app.state::<OverlayState>().face_shown();
    overlay::start_liveness(app.handle().clone());
    // 연결이 붙을 시간을 준다(붙었는지는 가짜가 받은 GET /v1/stream으로 아래에서 확인한다).
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(
        overlay::classify(&win.url().unwrap(), 47321),
        overlay::Page::Face,
        "붙어 있는 동안은 얼굴"
    );
    assert!(!app.state::<OverlayState>().face_is_stale());
    // 데몬이 끝난다.
    let started = std::time::Instant::now();
    let seen = fake.finish();
    while started.elapsed() < std::time::Duration::from_secs(3)
        && overlay::classify(&win.url().unwrap(), 47321) == overlay::Page::Face
    {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        overlay::classify(&win.url().unwrap(), 47321),
        overlay::Page::Loading,
        "곧바로 대체 화면으로"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    assert!(
        app.state::<OverlayState>().face_is_stale(),
        "옛 얼굴은 지난 세대"
    );
    assert!(
        seen.iter()
            .any(|s| s.first_line() == "GET /v1/stream HTTP/1.1"),
        "{seen:?}"
    );
    assert!(seen[0].first_line().starts_with("GET /v1/hello?n="));
    app.state::<OverlayState>().set_quitting();
    std::fs::remove_dir_all(&dir).ok();
}
