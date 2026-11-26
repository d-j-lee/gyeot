//! 이벤트 소싱 엔진: 항목(§3), 존재감·전달(§4), 제안(§5), 세션·channel(§7).
//!
//! 시간에 따라 일어나는 일(멈춤, 미룸 끝, 대기 승격, 기분 만료, 조용한 시간 경계)은 모두 "예정 시각"에
//! 정확히 처리한다. 그래서 `tick`을 몇 번 불렀는지와 상관없이, 같은 이벤트를 같은 순서로 다시 적용하면
//! 같은 상태가 나온다(이벤트 로그 재생 = 상태 복원).

use crate::event::{Agent, Event, EventBody, Source};
use crate::learn::{DISMISS_REASONS, Learn};
use crate::model::{
    Budget, Delivery, Item, ItemKind, ItemState, Level, Mood, Presence, Proposal, Session,
    SessionState, SettingsView, Snapshot, item_id, score, wait_to_reach,
};
use crate::policy::Policy;
use crate::text::{cwd_key, preview, project_name};
use std::collections::BTreeMap;
use std::fmt;

const MIN_MS: i64 = 60_000;
const HOUR_MS: i64 = 3_600_000;
/// `permission_request`가 승인 대기를 보강하는 창.
const PERM_WINDOW_MS: i64 = 10_000;
/// 같은 세션에 이만큼 안에 입력이 있으면 사람이 거기 있다고 본다.
const PRESENT_MS: i64 = 60_000;
/// 소리 후보가 되는 대기.
const INTERRUPT_WAIT_MS: i64 = 2 * MIN_MS;
/// 집중 모드·조용한 시간에 말풍선을 허락하는 대기.
const QUIET_NUDGE_WAIT_MS: i64 = 3 * MIN_MS;
/// 집중 모드·조용한 시간에 말풍선을 허락하는 점수.
const QUIET_NUDGE_SCORE: f64 = 95.0;
/// `alert` 기분이 이어지는 시간.
const ALERT_MS: i64 = 10 * MIN_MS;
/// `working` 기분의 창.
const WORKING_MS: i64 = 60_000;
/// `done` 기분의 길이.
const DONE_MS: i64 = 5_000;
/// 얼굴이 "입력 중"이라고 알린 뒤 저절로 풀리는 시간.
const LISTENING_TTL_MS: i64 = 2 * MIN_MS;
/// 집중 모드 기본·최대 분.
const FOCUS_DEFAULT_MIN: i64 = 60;
const FOCUS_MAX_MIN: i64 = 24 * 60;
/// 미루기 기본·최대 분.
const SNOOZE_DEFAULT_MIN: i64 = 1440;
const SNOOZE_MAX_MIN: i64 = 30 * 1440;
/// 끝난 세션·조용한 세션을 목록에서 지우는 시간(열린 항목이 없을 때).
const ENDED_KEEP_MS: i64 = HOUR_MS;
const IDLE_KEEP_MS: i64 = 48 * HOUR_MS;
/// 예산(계약 §4.6).
const NUDGE_CAP: i64 = 4;
const NUDGE_PERIOD_MS: i64 = 15 * MIN_MS;
const INTERRUPT_CAP: i64 = 1;
const INTERRUPT_PERIOD_MS: i64 = 30 * MIN_MS;
/// 한 번에 처리하는 예정 시각 수 상한(안전장치).
const MAX_TIMER_STEPS: usize = 100_000;

/// 데몬 판(스냅샷 `version`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 사람이 곁에서 하는 동작. 모두 `user_action` 이벤트로 기록되어 재생된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 열어 봄(상태 유지, 반응 기록).
    Open,
    /// 처리함.
    Done,
    /// 넘기기.
    Dismiss,
    /// 미루기.
    Snooze,
    /// 집중 모드 켜기.
    FocusOn,
    /// 집중 모드 끄기.
    FocusOff,
    /// 얼굴이 입력 중.
    ListeningOn,
    /// 입력 끝.
    ListeningOff,
    /// 제안 받아들이기.
    ProposalAccept,
    /// 제안 거절.
    ProposalReject,
    /// channel 결정에 답함(대상은 decision_id).
    Decide,
    /// 조용히 알리기 조정 되돌리기(대상은 종류 이름).
    Unquiet,
}

impl Action {
    const ALL: [Action; 12] = [
        Action::Open,
        Action::Done,
        Action::Dismiss,
        Action::Snooze,
        Action::FocusOn,
        Action::FocusOff,
        Action::ListeningOn,
        Action::ListeningOff,
        Action::ProposalAccept,
        Action::ProposalReject,
        Action::Decide,
        Action::Unquiet,
    ];

    /// `user_action.action`에 쓰는 이름.
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Open => "open",
            Action::Done => "done",
            Action::Dismiss => "dismiss",
            Action::Snooze => "snooze",
            Action::FocusOn => "focus_on",
            Action::FocusOff => "focus_off",
            Action::ListeningOn => "listening_on",
            Action::ListeningOff => "listening_off",
            Action::ProposalAccept => "proposal_accept",
            Action::ProposalReject => "proposal_reject",
            Action::Decide => "decide",
            Action::Unquiet => "unquiet",
        }
    }

    /// 이름으로 찾는다.
    pub fn parse(s: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.as_str() == s)
    }

    /// 항목에 하는 동작인가(`POST /v1/items/{id}/action`).
    pub fn is_item_action(self) -> bool {
        matches!(
            self,
            Action::Open | Action::Done | Action::Dismiss | Action::Snooze
        )
    }
}

/// 동작을 받지 못한 이유.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActError {
    /// 없는 항목.
    UnknownItem,
    /// 없는(열려 있지 않은) 제안.
    UnknownProposal,
    /// 없는 결정.
    UnknownDecision,
    /// 받지 않는 넘기기 이유.
    InvalidReason,
    /// 범위 밖의 분.
    InvalidMinutes,
    /// 없는 종류.
    InvalidKind,
}

impl fmt::Display for ActError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ActError::UnknownItem => "그런 항목이 없어요",
            ActError::UnknownProposal => "그런 제안이 없어요",
            ActError::UnknownDecision => "그런 결정이 없어요",
            ActError::InvalidReason => "넘기기 이유는 과민·이미 처리·무관 가운데 하나예요",
            ActError::InvalidMinutes => "분 값이 범위를 벗어났어요",
            ActError::InvalidKind => "그런 종류가 없어요",
        };
        f.write_str(s)
    }
}

impl std::error::Error for ActError {}

/// 엔진이 바깥에 알리는 변화. 서버가 SSE로 바꿔 보낸다.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// 항목이 새로 열리거나 바뀜(`item`).
    ItemUpsert(Item),
    /// 항목이 닫힘·처리·넘기기·미룸(`item_removed`).
    ItemRemoved {
        /// 항목 id.
        id: String,
    },
    /// 존재감이 바뀜(`presence`).
    Presence(Presence),
    /// 전달 판단(`deliver`).
    Deliver(Delivery),
    /// 제안이 열림(`proposal`).
    Proposal(Proposal),
    /// 세션 목록이 바뀜(`sessions`).
    Sessions(Vec<Session>),
    /// 위 효과로 나타낼 수 없는 변화(제안 답·조정 되돌리기). 스냅샷을 다시 보내야 한다.
    Resync,
}

/// channel 정보(서버의 라우팅용).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelInfo {
    /// channel id.
    pub cid: String,
    /// channel 프로세스 PID.
    pub pid: u32,
    /// 부모 PID.
    pub ppid: u32,
    /// 작업 폴더.
    pub cwd: String,
    /// 이어진 세션.
    pub session: Option<String>,
}

/// channel 결정 정보.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionInfo {
    /// 결정 id.
    pub decision_id: String,
    /// 결정 항목 id.
    pub item_id: String,
    /// 세션.
    pub session: String,
    /// 선택지.
    pub options: Vec<String>,
    /// 항목이 아직 열려(또는 미뤄져) 있는가.
    pub open: bool,
    /// 항목 상태.
    pub state: ItemState,
}

/// GCRA 토큰 통: `tokens(t) = clamp(floor((t − base) / period), 0, cap)`.
/// 상태가 `base` 하나라서 언제 몇 번 확인하든 결과가 같다.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Bucket {
    cap: i64,
    period: i64,
    base: i64,
}

impl Bucket {
    fn new(cap: i64, period: i64) -> Bucket {
        Bucket {
            cap,
            period,
            base: i64::MIN / 4,
        }
    }

    fn tokens(&self, t: i64) -> u32 {
        let n = t.saturating_sub(self.base).div_euclid(self.period);
        u32::try_from(n.clamp(0, self.cap)).unwrap_or(0)
    }

    fn try_take(&mut self, t: i64) -> bool {
        if self.tokens(t) == 0 {
            return false;
        }
        self.base = self.base.max(t - self.cap * self.period) + self.period;
        true
    }
}

/// 검사를 통과한 동작의 (이유, 분, 세션).
type Validated = (Option<String>, Option<i64>, Option<String>);

#[derive(Debug, Clone)]
struct ItemRec {
    item: Item,
    /// 마지막으로 판단한 후보 수준(예산 적용 전). 이보다 높아질 때만 다시 판단한다.
    cand: Option<Level>,
    /// 마지막으로 실제 전달한 수준.
    delivered: Option<Level>,
    /// 처리·넘기기가 묶인 서명.
    handled_sig: Option<String>,
    /// 게이트가 연 `failed`인가.
    from_gate: bool,
}

impl ItemRec {
    fn is_live(&self) -> bool {
        matches!(self.item.state, ItemState::Open | ItemState::Snoozed)
    }
    fn is_open(&self) -> bool {
        self.item.state == ItemState::Open
    }
}

#[derive(Debug, Clone)]
struct SessionRec {
    id: String,
    agent: Option<Agent>,
    cwd: Option<String>,
    cwd_key: Option<String>,
    project: Option<String>,
    pid_hint: Option<u32>,
    last_ms: i64,
    working: bool,
    last_work_ms: i64,
    last_prompt_ms: Option<i64>,
    ended: bool,
    channel: Option<String>,
    /// 한 번이라도 `activity`를 냈는가(계약 §12.3). 내지 않은 세션은 승인 여부를 알 수 없다.
    seen_activity: bool,
}

#[derive(Debug, Clone)]
struct ChannelRec {
    cid: String,
    pid: u32,
    ppid: u32,
    cwd: String,
    cwd_key: String,
    session: Option<String>,
    by_pid: bool,
    opened_ms: i64,
}

/// 열린 승인 요청 하나(계약 §14.2). 세션마다 목록으로 두고, 목록이 비어야 승인·병합 항목이 닫힌다.
#[derive(Debug, Clone)]
struct Req {
    /// 들어온 순서(같은 시각끼리 가른다).
    seq: u64,
    /// 요청한 하위 에이전트(주 에이전트면 `None`).
    agent_id: Option<String>,
    /// 도구. 알림만 오고 요청을 못 본 대기는 `None`(모르는 도구).
    tool: Option<String>,
    /// `tool_input` 미리보기(마스킹·300자).
    preview: String,
    /// 붙은 알림의 문구(미리보기가 없을 때 본문으로 쓴다).
    note: String,
    /// 요청(또는 알림) 시각.
    ts: i64,
    /// 병합 명령으로 보이는가.
    merge: bool,
    /// 승인 알림이 붙었는가.
    notified: bool,
    /// 병합 보류(10초)가 끝났는가(알림이 붙었거나 창이 지났다). 병합이 아니면 처음부터 참.
    held: bool,
}

/// `a`가 열려 있으면 `b`를 열지 않고, `a`가 열리면 `b`를 닫는가. 승인끼리(approval → merge)만(§3, §15.2).
fn overrides(a: ItemKind, b: ItemKind) -> bool {
    matches!((a, b), (ItemKind::Approval, ItemKind::Merge))
}

/// 세션마다 기억하는 열린 요청 수.
const REQS_MAX: usize = 64;

/// 목록에서 `pred`에 맞는 가장 오래된 요청 하나를 지운다.
fn take_oldest(list: &mut Vec<Req>, pred: impl Fn(&Req) -> bool) -> bool {
    let pos = list
        .iter()
        .enumerate()
        .filter(|(_, r)| pred(r))
        .min_by_key(|(_, r)| (r.ts, r.seq))
        .map(|(i, _)| i);
    match pos {
        Some(i) => {
            list.remove(i);
            true
        }
        None => false,
    }
}

/// 가장 최근 요청(시각, 들어온 순서).
fn latest(list: &[Req], pred: impl Fn(&Req) -> bool) -> Option<usize> {
    list.iter()
        .enumerate()
        .filter(|(_, r)| pred(r))
        .max_by_key(|(_, r)| (r.ts, r.seq))
        .map(|(i, _)| i)
}

/// 항목을 여는 신호.
#[derive(Debug, Clone)]
struct Signal {
    kind: ItemKind,
    /// 열린(갱신된) 시각.
    ts: i64,
    /// 서명에 쓰는 시각(보통 `ts`).
    sig_ts: i64,
    title: String,
    body: String,
    options: Option<Vec<String>>,
    decision_id: Option<String>,
    from_gate: bool,
}

impl Signal {
    fn new(kind: ItemKind, ts: i64, title: String, body: String) -> Signal {
        Signal {
            kind,
            ts,
            sig_ts: ts,
            title,
            body,
            options: None,
            decision_id: None,
            from_gate: false,
        }
    }
}

#[derive(Debug, Default)]
struct Fx {
    order: Vec<String>,
    items: BTreeMap<String, Option<Item>>,
    deliveries: Vec<Delivery>,
    proposals: Vec<Proposal>,
    resync: bool,
}

impl Fx {
    fn touch(&mut self, id: &str, v: Option<Item>) {
        if !self.items.contains_key(id) {
            self.order.push(id.to_owned());
        }
        self.items.insert(id.to_owned(), v);
    }
}

/// 곁의 엔진. 순수하고 결정적이다.
#[derive(Debug, Clone)]
pub struct Engine {
    policy: Policy,
    /// 마지막으로 처리한 시점(이벤트·예정 시각). `tick`만으로는 움직이지 않는다.
    clock: Option<i64>,
    sessions: BTreeMap<String, SessionRec>,
    items: BTreeMap<String, ItemRec>,
    channels: BTreeMap<String, ChannelRec>,
    /// 세션마다 열린 승인 요청(§14.2).
    reqs: BTreeMap<String, Vec<Req>>,
    req_seq: u64,
    focus_until: Option<i64>,
    listening_until: Option<i64>,
    done_until: Option<i64>,
    done_session: Option<String>,
    nudge: Bucket,
    interrupt: Bucket,
    learn: Learn,
    learn_rev: u64,
    presence: Presence,
    emitted_presence: Presence,
    sessions_key: String,
}

impl Engine {
    /// 빈 엔진.
    pub fn new(policy: Policy) -> Engine {
        Engine::with_learn(policy, Learn::default())
    }

    /// 저장된 학습 기록을 안고 시작한다(그 뒤 최근 이벤트 로그를 재생한다).
    pub fn with_learn(policy: Policy, learn: Learn) -> Engine {
        Engine {
            policy,
            clock: None,
            sessions: BTreeMap::new(),
            items: BTreeMap::new(),
            channels: BTreeMap::new(),
            reqs: BTreeMap::new(),
            req_seq: 0,
            focus_until: None,
            listening_until: None,
            done_until: None,
            done_session: None,
            nudge: Bucket::new(NUDGE_CAP, NUDGE_PERIOD_MS),
            interrupt: Bucket::new(INTERRUPT_CAP, INTERRUPT_PERIOD_MS),
            learn,
            learn_rev: 0,
            presence: Presence::default(),
            emitted_presence: Presence::default(),
            sessions_key: String::new(),
        }
    }

    /// 정책.
    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// 현지 오프셋을 바꾼다(서머타임 등). 재생에는 지금 값이 쓰인다.
    pub fn set_utc_offset_minutes(&mut self, minutes: i32) {
        self.policy.utc_offset_minutes = minutes;
    }

    /// 학습 기록.
    pub fn learn(&self) -> &Learn {
        &self.learn
    }

    /// 학습 기록이 바뀔 때마다 늘어나는 수(저장 판단용).
    pub fn learn_revision(&self) -> u64 {
        self.learn_rev
    }

    /// 마지막으로 처리한 시점.
    pub fn clock_ms(&self) -> Option<i64> {
        self.clock
    }

    /// 지금 존재감(마지막 처리 시점 기준).
    pub fn presence(&self) -> &Presence {
        &self.presence
    }

    /// 이벤트 하나를 적용한다.
    pub fn apply(&mut self, ev: &Event) -> Vec<Effect> {
        let mut fx = Fx::default();
        self.advance_to(ev.ts_ms, &mut fx);
        let now = self.clock.map_or(ev.ts_ms, |c| c.max(ev.ts_ms));
        self.clock = Some(now);
        self.process(ev, now, &mut fx);
        self.run_checks(now, &mut fx);
        self.finish(fx)
    }

    /// 시간을 `now_ms`까지 흘린다(멈춤·예산·미룸·기분 만료·대기 승격).
    pub fn tick(&mut self, now_ms: i64) -> Vec<Effect> {
        let mut fx = Fx::default();
        if self.clock.is_none() {
            self.clock = Some(now_ms);
            self.run_checks(now_ms, &mut fx);
        } else {
            self.advance_to(now_ms, &mut fx);
        }
        self.finish(fx)
    }

    /// 사람의 동작. 검사를 통과하면 기록할 `user_action` 이벤트와 효과를 돌려준다.
    pub fn act(
        &mut self,
        target: &str,
        action: Action,
        reason: Option<&str>,
        minutes: Option<i64>,
        now_ms: i64,
    ) -> Result<(Event, Vec<Effect>), ActError> {
        let (reason, minutes, session) = self.validate(target, action, reason, minutes)?;
        let ev = Event {
            id: format!("{now_ms}-0"),
            ts_ms: now_ms,
            source: Source::Gyeot,
            agent: None,
            agent_id: None,
            session,
            cwd: None,
            pid_hint: None,
            raw_event: None,
            body: EventBody::UserAction {
                item: target.to_owned(),
                action: action.as_str().to_owned(),
                reason,
                minutes,
            },
        };
        let fx = self.apply(&ev);
        Ok((ev, fx))
    }

    /// 집중 모드(분이 없으면 60분).
    pub fn set_focus(
        &mut self,
        on: bool,
        minutes: Option<i64>,
        now_ms: i64,
    ) -> Result<(Event, Vec<Effect>), ActError> {
        if on {
            self.act("", Action::FocusOn, None, minutes, now_ms)
        } else {
            self.act("", Action::FocusOff, None, None, now_ms)
        }
    }

    /// 얼굴이 입력 중인가(2분 뒤 저절로 풀린다).
    pub fn set_listening(&mut self, on: bool, now_ms: i64) -> (Event, Vec<Effect>) {
        let action = if on {
            Action::ListeningOn
        } else {
            Action::ListeningOff
        };
        match self.act("", action, None, None, now_ms) {
            Ok(r) => r,
            // 이 두 동작은 검사에서 실패하지 않는다.
            Err(_) => unreachable!("listening action cannot fail validation"),
        }
    }

    /// 제안에 답한다.
    pub fn respond_proposal(
        &mut self,
        id: &str,
        accept: bool,
        now_ms: i64,
    ) -> Result<(Event, Vec<Effect>), ActError> {
        let a = if accept {
            Action::ProposalAccept
        } else {
            Action::ProposalReject
        };
        self.act(id, a, None, None, now_ms)
    }

    /// channel 결정에 답했음을 기록한다(결정 항목이 처리됨으로 닫힌다).
    pub fn decide(
        &mut self,
        decision_id: &str,
        now_ms: i64,
    ) -> Result<(Event, Vec<Effect>), ActError> {
        self.act(decision_id, Action::Decide, None, None, now_ms)
    }

    /// 상태 전체. 존재감은 마지막 처리 시점 값이므로 먼저 `tick(now_ms)`를 부르는 것이 좋다.
    pub fn snapshot(&self, now_ms: i64) -> Snapshot {
        let mut items: Vec<Item> = self
            .items
            .values()
            .filter(|r| r.is_open())
            .map(|r| self.view(r, now_ms))
            .collect();
        sort_items(&mut items);
        Snapshot {
            version: VERSION.to_owned(),
            now_ms,
            presence: self.presence.clone(),
            items,
            sessions: self.session_views(),
            proposals: self.learn.open_proposals(),
            budget: Budget {
                nudge: self.nudge.tokens(now_ms),
                interrupt: self.interrupt.tokens(now_ms),
            },
            settings: SettingsView {
                nudge_min: self.policy.nudge_min,
                interrupt_min: self.policy.interrupt_min,
                stale_minutes: self.policy.stale_minutes,
                quiet_hours: self.policy.quiet_hours_strings(),
                quiet_kinds: self.learn.quiet_kinds.iter().copied().collect(),
                approval_interrupt: self.policy.approval_interrupt,
            },
        }
    }

    /// 항목 하나(상태와 상관없이, 점수는 `now_ms` 기준).
    pub fn item(&self, id: &str, now_ms: i64) -> Option<Item> {
        self.items.get(id).map(|r| self.view(r, now_ms))
    }

    /// 세션 목록(최근 순).
    pub fn sessions(&self) -> Vec<Session> {
        self.session_views()
    }

    /// channel 하나.
    pub fn channel(&self, cid: &str) -> Option<ChannelInfo> {
        self.channels.get(cid).map(channel_info)
    }

    /// 모든 channel.
    pub fn channels(&self) -> Vec<ChannelInfo> {
        self.channels.values().map(channel_info).collect()
    }

    /// 세션에 이어진 channel.
    pub fn channel_of_session(&self, session: &str) -> Option<String> {
        self.sessions.get(session).and_then(|s| s.channel.clone())
    }

    /// channel이 있는 세션 가운데 가장 최근에 움직인 것: (세션, channel).
    pub fn latest_channel_session(&self) -> Option<(String, String)> {
        self.sessions
            .values()
            .filter_map(|s| s.channel.as_ref().map(|c| (s, c)))
            .max_by(|(a, _), (b, _)| a.last_ms.cmp(&b.last_ms).then_with(|| a.id.cmp(&b.id)))
            .map(|(s, c)| (s.id.clone(), c.clone()))
    }

    /// 세션이 있는가.
    pub fn has_session(&self, session: &str) -> bool {
        self.sessions.contains_key(session)
    }

    /// 세션의 프로젝트 이름.
    pub fn session_project(&self, session: &str) -> Option<String> {
        self.sessions.get(session).and_then(|s| s.project.clone())
    }

    /// channel 결정을 찾는다.
    pub fn decision(&self, decision_id: &str) -> Option<DecisionInfo> {
        self.items.values().find_map(|r| {
            (r.item.decision_id.as_deref() == Some(decision_id)).then(|| DecisionInfo {
                decision_id: decision_id.to_owned(),
                item_id: r.item.id.clone(),
                session: r.item.session.clone(),
                options: r.item.options.clone().unwrap_or_default(),
                open: r.is_live(),
                state: r.item.state,
            })
        })
    }

    // ───────────────────────── 시간 ─────────────────────────

    fn advance_to(&mut self, t: i64, fx: &mut Fx) {
        let Some(mut c) = self.clock else {
            return;
        };
        for _ in 0..MAX_TIMER_STEPS {
            match self.next_due(c) {
                Some(d) if d <= t => {
                    self.clock = Some(d);
                    self.run_checks(d, fx);
                    c = d;
                }
                _ => return,
            }
        }
    }

    fn next_due(&self, c: i64) -> Option<i64> {
        let mut best: Option<i64> = None;
        let mut consider = |t: i64| {
            if t > c {
                best = Some(best.map_or(t, |b| b.min(t)));
            }
        };
        for t in [self.focus_until, self.listening_until, self.done_until]
            .into_iter()
            .flatten()
        {
            consider(t);
        }
        for r in self.reqs.values().flatten() {
            if !r.held {
                consider(r.ts + PERM_WINDOW_MS + 1);
            }
        }
        // 소리는 기본 점수로만 정하므로(§14.4) 대기 가산이 닿는 문턱은 말풍선 기준들뿐이다.
        let thresholds = [self.policy.nudge_min, QUIET_NUDGE_SCORE];
        for r in self.items.values() {
            let it = &r.item;
            match it.state {
                ItemState::Snoozed => {
                    if let Some(u) = it.snooze_until_ms {
                        consider(u);
                    }
                }
                ItemState::Open => {
                    if it.kind == ItemKind::Failed {
                        consider(it.updated_ms + ALERT_MS + 1);
                    }
                    if self.learn.quiet_kinds.contains(&it.kind) {
                        continue;
                    }
                    if it.blocking {
                        consider(it.updated_ms + INTERRUPT_WAIT_MS + 1);
                        consider(it.updated_ms + QUIET_NUDGE_WAIT_MS + 1);
                    }
                    for thr in thresholds {
                        if let Some(w) = wait_to_reach(it.kind, thr) {
                            consider(it.updated_ms + w + 1000);
                        }
                    }
                    if let Some(p) = self
                        .sessions
                        .get(&it.session)
                        .and_then(|s| s.last_prompt_ms)
                    {
                        consider(p + PRESENT_MS + 1);
                    }
                }
                _ => {}
            }
        }
        let stale = self.policy.stale_ms();
        for s in self.sessions.values() {
            if s.working && !s.ended {
                consider(s.last_ms + stale + 1);
                consider(s.last_work_ms + WORKING_MS + 1);
            }
            let keep = if s.ended { ENDED_KEEP_MS } else { IDLE_KEEP_MS };
            consider(s.last_ms + keep + 1);
        }
        if let Some(b) = self.policy.next_quiet_boundary(c) {
            consider(b);
        }
        best
    }

    /// 시각 `now`에서 상태로 판단하는 일들. 여러 번 불러도 결과가 같다.
    fn run_checks(&mut self, now: i64, fx: &mut Fx) {
        if self.focus_until.is_some_and(|u| u <= now) {
            self.focus_until = None;
        }
        if self.listening_until.is_some_and(|u| u <= now) {
            self.listening_until = None;
        }
        if self.done_until.is_some_and(|u| u <= now) {
            self.done_until = None;
            self.done_session = None;
        }
        self.check_pending_merges(now, fx);
        self.check_snoozes(now, fx);
        self.check_stalls(now, fx);
        self.prune_sessions(now, fx);
        self.check_escalations(now, fx);
        self.refresh_presence(now);
    }

    /// 병합처럼 보이는 요청의 10초 보류가 승인 알림 없이 끝나면 `merge`를 연다(§11, §14.3: 말풍선 없이).
    fn check_pending_merges(&mut self, now: i64, fx: &mut Fx) {
        let mut due: Vec<(String, i64)> = Vec::new();
        for (sid, list) in &mut self.reqs {
            for r in list.iter_mut() {
                if !r.held && now - r.ts > PERM_WINDOW_MS {
                    r.held = true;
                    due.push((sid.clone(), r.ts));
                }
            }
        }
        for (sid, ts) in due {
            let (title, body) = self.wait_view(&sid, ItemKind::Merge);
            let sig = Signal::new(ItemKind::Merge, ts, title, body);
            self.signal(&sid, sig, now, fx);
        }
    }

    fn check_snoozes(&mut self, now: i64, fx: &mut Fx) {
        let due: Vec<String> = self
            .items
            .values()
            .filter(|r| {
                r.item.state == ItemState::Snoozed
                    && r.item.snooze_until_ms.is_some_and(|u| u <= now)
            })
            .map(|r| r.item.id.clone())
            .collect();
        for id in due {
            if let Some(r) = self.items.get_mut(&id) {
                r.item.state = ItemState::Open;
                r.item.snooze_until_ms = None;
                r.cand = None;
                r.delivered = None;
            }
            self.emit_item(&id, now, fx);
            self.deliver(&id, now, fx);
        }
    }

    fn check_stalls(&mut self, now: i64, fx: &mut Fx) {
        let stale = self.policy.stale_ms();
        let due: Vec<(String, i64)> = self
            .sessions
            .values()
            .filter(|s| s.working && !s.ended && now - s.last_ms > stale)
            .map(|s| (s.id.clone(), s.last_ms))
            .collect();
        for (sid, last) in due {
            let mut sig = Signal::new(
                ItemKind::Stall,
                now,
                "작업이 멈춘 것 같아요".to_owned(),
                format!(
                    "마지막 신호 뒤로 {}분 넘게 조용해요",
                    self.policy.stale_minutes.max(1)
                ),
            );
            sig.sig_ts = last;
            self.signal(&sid, sig, now, fx);
        }
    }

    fn prune_sessions(&mut self, now: i64, fx: &mut Fx) {
        let gone: Vec<String> = self
            .sessions
            .values()
            .filter(|s| {
                let keep = if s.ended { ENDED_KEEP_MS } else { IDLE_KEEP_MS };
                now - s.last_ms > keep
                    && !self
                        .items
                        .values()
                        .any(|r| r.item.session == s.id && r.is_live())
            })
            .map(|s| s.id.clone())
            .collect();
        for sid in gone {
            self.sessions.remove(&sid);
            self.items.retain(|_, r| r.item.session != sid);
            self.reqs.remove(&sid);
            for ch in self.channels.values_mut() {
                if ch.session.as_deref() == Some(sid.as_str()) {
                    ch.session = None;
                    ch.by_pid = false;
                }
            }
            if self.done_session.as_deref() == Some(sid.as_str()) {
                self.done_session = None;
            }
        }
        let _ = fx;
    }

    fn check_escalations(&mut self, now: i64, fx: &mut Fx) {
        let mut up: Vec<(f64, String)> = self
            .items
            .values()
            .filter(|r| r.is_open())
            .filter_map(|r| {
                let (lvl, _) = self.candidate(r, now);
                (Some(lvl) > r.cand).then(|| {
                    (
                        score(r.item.kind, r.item.updated_ms, now).0,
                        r.item.id.clone(),
                    )
                })
            })
            .collect();
        up.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        for (_, id) in up {
            self.deliver(&id, now, fx);
        }
    }

    // ───────────────────────── 이벤트 ─────────────────────────

    fn process(&mut self, ev: &Event, now: i64, fx: &mut Fx) {
        match &ev.body {
            EventBody::UserAction {
                item,
                action,
                reason,
                minutes,
            } => {
                self.user_action(item, action, reason.as_deref(), *minutes, now, fx);
                return;
            }
            EventBody::ChannelOpen {
                cid,
                pid,
                ppid,
                cwd,
            } => {
                self.channels.insert(
                    cid.clone(),
                    ChannelRec {
                        cid: cid.clone(),
                        pid: *pid,
                        ppid: *ppid,
                        cwd: cwd.clone(),
                        cwd_key: cwd_key(cwd),
                        session: None,
                        by_pid: false,
                        opened_ms: now,
                    },
                );
                self.link_new_channel(cid);
                return;
            }
            EventBody::ChannelClose { cid, .. } => {
                if let Some(ch) = self.channels.remove(cid)
                    && let Some(sid) = ch.session
                    && let Some(s) = self.sessions.get_mut(&sid)
                    && s.channel.as_deref() == Some(cid.as_str())
                {
                    s.channel = None;
                }
                return;
            }
            _ => {}
        }
        let Some(sid) = self.resolve_session(ev) else {
            return;
        };
        self.ensure_session(&sid, ev);
        if ev.source.is_agent() {
            self.touch_session(&sid, ev);
            // 에이전트가 무엇이든 신호를 냈으면 멈춘 것이 아니다.
            self.close_kinds(&sid, &[ItemKind::Stall], now, fx);
        }
        let ts = ev.ts_ms;
        match &ev.body {
            EventBody::SessionEnd { .. } => {
                let ids: Vec<String> = self
                    .items
                    .values()
                    .filter(|r| r.item.session == sid && r.is_live())
                    .map(|r| r.item.id.clone())
                    .collect();
                for id in ids {
                    self.close(&id, now, fx);
                }
                self.reqs.remove(&sid);
                self.detach_session_channel(&sid);
            }
            EventBody::Prompt { .. } => {
                self.close_kinds(
                    &sid,
                    &[ItemKind::Decision, ItemKind::Unread, ItemKind::Followup],
                    now,
                    fx,
                );
                // 사람이 입력창에 새로 썼다면 기다리는 권한 창이 없다(앞쪽 창은 입력을 막고, 백그라운드 하위
                // 에이전트는 창 없이 자동 거절한다). 그 시각까지의 모든 요청을 지운다(§15.1, G-040).
                self.drop_reqs(&sid, |r| r.ts <= ts);
                self.sync_waits(&sid, now, fx);
            }
            EventBody::WaitingPermission { message, .. } => {
                self.attach_prompt(&sid, ev.agent_id.as_deref(), ts, message);
                let (title, body) = self.wait_view(&sid, ItemKind::Approval);
                let sig = Signal::new(ItemKind::Approval, ts, title, body);
                self.signal(&sid, sig, now, fx);
            }
            EventBody::PermissionRequest { tool, preview: pv } => {
                self.add_request(&sid, ev.agent_id.as_deref(), tool, pv, ts);
                self.sync_waits(&sid, now, fx);
            }
            EventBody::WaitingInput {
                notification_type,
                message,
            } => {
                // idle_prompt는 이미 있는 unread를 그대로 둔다(서명·대기 시작 유지). 새로 열지 않는다.
                if notification_type != "idle_prompt" {
                    let title = match notification_type.as_str() {
                        "elicitation_dialog" | "elicitation_url_dialog" => "질문에 답을 기다려요",
                        _ => "입력을 기다려요",
                    };
                    let sig =
                        Signal::new(ItemKind::Decision, ts, title.to_owned(), message.clone());
                    self.signal(&sid, sig, now, fx);
                }
            }
            EventBody::TurnDone {
                last_message,
                asks_question,
                human_followup,
            } => {
                self.close_turn_waits(&sid, ts, now, fx);
                self.done_until = Some(now + DONE_MS);
                self.done_session = Some(sid.clone());
                let (kind, title) = if *asks_question {
                    (ItemKind::Decision, "질문이 있어요")
                } else if *human_followup {
                    (ItemKind::Followup, "사람이 할 일이 남았어요")
                } else {
                    (ItemKind::Unread, "결과가 나왔어요")
                };
                let sig = Signal::new(kind, ts, title.to_owned(), last_message.clone());
                self.signal(&sid, sig, now, fx);
            }
            EventBody::TurnFailed { error_type } => {
                self.close_turn_waits(&sid, ts, now, fx);
                let sig = Signal::new(
                    ItemKind::Failed,
                    ts,
                    format!("턴이 오류로 끝났어요 · {error_type}"),
                    format!("오류 종류: {error_type}"),
                );
                self.signal(&sid, sig, now, fx);
            }
            EventBody::Activity {
                tool,
                tools,
                partial,
                preview: pv,
            } => {
                if let Some(s) = self.sessions.get_mut(&sid) {
                    s.seen_activity = true;
                }
                // 잘린 입력에서 건진 것은 작업 신호로만 센다(누구의 어떤 도구인지 믿을 수 없다, §14.2).
                if !partial {
                    let batch = ev.raw_event.as_deref() == Some("PostToolBatch");
                    let agent = ev.agent_id.as_deref();
                    self.on_activity(&sid, agent, tool.as_deref(), tools, pv, batch, ts);
                    self.sync_waits(&sid, now, fx);
                }
            }
            // 같은 에이전트·같은 도구·같은 미리보기의 실패만 그 요청을 지운다(§14.2, R5 M1a).
            EventBody::ToolFailed {
                tool, preview: pv, ..
            } => {
                let agent = ev.agent_id.as_deref();
                if !pv.is_empty() {
                    self.take_req(&sid, |r| {
                        r.agent_id.as_deref() == agent
                            && r.ts <= ts
                            && r.tool.as_deref() == Some(tool.as_str())
                            && r.preview == *pv
                    });
                    self.sync_waits(&sid, now, fx);
                }
            }
            // 하위 에이전트가 끝나면 그 에이전트의 요청은 더 기다리지 않는다(§14.2, R3 회귀 N3).
            EventBody::SubagentStop { .. } => {
                if let Some(a) = ev.agent_id.as_deref() {
                    self.drop_reqs(&sid, |r| r.agent_id.as_deref() == Some(a) && r.ts <= ts);
                    self.sync_waits(&sid, now, fx);
                }
            }
            EventBody::TaskDone { .. } => {
                self.done_until = Some(now + DONE_MS);
                self.done_session = Some(sid.clone());
            }
            EventBody::ContextCompact { trigger } => {
                if trigger == "auto" {
                    let sig = Signal::new(
                        ItemKind::Context,
                        ts,
                        "맥락이 자동으로 압축됐어요".to_owned(),
                        "앞부분 대화가 요약됐을 수 있어요".to_owned(),
                    );
                    self.signal(&sid, sig, now, fx);
                }
            }
            EventBody::Gate {
                result,
                harness,
                event,
            } => {
                self.on_gate(&sid, result, harness, event, ts, now, fx);
            }
            EventBody::DecisionOpen {
                decision_id,
                question,
                options,
            } => {
                let mut sig = Signal::new(
                    ItemKind::Decision,
                    ts,
                    preview(question, 120),
                    options.join(" · "),
                );
                sig.options = Some(options.clone());
                sig.decision_id = Some(decision_id.clone());
                self.signal(&sid, sig, now, fx);
            }
            EventBody::SessionStart { .. }
            | EventBody::SubagentStart { .. }
            | EventBody::Notice { .. }
            | EventBody::UserAction { .. }
            | EventBody::ChannelOpen { .. }
            | EventBody::ChannelClose { .. } => {}
        }
    }

    /// 턴이 끝났으면(성공·실패) 그 턴 안의 주 에이전트 대기(승인·병합 요청·결정)는 더 이상 기다리지 않는다.
    /// 하위 에이전트의 요청은 그 에이전트의 신호로만 지운다(§14.2, R5 L1). channel 결정은 턴과 따로 살아 있다.
    fn close_turn_waits(&mut self, sid: &str, ts: i64, now: i64, fx: &mut Fx) {
        let ids: Vec<String> = self
            .items
            .values()
            .filter(|r| {
                r.item.session == sid
                    && r.is_live()
                    && r.item.updated_ms <= ts
                    && r.item.kind == ItemKind::Decision
                    && r.item.decision_id.is_none()
            })
            .map(|r| r.item.id.clone())
            .collect();
        for id in ids {
            self.close(&id, now, fx);
        }
        self.drop_reqs(sid, |r| r.agent_id.is_none() && r.ts <= ts);
        self.sync_waits(sid, now, fx);
    }

    // ───────────────────────── 승인 요청 목록(§14.2) ─────────────────────────

    fn next_seq(&mut self) -> u64 {
        self.req_seq += 1;
        self.req_seq
    }

    /// `permission_request`: 요청 하나를 더한다. 알림이 먼저 와서 도구를 모르는 요청(10초 안, 에이전트를 모르거나
    /// 같음)이 있으면 그것을 채운다(같은 대기다).
    fn add_request(&mut self, sid: &str, agent: Option<&str>, tool: &str, pv: &str, ts: i64) {
        let merge = crate::text::is_merge_command(pv);
        let seq = self.next_seq();
        let list = self.reqs.entry(sid.to_owned()).or_default();
        let unknown = latest(list, |r| {
            r.tool.is_none()
                && r.ts <= ts
                && ts - r.ts <= PERM_WINDOW_MS
                && (r.agent_id.is_none() || r.agent_id.as_deref() == agent)
        });
        if let Some(i) = unknown {
            let r = &mut list[i];
            r.tool = Some(tool.to_owned());
            r.preview = pv.to_owned();
            r.agent_id = agent.map(str::to_owned);
            r.merge = merge;
            return;
        }
        list.push(Req {
            seq,
            agent_id: agent.map(str::to_owned),
            tool: Some(tool.to_owned()),
            preview: pv.to_owned(),
            note: String::new(),
            ts,
            merge,
            notified: false,
            held: !merge,
        });
        if list.len() > REQS_MAX {
            take_oldest(list, |_| true);
        }
    }

    /// `waiting_permission`: 10초 안의 가장 최근 요청에, 없으면 가장 최근에 열린 요청에 붙는다. `agent_id`가 있으면
    /// 그 에이전트의 요청에서만 고른다. 붙을 요청이 없으면 도구를 모르는 요청을 하나 연다.
    fn attach_prompt(&mut self, sid: &str, agent: Option<&str>, ts: i64, message: &str) {
        let seq = self.next_seq();
        let list = self.reqs.entry(sid.to_owned()).or_default();
        let mine = |r: &Req| agent.is_none() || r.agent_id.as_deref() == agent;
        let target = latest(list, |r| {
            mine(r) && r.ts <= ts && ts - r.ts <= PERM_WINDOW_MS
        })
        .or_else(|| latest(list, mine));
        match target {
            Some(i) => {
                let r = &mut list[i];
                r.notified = true;
                r.held = true;
                if !message.is_empty() {
                    r.note = message.to_owned();
                }
            }
            None => {
                list.push(Req {
                    seq,
                    agent_id: agent.map(str::to_owned),
                    tool: None,
                    preview: String::new(),
                    note: message.to_owned(),
                    ts,
                    merge: false,
                    notified: true,
                    held: true,
                });
                if list.len() > REQS_MAX {
                    take_oldest(list, |_| true);
                }
            }
        }
    }

    /// activity: 같은 에이전트이고 도구가 맞거나 모르는 가장 오래된 요청 하나를 지운다(이름마다, 도구가 맞는
    /// 것을 먼저). 도구 목록이 없는 `PostToolBatch`는 그 에이전트의 요청을 모두 지운다.
    #[allow(clippy::too_many_arguments)]
    fn on_activity(
        &mut self,
        sid: &str,
        agent: Option<&str>,
        tool: Option<&str>,
        tools: &[String],
        pv: &str,
        batch: bool,
        ts: i64,
    ) {
        let Some(list) = self.reqs.get_mut(sid) else {
            return;
        };
        // 둘 다 미리보기가 있으면 같아야 같은 호출이다(같은 이름의 병렬 호출을 가린다, §15.3).
        let mine = |r: &Req| {
            r.agent_id.as_deref() == agent
                && r.ts <= ts
                && (pv.is_empty() || r.preview.is_empty() || r.preview == pv)
        };
        let names: Vec<&str> = if tools.is_empty() {
            tool.into_iter().collect()
        } else {
            tools.iter().map(String::as_str).collect()
        };
        if names.is_empty() {
            if batch {
                list.retain(|r| !mine(r));
            } else {
                take_oldest(list, mine);
            }
            return;
        }
        for n in names {
            let _ = take_oldest(list, |r| mine(r) && r.tool.as_deref() == Some(n))
                || take_oldest(list, |r| mine(r) && r.tool.is_none());
        }
    }

    fn take_req(&mut self, sid: &str, pred: impl Fn(&Req) -> bool) {
        if let Some(list) = self.reqs.get_mut(sid) {
            take_oldest(list, pred);
        }
    }

    fn drop_reqs(&mut self, sid: &str, pred: impl Fn(&Req) -> bool) {
        if let Some(list) = self.reqs.get_mut(sid) {
            list.retain(|r| !pred(r));
        }
    }

    /// 승인·병합 항목의 제목·본문: 가장 최근 요청, 본문 끝에 "외 n건"(§14.2).
    fn wait_view(&self, sid: &str, kind: ItemKind) -> (String, String) {
        let prefix = if kind == ItemKind::Merge {
            "병합 승인 대기"
        } else {
            "권한 승인 대기"
        };
        let list = self.reqs.get(sid).map(Vec::as_slice).unwrap_or_default();
        let Some(i) = latest(list, |_| true) else {
            return (prefix.to_owned(), String::new());
        };
        let r = &list[i];
        let title = match &r.tool {
            Some(t) => format!("{prefix} · {t}"),
            None => prefix.to_owned(),
        };
        let base = if r.preview.is_empty() {
            r.note.as_str()
        } else {
            r.preview.as_str()
        };
        let body = match (list.len() - 1, base.is_empty()) {
            (0, _) => base.to_owned(),
            (n, true) => format!("외 {n}건"),
            (n, false) => format!("{base} 외 {n}건"),
        };
        (title, body)
    }

    /// 요청 목록이 바뀐 뒤: 비었으면 승인·병합 항목을 닫고, 아니면 보이는 제목·본문을 맞춘다(새 전달은 없다).
    fn sync_waits(&mut self, sid: &str, now: i64, fx: &mut Fx) {
        if self.reqs.get(sid).is_none_or(Vec::is_empty) {
            self.reqs.remove(sid);
            self.close_kinds(sid, &[ItemKind::Approval, ItemKind::Merge], now, fx);
            return;
        }
        for kind in [ItemKind::Approval, ItemKind::Merge] {
            let id = item_id(sid, kind);
            if !self.items.get(&id).is_some_and(ItemRec::is_live) {
                continue;
            }
            let (title, body) = self.wait_view(sid, kind);
            if let Some(r) = self.items.get_mut(&id)
                && (r.item.title != title || r.item.body != body)
            {
                r.item.title = title;
                r.item.body = body;
                self.emit_item(&id, now, fx);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn on_gate(
        &mut self,
        sid: &str,
        result: &str,
        harness: &str,
        event: &str,
        ts: i64,
        now: i64,
        fx: &mut Fx,
    ) {
        let word = match result {
            "fail" => Some("실패"),
            "cannot" => Some("판단 불가"),
            "block" => Some("막힘"),
            "push-error" => Some("푸시 오류"),
            _ => None,
        };
        if let Some(word) = word {
            let mut sig = Signal::new(
                ItemKind::Failed,
                ts,
                format!("{harness} 게이트 {word}"),
                format!("{event} → {result}"),
            );
            sig.from_gate = true;
            self.signal(sid, sig, now, fx);
        } else if result == "push-uncommitted" {
            let sig = Signal::new(
                ItemKind::Volatile,
                ts,
                format!("커밋하지 않은 변경이 푸시됐어요 · {harness}"),
                format!("{event} → {result}"),
            );
            self.signal(sid, sig, now, fx);
        } else if result == "pass" {
            let fid = item_id(sid, ItemKind::Failed);
            if self
                .items
                .get(&fid)
                .is_some_and(|r| r.is_live() && r.from_gate && r.item.updated_ms <= ts)
            {
                self.close(&fid, now, fx);
            }
        }
    }

    fn user_action(
        &mut self,
        target: &str,
        action: &str,
        reason: Option<&str>,
        minutes: Option<i64>,
        now: i64,
        fx: &mut Fx,
    ) {
        let Some(a) = Action::parse(action) else {
            return;
        };
        match a {
            Action::Open => {
                if let Some(r) = self.items.get(target) {
                    let key = format!("a|{}|{}|open", r.item.id, r.item.signature);
                    let kind = r.item.kind;
                    self.learn_record(key, now, kind, "open", None);
                }
            }
            Action::Done | Action::Dismiss => {
                let Some(r) = self.items.get_mut(target) else {
                    return;
                };
                if !r.is_live() {
                    return;
                }
                r.item.state = if a == Action::Done {
                    ItemState::Done
                } else {
                    ItemState::Dismissed
                };
                r.handled_sig = Some(r.item.signature.clone());
                r.item.snooze_until_ms = None;
                let kind = r.item.kind;
                let key = format!("a|{}|{}|{}", r.item.id, r.item.signature, a.as_str());
                fx.touch(target, None);
                if a == Action::Done {
                    self.learn_record(key, now, kind, "done", None);
                } else {
                    self.learn_record(key, now, kind, "dismiss", reason);
                    if let Some(p) = self.learn.maybe_propose(kind, now) {
                        self.learn_rev += 1;
                        fx.proposals.push(p);
                    }
                }
            }
            Action::Snooze => {
                let Some(r) = self.items.get_mut(target) else {
                    return;
                };
                if !r.is_live() {
                    return;
                }
                let m = minutes
                    .unwrap_or(SNOOZE_DEFAULT_MIN)
                    .clamp(1, SNOOZE_MAX_MIN);
                r.item.state = ItemState::Snoozed;
                r.item.snooze_until_ms = Some(now + m * MIN_MS);
                let kind = r.item.kind;
                let key = format!("a|{}|{}|snooze", r.item.id, r.item.signature);
                fx.touch(target, None);
                self.learn_record(key, now, kind, "snooze", None);
            }
            Action::FocusOn => {
                let m = minutes.unwrap_or(FOCUS_DEFAULT_MIN).clamp(1, FOCUS_MAX_MIN);
                self.focus_until = Some(now + m * MIN_MS);
            }
            Action::FocusOff => self.focus_until = None,
            Action::ListeningOn => self.listening_until = Some(now + LISTENING_TTL_MS),
            Action::ListeningOff => self.listening_until = None,
            Action::ProposalAccept | Action::ProposalReject => {
                if self.learn.respond(target, a == Action::ProposalAccept, now) {
                    self.learn_rev += 1;
                    fx.resync = true;
                }
            }
            Action::Decide => {
                let id = self
                    .items
                    .values()
                    .find(|r| r.item.decision_id.as_deref() == Some(target))
                    .map(|r| r.item.id.clone());
                if let Some(id) = id {
                    self.user_action(&id, Action::Done.as_str(), None, None, now, fx);
                }
            }
            Action::Unquiet => {
                if let Some(k) = ItemKind::parse(target)
                    && self.learn.quiet_kinds.remove(&k)
                {
                    self.learn_rev += 1;
                    fx.resync = true;
                }
            }
        }
    }

    fn validate(
        &self,
        target: &str,
        action: Action,
        reason: Option<&str>,
        minutes: Option<i64>,
    ) -> Result<Validated, ActError> {
        match action {
            Action::Open | Action::Done | Action::Dismiss | Action::Snooze => {
                let r = self.items.get(target).ok_or(ActError::UnknownItem)?;
                let session = Some(r.item.session.clone());
                match action {
                    Action::Dismiss => {
                        let reason = match reason.map(str::trim).filter(|s| !s.is_empty()) {
                            None => None,
                            Some(x) if DISMISS_REASONS.contains(&x) => Some(x.to_owned()),
                            Some(_) => return Err(ActError::InvalidReason),
                        };
                        Ok((reason, None, session))
                    }
                    Action::Snooze => {
                        let m = minutes.unwrap_or(SNOOZE_DEFAULT_MIN);
                        if !(1..=SNOOZE_MAX_MIN).contains(&m) {
                            return Err(ActError::InvalidMinutes);
                        }
                        Ok((None, Some(m), session))
                    }
                    _ => Ok((None, None, session)),
                }
            }
            Action::FocusOn => {
                let m = minutes.unwrap_or(FOCUS_DEFAULT_MIN);
                if !(1..=FOCUS_MAX_MIN).contains(&m) {
                    return Err(ActError::InvalidMinutes);
                }
                Ok((None, Some(m), None))
            }
            Action::FocusOff | Action::ListeningOn | Action::ListeningOff => Ok((None, None, None)),
            Action::ProposalAccept | Action::ProposalReject => {
                if self.learn.proposals.contains_key(target) {
                    Ok((None, None, None))
                } else {
                    Err(ActError::UnknownProposal)
                }
            }
            Action::Decide => self
                .items
                .values()
                .find(|r| r.item.decision_id.as_deref() == Some(target))
                .map(|r| (None, None, Some(r.item.session.clone())))
                .ok_or(ActError::UnknownDecision),
            Action::Unquiet => ItemKind::parse(target)
                .map(|_| (None, None, None))
                .ok_or(ActError::InvalidKind),
        }
    }

    fn learn_record(
        &mut self,
        key: String,
        now: i64,
        kind: ItemKind,
        what: &str,
        reason: Option<&str>,
    ) {
        if self.learn.record(key, now, kind, what, reason) {
            self.learn_rev += 1;
        }
    }

    // ───────────────────────── 세션·channel ─────────────────────────

    /// 세션 키: 이벤트의 `session`, 없으면 같은 작업 폴더의 가장 최근 세션, 그것도 없으면 `cwd:<폴더>`.
    fn resolve_session(&self, ev: &Event) -> Option<String> {
        if let Some(s) = ev.session.as_deref().filter(|s| !s.is_empty()) {
            return Some(s.to_owned());
        }
        let key = cwd_key(ev.cwd.as_deref()?);
        let found = self
            .sessions
            .values()
            .filter(|s| {
                !s.ended && s.cwd_key.as_deref() == Some(key.as_str()) && !s.id.starts_with("cwd:")
            })
            .max_by(|a, b| a.last_ms.cmp(&b.last_ms).then_with(|| a.id.cmp(&b.id)))
            .map(|s| s.id.clone());
        Some(found.unwrap_or_else(|| format!("cwd:{key}")))
    }

    fn ensure_session(&mut self, sid: &str, ev: &Event) {
        if self.sessions.contains_key(sid) {
            return;
        }
        let cwd = ev
            .cwd
            .clone()
            .or_else(|| sid.strip_prefix("cwd:").map(str::to_owned));
        self.sessions.insert(
            sid.to_owned(),
            SessionRec {
                id: sid.to_owned(),
                agent: ev.agent,
                cwd_key: cwd.as_deref().map(cwd_key),
                project: cwd.as_deref().map(project_name),
                cwd,
                pid_hint: None,
                last_ms: ev.ts_ms,
                working: false,
                last_work_ms: 0,
                last_prompt_ms: None,
                ended: false,
                channel: None,
                seen_activity: false,
            },
        );
    }

    fn touch_session(&mut self, sid: &str, ev: &Event) {
        let Some(s) = self.sessions.get_mut(sid) else {
            return;
        };
        if ev.ts_ms > s.last_ms {
            s.last_ms = ev.ts_ms;
        }
        if let Some(c) = ev.cwd.as_deref().filter(|c| !c.is_empty()) {
            s.cwd = Some(c.to_owned());
            s.cwd_key = Some(cwd_key(c));
            s.project = Some(project_name(c));
        }
        if ev.agent.is_some() {
            s.agent = ev.agent;
        }
        if ev.pid_hint.is_some() {
            s.pid_hint = ev.pid_hint;
        }
        match &ev.body {
            EventBody::SessionEnd { .. } => {
                s.ended = true;
                s.working = false;
            }
            b => {
                s.ended = false;
                s.working = b.is_work_signal();
                if s.working {
                    s.last_work_ms = s.last_work_ms.max(ev.ts_ms);
                }
                if matches!(b, EventBody::Prompt { .. }) {
                    s.last_prompt_ms = Some(s.last_prompt_ms.map_or(ev.ts_ms, |p| p.max(ev.ts_ms)));
                }
            }
        }
        if !s.ended {
            self.link_channels_for_session(sid);
        }
    }

    /// 새 channel: `pid_hint == ppid`인 세션 → 같은 작업 폴더의 가장 최근 세션(channel 없는 것).
    fn link_new_channel(&mut self, cid: &str) {
        let Some(ch) = self.channels.get(cid) else {
            return;
        };
        let (ppid, key) = (ch.ppid, ch.cwd_key.clone());
        let by_pid = self
            .sessions
            .values()
            .filter(|s| !s.ended && s.pid_hint == Some(ppid) && !s.id.starts_with("cwd:"))
            .max_by(|a, b| a.last_ms.cmp(&b.last_ms).then_with(|| a.id.cmp(&b.id)))
            .map(|s| s.id.clone());
        if let Some(sid) = by_pid {
            self.attach(cid, &sid, true);
            return;
        }
        let by_cwd = self
            .sessions
            .values()
            .filter(|s| {
                !s.ended
                    && s.channel.is_none()
                    && s.cwd_key.as_deref() == Some(key.as_str())
                    && !s.id.starts_with("cwd:")
            })
            .max_by(|a, b| a.last_ms.cmp(&b.last_ms).then_with(|| a.id.cmp(&b.id)))
            .map(|s| s.id.clone());
        if let Some(sid) = by_cwd {
            self.attach(cid, &sid, false);
        }
    }

    /// 세션에 새 이벤트가 오면: 같은 PID의 channel을 이 세션으로 옮기고, 못 이은 channel은 작업 폴더로 잇는다.
    fn link_channels_for_session(&mut self, sid: &str) {
        let Some(s) = self.sessions.get(sid) else {
            return;
        };
        if sid.starts_with("cwd:") {
            return;
        }
        let (pid, key, has_channel) = (s.pid_hint, s.cwd_key.clone(), s.channel.is_some());
        if let Some(pid) = pid {
            let movers: Vec<String> = self
                .channels
                .values()
                .filter(|c| c.ppid == pid && (c.session.as_deref() != Some(sid) || !c.by_pid))
                .map(|c| c.cid.clone())
                .collect();
            if let Some(cid) = movers.last() {
                self.attach(cid, sid, true);
                return;
            }
        }
        if has_channel {
            return;
        }
        let Some(key) = key else {
            return;
        };
        let cand = self
            .channels
            .values()
            .filter(|c| c.session.is_none() && c.cwd_key == key)
            .max_by(|a, b| {
                a.opened_ms
                    .cmp(&b.opened_ms)
                    .then_with(|| a.cid.cmp(&b.cid))
            })
            .map(|c| c.cid.clone());
        if let Some(cid) = cand {
            self.attach(&cid, sid, false);
        }
    }

    fn attach(&mut self, cid: &str, sid: &str, by_pid: bool) {
        let old = self.channels.get_mut(cid).and_then(|c| c.session.take());
        if let Some(old) = old
            && let Some(s) = self.sessions.get_mut(&old)
            && s.channel.as_deref() == Some(cid)
        {
            s.channel = None;
        }
        let prev = self
            .sessions
            .get_mut(sid)
            .and_then(|s| s.channel.replace(cid.to_owned()));
        if let Some(prev) = prev
            && prev != cid
            && let Some(c) = self.channels.get_mut(&prev)
        {
            c.session = None;
            c.by_pid = false;
        }
        if let Some(c) = self.channels.get_mut(cid) {
            c.session = Some(sid.to_owned());
            c.by_pid = by_pid;
        }
    }

    fn detach_session_channel(&mut self, sid: &str) {
        let cid = self.sessions.get_mut(sid).and_then(|s| s.channel.take());
        if let Some(cid) = cid
            && let Some(c) = self.channels.get_mut(&cid)
        {
            c.session = None;
            c.by_pid = false;
        }
    }

    // ───────────────────────── 항목 ─────────────────────────

    fn is_live_kind(&self, sid: &str, kind: ItemKind) -> bool {
        self.items
            .get(&item_id(sid, kind))
            .is_some_and(ItemRec::is_live)
    }

    fn signal(&mut self, sid: &str, sig: Signal, now: i64, fx: &mut Fx) {
        let kind = sig.kind;
        let id = item_id(sid, kind);
        let signature = format!("{}:{}", kind.as_str(), sig.sig_ts);
        if kind.decision_rank().is_some() {
            // 판정 순서는 승인끼리(approval → merge)만 덮는다. 질문·결정은 승인이 열려 있어도 따로 연다(§15.2).
            let higher_open = [ItemKind::Approval, ItemKind::Merge]
                .into_iter()
                .any(|k| overrides(k, kind) && self.is_live_kind(sid, k));
            if higher_open {
                return;
            }
            // channel 결정이 열려 있으면 턴 끝 질문 등으로 덮어쓰지 않는다.
            if kind == ItemKind::Decision
                && sig.decision_id.is_none()
                && self
                    .items
                    .get(&id)
                    .is_some_and(|r| r.is_live() && r.item.decision_id.is_some())
            {
                return;
            }
        }
        enum Outcome {
            New,
            Same,
            Renew,
            Reopen,
        }
        let outcome = match self.items.get(&id) {
            None => Outcome::New,
            Some(r) => match r.item.state {
                ItemState::Open if r.item.signature == signature => Outcome::Same,
                ItemState::Open => Outcome::Renew,
                ItemState::Done | ItemState::Dismissed => {
                    if r.handled_sig.as_deref() == Some(signature.as_str()) {
                        return;
                    }
                    Outcome::Reopen
                }
                ItemState::Closed | ItemState::Snoozed => {
                    if r.item.signature == signature {
                        return;
                    }
                    Outcome::Reopen
                }
            },
        };
        if let Outcome::Same = outcome {
            let mut changed = false;
            if let Some(r) = self.items.get_mut(&id)
                && (r.item.title != sig.title || r.item.body != sig.body)
            {
                r.item.title = sig.title;
                r.item.body = sig.body;
                changed = true;
            }
            if changed {
                self.emit_item(&id, now, fx);
            }
            return;
        }
        if kind.decision_rank().is_some() {
            for k in [ItemKind::Approval, ItemKind::Merge] {
                if overrides(kind, k) {
                    let kid = item_id(sid, k);
                    if self.items.get(&kid).is_some_and(ItemRec::is_live) {
                        self.close(&kid, now, fx);
                    }
                }
            }
            self.close_kinds(sid, &[ItemKind::Unread, ItemKind::Followup], now, fx);
        }
        let created = match (&outcome, self.items.get(&id)) {
            (Outcome::Renew, Some(r)) => r.item.created_ms,
            _ => sig.ts,
        };
        let (agent, project, cwd) = self
            .sessions
            .get(sid)
            .map(|s| {
                (
                    s.agent,
                    s.project.clone().unwrap_or_default(),
                    s.cwd.clone().unwrap_or_default(),
                )
            })
            .unwrap_or_default();
        let item = Item {
            id: id.clone(),
            kind,
            session: sid.to_owned(),
            agent,
            project,
            cwd,
            title: sig.title,
            body: sig.body,
            blocking: kind.blocking(),
            score: 0.0,
            reasons: Vec::new(),
            created_ms: created,
            updated_ms: sig.ts,
            signature: signature.clone(),
            state: ItemState::Open,
            snooze_until_ms: None,
            options: sig.options,
            decision_id: sig.decision_id,
        };
        self.items.insert(
            id.clone(),
            ItemRec {
                item,
                cand: None,
                delivered: None,
                handled_sig: None,
                from_gate: sig.from_gate,
            },
        );
        self.emit_item(&id, now, fx);
        self.learn_record(format!("o|{id}|{signature}"), now, kind, "opened", None);
        self.deliver(&id, now, fx);
    }

    fn close(&mut self, id: &str, now: i64, fx: &mut Fx) {
        if let Some(r) = self.items.get_mut(id)
            && r.is_live()
        {
            r.item.state = ItemState::Closed;
            r.item.snooze_until_ms = None;
            fx.touch(id, None);
        }
        let _ = now;
    }

    fn close_kinds(&mut self, sid: &str, kinds: &[ItemKind], now: i64, fx: &mut Fx) {
        for k in kinds {
            let id = item_id(sid, *k);
            self.close(&id, now, fx);
        }
    }

    fn emit_item(&self, id: &str, now: i64, fx: &mut Fx) {
        if let Some(r) = self.items.get(id) {
            if r.is_open() {
                fx.touch(id, Some(self.view(r, now)));
            } else {
                fx.touch(id, None);
            }
        }
    }

    fn view(&self, r: &ItemRec, now: i64) -> Item {
        let mut it = r.item.clone();
        let (s, reasons) = score(it.kind, it.updated_ms, now);
        it.score = s;
        it.reasons = reasons;
        it
    }

    // ───────────────────────── 전달 판단 ─────────────────────────

    fn place(&self, it: &Item) -> String {
        match it.project.as_str() {
            p if !p.is_empty() => p.to_owned(),
            _ => {
                let short: String = it.session.chars().take(8).collect();
                format!("세션 {short}")
            }
        }
    }

    /// 규칙 1–5(예산 전)의 후보 수준과 근거.
    fn candidate(&self, r: &ItemRec, now: i64) -> (Level, Vec<String>) {
        let it = &r.item;
        let (s, _) = score(it.kind, it.updated_ms, now);
        let wait = (now - it.updated_ms).max(0);
        let mut why = vec![format!("{} · 점수 {:.1}", it.kind.label(), s)];
        if let Some(p) = self
            .sessions
            .get(&it.session)
            .and_then(|x| x.last_prompt_ms)
            && now - p <= PRESENT_MS
            && now >= p
        {
            why.push("같은 세션에 1분 안에 입력이 있었어요. 이미 보고 계신 것 같아요".to_owned());
            return (Level::Ambient, why);
        }
        if self.learn.quiet_kinds.contains(&it.kind) {
            why.push(format!("‘{}’은 조용히 알리기로 하셨어요", it.kind.label()));
            return (Level::Ambient, why);
        }
        // `merge`는 병합 보류가 승인 알림 없이 끝났을 때만 열린다(알림이 오면 `approval`이다). 사람이 이미 답한
        // 것이므로 말풍선을 띄우지 않는다(§14.3, G-038).
        if it.kind == ItemKind::Merge {
            why.push(
                "승인 알림 없이 끝난 병합 요청이에요. 이미 답하신 것 같아 표정으로만 알려요"
                    .to_owned(),
            );
            return (Level::Ambient, why);
        }
        if s < self.policy.nudge_min {
            why.push(format!(
                "점수가 말풍선 기준 {}보다 낮아요",
                fmt_num(self.policy.nudge_min)
            ));
            return (Level::Ambient, why);
        }
        let focus = self.focus_until.is_some_and(|u| u > now);
        let quiet = self.policy.in_quiet_hours(now);
        if focus || quiet {
            if focus {
                why.push("집중 모드예요".to_owned());
            } else {
                why.push(format!(
                    "조용한 시간이에요({})",
                    self.policy.quiet_hours_strings().join(", ")
                ));
            }
            if s >= QUIET_NUDGE_SCORE && it.blocking && wait > QUIET_NUDGE_WAIT_MS {
                why.push("95점 넘는 막힌 일이 3분 넘게 기다려서 말풍선만 띄워요".to_owned());
                return (Level::Nudge, why);
            }
            why.push("급하게 막힌 일이 아니면 표정으로만 알려요".to_owned());
            return (Level::Ambient, why);
        }
        // 소리는 기본 점수로만 정한다. 대기 가산은 순서와 말풍선에만 쓴다(§14.4, G-039).
        if it.kind.base() >= self.policy.interrupt_min && it.blocking && wait > INTERRUPT_WAIT_MS {
            if matches!(it.kind, ItemKind::Approval | ItemKind::Merge) {
                // 승인 순간을 알리는 훅이 없다. 켜지 않으면 말풍선까지만(§13.3, G-035).
                if !self.policy.approval_interrupt {
                    why.push("승인했는지 곁이 알 수 없어 말풍선까지만 알려요".to_owned());
                    return (Level::Nudge, why);
                }
                // 작업 신호가 없는 세션은 사람이 이미 승인했는지 알 수 없다(§12.3). 말풍선까지만.
                if !self
                    .sessions
                    .get(&it.session)
                    .is_some_and(|x| x.seen_activity)
                {
                    why.push(
                        "이 세션은 작업 신호가 없어 승인 여부를 알 수 없어요 → 말풍선까지만"
                            .to_owned(),
                    );
                    return (Level::Nudge, why);
                }
            }
            why.push(format!(
                "기본 점수 {}점 이상인 막힌 일이 2분 넘게 기다려요",
                fmt_num(self.policy.interrupt_min)
            ));
            return (Level::Interrupt, why);
        }
        why.push("말풍선으로 알려요".to_owned());
        (Level::Nudge, why)
    }

    fn deliver(&mut self, id: &str, now: i64, fx: &mut Fx) {
        let Some(r) = self.items.get(id) else {
            return;
        };
        if !r.is_open() {
            return;
        }
        let (cand, mut why) = self.candidate(r, now);
        let line = self.delivery_line(r, now);
        let mut level = cand;
        if level == Level::Interrupt && !self.interrupt.try_take(now) {
            why.push("소리 예산(30분에 1번)을 다 써서 한 단계 내렸어요".to_owned());
            level = Level::Nudge;
        }
        if level == Level::Nudge && !self.nudge.try_take(now) {
            why.push("말풍선 예산(15분에 1개, 최대 4개)을 다 써서 표정으로만 알려요".to_owned());
            level = Level::Ambient;
        }
        why.push(format!("→ {}", level.label()));
        if let Some(r) = self.items.get_mut(id) {
            r.cand = Some(cand);
            r.delivered = Some(level);
        }
        fx.deliveries.push(Delivery {
            item: id.to_owned(),
            level,
            line,
            speak: level == Level::Interrupt,
            reasons: why,
            at_ms: now,
        });
    }

    fn delivery_line(&self, r: &ItemRec, now: i64) -> String {
        let it = &r.item;
        let p = self.place(it);
        let m = (now - it.updated_ms).max(0) / MIN_MS;
        let waited = if m >= 1 {
            format!("{m}분째 ")
        } else {
            String::new()
        };
        match it.kind {
            ItemKind::Approval => format!("{p}에서 권한 승인을 {waited}기다려요"),
            ItemKind::Merge => format!("{p}에서 병합 승인을 {waited}기다려요"),
            ItemKind::Decision => format!("{p}에서 결정을 {waited}기다려요"),
            ItemKind::Failed => format!("{p}에서 실패가 있었어요 · {}", it.title),
            ItemKind::Volatile => format!("{p}에서 커밋하지 않은 변경이 푸시됐어요"),
            ItemKind::Stall => format!(
                "{p} 작업이 {}분 넘게 조용해요",
                self.policy.stale_minutes.max(1)
            ),
            ItemKind::Followup => format!("{p}에 사람이 할 일이 남았어요"),
            ItemKind::Unread => format!("{p}에서 결과가 나왔어요"),
            ItemKind::Context => format!("{p}에서 맥락이 자동으로 압축됐어요"),
        }
    }

    // ───────────────────────── 존재감·세션 보기 ─────────────────────────

    fn refresh_presence(&mut self, now: i64) {
        let mut open: Vec<(f64, &ItemRec)> = self
            .items
            .values()
            .filter(|r| r.is_open())
            .map(|r| (score(r.item.kind, r.item.updated_ms, now).0, r))
            .collect();
        open.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then_with(|| b.1.item.updated_ms.cmp(&a.1.item.updated_ms))
                .then_with(|| a.1.item.id.cmp(&b.1.item.id))
        });
        let failed = open
            .iter()
            .find(|(_, r)| r.item.kind == ItemKind::Failed && now - r.item.updated_ms <= ALERT_MS);
        let blocking: Vec<&(f64, &ItemRec)> =
            open.iter().filter(|(_, r)| r.item.blocking).collect();
        let focus_mode = self.focus_until.is_some_and(|u| u > now);
        let quiet = self.policy.in_quiet_hours(now);
        let working: Vec<&SessionRec> = self
            .sessions
            .values()
            .filter(|s| !s.ended && s.working && now - s.last_work_ms <= WORKING_MS)
            .collect();
        let (mood, focus, line) = if let Some((_, r)) = failed {
            (
                Mood::Alert,
                Some(*r),
                format!("{}에서 실패가 있었어요", self.place(&r.item)),
            )
        } else if let Some((_, r)) = blocking.first() {
            let line = if blocking.len() == 1 {
                format!("{} 1건이 기다려요", r.item.kind.label())
            } else {
                format!(
                    "{}건이 기다려요 · 먼저 {}",
                    blocking.len(),
                    r.item.kind.label()
                )
            };
            (Mood::Waiting, Some(*r), line)
        } else {
            let top = open.first().map(|(_, r)| *r);
            if self.listening_until.is_some_and(|u| u > now) {
                (Mood::Listening, top, "듣고 있어요".to_owned())
            } else if !working.is_empty() {
                let line = if working.len() == 1 {
                    let p = working[0]
                        .project
                        .clone()
                        .unwrap_or_else(|| "세션".to_owned());
                    format!("{p}에서 일하는 중이에요")
                } else {
                    format!("{}개 세션이 일하는 중이에요", working.len())
                };
                (Mood::Working, top, line)
            } else if self.done_until.is_some_and(|u| u > now) {
                let p = self
                    .done_session
                    .as_deref()
                    .and_then(|s| self.sessions.get(s))
                    .and_then(|s| s.project.clone());
                let line = match p {
                    Some(p) => format!("{p} 작업이 끝났어요"),
                    None => "작업이 끝났어요".to_owned(),
                };
                (Mood::Done, top, line)
            } else if focus_mode {
                (Mood::Sleep, top, "집중 모드라 조용히 있을게요".to_owned())
            } else if quiet {
                (Mood::Sleep, top, "조용한 시간이라 쉬고 있을게요".to_owned())
            } else {
                (Mood::Idle, top, String::new())
            }
        };
        let since = if mood == self.presence.mood {
            self.presence.since_ms
        } else {
            now
        };
        self.presence = Presence {
            mood,
            level: focus.and_then(|r| r.delivered).unwrap_or(Level::Ambient),
            focus_item: focus.map(|r| r.item.id.clone()),
            line,
            since_ms: since,
            focus_mode_until_ms: self.focus_until.filter(|u| *u > now),
        };
    }

    fn session_views(&self) -> Vec<Session> {
        let mut v: Vec<Session> = self
            .sessions
            .values()
            .map(|s| {
                let state = if s.ended {
                    SessionState::Ended
                } else if self
                    .items
                    .values()
                    .any(|r| r.item.session == s.id && r.is_open() && r.item.blocking)
                {
                    SessionState::Waiting
                } else if s.working {
                    SessionState::Working
                } else {
                    SessionState::Idle
                };
                Session {
                    id: s.id.clone(),
                    agent: s.agent,
                    project: s.project.clone().unwrap_or_default(),
                    cwd: s.cwd.clone().unwrap_or_default(),
                    state,
                    last_ms: s.last_ms,
                    channel: s.channel.clone(),
                }
            })
            .collect();
        v.sort_by(|a, b| b.last_ms.cmp(&a.last_ms).then_with(|| a.id.cmp(&b.id)));
        v
    }

    fn finish(&mut self, fx: Fx) -> Vec<Effect> {
        let Fx {
            order,
            mut items,
            deliveries,
            proposals,
            resync,
        } = fx;
        let mut out = Vec::new();
        for id in order {
            match items.remove(&id).flatten() {
                Some(item) => out.push(Effect::ItemUpsert(item)),
                None => out.push(Effect::ItemRemoved { id }),
            }
        }
        out.extend(deliveries.into_iter().map(Effect::Deliver));
        out.extend(proposals.into_iter().map(Effect::Proposal));
        let views = self.session_views();
        let key = sessions_key(&views);
        if key != self.sessions_key {
            self.sessions_key = key;
            out.push(Effect::Sessions(views));
        }
        if self.presence != self.emitted_presence {
            self.emitted_presence = self.presence.clone();
            out.push(Effect::Presence(self.presence.clone()));
        }
        if resync {
            out.push(Effect::Resync);
        }
        out
    }
}

/// 세션 목록 변화 판단 열쇠(`last_ms`는 이벤트마다 바뀌므로 뺀다).
fn sessions_key(v: &[Session]) -> String {
    let mut s = String::new();
    for x in v {
        s.push_str(&x.id);
        s.push('\u{1f}');
        s.push_str(&x.project);
        s.push('\u{1f}');
        s.push_str(&x.cwd);
        s.push('\u{1f}');
        s.push_str(x.agent.map_or("", Agent::as_str));
        s.push('\u{1f}');
        s.push_str(match x.state {
            SessionState::Working => "w",
            SessionState::Waiting => "a",
            SessionState::Idle => "i",
            SessionState::Ended => "e",
        });
        s.push('\u{1f}');
        s.push_str(x.channel.as_deref().unwrap_or(""));
        s.push('\u{1e}');
    }
    s
}

fn channel_info(c: &ChannelRec) -> ChannelInfo {
    ChannelInfo {
        cid: c.cid.clone(),
        pid: c.pid,
        ppid: c.ppid,
        cwd: c.cwd.clone(),
        session: c.session.clone(),
    }
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// 항목 정렬: 점수 높은 순 → 최근 순 → id.
pub fn sort_items(items: &mut [Item]) {
    items.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| b.updated_ms.cmp(&a.updated_ms))
            .then_with(|| a.id.cmp(&b.id))
    });
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
