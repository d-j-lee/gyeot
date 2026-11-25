//! 학습 기록(`learn.json`)과 제안(계약 §5).
//!
//! 기록은 열쇠(항목 id·서명·동작)로 중복을 막는다. 그래서 `learn.json`을 읽은 뒤 최근 이벤트 로그를
//! 다시 적용해도 같은 반응이 두 번 세어지지 않는다.

use crate::model::{Evidence, ItemKind, Proposal};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// 제안을 판단하는 기간(일).
pub const WINDOW_DAYS: u32 = 14;
/// 제안을 여는 최소 `과민` 넘기기 수.
pub const MIN_DISMISSED: u32 = 5;
/// 제안을 여는 최소 넘기기 비율.
pub const MIN_RATIO: f64 = 0.7;
/// 제안에 셈하는 넘기기 이유.
pub const COUNTED_REASON: &str = "과민";
/// 넘기기 이유로 받는 값.
pub const DISMISS_REASONS: [&str; 3] = ["과민", "이미 처리", "무관"];

const DAY_MS: i64 = 86_400_000;
const WINDOW_MS: i64 = WINDOW_DAYS as i64 * DAY_MS;

/// 종류별 누적 반응 집계(표시·진단용).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KindStats {
    /// 열린 알림 수.
    #[serde(default)]
    pub opened: u64,
    /// 열어 본 수.
    #[serde(default)]
    pub open: u64,
    /// 처리한 수.
    #[serde(default)]
    pub done: u64,
    /// 미룬 수.
    #[serde(default)]
    pub snoozed: u64,
    /// 넘긴 수(이유별, 이유가 없으면 빈 문자열).
    #[serde(default)]
    pub dismissed: BTreeMap<String, u64>,
}

/// 최근 기록 하나.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearnRecord {
    /// 시각.
    pub ts_ms: i64,
    /// 종류.
    pub kind: ItemKind,
    /// `opened`·`open`·`done`·`snooze`·`dismiss`.
    pub what: String,
    /// 넘기기 이유.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `learn.json`의 내용.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Learn {
    /// 형식 판.
    #[serde(default = "one")]
    pub version: u32,
    /// 종류별 누적 집계.
    #[serde(default)]
    pub kinds: BTreeMap<ItemKind, KindStats>,
    /// 사용자가 받아들인 `quiet` 조정.
    #[serde(default)]
    pub quiet_kinds: BTreeSet<ItemKind>,
    /// 거절한 제안을 다시 열지 않는 끝 시각(제안 id → ms).
    #[serde(default)]
    pub suppressed_until: BTreeMap<String, i64>,
    /// 열린 제안.
    #[serde(default)]
    pub proposals: BTreeMap<String, Proposal>,
    /// 최근 14일 기록(열쇠 → 기록).
    #[serde(default)]
    pub recent: BTreeMap<String, LearnRecord>,
}

fn one() -> u32 {
    1
}

impl Default for Learn {
    fn default() -> Self {
        Learn {
            version: 1,
            kinds: BTreeMap::new(),
            quiet_kinds: BTreeSet::new(),
            suppressed_until: BTreeMap::new(),
            proposals: BTreeMap::new(),
            recent: BTreeMap::new(),
        }
    }
}

/// 종류에 대한 제안 id.
pub fn proposal_id(kind: ItemKind) -> String {
    format!("pr_{}_quiet", kind.as_str())
}

impl Learn {
    /// 기록을 더한다. 같은 열쇠가 이미 있으면 아무것도 하지 않고 `false`.
    pub(crate) fn record(
        &mut self,
        key: String,
        ts_ms: i64,
        kind: ItemKind,
        what: &str,
        reason: Option<&str>,
    ) -> bool {
        self.prune(ts_ms);
        if self.recent.contains_key(&key) {
            return false;
        }
        let st = self.kinds.entry(kind).or_default();
        match what {
            "opened" => st.opened += 1,
            "open" => st.open += 1,
            "done" => st.done += 1,
            "snooze" => st.snoozed += 1,
            "dismiss" => {
                *st.dismissed
                    .entry(reason.unwrap_or("").to_owned())
                    .or_default() += 1
            }
            _ => {}
        }
        self.recent.insert(
            key,
            LearnRecord {
                ts_ms,
                kind,
                what: what.to_owned(),
                reason: reason.map(str::to_owned),
            },
        );
        true
    }

    /// 기간이 지난 기록과 풀린 억제를 지운다.
    pub(crate) fn prune(&mut self, now_ms: i64) {
        let floor = now_ms - WINDOW_MS;
        self.recent.retain(|_, r| r.ts_ms >= floor);
        self.suppressed_until.retain(|_, until| *until > now_ms);
    }

    /// 최근 14일의 (`과민` 넘기기 수, 열린 알림 수).
    pub fn window_counts(&self, kind: ItemKind, now_ms: i64) -> (u32, u32) {
        let floor = now_ms - WINDOW_MS;
        let mut dismissed = 0u32;
        let mut total = 0u32;
        for r in self.recent.values() {
            if r.kind != kind || r.ts_ms < floor || r.ts_ms > now_ms {
                continue;
            }
            match r.what.as_str() {
                "opened" => total = total.saturating_add(1),
                "dismiss" if r.reason.as_deref() == Some(COUNTED_REASON) => {
                    dismissed = dismissed.saturating_add(1)
                }
                _ => {}
            }
        }
        (dismissed, total.max(dismissed))
    }

    /// 조건이 맞으면 제안을 연다. 새로 열었으면 돌려준다.
    pub(crate) fn maybe_propose(&mut self, kind: ItemKind, now_ms: i64) -> Option<Proposal> {
        self.prune(now_ms);
        let id = proposal_id(kind);
        if self.quiet_kinds.contains(&kind)
            || self.proposals.contains_key(&id)
            || self.suppressed_until.get(&id).is_some_and(|u| *u > now_ms)
        {
            return None;
        }
        let (dismissed, total) = self.window_counts(kind, now_ms);
        if dismissed < MIN_DISMISSED || total == 0 {
            return None;
        }
        if f64::from(dismissed) / f64::from(total) < MIN_RATIO {
            return None;
        }
        let p = Proposal {
            id: id.clone(),
            kind,
            change: "quiet".to_owned(),
            line: format!(
                "최근 ‘{}’ 알림을 {}번 중 {}번 넘기셨어요. 이 종류는 앞으로 조용히(표정만) 알릴까요?",
                kind.label(),
                total,
                dismissed
            ),
            evidence: Evidence {
                dismissed,
                total,
                window_days: WINDOW_DAYS,
            },
            opened_ms: now_ms,
        };
        self.proposals.insert(id, p.clone());
        Some(p)
    }

    /// 제안에 답한다. 없는 제안이면 `false`.
    pub(crate) fn respond(&mut self, id: &str, accept: bool, now_ms: i64) -> bool {
        let Some(p) = self.proposals.remove(id) else {
            return false;
        };
        if accept {
            self.quiet_kinds.insert(p.kind);
        } else {
            self.suppressed_until
                .insert(id.to_owned(), now_ms + WINDOW_MS);
        }
        true
    }

    /// 열린 제안(id 순).
    pub fn open_proposals(&self) -> Vec<Proposal> {
        self.proposals.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dismiss(l: &mut Learn, n: u32, ts: i64, reason: &str) {
        for _ in 0..n {
            let i = l.recent.len();
            let key = format!("o|{reason}|{ts}|{i}");
            assert!(l.record(key, ts, ItemKind::Unread, "opened", None));
            let key = format!("a|{reason}|{ts}|{i}");
            assert!(l.record(key, ts, ItemKind::Unread, "dismiss", Some(reason)));
        }
    }

    #[test]
    fn opens_after_five_oversensitive_dismissals() {
        let mut l = Learn::default();
        dismiss(&mut l, 4, 1000, "과민");
        assert!(l.maybe_propose(ItemKind::Unread, 1000).is_none());
        dismiss(&mut l, 1, 2000, "과민");
        let p = l.maybe_propose(ItemKind::Unread, 2000).expect("proposal");
        assert_eq!(p.id, "pr_unread_quiet");
        assert_eq!(p.evidence.dismissed, 5);
        assert_eq!(p.evidence.total, 5);
        assert!(
            p.line.contains("‘안 읽은 결과’ 알림을 5번 중 5번"),
            "{}",
            p.line
        );
        // 이미 열린 제안은 다시 열지 않는다.
        assert!(l.maybe_propose(ItemKind::Unread, 2000).is_none());
    }

    #[test]
    fn other_reasons_are_counted_but_not_toward_ratio() {
        let mut l = Learn::default();
        dismiss(&mut l, 6, 1000, "이미 처리");
        dismiss(&mut l, 3, 1000, "무관");
        assert!(l.maybe_propose(ItemKind::Unread, 1000).is_none());
        assert_eq!(l.kinds[&ItemKind::Unread].dismissed["이미 처리"], 6);
        assert_eq!(l.kinds[&ItemKind::Unread].dismissed["무관"], 3);
    }

    #[test]
    fn ratio_must_reach_point_seven() {
        let mut l = Learn::default();
        dismiss(&mut l, 5, 1000, "과민");
        for i in 0..3 {
            l.record(format!("o|x|{i}"), 1000, ItemKind::Unread, "opened", None);
        }
        // 5/8 = 0.625
        assert!(l.maybe_propose(ItemKind::Unread, 1000).is_none());
        dismiss(&mut l, 1, 1000, "과민");
        // 6/9 = 0.667
        assert!(l.maybe_propose(ItemKind::Unread, 1000).is_none());
        dismiss(&mut l, 1, 1000, "과민");
        // 7/10 = 0.7
        assert!(l.maybe_propose(ItemKind::Unread, 1000).is_some());
    }

    #[test]
    fn records_outside_window_do_not_count() {
        let mut l = Learn::default();
        dismiss(&mut l, 5, 0, "과민");
        let later = 15 * DAY_MS;
        assert_eq!(l.window_counts(ItemKind::Unread, later), (0, 0));
        assert!(l.maybe_propose(ItemKind::Unread, later).is_none());
        assert!(l.recent.is_empty());
        // 누적 집계는 남는다.
        assert_eq!(l.kinds[&ItemKind::Unread].opened, 5);
    }

    #[test]
    fn duplicate_keys_are_ignored() {
        let mut l = Learn::default();
        assert!(l.record("k".into(), 1, ItemKind::Stall, "done", None));
        assert!(!l.record("k".into(), 1, ItemKind::Stall, "done", None));
        assert_eq!(l.kinds[&ItemKind::Stall].done, 1);
    }

    #[test]
    fn accept_and_reject() {
        let mut l = Learn::default();
        dismiss(&mut l, 5, 1000, "과민");
        l.maybe_propose(ItemKind::Unread, 1000).unwrap();
        assert!(l.respond("pr_unread_quiet", true, 1000));
        assert!(l.quiet_kinds.contains(&ItemKind::Unread));
        assert!(!l.respond("pr_unread_quiet", true, 1000));
        // 받아들인 종류는 다시 제안하지 않는다.
        dismiss(&mut l, 5, 2000, "과민");
        assert!(l.maybe_propose(ItemKind::Unread, 2000).is_none());

        let mut l = Learn::default();
        dismiss(&mut l, 5, 1000, "과민");
        l.maybe_propose(ItemKind::Unread, 1000).unwrap();
        assert!(l.respond("pr_unread_quiet", false, 1000));
        assert!(l.quiet_kinds.is_empty());
        dismiss(&mut l, 5, 2000, "과민");
        assert!(l.maybe_propose(ItemKind::Unread, 2000).is_none());
        // 14일 뒤에는 다시 열 수 있다.
        let later = 1000 + 14 * DAY_MS + 1;
        dismiss(&mut l, 5, later, "과민");
        assert!(l.maybe_propose(ItemKind::Unread, later).is_some());
    }

    #[test]
    fn serde_roundtrip() {
        let mut l = Learn::default();
        dismiss(&mut l, 5, 1000, "과민");
        l.maybe_propose(ItemKind::Unread, 1000);
        let s = serde_json::to_string(&l).unwrap();
        let back: Learn = serde_json::from_str(&s).unwrap();
        assert_eq!(back, l);
        let empty: Learn = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, Learn::default());
    }
}
