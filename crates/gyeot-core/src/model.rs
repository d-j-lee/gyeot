//! 바깥으로 보이는 모양: 항목(§3), 존재감·전달(§4), 제안(§5), 세션·스냅샷(§7).

use crate::event::Agent;
use serde::{Deserialize, Serialize, Serializer};

/// 정수로 떨어지는 실수는 정수로 쓴다(`80` 그대로 보이게).
pub(crate) fn ser_num<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 1e15 {
        // 위에서 범위를 확인했으므로 잃는 값이 없다.
        #[allow(clippy::cast_possible_truncation)]
        s.serialize_i64(*v as i64)
    } else {
        s.serialize_f64(*v)
    }
}

/// 당신 차례 항목의 종류(계약 §3 표).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// 권한 승인 대기.
    Approval,
    /// 병합 승인 대기.
    Merge,
    /// 결정·입력 대기.
    Decision,
    /// 실패.
    Failed,
    /// 커밋하지 않은 채 푸시(측정).
    Volatile,
    /// 멈춘 것 같은 작업.
    Stall,
    /// 사람 후속 일.
    Followup,
    /// 안 읽은 결과.
    Unread,
    /// 맥락 자동 압축.
    Context,
}

impl ItemKind {
    /// 모든 값(점수 순).
    pub const ALL: [ItemKind; 9] = [
        ItemKind::Approval,
        ItemKind::Merge,
        ItemKind::Decision,
        ItemKind::Failed,
        ItemKind::Volatile,
        ItemKind::Stall,
        ItemKind::Followup,
        ItemKind::Unread,
        ItemKind::Context,
    ];

    /// 계약의 이름.
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Approval => "approval",
            ItemKind::Merge => "merge",
            ItemKind::Decision => "decision",
            ItemKind::Failed => "failed",
            ItemKind::Volatile => "volatile",
            ItemKind::Stall => "stall",
            ItemKind::Followup => "followup",
            ItemKind::Unread => "unread",
            ItemKind::Context => "context",
        }
    }

    /// 이름으로 찾는다.
    pub fn parse(s: &str) -> Option<ItemKind> {
        ItemKind::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// 기본 점수.
    pub fn base(self) -> f64 {
        match self {
            ItemKind::Approval => 95.0,
            ItemKind::Merge => 90.0,
            ItemKind::Decision => 85.0,
            ItemKind::Failed => 80.0,
            ItemKind::Volatile => 78.0,
            ItemKind::Stall => 70.0,
            ItemKind::Followup => 60.0,
            ItemKind::Unread => 55.0,
            ItemKind::Context => 45.0,
        }
    }

    /// 사람이 답해야 에이전트가 움직이는가.
    pub fn blocking(self) -> bool {
        matches!(
            self,
            ItemKind::Approval | ItemKind::Merge | ItemKind::Decision
        )
    }

    /// 결정류 판정 순서(approval → merge → decision). 결정류가 아니면 `None`.
    pub fn decision_rank(self) -> Option<u8> {
        match self {
            ItemKind::Approval => Some(0),
            ItemKind::Merge => Some(1),
            ItemKind::Decision => Some(2),
            _ => None,
        }
    }

    /// 시간이 지나도 감쇠하지 않는가.
    pub fn never_decays(self) -> bool {
        self == ItemKind::Failed
    }

    /// 사람에게 보이는 이름.
    pub fn label(self) -> &'static str {
        match self {
            ItemKind::Approval => "권한 승인",
            ItemKind::Merge => "병합 승인",
            ItemKind::Decision => "결정",
            ItemKind::Failed => "실패",
            ItemKind::Volatile => "커밋 안 된 푸시",
            ItemKind::Stall => "멈춘 작업",
            ItemKind::Followup => "사람 후속",
            ItemKind::Unread => "안 읽은 결과",
            ItemKind::Context => "맥락 압축",
        }
    }
}

/// 항목 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemState {
    /// 열림.
    Open,
    /// 사람이 처리함(서명에 묶임).
    Done,
    /// 사람이 넘김(서명에 묶임).
    Dismissed,
    /// 미룸.
    Snoozed,
    /// 자동으로 닫힘.
    Closed,
}

/// 당신 차례 항목(계약 §3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// `it_` + FNV-1a 64비트 hex(`session|kind`).
    pub id: String,
    /// 종류.
    pub kind: ItemKind,
    /// 세션 id.
    pub session: String,
    /// 에이전트.
    pub agent: Option<Agent>,
    /// 프로젝트 이름(모르면 빈 문자열).
    pub project: String,
    /// 작업 폴더(모르면 빈 문자열).
    pub cwd: String,
    /// 제목.
    pub title: String,
    /// 본문.
    pub body: String,
    /// 막힘 여부.
    pub blocking: bool,
    /// 지금 점수(소수 첫째 자리).
    #[serde(serialize_with = "ser_num")]
    pub score: f64,
    /// 점수 근거(한국어).
    pub reasons: Vec<String>,
    /// 이번 신호로 처음 열린 시각.
    pub created_ms: i64,
    /// 마지막 신호 시각(대기 시간의 기준).
    pub updated_ms: i64,
    /// 신호 서명. 처리·넘기기는 이것에 묶인다.
    pub signature: String,
    /// 상태.
    pub state: ItemState,
    /// 미룸 끝 시각.
    pub snooze_until_ms: Option<i64>,
    /// 결정 선택지(channel 결정).
    pub options: Option<Vec<String>>,
    /// channel 결정 id.
    pub decision_id: Option<String>,
}

/// 전달 수준.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// 표정·자세만.
    Ambient,
    /// 말풍선(소리 없음).
    Nudge,
    /// 말풍선 + 소리·음성.
    Interrupt,
}

impl Level {
    /// 사람에게 보이는 이름.
    pub fn label(self) -> &'static str {
        match self {
            Level::Ambient => "표정만",
            Level::Nudge => "말풍선",
            Level::Interrupt => "말풍선과 소리",
        }
    }
}

/// 기분(아바타 표정).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mood {
    /// 10분 안에 열린 실패.
    Alert,
    /// 열린 막힘 항목.
    Waiting,
    /// 얼굴이 입력 중.
    Listening,
    /// 60초 안에 작업 신호가 있는 세션.
    Working,
    /// 막 끝남(5초).
    Done,
    /// 집중 모드·조용한 시간.
    Sleep,
    /// 그 밖.
    Idle,
}

/// 존재감(계약 §4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Presence {
    /// 기분.
    pub mood: Mood,
    /// 초점 항목에 마지막으로 정한 전달 수준.
    pub level: Level,
    /// 초점 항목.
    pub focus_item: Option<String>,
    /// 한 줄(한국어).
    pub line: String,
    /// 기분이 바뀐 시각.
    pub since_ms: i64,
    /// 집중 모드 끝 시각.
    pub focus_mode_until_ms: Option<i64>,
}

impl Default for Presence {
    fn default() -> Self {
        Presence {
            mood: Mood::Idle,
            level: Level::Ambient,
            focus_item: None,
            line: String::new(),
            since_ms: 0,
            focus_mode_until_ms: None,
        }
    }
}

/// 전달 판단 하나(계약 §4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Delivery {
    /// 항목 id.
    pub item: String,
    /// 수준.
    pub level: Level,
    /// 말풍선 문구.
    pub line: String,
    /// 소리·음성을 낼까.
    pub speak: bool,
    /// 판단 근거(한국어). 얼굴의 "왜?"가 보여 준다.
    pub reasons: Vec<String>,
    /// 판단 시각.
    pub at_ms: i64,
}

/// 제안 근거.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// `과민`으로 넘긴 수.
    pub dismissed: u32,
    /// 그 기간에 열린 알림 수.
    pub total: u32,
    /// 기간(일).
    pub window_days: u32,
}

/// 제안(계약 §5). 곁은 스스로 바꾸지 않고 묻는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    /// `pr_<kind>_quiet`.
    pub id: String,
    /// 대상 종류.
    pub kind: ItemKind,
    /// 바꿀 것(`quiet`).
    pub change: String,
    /// 묻는 문구.
    pub line: String,
    /// 근거.
    pub evidence: Evidence,
    /// 연 시각.
    pub opened_ms: i64,
}

/// 세션 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// 작업 중.
    Working,
    /// 열린 막힘 항목이 있음.
    Waiting,
    /// 쉬는 중.
    Idle,
    /// 끝남.
    Ended,
}

/// 세션(계약 §7 `Snapshot.sessions`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// 세션 id.
    pub id: String,
    /// 에이전트.
    pub agent: Option<Agent>,
    /// 프로젝트 이름(모르면 빈 문자열).
    pub project: String,
    /// 작업 폴더(모르면 빈 문자열).
    pub cwd: String,
    /// 상태.
    pub state: SessionState,
    /// 마지막 이벤트 시각.
    pub last_ms: i64,
    /// 이어진 channel id.
    pub channel: Option<String>,
}

/// 알림 예산의 남은 개수.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    /// 말풍선.
    pub nudge: u32,
    /// 소리.
    pub interrupt: u32,
}

/// 스냅샷의 설정 부분.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsView {
    /// 말풍선 기준 점수.
    #[serde(serialize_with = "ser_num")]
    pub nudge_min: f64,
    /// 소리 기준 점수.
    #[serde(serialize_with = "ser_num")]
    pub interrupt_min: f64,
    /// 멈춤 판단 분.
    pub stale_minutes: u32,
    /// 조용한 시간.
    pub quiet_hours: Vec<String>,
    /// 조용히 알리기로 한 종류.
    pub quiet_kinds: Vec<ItemKind>,
    /// 승인·병합 대기를 소리까지 올리는가(§13.3).
    pub approval_interrupt: bool,
}

/// 상태 전체(계약 §7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// 데몬 판.
    pub version: String,
    /// 스냅샷 시각.
    pub now_ms: i64,
    /// 존재감.
    pub presence: Presence,
    /// 열린 항목(점수 높은 순).
    pub items: Vec<Item>,
    /// 세션(최근 순).
    pub sessions: Vec<Session>,
    /// 열린 제안.
    pub proposals: Vec<Proposal>,
    /// 남은 예산.
    pub budget: Budget,
    /// 설정.
    pub settings: SettingsView,
}

/// FNV-1a 64비트.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 항목 id: `it_` + FNV-1a 64비트 hex(`session|kind`).
pub fn item_id(session: &str, kind: ItemKind) -> String {
    let key = format!("{session}|{}", kind.as_str());
    format!("it_{:016x}", fnv1a64(key.as_bytes()))
}

const HOUR_MS: f64 = 3_600_000.0;
const DAY_MS_I: i64 = 86_400_000;

/// 대기 가산: `min(15, 3·log2(시간+1))`.
pub fn waiting_bonus(wait_ms: i64) -> f64 {
    let hours = (wait_ms.max(0) as f64) / HOUR_MS;
    (3.0 * (hours + 1.0).log2()).min(15.0)
}

/// 감쇠: 7일이 지나면 하루(다 채운 날)마다 2점, 최대 40. `failed`는 0.
pub fn decay(kind: ItemKind, age_ms: i64) -> f64 {
    if kind.never_decays() {
        return 0.0;
    }
    let past = age_ms - 7 * DAY_MS_I;
    if past <= 0 {
        return 0.0;
    }
    let days = past / DAY_MS_I;
    // days는 0 이상이고 아래에서 40으로 묶인다.
    #[allow(clippy::cast_precision_loss)]
    let d = (2 * days) as f64;
    d.min(40.0)
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// 기다린 시간을 사람 말로.
pub fn fmt_duration(ms: i64) -> String {
    let m = ms.max(0) / 60_000;
    if m < 1 {
        "1분 안".to_owned()
    } else if m < 60 {
        format!("{m}분")
    } else if m < 48 * 60 {
        format!("{}시간", m / 60)
    } else {
        format!("{}일", m / (24 * 60))
    }
}

/// 점수와 근거(계약 §3: 기본 + 대기 가산 − 감쇠, 소수 첫째 자리).
pub fn score(kind: ItemKind, updated_ms: i64, now_ms: i64) -> (f64, Vec<String>) {
    let wait = (now_ms - updated_ms).max(0);
    let base = kind.base();
    let bonus = waiting_bonus(wait);
    let dec = decay(kind, wait);
    let total = round1(base + bonus - dec);
    let mut reasons = vec![format!("{} 대기: 기본 {}", kind.label(), base)];
    if wait >= 60_000 {
        reasons.push(format!(
            "{} 기다림: +{:.1}",
            fmt_duration(wait),
            round1(bonus)
        ));
    }
    if dec > 0.0 {
        reasons.push(format!("{} 지남: −{:.1}", fmt_duration(wait), dec));
    }
    (total, reasons)
}

/// 대기 가산만으로 (소수 첫째 자리로 반올림한) 점수가 `threshold`에 처음 닿는 대기 시간(밀리초).
/// 이미 닿았거나 끝내 닿지 않으면 `None`.
pub fn wait_to_reach(kind: ItemKind, threshold: f64) -> Option<i64> {
    if kind.base() >= threshold {
        return None;
    }
    // 반올림한 점수 ≥ threshold ⇔ 원래 점수 ≥ threshold − 0.05
    let need = threshold - 0.05 - kind.base();
    if need <= 0.0 || need > 15.0 {
        return None;
    }
    let hours = (need / 3.0).exp2() - 1.0;
    let ms = (hours * HOUR_MS).ceil();
    if ms.is_finite() && (0.0..1e15).contains(&ms) {
        // 위에서 범위를 확인했다.
        #[allow(clippy::cast_possible_truncation)]
        Some(ms as i64)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn item_ids_are_stable_and_distinct() {
        let a = item_id("abc123", ItemKind::Approval);
        assert!(a.starts_with("it_"));
        assert_eq!(a.len(), 3 + 16);
        assert_eq!(a, item_id("abc123", ItemKind::Approval));
        assert_ne!(a, item_id("abc123", ItemKind::Merge));
        assert_ne!(a, item_id("abc124", ItemKind::Approval));
        assert_eq!(
            a,
            format!("it_{:016x}", fnv1a64("abc123|approval".as_bytes()))
        );
    }

    #[test]
    fn base_scores_and_blocking() {
        let want = [
            (ItemKind::Approval, 95.0, true),
            (ItemKind::Merge, 90.0, true),
            (ItemKind::Decision, 85.0, true),
            (ItemKind::Failed, 80.0, false),
            (ItemKind::Volatile, 78.0, false),
            (ItemKind::Stall, 70.0, false),
            (ItemKind::Followup, 60.0, false),
            (ItemKind::Unread, 55.0, false),
            (ItemKind::Context, 45.0, false),
        ];
        for (k, b, bl) in want {
            assert_eq!(k.base(), b);
            assert_eq!(k.blocking(), bl);
            assert_eq!(ItemKind::parse(k.as_str()), Some(k));
        }
    }

    #[test]
    fn waiting_bonus_formula() {
        assert_eq!(waiting_bonus(0), 0.0);
        // 1시간: 3·log2(2) = 3
        assert!((waiting_bonus(3_600_000) - 3.0).abs() < 1e-9);
        // 3시간: 3·log2(4) = 6
        assert!((waiting_bonus(3 * 3_600_000) - 6.0).abs() < 1e-9);
        // 31시간: 3·log2(32) = 15, 그 뒤로 15에 묶인다
        assert!((waiting_bonus(31 * 3_600_000) - 15.0).abs() < 1e-9);
        assert_eq!(waiting_bonus(100 * 3_600_000), 15.0);
        assert_eq!(waiting_bonus(-5), 0.0);
    }

    #[test]
    fn score_matches_contract_example() {
        // 4분 기다린 승인: 95 + 3·log2(4/60+1) = 95.279… → 95.3
        let (s, r) = score(ItemKind::Approval, 0, 4 * 60_000);
        assert_eq!(s, 95.3);
        assert_eq!(r, vec!["권한 승인 대기: 기본 95", "4분 기다림: +0.3"]);
        let (s, r) = score(ItemKind::Unread, 1000, 1000);
        assert_eq!(s, 55.0);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn decay_after_seven_days() {
        let day = 86_400_000;
        assert_eq!(decay(ItemKind::Unread, 7 * day), 0.0);
        assert_eq!(decay(ItemKind::Unread, 7 * day + 1), 0.0);
        assert_eq!(decay(ItemKind::Unread, 8 * day), 2.0);
        assert_eq!(decay(ItemKind::Unread, 10 * day + 5), 6.0);
        assert_eq!(decay(ItemKind::Unread, 100 * day), 40.0);
        assert_eq!(decay(ItemKind::Failed, 100 * day), 0.0);
        // 8일 기다린 unread: 55 + 15 − 2 = 68
        let (s, r) = score(ItemKind::Unread, 0, 8 * day);
        assert_eq!(s, 68.0);
        assert!(r.iter().any(|x| x.contains("−2.0")), "{r:?}");
        // failed는 오래 두어도 줄지 않는다: 80 + 15
        assert_eq!(score(ItemKind::Failed, 0, 100 * day).0, 95.0);
    }

    #[test]
    fn wait_to_reach_threshold() {
        assert_eq!(wait_to_reach(ItemKind::Approval, 95.0), None);
        assert_eq!(wait_to_reach(ItemKind::Context, 80.0), None);
        let w = wait_to_reach(ItemKind::Stall, 80.0).unwrap();
        assert!(score(ItemKind::Stall, 0, w + 1000).0 >= 80.0);
        assert!(score(ItemKind::Stall, 0, w - 60_000).0 < 80.0);
        // 80→95: 거의 15점이 필요하므로 31시간 가까이
        let w = wait_to_reach(ItemKind::Failed, 95.0).unwrap();
        assert!(w < 31 * 3_600_000 && w > 30 * 3_600_000, "{w}");
        assert_eq!(score(ItemKind::Failed, 0, w + 1000).0, 95.0);
        assert!(score(ItemKind::Failed, 0, w - 60_000).0 < 95.0);
    }

    #[test]
    fn numbers_serialize_as_integers_when_whole() {
        let v = SettingsView {
            nudge_min: 80.0,
            interrupt_min: 95.5,
            stale_minutes: 30,
            quiet_hours: vec![],
            quiet_kinds: vec![ItemKind::Unread],
            approval_interrupt: false,
        };
        let j = serde_json::to_string(&v).unwrap();
        assert!(j.contains("\"nudge_min\":80,"), "{j}");
        assert!(j.contains("\"interrupt_min\":95.5"), "{j}");
        assert!(j.contains("\"quiet_kinds\":[\"unread\"]"), "{j}");
        assert!(j.contains("\"approval_interrupt\":false"), "{j}");
    }
}
