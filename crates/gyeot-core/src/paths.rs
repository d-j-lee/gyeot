//! 데이터 폴더·파일 이름 계산(순수 함수). 환경 변수와 OS 폴더는 부르는 쪽이 읽어서 넘긴다.

use std::path::PathBuf;

/// 데이터 폴더를 정하는 데 쓰는 값들(계약 §1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HomeEnv {
    /// `GYEOT_HOME`.
    pub gyeot_home: Option<PathBuf>,
    /// Windows인가.
    pub windows: bool,
    /// `%LOCALAPPDATA%`(Windows).
    pub local_app_data: Option<PathBuf>,
    /// `$XDG_STATE_HOME`.
    pub xdg_state_home: Option<PathBuf>,
    /// 사용자 홈.
    pub home_dir: Option<PathBuf>,
}

/// 데이터 폴더: `GYEOT_HOME` → Windows `%LOCALAPPDATA%\gyeot` → `$XDG_STATE_HOME/gyeot` → `~/.local/state/gyeot`.
/// 빈 값과 상대 경로인 XDG 값은 무시한다(XDG 규격).
pub fn resolve_home(env: &HomeEnv) -> Option<PathBuf> {
    let nonempty = |p: &Option<PathBuf>| p.clone().filter(|p| !p.as_os_str().is_empty());
    if let Some(h) = nonempty(&env.gyeot_home) {
        return Some(h);
    }
    if env.windows {
        return nonempty(&env.local_app_data).map(|p| p.join("gyeot"));
    }
    if let Some(x) = nonempty(&env.xdg_state_home).filter(|p| p.is_absolute()) {
        return Some(x.join("gyeot"));
    }
    nonempty(&env.home_dir).map(|h| h.join(".local").join("state").join("gyeot"))
}

/// 스풀 파일 이름: `<ts_ms>-<pid>-<n>.json`.
pub fn spool_file_name(ts_ms: i64, pid: u32, n: u32) -> String {
    format!("{ts_ms:013}-{pid}-{n}.json")
}

/// UTC 날짜(연, 월, 일). 1970-01-01 이전도 맞게 셈한다.
pub fn utc_ymd(ms: i64) -> (i64, u32, u32) {
    // Howard Hinnant, "civil_from_days".
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (
        y,
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

/// 이벤트 로그 파일 이름: `YYYY-MM-DD.jsonl`(UTC).
pub fn event_log_name(ms: i64) -> String {
    let (y, m, d) = utc_ymd(ms);
    format!("{y:04}-{m:02}-{d:02}.jsonl")
}

/// `YYYY-MM-DD.jsonl` 이름에서 그날 0시(UTC 밀리초)를 읽는다.
pub fn parse_event_log_name(name: &str) -> Option<i64> {
    let stem = name.strip_suffix(".jsonl")?;
    let mut it = stem.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.parse().ok()?;
    if it.next().is_some() || stem.len() != 10 || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // days_from_civil
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let ms = days * 86_400_000;
    // 31일이 없는 달 같은 잘못된 날짜는 되돌려 확인한다.
    (event_log_name(ms) == name).then_some(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_resolution_order() {
        let mut env = HomeEnv {
            gyeot_home: Some("/custom".into()),
            windows: true,
            local_app_data: Some("C:\\Users\\me\\AppData\\Local".into()),
            xdg_state_home: Some("/xdg".into()),
            home_dir: Some("/home/me".into()),
        };
        assert_eq!(resolve_home(&env), Some(PathBuf::from("/custom")));
        env.gyeot_home = Some(PathBuf::new());
        assert_eq!(
            resolve_home(&env),
            Some(PathBuf::from("C:\\Users\\me\\AppData\\Local").join("gyeot"))
        );
        env.windows = false;
        assert_eq!(resolve_home(&env), Some(PathBuf::from("/xdg/gyeot")));
        env.xdg_state_home = Some("relative".into());
        assert_eq!(
            resolve_home(&env),
            Some(PathBuf::from("/home/me/.local/state/gyeot"))
        );
        env.home_dir = None;
        assert_eq!(resolve_home(&env), None);
    }

    #[test]
    fn file_names() {
        assert_eq!(
            spool_file_name(1_764_126_720_123, 42, 0),
            "1764126720123-42-0.json"
        );
        assert_eq!(spool_file_name(5, 1, 2), "0000000000005-1-2.json");
        assert_eq!(event_log_name(0), "1970-01-01.jsonl");
        assert_eq!(event_log_name(1_764_126_720_123), "2025-11-26.jsonl");
        assert_eq!(event_log_name(951_782_400_000), "2000-02-29.jsonl");
        assert_eq!(event_log_name(-1), "1969-12-31.jsonl");
    }

    #[test]
    fn parse_log_names() {
        for ms in [0_i64, 951_782_400_000, 1_764_115_200_000, 4_102_444_800_000] {
            let name = event_log_name(ms);
            assert_eq!(
                parse_event_log_name(&name),
                Some(ms - ms.rem_euclid(86_400_000)),
                "{name}"
            );
        }
        for bad in [
            "2025-02-30.jsonl",
            "2025-13-01.jsonl",
            "x.jsonl",
            "2025-1-1.jsonl",
            "2025-11-26.json",
        ] {
            assert_eq!(parse_event_log_name(bad), None, "{bad}");
        }
    }
}
