//! 운영체제별 작은 기능. Windows는 user32·shell32를 직접 부른다(의존성을 늘리지 않으려고).

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::iter::once;

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetCursorPos(point: *mut Point) -> i32;
        fn GetAsyncKeyState(vkey: i32) -> i16;
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            hwnd: *mut c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show_cmd: i32,
        ) -> *mut c_void;
    }

    const VK_LBUTTON: i32 = 0x01;
    const VK_RBUTTON: i32 = 0x02;
    const VK_MBUTTON: i32 = 0x04;
    const SW_SHOWNORMAL: i32 = 1;

    /// 화면 물리 좌표의 마우스 위치. Tauri 프로세스는 모니터별 DPI 인식(v2)이라 물리 px가 온다.
    pub fn cursor_position() -> Option<(f64, f64)> {
        let mut p = Point { x: 0, y: 0 };
        // SAFETY: p는 유효한 POINT 크기의 지역 변수다.
        let ok = unsafe { GetCursorPos(&mut p) };
        (ok != 0).then_some((f64::from(p.x), f64::from(p.y)))
    }

    /// 마우스 단추가 눌려 있는지(끌기 도중에 클릭 통과로 바뀌지 않게).
    pub fn mouse_button_down() -> bool {
        [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].iter().any(|&vk| {
            // SAFETY: 인자 없는 상태 질의다.
            let state = unsafe { GetAsyncKeyState(vk) };
            (state as u16) & 0x8000 != 0
        })
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(once(0)).collect()
    }

    /// 기본 브라우저로 연다.
    pub fn open_in_browser(url: &str) -> bool {
        let op = wide("open");
        let file = wide(url);
        // SAFETY: 두 문자열은 NUL로 끝나고 호출 동안 살아 있다.
        let r = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                op.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // 문서: 32보다 크면 성공
        (r as usize) > 32
    }
}

#[cfg(not(windows))]
mod imp {
    /// Windows 밖에서는 Tauri에게 묻는다(개발용).
    pub fn cursor_position() -> Option<(f64, f64)> {
        None
    }

    pub fn mouse_button_down() -> bool {
        false
    }

    pub fn open_in_browser(url: &str) -> bool {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(opener).arg(url).spawn().is_ok()
    }
}

pub use imp::{cursor_position, mouse_button_down, open_in_browser};

/// 바깥 브라우저로 넘겨도 되는 주소인지: https만, 길이 2 KiB 이하, 사용자 정보 없음.
pub fn is_safe_external(url: &tauri::Url) -> bool {
    url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.as_str().len() <= 2048
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_https_goes_out() {
        let ok: tauri::Url = "https://claude.ai/code/abc".parse().unwrap();
        assert!(is_safe_external(&ok));
        for bad in [
            "http://example.com/",
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "https://user:pw@example.com/",
            "ms-settings:privacy",
        ] {
            let u: tauri::Url = bad.parse().unwrap();
            assert!(!is_safe_external(&u), "{bad}");
        }
        let long: tauri::Url = format!("https://e.com/{}", "a".repeat(3000))
            .parse()
            .unwrap();
        assert!(!is_safe_external(&long));
    }
}
