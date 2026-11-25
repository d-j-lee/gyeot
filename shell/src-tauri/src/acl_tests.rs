//! 실제 capabilities(ACL)를 가짜 런타임으로 확인한다.
//!
//! 지키는 것: 데몬이 서빙하는 얼굴(원격 출처)은 `set_hit_rect`·`set_expanded` 두 명령만 부를 수
//! 있고, 다른 포트·다른 출처·다른 창·Tauri 코어 API·`retry_connect`는 거절된다.

use serde_json::json;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::overlay::{self, OverlayState};

/// 대체 화면용 명령의 가짜(실제는 Wry 전용). ACL은 명령 이름과 출처로만 판정한다.
#[tauri::command]
fn retry_connect(restart: Option<bool>) -> bool {
    restart.unwrap_or(false)
}

fn app() -> App<MockRuntime> {
    mock_builder()
        .manage(OverlayState::default())
        .invoke_handler(tauri::generate_handler![
            overlay::set_hit_rect,
            overlay::set_expanded,
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

fn invoke(
    win: &WebviewWindow<MockRuntime>,
    origin: &str,
    cmd: &str,
    body: serde_json::Value,
) -> bool {
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
    .is_ok()
}

const FACE: &str = "http://127.0.0.1:47321/?mode=overlay";

fn rect() -> serde_json::Value {
    json!({ "x": 10, "y": 20, "width": 96, "height": 120 })
}

#[test]
fn face_may_set_hit_rect_and_expanded() {
    let app = app();
    let win = window(&app, overlay::MAIN);
    assert!(invoke(&win, FACE, "set_hit_rect", rect()));
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
    let local = if cfg!(windows) {
        "http://tauri.localhost/index.html"
    } else {
        "tauri://localhost/index.html"
    };
    assert!(invoke(
        &win,
        local,
        "retry_connect",
        json!({ "restart": true })
    ));
    assert!(invoke(&win, local, "set_hit_rect", rect()));
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
