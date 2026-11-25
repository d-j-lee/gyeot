//! 데이터 폴더·포트·토큰 (docs/contracts.md §1).

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

/// 데몬 기본 포트.
pub const DEFAULT_PORT: u16 = 47321;
const APP_DIR: &str = "gyeot";

/// 데이터 폴더. `GYEOT_HOME` → Windows `%LOCALAPPDATA%\gyeot` → `$XDG_STATE_HOME/gyeot` → `~/.local/state/gyeot`.
pub fn data_dir() -> Option<PathBuf> {
    data_dir_with(cfg!(windows), |k| std::env::var_os(k))
}

/// 환경을 주입받는 판(시험용).
pub fn data_dir_with(windows: bool, get: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let non_empty = |k: &str| get(k).filter(|v| !v.is_empty());
    if let Some(home) = non_empty("GYEOT_HOME") {
        return Some(PathBuf::from(home));
    }
    if windows {
        return non_empty("LOCALAPPDATA").map(|v| PathBuf::from(v).join(APP_DIR));
    }
    if let Some(state) = non_empty("XDG_STATE_HOME") {
        return Some(PathBuf::from(state).join(APP_DIR));
    }
    non_empty("HOME").map(|h| PathBuf::from(h).join(".local").join("state").join(APP_DIR))
}

/// `config.toml`의 맨 위 `port = N`만 읽는다(표 `[...]`가 시작되면 멈춘다).
/// 전체 TOML 해석기를 들이지 않으려고 이 한 줄만 본다. 잘못된 값이면 `None`.
pub fn parse_config_port(text: &str) -> Option<u16> {
    for raw in text.trim_start_matches('\u{feff}').lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            break;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "port" {
            continue;
        }
        return value
            .trim()
            .replace('_', "")
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0);
    }
    None
}

/// 포트: 환경 변수 `GYEOT_PORT` → 설정 `port` → 47321.
pub fn resolve_port(env_port: Option<&str>, config_text: Option<&str>) -> u16 {
    env_port
        .and_then(|v| v.trim().parse::<u16>().ok())
        .filter(|p| *p != 0)
        .or_else(|| config_text.and_then(parse_config_port))
        .unwrap_or(DEFAULT_PORT)
}

/// 이 프로세스의 환경과 데이터 폴더의 설정으로 포트를 정한다.
pub fn port(data_dir: Option<&Path>) -> u16 {
    let env_port = std::env::var("GYEOT_PORT").ok();
    let config = data_dir.and_then(|d| fs::read_to_string(d.join("config.toml")).ok());
    resolve_port(env_port.as_deref(), config.as_deref())
}

/// 토큰이 계약 모양(소문자 hex 64자)인지.
pub fn is_valid_token(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `<데이터>/token`을 읽는다. 없거나 모양이 틀리면 이유를 돌려준다.
pub fn read_token(data_dir: Option<&Path>) -> Result<String, String> {
    let dir = data_dir.ok_or_else(|| "데이터 폴더를 정하지 못했어요".to_string())?;
    let path = dir.join("token");
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("토큰 파일을 읽지 못했어요({}): {e}", path.display()))?;
    let token = text.trim();
    if is_valid_token(token) {
        Ok(token.to_string())
    } else {
        Err(format!("토큰 파일 모양이 맞지 않아요({})", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        move |k| map.get(k).cloned()
    }

    #[test]
    fn gyeot_home_wins_everywhere() {
        let e = env(&[
            ("GYEOT_HOME", "/x/y"),
            ("LOCALAPPDATA", "C:\\L"),
            ("HOME", "/h"),
        ]);
        assert_eq!(data_dir_with(true, &e), Some(PathBuf::from("/x/y")));
        assert_eq!(data_dir_with(false, &e), Some(PathBuf::from("/x/y")));
    }

    #[test]
    fn windows_uses_localappdata() {
        let e = env(&[
            ("LOCALAPPDATA", "C:\\Users\\dj\\AppData\\Local"),
            ("HOME", "/h"),
        ]);
        assert_eq!(
            data_dir_with(true, &e),
            Some(PathBuf::from("C:\\Users\\dj\\AppData\\Local").join("gyeot"))
        );
        assert_eq!(data_dir_with(true, env(&[("HOME", "/h")])), None);
    }

    #[test]
    fn unix_prefers_xdg_state_then_home() {
        let e = env(&[("XDG_STATE_HOME", "/s"), ("HOME", "/h")]);
        assert_eq!(data_dir_with(false, &e), Some(PathBuf::from("/s/gyeot")));
        let e = env(&[("HOME", "/h"), ("XDG_STATE_HOME", "")]);
        assert_eq!(
            data_dir_with(false, &e),
            Some(PathBuf::from("/h/.local/state/gyeot"))
        );
    }

    #[test]
    fn empty_gyeot_home_is_ignored() {
        let e = env(&[("GYEOT_HOME", ""), ("HOME", "/h")]);
        assert_eq!(
            data_dir_with(false, &e),
            Some(PathBuf::from("/h/.local/state/gyeot"))
        );
    }

    #[test]
    fn config_port_top_level_only() {
        assert_eq!(parse_config_port("port = 48000\n"), Some(48000));
        assert_eq!(
            parse_config_port("\u{feff}  port=47_999 # 주석\n"),
            Some(47999)
        );
        assert_eq!(parse_config_port("# port = 1\nstale_minutes = 30\n"), None);
        assert_eq!(parse_config_port("[[tail]]\nport = 1234\n"), None);
        assert_eq!(parse_config_port("port = 0\n"), None);
        assert_eq!(parse_config_port("port = 70000\n"), None);
        assert_eq!(parse_config_port("portal = 5\nport = 5000"), Some(5000));
    }

    #[test]
    fn port_precedence_env_config_default() {
        assert_eq!(resolve_port(Some("48001"), Some("port = 48002")), 48001);
        assert_eq!(resolve_port(Some("nope"), Some("port = 48002")), 48002);
        assert_eq!(resolve_port(Some("0"), None), DEFAULT_PORT);
        assert_eq!(resolve_port(None, None), DEFAULT_PORT);
    }

    #[test]
    fn token_shape() {
        let good = "0123456789abcdef".repeat(4);
        assert!(is_valid_token(&good));
        assert!(!is_valid_token(&good.to_uppercase()));
        assert!(!is_valid_token(&good[..63]));
        assert!(!is_valid_token(&format!("{}g", &good[..63])));
    }

    #[test]
    fn read_token_trims_and_validates() {
        let dir = std::env::temp_dir().join(format!("gyeot-shell-token-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let good = "ab".repeat(32);
        fs::write(dir.join("token"), format!("{good}\r\n")).unwrap();
        assert_eq!(read_token(Some(&dir)), Ok(good));
        fs::write(dir.join("token"), "short").unwrap();
        assert!(read_token(Some(&dir)).is_err());
        fs::remove_dir_all(&dir).unwrap();
        assert!(read_token(Some(&dir)).is_err());
        assert!(read_token(None).is_err());
    }
}
