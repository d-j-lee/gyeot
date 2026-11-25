//! 정책(설정 §6에서 엔진이 쓰는 부분)과 조용한 시간 계산.

use serde::{Deserialize, Serialize};
use std::fmt;

const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 86_400_000;

/// 하루 안의 조용한 시간 한 구간(현지 시각, 분 단위). `start > end`이면 자정을 넘긴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuietRange {
    /// 시작(0..1440).
    pub start_min: u32,
    /// 끝(0..=1440). 이 분은 포함하지 않는다.
    pub end_min: u32,
}

impl QuietRange {
    /// `"22:00-08:00"` 꼴을 읽는다.
    pub fn parse(s: &str) -> Result<QuietRange, String> {
        let (a, b) = s
            .trim()
            .split_once('-')
            .ok_or_else(|| format!("조용한 시간 '{s}'은 HH:MM-HH:MM 꼴이어야 해요"))?;
        let start = parse_hhmm(a, false)
            .ok_or_else(|| format!("조용한 시간 시작 '{a}'을 읽지 못했어요"))?;
        let end =
            parse_hhmm(b, true).ok_or_else(|| format!("조용한 시간 끝 '{b}'을 읽지 못했어요"))?;
        Ok(QuietRange {
            start_min: start,
            end_min: end,
        })
    }

    /// 하루 안의 분(0..1440)이 이 구간에 드는가.
    pub fn contains(&self, minute_of_day: u32) -> bool {
        let (s, e) = (self.start_min, self.end_min.min(1440));
        if s == e {
            false
        } else if s < e {
            minute_of_day >= s && minute_of_day < e
        } else {
            minute_of_day >= s || minute_of_day < e
        }
    }
}

impl fmt::Display for QuietRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02}:{:02}-{:02}:{:02}",
            self.start_min / 60,
            self.start_min % 60,
            self.end_min / 60,
            self.end_min % 60
        )
    }
}

fn parse_hhmm(s: &str, allow_24: bool) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 {
        return None;
    }
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    if m >= 60 {
        return None;
    }
    if h == 24 && m == 0 && allow_24 {
        return Some(1440);
    }
    if h >= 24 {
        return None;
    }
    Some(h * 60 + m)
}

/// 엔진이 판단에 쓰는 설정.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    /// 이만큼(분) 조용하면 작업 중 세션을 `stall`로 본다.
    pub stale_minutes: u32,
    /// 이 점수 밑은 표정만.
    pub nudge_min: f64,
    /// 이 점수 이상·막힘·2분 넘게 기다림이면 소리 후보.
    pub interrupt_min: f64,
    /// 조용한 시간(현지 시각).
    pub quiet_hours: Vec<QuietRange>,
    /// 현지 시각 = UTC + 이 분.
    pub utc_offset_minutes: i32,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            stale_minutes: 30,
            nudge_min: 80.0,
            interrupt_min: 95.0,
            quiet_hours: Vec::new(),
            utc_offset_minutes: 0,
        }
    }
}

impl Policy {
    /// 문자열 목록(`["22:00-08:00"]`)을 읽는다.
    pub fn parse_quiet_hours<S: AsRef<str>>(list: &[S]) -> Result<Vec<QuietRange>, String> {
        list.iter().map(|s| QuietRange::parse(s.as_ref())).collect()
    }

    /// `stale_minutes`를 밀리초로.
    pub fn stale_ms(&self) -> i64 {
        i64::from(self.stale_minutes.max(1)) * MINUTE_MS
    }

    fn local_ms(&self, now_ms: i64) -> i64 {
        now_ms + i64::from(self.utc_offset_minutes) * MINUTE_MS
    }

    /// 현지 시각의 하루 안 분.
    pub fn local_minute_of_day(&self, now_ms: i64) -> u32 {
        // 0..1440 안이므로 u32로 바꿔도 잃는 값이 없다.
        u32::try_from(self.local_ms(now_ms).rem_euclid(DAY_MS) / MINUTE_MS).unwrap_or(0)
    }

    /// 지금이 조용한 시간인가.
    pub fn in_quiet_hours(&self, now_ms: i64) -> bool {
        let m = self.local_minute_of_day(now_ms);
        self.quiet_hours.iter().any(|r| r.contains(m))
    }

    /// `now_ms`보다 뒤에 오는 가장 가까운 조용한 시간 경계(UTC 밀리초).
    pub fn next_quiet_boundary(&self, now_ms: i64) -> Option<i64> {
        let local = self.local_ms(now_ms);
        let day_start = local.div_euclid(DAY_MS) * DAY_MS;
        let offset = i64::from(self.utc_offset_minutes) * MINUTE_MS;
        self.quiet_hours
            .iter()
            .filter(|r| r.start_min != r.end_min.min(1440))
            .flat_map(|r| [r.start_min, r.end_min.min(1440) % 1440])
            .map(|m| {
                let mut t = day_start + i64::from(m) * MINUTE_MS;
                if t <= local {
                    t += DAY_MS;
                }
                t - offset
            })
            .min()
    }

    /// 표시용 문자열 목록.
    pub fn quiet_hours_strings(&self) -> Vec<String> {
        self.quiet_hours.iter().map(ToString::to_string).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ranges() {
        let r = QuietRange::parse("22:00-08:00").unwrap();
        assert_eq!((r.start_min, r.end_min), (1320, 480));
        assert_eq!(r.to_string(), "22:00-08:00");
        let r = QuietRange::parse(" 9:30 - 24:00 ").unwrap();
        assert_eq!((r.start_min, r.end_min), (570, 1440));
        for bad in [
            "",
            "22:00",
            "25:00-01:00",
            "22:60-01:00",
            "24:00-01:00",
            "aa:bb-cc:dd",
        ] {
            assert!(QuietRange::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn contains_wraps_midnight() {
        let r = QuietRange::parse("22:00-08:00").unwrap();
        assert!(r.contains(22 * 60));
        assert!(r.contains(23 * 60 + 59));
        assert!(r.contains(0));
        assert!(r.contains(7 * 60 + 59));
        assert!(!r.contains(8 * 60));
        assert!(!r.contains(12 * 60));
        let same = QuietRange::parse("10:00-10:00").unwrap();
        assert!(!same.contains(600));
    }

    #[test]
    fn quiet_hours_use_offset() {
        let p = Policy {
            quiet_hours: Policy::parse_quiet_hours(&["22:00-08:00"]).unwrap(),
            utc_offset_minutes: 540, // KST
            ..Policy::default()
        };
        // 2025-11-26 13:00 UTC = 22:00 KST
        let t = 1_764_162_000_000;
        assert_eq!(p.local_minute_of_day(t), 22 * 60);
        assert!(p.in_quiet_hours(t));
        assert!(!p.in_quiet_hours(t - 60_000));
        // 다음 경계는 08:00 KST = 23:00 UTC
        assert_eq!(p.next_quiet_boundary(t), Some(t + 10 * 3_600_000));
        assert_eq!(p.next_quiet_boundary(t - 60_000), Some(t));
        assert_eq!(Policy::default().next_quiet_boundary(t), None);
    }

    #[test]
    fn negative_offset_and_epoch() {
        let p = Policy {
            quiet_hours: Policy::parse_quiet_hours(&["00:00-01:00"]).unwrap(),
            utc_offset_minutes: -300,
            ..Policy::default()
        };
        // 1970-01-01 05:00 UTC = 00:00 현지
        assert!(p.in_quiet_hours(5 * 3_600_000));
        assert!(!p.in_quiet_hours(4 * 3_600_000));
        assert_eq!(p.local_minute_of_day(0), 19 * 60);
    }
}
