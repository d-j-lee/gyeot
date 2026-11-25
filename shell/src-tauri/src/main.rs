//! 곁(gyeot) 데스크톱 셸.
//!
//! UI만 맡는 얇은 껍데기다(G-002). 서버는 따로 사는 `gyeotd`가 맡고, 셸은
//!
//! - 데몬이 없으면 창 없이 띄우고,
//! - 그 포트의 서버가 내 곁 데몬인지 확인한 연결로(§13.1) 토큰을 보내 일회용 부트 코드를 받고,
//!   데몬이 서빙하는 얼굴(`/?mode=overlay`)을 투명·항상 위 창에 띄우고(얼굴이 세션 열쇠로 바꾼다, §12.1),
//! - 얼굴이 알려 준 영역에서만 클릭을 받으며,
//! - 트레이·전역 단축키(Ctrl+Alt+Space)를 제공한다.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(test)]
mod acl_tests;
mod daemon;
mod geometry;
mod hello;
mod http;
mod logging;
mod overlay;
mod paths;
mod platform;
#[cfg(test)]
mod testing;
mod tray;

use tauri::{Manager, RunEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use overlay::{AppConfig, OverlayState};

fn main() {
    let data_dir = paths::data_dir();
    logging::init(data_dir.clone());
    let port = paths::port(data_dir.as_deref());
    info!(
        "곁 셸 v{} 시작(포트 {port}, 데이터 {})",
        env!("CARGO_PKG_VERSION"),
        data_dir
            .as_deref()
            .map(|d| d.display().to_string())
            .unwrap_or_else(|| "?".into())
    );

    let app = tauri::Builder::default()
        // 하나만 뜬다. 두 번째 실행은 "곁 열기"처럼 기존 창을 보이고 끝난다(반드시 첫 플러그인).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            overlay::open(app);
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name(tray::AUTOSTART_NAME)
                .arg(tray::AUTOSTART_ARG)
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppConfig {
            port,
            data_dir: data_dir.clone(),
        })
        .manage(OverlayState::default())
        .invoke_handler(tauri::generate_handler![
            overlay::set_hit_rect,
            overlay::set_expanded,
            overlay::renew_session,
            overlay::retry_connect
        ])
        .setup(move |app| {
            let handle = app.handle();
            if let Err(e) = overlay::allow_custom_port(handle, port) {
                warn!("포트 {port}용 권한을 더하지 못했어요: {e}");
            }
            overlay::create(handle)?;
            tray::build(handle)?;
            register_shortcut(handle);
            overlay::start_pointer_thread(handle.clone());
            overlay::connect_async(handle.clone(), false);
            overlay::start_watchdog(handle.clone());
            Ok(())
        })
        .build(tauri::generate_context!());

    let app = match app {
        Ok(app) => app,
        Err(e) => {
            warn!("셸을 시작하지 못했어요: {e}");
            std::process::exit(1);
        }
    };

    app.run(|app, event| {
        if let RunEvent::ExitRequested { api, code, .. } = event {
            // 창이 모두 닫혀도(사용자 동작) 트레이로 계속 산다. 메뉴의 "종료"만 끝낸다.
            if code.is_none() && !app.state::<OverlayState>().is_quitting() {
                api.prevent_exit();
            }
        }
    });
}

/// Ctrl+Alt+Space: 보이기/숨기기. 다른 프로그램이 이미 쓰고 있으면 기록만 하고 넘어간다.
fn register_shortcut(app: &tauri::AppHandle) {
    let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
    let result = app
        .global_shortcut()
        .on_shortcut(shortcut, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                overlay::toggle(app);
            }
        });
    match result {
        Ok(()) => info!("전역 단축키 Ctrl+Alt+Space 등록"),
        Err(e) => warn!("전역 단축키 Ctrl+Alt+Space를 등록하지 못했어요(다른 프로그램이 쓰는 중일 수 있어요): {e}"),
    }
}
