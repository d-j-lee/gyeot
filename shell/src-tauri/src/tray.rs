//! 알림 영역(트레이) 아이콘과 메뉴.

use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::thread;
use std::time::Duration;

use tauri::menu::{CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuEvent, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt as _;

use crate::daemon;
use crate::overlay::{self, AppConfig, OverlayState};

const TRAY_ID: &str = "gyeot";
const TIP: &str = "곁";
pub const FOCUS_MINUTES: u32 = 30;

/// 자동 시작 등록 이름(HKCU\…\Run 값 이름). uninstall.ps1도 이 이름을 지운다.
pub const AUTOSTART_NAME: &str = "gyeot-shell";
pub const AUTOSTART_ARG: &str = "--autostart";

mod id {
    pub const OPEN: &str = "open";
    pub const TOGGLE: &str = "toggle";
    pub const FOCUS_ON: &str = "focus_on";
    pub const FOCUS_OFF: &str = "focus_off";
    pub const RESTART: &str = "restart_daemon";
    pub const AUTOSTART: &str = "autostart";
    pub const QUIT: &str = "quit";
}

/// 메뉴 순서. `None`은 구분선.
const LAYOUT: &[Option<&str>] = &[
    Some(id::OPEN),
    Some(id::TOGGLE),
    Some(id::FOCUS_ON),
    Some(id::FOCUS_OFF),
    None,
    Some(id::RESTART),
    Some(id::AUTOSTART),
    None,
    Some(id::QUIT),
];

/// 메뉴 문구.
fn label(item: &str) -> String {
    match item {
        id::OPEN => "곁 열기".into(),
        id::TOGGLE => "보이기/숨기기".into(),
        id::FOCUS_ON => format!("집중 모드 {FOCUS_MINUTES}분"),
        id::FOCUS_OFF => "집중 모드 끄기".into(),
        id::RESTART => "곁 데몬 다시 시작".into(),
        id::AUTOSTART => "로그인 시 시작".into(),
        id::QUIT => "종료".into(),
        _ => String::new(),
    }
}

/// 말풍선 문구를 잠시 바꿨다가 되돌릴 때, 더 새 문구를 덮지 않게 세대를 센다.
static TIP_GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItemBuilder::with_id(id::AUTOSTART, label(id::AUTOSTART))
        .checked(autostart_on)
        .build(app)?;
    let mut menu = MenuBuilder::new(app);
    for entry in LAYOUT {
        menu = match *entry {
            None => menu.separator(),
            Some(id::AUTOSTART) => menu.item(&autostart),
            Some(item) => menu.item(&MenuItemBuilder::with_id(item, label(item)).build(app)?),
        };
    }
    let menu = menu.build()?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(TIP)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| on_menu(app, &event, &autostart))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                overlay::toggle(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

fn on_menu(app: &AppHandle, event: &MenuEvent, autostart: &CheckMenuItem<tauri::Wry>) {
    match event.id().as_ref() {
        id::OPEN => overlay::open(app),
        id::TOGGLE => overlay::toggle(app),
        id::FOCUS_ON => focus(app, true),
        id::FOCUS_OFF => focus(app, false),
        id::RESTART => {
            set_tip(app, "곁 · 데몬을 다시 시작하는 중…", None);
            // 얼굴을 먼저 닫는다: 옛 얼굴이 새 데몬에서 401 → renew_session으로 따로 다시 열지 않게(§14.1).
            overlay::before_daemon_restart(app);
            let app = app.clone();
            thread::spawn(move || {
                let cfg = app.state::<AppConfig>();
                match daemon::restart(cfg.port, cfg.data_dir.as_deref()) {
                    Ok(_) => {
                        set_tip(&app, TIP, None);
                        // 새 부트 코드로 여는 것은 감시가 한 번 한다(붙여 둔 연결이 다시 이어지면 깨어난다).
                        overlay::wake_watchdog(&app);
                    }
                    Err(reason) => {
                        crate::warn!("데몬 다시 시작 실패: {reason}");
                        set_tip(
                            &app,
                            &format!("곁 · 데몬 시작 실패: {reason}"),
                            Some(Duration::from_secs(120)),
                        );
                        overlay::connect_async(app.clone(), false);
                    }
                }
            });
        }
        id::AUTOSTART => toggle_autostart(app, autostart),
        id::QUIT => {
            app.state::<OverlayState>().set_quitting();
            app.exit(0);
        }
        _ => {}
    }
}

fn focus(app: &AppHandle, on: bool) {
    let app = app.clone();
    thread::spawn(move || {
        let cfg = app.state::<AppConfig>();
        let minutes = on.then_some(FOCUS_MINUTES);
        match daemon::set_focus(cfg.port, cfg.data_dir.as_deref(), on, minutes) {
            Ok(_) if on => {
                crate::info!("집중 모드 {FOCUS_MINUTES}분");
                let secs = u64::from(FOCUS_MINUTES) * 60;
                set_tip(
                    &app,
                    &format!("곁 · 집중 모드 {FOCUS_MINUTES}분"),
                    Some(Duration::from_secs(secs)),
                );
            }
            Ok(_) => {
                crate::info!("집중 모드 끔");
                set_tip(&app, TIP, None);
            }
            Err(reason) => {
                crate::warn!("집중 모드 요청 실패: {reason}");
                set_tip(
                    &app,
                    &format!("곁 · 집중 모드를 바꾸지 못했어요: {reason}"),
                    Some(Duration::from_secs(60)),
                );
            }
        }
    });
}

/// 트레이 말풍선을 바꾼다. `revert_after`가 있으면 그 뒤 기본 문구로 되돌린다(그 사이 더 새 문구가 없을 때만).
fn set_tip(app: &AppHandle, text: &str, revert_after: Option<Duration>) {
    let generation = TIP_GENERATION.fetch_add(1, SeqCst) + 1;
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(truncate_tip(text)));
    }
    if let Some(after) = revert_after {
        let app = app.clone();
        thread::spawn(move || {
            thread::sleep(after);
            if TIP_GENERATION.load(SeqCst) == generation {
                if let Some(tray) = app.tray_by_id(TRAY_ID) {
                    let _ = tray.set_tooltip(Some(TIP));
                }
            }
        });
    }
}

/// Windows 알림 영역 말풍선은 127자까지라서 자른다.
pub fn truncate_tip(text: &str) -> String {
    const MAX: usize = 120;
    if text.chars().count() <= MAX {
        text.to_string()
    } else {
        let mut s: String = text.chars().take(MAX - 1).collect();
        s.push('…');
        s
    }
}

fn toggle_autostart(app: &AppHandle, item: &CheckMenuItem<tauri::Wry>) {
    let manager = app.autolaunch();
    let want = !manager.is_enabled().unwrap_or(false);
    let result = if want {
        manager.enable().map(|()| quote_run_value())
    } else {
        manager.disable()
    };
    if let Err(e) = result {
        crate::warn!("로그인 시 시작 설정 실패: {e}");
        set_tip(
            app,
            "곁 · 로그인 시 시작을 바꾸지 못했어요",
            Some(Duration::from_secs(60)),
        );
    }
    // 실제 상태로 표시를 맞춘다(메뉴는 누르면 스스로 체크가 뒤집힌다).
    let _ = item.set_checked(manager.is_enabled().unwrap_or(false));
}

/// tauri-plugin-autostart(auto-launch 0.6)는 Run 값에 실행 파일 경로를 따옴표 없이 쓴다.
/// 경로에 공백이 있으면(사용자 이름 등) 모호해지므로 `"경로" --autostart`로 다시 쓴다.
#[cfg(windows)]
fn quote_run_value() {
    const RUN: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let value = quoted_command(&exe.display().to_string(), AUTOSTART_ARG);
    let result = windows_registry::CURRENT_USER.create(RUN).and_then(|key| {
        // 플러그인이 HKCU에 썼을 때만 고친다(관리자 권한이면 HKLM에 썼을 수 있다).
        if key.get_string(AUTOSTART_NAME).is_ok() {
            key.set_string(AUTOSTART_NAME, &value)
        } else {
            Ok(())
        }
    });
    if let Err(e) = result {
        crate::warn!("Run 값을 따옴표로 고치지 못했어요: {e}");
    }
}

#[cfg(not(windows))]
fn quote_run_value() {}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn quoted_command(exe: &str, arg: &str) -> String {
    format!("\"{}\" {arg}", exe.trim_matches('"'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_value_is_quoted() {
        assert_eq!(
            quoted_command(
                r"C:\Users\Kim Dj\AppData\Local\곁\gyeot-shell.exe",
                "--autostart"
            ),
            r#""C:\Users\Kim Dj\AppData\Local\곁\gyeot-shell.exe" --autostart"#
        );
        assert_eq!(
            quoted_command(r#""C:\a b\x.exe""#, "--autostart"),
            r#""C:\a b\x.exe" --autostart"#
        );
    }

    #[test]
    fn menu_starts_with_gyeot_open_and_keeps_old_items() {
        let ids: Vec<&str> = LAYOUT.iter().flatten().copied().collect();
        assert_eq!(ids.first(), Some(&id::OPEN), "{ids:?}");
        assert_eq!(label(id::OPEN), "곁 열기");
        for (item, want) in [
            (id::TOGGLE, "보이기/숨기기"),
            (id::FOCUS_ON, "집중 모드 30분"),
            (id::FOCUS_OFF, "집중 모드 끄기"),
            (id::RESTART, "곁 데몬 다시 시작"),
            (id::AUTOSTART, "로그인 시 시작"),
            (id::QUIT, "종료"),
        ] {
            assert_eq!(ids.iter().filter(|i| **i == item).count(), 1, "{item}");
            assert_eq!(label(item), want);
        }
        assert_eq!(ids.len(), 7);
        assert_eq!(ids.last(), Some(&id::QUIT));
    }

    #[test]
    fn tooltip_is_truncated_by_chars() {
        assert_eq!(truncate_tip("곁"), "곁");
        let long = "가".repeat(300);
        let t = truncate_tip(&long);
        assert_eq!(t.chars().count(), 120);
        assert!(t.ends_with('…'));
    }
}
