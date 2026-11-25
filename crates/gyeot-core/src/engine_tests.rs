use super::*;
use crate::learn::proposal_id;
use crate::model::Evidence;
use crate::normalize::normalize;
use serde_json::{Value, json};

const T0: i64 = 1_764_126_720_000;
const S: &str = "abc123";
const CWD: &str = "C:\\work\\proj";
const SEC: i64 = 1000;
const MIN: i64 = 60_000;

fn hook_raw(name: &str, extra: Value) -> Value {
    let mut v = json!({
        "session_id": S,
        "transcript_path": "/t.jsonl",
        "cwd": CWD,
        "hook_event_name": name,
    });
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        for (k, x) in e {
            o.insert(k.clone(), x.clone());
        }
    }
    v
}

fn hook(ts: i64, name: &str, extra: Value) -> Event {
    let mut evs = normalize(Source::ClaudeCode, &hook_raw(name, extra), ts, Some(4321));
    assert_eq!(evs.len(), 1, "{name}");
    evs.remove(0)
}

fn hook_in(ts: i64, session: &str, cwd: &str, pid: u32, name: &str, extra: Value) -> Event {
    let mut raw = hook_raw(name, extra);
    raw["session_id"] = json!(session);
    raw["cwd"] = json!(cwd);
    normalize(Source::ClaudeCode, &raw, ts, Some(pid)).remove(0)
}

fn perm_prompt(ts: i64) -> Event {
    hook(
        ts,
        "Notification",
        json!({"notification_type": "permission_prompt", "message": "Claude needs your permission to use Bash"}),
    )
}

fn perm_request(ts: i64, tool: &str, command: &str) -> Event {
    hook(
        ts,
        "PermissionRequest",
        json!({"tool_name": tool, "tool_input": {"command": command}}),
    )
}

fn stop(ts: i64, msg: &str) -> Event {
    hook(ts, "Stop", json!({ "last_assistant_message": msg }))
}

fn tool(ts: i64, name: &str) -> Event {
    hook(ts, "PostToolUse", json!({ "tool_name": name }))
}

fn prompt(ts: i64) -> Event {
    hook(ts, "UserPromptSubmit", json!({"prompt": "hi"}))
}

fn gate(ts: i64, session: Option<&str>, cwd: Option<&str>, result: &str) -> Event {
    let mut raw = json!({"t": ts as f64 / 1000.0, "event": "gate", "result": result, "ms": 3});
    if let Some(s) = session {
        raw["session"] = json!(s);
    }
    if let Some(c) = cwd {
        raw["cwd"] = json!(c);
    }
    normalize(Source::FsmapHarness, &raw, ts, None).remove(0)
}

fn channel_open(ts: i64, cid: &str, ppid: u32, cwd: &str) -> Event {
    Event {
        id: String::new(),
        ts_ms: ts,
        source: Source::Gyeot,
        agent: None,
        agent_id: None,
        session: None,
        cwd: Some(cwd.to_owned()),
        pid_hint: None,
        raw_event: None,
        body: EventBody::ChannelOpen {
            cid: cid.to_owned(),
            pid: 9000,
            ppid,
            cwd: cwd.to_owned(),
        },
    }
}

fn decision_open(ts: i64, session: &str, id: &str, options: &[&str]) -> Event {
    Event {
        id: String::new(),
        ts_ms: ts,
        source: Source::Gyeot,
        agent: None,
        agent_id: None,
        session: Some(session.to_owned()),
        cwd: None,
        pid_hint: None,
        raw_event: None,
        body: EventBody::DecisionOpen {
            decision_id: id.to_owned(),
            question: "어느 쪽으로 갈까요?".to_owned(),
            options: options.iter().map(|s| (*s).to_owned()).collect(),
        },
    }
}

/// 엔진과 그 엔진에 들어간 이벤트 기록.
struct H {
    e: Engine,
    log: Vec<Event>,
}

impl H {
    fn new() -> H {
        H::with(Policy::default())
    }
    fn with(p: Policy) -> H {
        H {
            e: Engine::new(p),
            log: Vec::new(),
        }
    }
}

/// 승인·병합 대기도 소리까지 올리는 설정(`approval_interrupt = true`, §13.3).
fn loud_policy() -> Policy {
    Policy {
        approval_interrupt: true,
        ..Policy::default()
    }
}

fn loud() -> H {
    H::with(loud_policy())
}

impl H {
    fn apply(&mut self, ev: Event) -> Vec<Effect> {
        self.log.push(ev.clone());
        self.e.apply(&ev)
    }
    fn tick(&mut self, t: i64) -> Vec<Effect> {
        self.e.tick(t)
    }
    fn act(
        &mut self,
        target: &str,
        a: Action,
        reason: Option<&str>,
        minutes: Option<i64>,
        now: i64,
    ) -> Vec<Effect> {
        let (ev, fx) = self.e.act(target, a, reason, minutes, now).expect("act");
        self.log.push(ev);
        fx
    }
    fn focus(&mut self, on: bool, minutes: Option<i64>, now: i64) -> Vec<Effect> {
        let (ev, fx) = self.e.set_focus(on, minutes, now).expect("focus");
        self.log.push(ev);
        fx
    }
    fn open(&self, now: i64) -> Vec<Item> {
        self.e.snapshot(now).items
    }
    fn get(&self, kind: ItemKind, now: i64) -> Option<Item> {
        self.e.item(&item_id(S, kind), now)
    }
    fn state(&self, kind: ItemKind) -> Option<ItemState> {
        self.get(kind, 0).map(|i| i.state)
    }
    fn kinds(&self, now: i64) -> Vec<ItemKind> {
        self.open(now).iter().map(|i| i.kind).collect()
    }
}

fn deliveries(fx: &[Effect]) -> Vec<Delivery> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Deliver(d) => Some(d.clone()),
            _ => None,
        })
        .collect()
}

fn upserts(fx: &[Effect]) -> Vec<Item> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::ItemUpsert(i) => Some(i.clone()),
            _ => None,
        })
        .collect()
}

fn removed(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::ItemRemoved { id } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

fn last_presence(fx: &[Effect]) -> Option<Presence> {
    fx.iter().rev().find_map(|e| match e {
        Effect::Presence(p) => Some(p.clone()),
        _ => None,
    })
}

// ───────────────────── 항목 열기 ─────────────────────

#[test]
fn waiting_permission_opens_approval() {
    let mut h = H::new();
    let fx = h.apply(perm_prompt(T0));
    let ups = upserts(&fx);
    assert_eq!(ups.len(), 1);
    let it = &ups[0];
    assert_eq!(it.id, item_id(S, ItemKind::Approval));
    assert_eq!(it.kind, ItemKind::Approval);
    assert_eq!(it.title, "권한 승인 대기");
    assert_eq!(it.body, "Claude needs your permission to use Bash");
    assert!(it.blocking);
    assert_eq!(it.score, 95.0);
    assert_eq!(it.signature, format!("approval:{T0}"));
    assert_eq!(it.project, "proj");
    assert_eq!(it.agent, Some(Agent::ClaudeCode));
    assert_eq!(it.state, ItemState::Open);
    assert_eq!(it.created_ms, T0);
    // 4분 뒤 점수 95.3(계약 예시)
    let it = h.get(ItemKind::Approval, T0 + 4 * MIN).unwrap();
    assert_eq!(it.score, 95.3);
    assert_eq!(
        it.reasons,
        vec!["권한 승인 대기: 기본 95", "4분 기다림: +0.3"]
    );
    let d = deliveries(&fx);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].line, "proj에서 권한 승인을 기다려요");
}

#[test]
fn permission_request_enriches_within_ten_seconds() {
    // 요청이 먼저
    let mut h = H::new();
    h.apply(perm_request(T0, "Bash", "npm install --save-dev vitest"));
    h.apply(perm_prompt(T0 + 9 * SEC));
    let it = h.get(ItemKind::Approval, T0 + 9 * SEC).unwrap();
    assert_eq!(it.title, "권한 승인 대기 · Bash");
    assert_eq!(it.body, "npm install --save-dev vitest");

    // 10초를 넘기면 보강하지 않는다
    let mut h = H::new();
    h.apply(perm_request(T0, "Bash", "npm install"));
    h.apply(perm_prompt(T0 + 11 * SEC));
    let it = h.get(ItemKind::Approval, T0 + 11 * SEC).unwrap();
    assert_eq!(it.title, "권한 승인 대기");

    // 알림이 먼저 와도 10초 안의 요청으로 보강한다(같은 서명, 새 전달 없음)
    let mut h = H::new();
    h.apply(perm_prompt(T0));
    let fx = h.apply(perm_request(T0 + 2 * SEC, "Edit", "src/a.rs"));
    assert!(deliveries(&fx).is_empty());
    let ups = upserts(&fx);
    assert_eq!(ups.len(), 1);
    assert_eq!(ups[0].title, "권한 승인 대기 · Edit");
    assert_eq!(ups[0].body, "src/a.rs");
    assert_eq!(ups[0].signature, format!("approval:{T0}"));
}

#[test]
fn merge_request_opens_merge_after_window() {
    let mut h = H::new();
    h.apply(perm_request(T0, "Bash", "gh pr merge 12 --squash"));
    assert!(h.open(T0).is_empty());
    h.tick(T0 + PERM_WINDOW_MS);
    assert!(h.open(T0 + PERM_WINDOW_MS).is_empty());
    let fx = h.tick(T0 + PERM_WINDOW_MS + 1);
    let ups = upserts(&fx);
    assert_eq!(ups.len(), 1);
    assert_eq!(ups[0].kind, ItemKind::Merge);
    assert_eq!(ups[0].title, "병합 승인 대기 · Bash");
    assert_eq!(ups[0].body, "gh pr merge 12 --squash");
    assert_eq!(ups[0].score, 90.0);
    assert_eq!(ups[0].updated_ms, T0);
    // 같은 도구의 작업 신호가 오면 닫힌다
    h.apply(tool(T0 + 20 * SEC, "Bash"));
    assert!(h.open(T0 + 20 * SEC).is_empty());
}

#[test]
fn approval_wins_over_merge() {
    let mut h = H::new();
    h.apply(perm_request(T0, "Bash", "git push origin main"));
    h.apply(perm_prompt(T0 + SEC));
    h.tick(T0 + 30 * SEC);
    assert_eq!(h.kinds(T0 + 30 * SEC), vec![ItemKind::Approval]);
    assert_eq!(
        h.get(ItemKind::Approval, T0).unwrap().body,
        "git push origin main"
    );
    // 승인이 열려 있으면 그 뒤 병합 신호는 열지 않는다
    h.apply(perm_request(T0 + 40 * SEC, "Bash", "git merge topic"));
    h.tick(T0 + 60 * SEC);
    assert_eq!(h.kinds(T0 + 60 * SEC), vec![ItemKind::Approval]);
}

#[test]
fn decision_class_precedence() {
    let mut h = H::new();
    // 결정이 먼저 열려 있다가
    h.apply(hook(
        T0,
        "Notification",
        json!({"notification_type": "elicitation_dialog", "message": "Pick one"}),
    ));
    assert_eq!(h.kinds(T0), vec![ItemKind::Decision]);
    assert_eq!(
        h.get(ItemKind::Decision, T0).unwrap().title,
        "질문에 답을 기다려요"
    );
    // 승인이 오면 결정은 닫히고 승인만 남는다
    let fx = h.apply(perm_prompt(T0 + SEC));
    assert!(removed(&fx).contains(&item_id(S, ItemKind::Decision)));
    assert_eq!(h.kinds(T0 + SEC), vec![ItemKind::Approval]);
    // 승인이 열려 있는 동안 결정 신호는 열리지 않는다
    h.apply(hook(
        T0 + 2 * SEC,
        "Notification",
        json!({"notification_type": "agent_needs_input", "message": "?"}),
    ));
    assert_eq!(h.kinds(T0 + 2 * SEC), vec![ItemKind::Approval]);
}

#[test]
fn decision_closes_unread_and_followup() {
    let mut h = H::new();
    h.apply(stop(T0, "PR을 열었어요. 리뷰는 직접 해 주세요."));
    assert_eq!(h.kinds(T0), vec![ItemKind::Followup]);
    h.apply(stop(T0 + MIN, "시험 통과"));
    let mut k = h.kinds(T0 + MIN);
    k.sort();
    assert_eq!(k, vec![ItemKind::Followup, ItemKind::Unread]);
    h.apply(hook(
        T0 + 2 * MIN,
        "Notification",
        json!({"notification_type": "agent_needs_input", "message": "입력해 주세요"}),
    ));
    assert_eq!(h.kinds(T0 + 2 * MIN), vec![ItemKind::Decision]);
    assert_eq!(h.state(ItemKind::Unread), Some(ItemState::Closed));
    assert_eq!(h.state(ItemKind::Followup), Some(ItemState::Closed));
}

#[test]
fn turn_done_classification() {
    let mut h = H::new();
    h.apply(stop(T0, "다 했어요.\n병합할까요?"));
    let it = h.get(ItemKind::Decision, T0).unwrap();
    assert_eq!(it.title, "질문이 있어요");
    assert_eq!(it.body, "다 했어요. 병합할까요?");
    assert_eq!(it.score, 85.0);

    let mut h = H::new();
    h.apply(stop(T0, "모든 시험 통과. 병합 완료."));
    assert_eq!(h.kinds(T0), vec![ItemKind::Unread]);
    assert_eq!(h.get(ItemKind::Unread, T0).unwrap().score, 55.0);
}

#[test]
fn idle_prompt_does_not_open_or_reset_unread() {
    let mut h = H::new();
    let idle = |ts| {
        hook(
            ts,
            "Notification",
            json!({"notification_type": "idle_prompt", "message": "Claude is waiting for your input"}),
        )
    };
    h.apply(idle(T0));
    assert!(h.open(T0).is_empty());
    h.apply(stop(T0 + SEC, "끝"));
    let before = h.get(ItemKind::Unread, T0 + SEC).unwrap();
    let fx = h.apply(idle(T0 + MIN));
    assert!(deliveries(&fx).is_empty());
    let after = h.get(ItemKind::Unread, T0 + MIN).unwrap();
    assert_eq!(after.signature, before.signature);
    assert_eq!(after.updated_ms, before.updated_ms);
    assert_eq!(h.kinds(T0 + MIN), vec![ItemKind::Unread]);
}

#[test]
fn turn_done_closes_waits_of_that_turn() {
    let mut h = H::new();
    h.apply(perm_prompt(T0));
    h.apply(stop(T0 + MIN, "거절당해서 멈췄어요."));
    assert_eq!(h.state(ItemKind::Approval), Some(ItemState::Closed));
    assert_eq!(h.kinds(T0 + MIN), vec![ItemKind::Unread]);
}

#[test]
fn failed_volatile_and_context() {
    let mut h = H::new();
    h.apply(hook(T0, "StopFailure", json!({"error_type": "rate_limit"})));
    let it = h.get(ItemKind::Failed, T0).unwrap();
    assert_eq!(it.score, 80.0);
    assert!(!it.blocking);
    assert_eq!(it.title, "턴이 오류로 끝났어요 · rate_limit");
    h.apply(gate(T0 + SEC, Some(S), None, "push-uncommitted"));
    assert_eq!(h.get(ItemKind::Volatile, T0).unwrap().score, 78.0);
    h.apply(hook(
        T0 + 2 * SEC,
        "PreCompact",
        json!({"trigger": "manual"}),
    ));
    assert!(h.get(ItemKind::Context, T0).is_none());
    h.apply(hook(T0 + 3 * SEC, "PreCompact", json!({"trigger": "auto"})));
    assert_eq!(h.get(ItemKind::Context, T0).unwrap().score, 45.0);
    // failed는 오래 두어도 감쇠하지 않는다
    assert_eq!(
        h.get(ItemKind::Failed, T0 + 100 * 86_400_000)
            .unwrap()
            .score,
        95.0
    );
}

#[test]
fn gate_results_open_failed() {
    for (r, opens) in [
        ("fail", true),
        ("cannot", true),
        ("block", true),
        ("push-error", true),
        ("pass", false),
        ("released", false),
        ("unchanged", false),
        ("skip-background", false),
        ("push-bypass", false),
    ] {
        let mut h = H::new();
        h.apply(gate(T0, Some(S), None, r));
        assert_eq!(h.get(ItemKind::Failed, T0).is_some(), opens, "{r}");
    }
}

#[test]
fn gate_without_session_matches_cwd() {
    let mut h = H::new();
    h.apply(tool(T0, "Read"));
    h.apply(gate(T0 + SEC, None, Some("c:/work/proj/"), "block"));
    let it = h.get(ItemKind::Failed, T0 + SEC).expect("matched by cwd");
    assert_eq!(it.title, "fsmap 게이트 막힘");
    // 맞는 세션이 없으면 저장소 경로로 따로 둔다
    let mut h = H::new();
    h.apply(gate(T0, None, Some("/repo/x"), "fail"));
    let snap = h.e.snapshot(T0);
    assert_eq!(snap.items.len(), 1);
    assert_eq!(snap.items[0].session, "cwd:/repo/x");
    assert_eq!(snap.items[0].project, "x");
}

// ───────────────────── 자동 닫힘 ─────────────────────

#[test]
fn prompt_closes_decision_class_unread_followup_but_not_failed() {
    let mut h = H::new();
    h.apply(hook(T0, "StopFailure", json!({"error_type": "x"})));
    h.apply(stop(T0 + SEC, "남은 일: 배포"));
    h.apply(perm_prompt(T0 + 2 * SEC));
    let mut k = h.kinds(T0 + 2 * SEC);
    k.sort();
    assert_eq!(k, vec![ItemKind::Approval, ItemKind::Failed]);
    let fx = h.apply(prompt(T0 + 3 * SEC));
    assert!(removed(&fx).contains(&item_id(S, ItemKind::Approval)));
    assert_eq!(h.kinds(T0 + 3 * SEC), vec![ItemKind::Failed]);

    let mut h = H::new();
    h.apply(stop(T0, "끝"));
    h.apply(prompt(T0 + SEC));
    assert!(h.open(T0 + SEC).is_empty());
}

#[test]
fn activity_closes_approval_for_same_or_unknown_tool() {
    let mut h = H::new();
    h.apply(perm_request(T0, "Bash", "ls"));
    h.apply(perm_prompt(T0 + SEC));
    // 병렬로 돈 다른 도구의 신호로는 닫지 않는다
    h.apply(tool(T0 + 2 * SEC, "Read"));
    assert_eq!(h.kinds(T0 + 2 * SEC), vec![ItemKind::Approval]);
    h.apply(tool(T0 + 3 * SEC, "Bash"));
    assert!(h.open(T0 + 3 * SEC).is_empty());

    // 도구를 모르면 어떤 작업 신호든 닫는다(계약 그대로)
    let mut h = H::new();
    h.apply(perm_prompt(T0));
    h.apply(hook(T0 + SEC, "PostToolBatch", json!({})));
    assert!(h.open(T0 + SEC).is_empty());
    let mut h = H::new();
    h.apply(perm_prompt(T0));
    h.apply(tool(T0 + SEC, "Read"));
    assert!(h.open(T0 + SEC).is_empty());
}

#[test]
fn session_end_closes_everything() {
    let mut h = H::new();
    h.apply(hook(T0, "StopFailure", json!({})));
    h.apply(perm_prompt(T0 + SEC));
    h.apply(gate(T0 + 2 * SEC, Some(S), None, "push-uncommitted"));
    assert_eq!(h.open(T0).len(), 3);
    h.apply(hook(T0 + 3 * SEC, "SessionEnd", json!({"reason": "exit"})));
    assert!(h.open(T0 + 3 * SEC).is_empty());
    assert_eq!(h.e.sessions()[0].state, SessionState::Ended);
}

#[test]
fn gate_pass_closes_gate_failed_only() {
    let mut h = H::new();
    h.apply(gate(T0, Some(S), None, "block"));
    h.apply(gate(T0 + SEC, Some(S), None, "pass"));
    assert!(h.open(T0 + SEC).is_empty());

    let mut h = H::new();
    h.apply(hook(T0, "StopFailure", json!({"error_type": "x"})));
    h.apply(gate(T0 + SEC, Some(S), None, "pass"));
    assert_eq!(h.kinds(T0 + SEC), vec![ItemKind::Failed]);
}

#[test]
fn stall_opens_via_tick_and_closes_on_activity() {
    let p = Policy {
        stale_minutes: 30,
        ..Policy::default()
    };
    let mut h = H::with(p);
    h.apply(tool(T0, "Bash"));
    h.tick(T0 + 30 * MIN);
    assert!(h.open(T0 + 30 * MIN).is_empty());
    let fx = h.tick(T0 + 30 * MIN + 1);
    let ups = upserts(&fx);
    assert_eq!(ups.len(), 1);
    assert_eq!(ups[0].kind, ItemKind::Stall);
    assert_eq!(ups[0].signature, format!("stall:{T0}"));
    assert_eq!(ups[0].score, 70.0);
    assert_eq!(ups[0].updated_ms, T0 + 30 * MIN + 1);
    // 작업 신호가 오면 닫힌다
    h.apply(tool(T0 + 40 * MIN, "Read"));
    assert!(h.open(T0 + 40 * MIN).is_empty());
    // turn_done도 닫는다
    h.tick(T0 + 71 * MIN);
    assert_eq!(h.kinds(T0 + 71 * MIN), vec![ItemKind::Stall]);
    h.apply(stop(T0 + 72 * MIN, "끝"));
    assert_eq!(h.kinds(T0 + 72 * MIN), vec![ItemKind::Unread]);
}

#[test]
fn stall_needs_working_session() {
    let mut h = H::new();
    h.apply(stop(T0, "끝"));
    h.apply(prompt(T0 + 1));
    h.apply(stop(T0 + 2, "끝"));
    h.tick(T0 + 5 * 3_600_000);
    assert!(!h.kinds(T0).contains(&ItemKind::Stall));
}

#[test]
fn stall_dismissal_is_bound_to_signature() {
    let mut h = H::new();
    h.apply(prompt(T0));
    h.tick(T0 + 31 * MIN);
    let id = item_id(S, ItemKind::Stall);
    h.act(&id, Action::Dismiss, Some("무관"), None, T0 + 32 * MIN);
    h.tick(T0 + 90 * MIN);
    assert!(h.open(T0 + 90 * MIN).is_empty());
    // 새 작업 신호 뒤 다시 멈추면 새 서명으로 다시 열린다
    h.apply(tool(T0 + 100 * MIN, "Bash"));
    h.tick(T0 + 131 * MIN);
    let it = h.get(ItemKind::Stall, T0 + 131 * MIN).unwrap();
    assert_eq!(it.state, ItemState::Open);
    assert_eq!(it.signature, format!("stall:{}", T0 + 100 * MIN));
}

// ───────────────────── 사람 동작·서명 ─────────────────────

#[test]
fn done_and_dismiss_are_bound_to_signature() {
    let mut h = H::new();
    let ev = perm_prompt(T0);
    h.apply(ev.clone());
    let id = item_id(S, ItemKind::Approval);
    let fx = h.act(&id, Action::Done, None, None, T0 + SEC);
    assert_eq!(removed(&fx), vec![id.clone()]);
    assert_eq!(h.state(ItemKind::Approval), Some(ItemState::Done));
    // 같은 신호가 또 와도(중복 전송) 다시 열리지 않는다
    h.apply(ev);
    assert!(h.open(T0 + 2 * SEC).is_empty());
    // 새 신호(서명이 다름)는 다시 연다
    let fx = h.apply(perm_prompt(T0 + 5 * SEC));
    assert_eq!(upserts(&fx).len(), 1);
    assert_eq!(h.state(ItemKind::Approval), Some(ItemState::Open));
    assert_eq!(
        h.get(ItemKind::Approval, 0).unwrap().signature,
        format!("approval:{}", T0 + 5 * SEC)
    );
    // 넘기기도 같다
    h.act(&id, Action::Dismiss, Some("이미 처리"), None, T0 + 6 * SEC);
    assert_eq!(h.state(ItemKind::Approval), Some(ItemState::Dismissed));
    h.apply(perm_prompt(T0 + 7 * SEC));
    assert_eq!(h.state(ItemKind::Approval), Some(ItemState::Open));
}

#[test]
fn renewed_signal_on_open_item_redelivers() {
    let mut h = H::new();
    h.apply(stop(T0, "a"));
    let first = h.get(ItemKind::Unread, T0).unwrap();
    let fx = h.apply(stop(T0 + MIN, "b"));
    assert_eq!(deliveries(&fx).len(), 1);
    let it = h.get(ItemKind::Unread, T0 + MIN).unwrap();
    assert_eq!(it.body, "b");
    assert_eq!(it.created_ms, first.created_ms);
    assert_eq!(it.updated_ms, T0 + MIN);
}

#[test]
fn snooze_hides_and_returns() {
    let mut h = H::new();
    h.apply(stop(T0, "끝"));
    let id = item_id(S, ItemKind::Unread);
    let fx = h.act(&id, Action::Snooze, None, Some(10), T0 + SEC);
    assert_eq!(removed(&fx), vec![id.clone()]);
    let it = h.get(ItemKind::Unread, 0).unwrap();
    assert_eq!(it.state, ItemState::Snoozed);
    assert_eq!(it.snooze_until_ms, Some(T0 + SEC + 10 * MIN));
    h.tick(T0 + SEC + 10 * MIN - 1);
    assert!(h.open(T0 + 10 * MIN).is_empty());
    let fx = h.tick(T0 + SEC + 10 * MIN);
    assert_eq!(upserts(&fx).len(), 1);
    assert_eq!(deliveries(&fx).len(), 1);
    assert_eq!(h.state(ItemKind::Unread), Some(ItemState::Open));
    // 기본은 1440분
    h.act(&id, Action::Snooze, None, None, T0 + 20 * MIN);
    assert_eq!(
        h.get(ItemKind::Unread, 0).unwrap().snooze_until_ms,
        Some(T0 + 20 * MIN + 1440 * MIN)
    );
}

#[test]
fn new_signal_ends_snooze() {
    let mut h = H::new();
    h.apply(stop(T0, "a"));
    let id = item_id(S, ItemKind::Unread);
    h.act(&id, Action::Snooze, None, Some(60), T0 + SEC);
    h.apply(stop(T0 + 2 * MIN, "b"));
    assert_eq!(h.state(ItemKind::Unread), Some(ItemState::Open));
}

#[test]
fn act_validation_errors() {
    let mut h = H::new();
    h.apply(stop(T0, "끝"));
    let id = item_id(S, ItemKind::Unread);
    assert_eq!(
        h.e.act("it_nope", Action::Done, None, None, T0)
            .unwrap_err(),
        ActError::UnknownItem
    );
    assert_eq!(
        h.e.act(&id, Action::Dismiss, Some("싫음"), None, T0)
            .unwrap_err(),
        ActError::InvalidReason
    );
    assert_eq!(
        h.e.act(&id, Action::Snooze, None, Some(0), T0).unwrap_err(),
        ActError::InvalidMinutes
    );
    assert_eq!(
        h.e.set_focus(true, Some(100_000), T0).unwrap_err(),
        ActError::InvalidMinutes
    );
    assert_eq!(
        h.e.respond_proposal("pr_x", true, T0).unwrap_err(),
        ActError::UnknownProposal
    );
    assert_eq!(
        h.e.decide("dc_x", T0).unwrap_err(),
        ActError::UnknownDecision
    );
    assert_eq!(
        h.e.act("bogus", Action::Unquiet, None, None, T0)
            .unwrap_err(),
        ActError::InvalidKind
    );
    // 이유 없는 넘기기와 open은 받는다
    let (ev, _) = h.e.act(&id, Action::Open, None, None, T0).unwrap();
    assert_eq!(ev.kind(), "user_action");
    assert_eq!(ev.source, Source::Gyeot);
    assert_eq!(ev.session.as_deref(), Some(S));
    assert!(h.e.act(&id, Action::Dismiss, None, None, T0).is_ok());
}

#[test]
fn user_actions_on_closed_items_are_noops() {
    let mut h = H::new();
    h.apply(stop(T0, "끝"));
    h.apply(prompt(T0 + SEC));
    let id = item_id(S, ItemKind::Unread);
    let fx = h.act(&id, Action::Dismiss, Some("과민"), None, T0 + 2 * SEC);
    assert!(removed(&fx).is_empty());
    assert_eq!(h.state(ItemKind::Unread), Some(ItemState::Closed));
}

// ───────────────────── 존재감 ─────────────────────

fn mood_at(h: &mut H, t: i64) -> Mood {
    h.tick(t);
    h.e.presence().mood
}

#[test]
fn mood_priority_order() {
    let mut h = H::new();
    assert_eq!(mood_at(&mut h, T0), Mood::Idle);
    // working: 60초 안에 작업 신호
    h.apply(tool(T0, "Bash"));
    assert_eq!(h.e.presence().mood, Mood::Working);
    assert_eq!(h.e.presence().line, "proj에서 일하는 중이에요");
    assert_eq!(mood_at(&mut h, T0 + 60 * SEC), Mood::Working);
    assert_eq!(mood_at(&mut h, T0 + 60 * SEC + 1), Mood::Idle);
    // listening > working
    h.apply(tool(T0 + 2 * MIN, "Bash"));
    h.e.set_listening(true, T0 + 2 * MIN);
    assert_eq!(h.e.presence().mood, Mood::Listening);
    // waiting > listening
    h.apply(perm_prompt(T0 + 2 * MIN + SEC));
    assert_eq!(h.e.presence().mood, Mood::Waiting);
    assert_eq!(h.e.presence().line, "권한 승인 1건이 기다려요");
    assert_eq!(
        h.e.presence().focus_item.as_deref(),
        Some(item_id(S, ItemKind::Approval).as_str())
    );
    // alert > waiting
    h.apply(gate(T0 + 2 * MIN + 2 * SEC, Some(S), None, "fail"));
    assert_eq!(h.e.presence().mood, Mood::Alert);
    assert_eq!(h.e.presence().line, "proj에서 실패가 있었어요");
    // 10분 지나면 alert가 풀리고 waiting으로
    assert_eq!(mood_at(&mut h, T0 + 12 * MIN + 2 * SEC), Mood::Alert);
    assert_eq!(mood_at(&mut h, T0 + 12 * MIN + 3 * SEC), Mood::Waiting);
}

#[test]
fn done_mood_lasts_five_seconds() {
    let mut h = H::new();
    h.apply(hook(T0, "TaskCompleted", json!({"task_subject": "x"})));
    assert_eq!(h.e.presence().mood, Mood::Done);
    assert_eq!(h.e.presence().line, "proj 작업이 끝났어요");
    assert_eq!(h.e.presence().since_ms, T0);
    assert_eq!(mood_at(&mut h, T0 + 4999), Mood::Done);
    let fx = h.tick(T0 + 5000);
    assert_eq!(last_presence(&fx).unwrap().mood, Mood::Idle);
    assert_eq!(h.e.presence().since_ms, T0 + 5000);
    // turn_done도 done(항목은 따로 열린다)
    h.apply(stop(T0 + 10 * SEC, "끝"));
    assert_eq!(h.e.presence().mood, Mood::Done);
}

#[test]
fn sleep_in_focus_or_quiet_hours_unless_blocking() {
    let mut h = H::new();
    h.focus(true, Some(30), T0);
    assert_eq!(h.e.presence().mood, Mood::Sleep);
    assert_eq!(h.e.presence().focus_mode_until_ms, Some(T0 + 30 * MIN));
    h.apply(perm_prompt(T0 + MIN));
    assert_eq!(h.e.presence().mood, Mood::Waiting);
    h.apply(tool(T0 + 2 * MIN, "Bash"));
    // working > sleep
    assert_eq!(h.e.presence().mood, Mood::Working);
    assert_eq!(mood_at(&mut h, T0 + 4 * MIN), Mood::Sleep);
    // 집중 모드가 끝나면 idle
    assert_eq!(mood_at(&mut h, T0 + 30 * MIN), Mood::Idle);
    assert_eq!(h.e.presence().focus_mode_until_ms, None);

    // 조용한 시간(KST 22:00-08:00), T = 2025-11-26 13:00 UTC = 22:00 KST
    let t = 1_764_162_000_000;
    let p = Policy {
        quiet_hours: Policy::parse_quiet_hours(&["22:00-08:00"]).unwrap(),
        utc_offset_minutes: 540,
        ..Policy::default()
    };
    let mut h = H::with(p);
    assert_eq!(mood_at(&mut h, t - MIN), Mood::Idle);
    let fx = h.tick(t);
    let pr = last_presence(&fx).unwrap();
    assert_eq!(pr.mood, Mood::Sleep);
    assert_eq!(pr.since_ms, t);
    assert_eq!(mood_at(&mut h, t + 10 * 3_600_000), Mood::Idle);
}

#[test]
fn waiting_line_counts_blocking_items() {
    let mut h = H::new();
    h.apply(perm_prompt(T0));
    h.apply(hook_in(
        T0 + SEC,
        "s2",
        "/w/other",
        1,
        "Stop",
        json!({"last_assistant_message": "할까요?"}),
    ));
    assert_eq!(h.e.presence().line, "2건이 기다려요 · 먼저 권한 승인");
}

// ───────────────────── 전달 판단 ─────────────────────

const NO_APPROVAL_SIGNAL_LINE: &str = "승인했는지 곁이 알 수 없어 말풍선까지만 알려요";

/// R2 N2(계약 §13.3, G-035): 기본 설정에서는 승인·병합 대기가 오래 기다려도 소리(interrupt)까지 가지 않는다.
#[test]
fn approval_and_merge_stop_at_nudge_by_default() {
    // 작업 신호를 낸 세션이라도(§12.3 안전장치와 상관없이)
    let mut h = H::new();
    h.apply(tool(T0 - 10 * SEC, "Read"));
    let fx = h.apply(perm_prompt(T0));
    assert_eq!(deliveries(&fx)[0].level, Level::Nudge);
    let mut later = Vec::new();
    let mut t = T0;
    while t < T0 + 6 * 3_600_000 {
        t += MIN;
        later.extend(deliveries(&h.tick(t)));
    }
    assert!(
        later.iter().all(|d| d.level != Level::Interrupt),
        "{later:?}"
    );
    assert_eq!(h.e.presence().level, Level::Nudge);
    assert_eq!(h.e.snapshot(t).budget.interrupt, 1);

    // 늦게 온(스풀) 승인 대기: 처음 판단부터 2분 넘게 기다린 상태 → 이유 줄과 함께 말풍선
    let mut h = H::new();
    h.apply(tool(T0 - 10 * SEC, "Read"));
    h.apply(hook_in(T0 + 5 * MIN, "other", "/w/o", 9, "Stop", json!({})));
    let d = deliveries(&h.apply(perm_prompt(T0)));
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].level, Level::Nudge);
    assert!(!d[0].speak);
    assert!(
        d[0].reasons.iter().any(|r| r == NO_APPROVAL_SIGNAL_LINE),
        "{:?}",
        d[0].reasons
    );

    // 병합도 같다
    let mut h = H::new();
    h.apply(tool(T0 - 10 * SEC, "Read"));
    h.apply(perm_request(T0, "Bash", "git push origin main"));
    let mut all = deliveries(&h.tick(T0 + 11 * SEC));
    assert_eq!(h.kinds(T0 + 11 * SEC), vec![ItemKind::Merge]);
    let mut t = T0 + 11 * SEC;
    while t < T0 + 6 * 3_600_000 {
        t += MIN;
        all.extend(deliveries(&h.tick(t)));
    }
    assert!(!all.is_empty());
    assert!(all.iter().all(|d| d.level != Level::Interrupt), "{all:?}");
}

/// `approval_interrupt = true`: 지금까지의 규칙(2분 → 소리). 스냅샷 설정에도 보인다.
#[test]
fn approval_interrupt_setting_restores_sound() {
    let mut h = loud();
    h.apply(tool(T0 - 10 * SEC, "Read"));
    h.apply(perm_prompt(T0));
    let d = deliveries(&h.tick(T0 + 2 * MIN + 1));
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].level, Level::Interrupt);
    assert!(d[0].reasons.iter().all(|r| r != NO_APPROVAL_SIGNAL_LINE));
    let snap = serde_json::to_value(h.e.snapshot(T0)).unwrap();
    assert_eq!(snap["settings"]["approval_interrupt"], json!(true));
    let snap = serde_json::to_value(H::new().e.snapshot(T0)).unwrap();
    assert_eq!(snap["settings"]["approval_interrupt"], json!(false));
    assert!(!Policy::default().approval_interrupt);
}

#[test]
fn delivery_nudge_then_interrupt_after_two_minutes() {
    let mut h = loud();
    // 작업 신호를 낸 세션(§12.3)
    h.apply(tool(T0 - 10 * SEC, "Read"));
    let fx = h.apply(perm_prompt(T0));
    let d = deliveries(&fx);
    assert_eq!(d[0].level, Level::Nudge);
    assert!(!d[0].speak);
    assert_eq!(d[0].at_ms, T0);
    assert!(
        d[0].reasons.iter().any(|r| r == "→ 말풍선"),
        "{:?}",
        d[0].reasons
    );
    assert!(deliveries(&h.tick(T0 + 2 * MIN)).is_empty());
    let fx = h.tick(T0 + 2 * MIN + 1);
    let d = deliveries(&fx);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].level, Level::Interrupt);
    assert!(d[0].speak);
    assert_eq!(d[0].line, "proj에서 권한 승인을 2분째 기다려요");
    assert_eq!(d[0].at_ms, T0 + 2 * MIN + 1);
    assert_eq!(h.e.presence().level, Level::Interrupt);
    // 한 번 승격하면 다시 판단하지 않는다
    assert!(deliveries(&h.tick(T0 + 30 * MIN)).is_empty());
}

const NO_ACTIVITY_LINE: &str = "이 세션은 작업 신호가 없어 승인 여부를 알 수 없어요 → 말풍선까지만";

/// R1 H2(계약 §12.3): `activity`를 한 번도 내지 않은 세션의 승인·병합은 대기로 소리까지 올리지 않는다.
#[test]
fn approval_without_activity_never_interrupts() {
    let mut h = loud();
    let fx = h.apply(perm_prompt(T0));
    assert_eq!(deliveries(&fx)[0].level, Level::Nudge);
    // 2분이 지나도, 한참 지나도 소리로 올리지 않는다
    assert!(deliveries(&h.tick(T0 + 2 * MIN + 1)).is_empty());
    assert!(deliveries(&h.tick(T0 + 30 * MIN)).is_empty());
    assert_eq!(h.e.presence().level, Level::Nudge);
    assert_eq!(h.e.snapshot(T0 + 30 * MIN).budget.interrupt, 1);

    // 늦게 온(스풀) 승인 대기: 처음 판단부터 2분 넘게 기다린 상태 → 이유 줄과 함께 말풍선
    let mut h = loud();
    h.apply(hook_in(T0 + 5 * MIN, "other", "/w/o", 9, "Stop", json!({})));
    let fx = h.apply(perm_prompt(T0));
    let d = deliveries(&fx);
    assert_eq!(d.len(), 1, "{fx:?}");
    assert_eq!(d[0].level, Level::Nudge);
    assert!(!d[0].speak);
    assert!(
        d[0].reasons.iter().any(|r| r == NO_ACTIVITY_LINE),
        "{:?}",
        d[0].reasons
    );

    // 작업 신호를 낸 적이 있는 세션은 그대로 소리 후보
    let mut h = loud();
    h.apply(tool(T0 - 10 * SEC, "Read"));
    h.apply(perm_prompt(T0));
    let d = deliveries(&h.tick(T0 + 2 * MIN + 1));
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].level, Level::Interrupt);
    assert!(d[0].reasons.iter().all(|r| r != NO_ACTIVITY_LINE));
}

#[test]
fn merge_without_activity_never_interrupts() {
    let five_hours = 5 * 3_600_000;
    let run = |with_activity: bool| {
        let mut h = loud();
        if with_activity {
            h.apply(tool(T0 - 10 * SEC, "Read"));
        }
        h.apply(perm_request(T0, "Bash", "git -C . push origin main"));
        let mut all = deliveries(&h.tick(T0 + 11 * SEC));
        assert_eq!(h.kinds(T0 + 11 * SEC), vec![ItemKind::Merge]);
        let mut t = T0 + 11 * SEC;
        while t < T0 + five_hours {
            t += MIN;
            all.extend(deliveries(&h.tick(t)));
        }
        all
    };
    let quiet = run(false);
    assert!(!quiet.is_empty());
    assert!(
        quiet.iter().all(|d| d.level != Level::Interrupt),
        "{quiet:?}"
    );
    let loud = run(true);
    assert!(loud.iter().any(|d| d.level == Level::Interrupt), "{loud:?}");
}

/// `PostToolBatch`는 도구 이름이 없으니 그 세션의 승인·병합(도구를 아는 것도)을 닫는다(§12.3).
#[test]
fn post_tool_batch_closes_approval_and_merge() {
    // 보강된(도구를 아는) 승인
    let mut h = H::new();
    h.apply(perm_request(T0, "Bash", "npm install"));
    h.apply(perm_prompt(T0 + SEC));
    assert_eq!(h.kinds(T0 + SEC), vec![ItemKind::Approval]);
    let fx = h.apply(hook(T0 + 2 * SEC, "PostToolBatch", json!({})));
    assert_eq!(removed(&fx), vec![item_id(S, ItemKind::Approval)]);
    assert!(h.open(T0 + 2 * SEC).is_empty());

    // 병합(보류 창이 지나 열린 것)
    let mut h = H::new();
    h.apply(perm_request(T0, "Bash", "gh pr merge 12 --squash"));
    h.tick(T0 + 11 * SEC);
    assert_eq!(h.kinds(T0 + 11 * SEC), vec![ItemKind::Merge]);
    let fx = h.apply(hook(T0 + 12 * SEC, "PostToolBatch", json!({})));
    assert_eq!(removed(&fx), vec![item_id(S, ItemKind::Merge)]);
    assert!(h.open(T0 + 12 * SEC).is_empty());

    // 보류 중인 병합 요청도 버린다(창이 지나도 열리지 않는다)
    let mut h = H::new();
    h.apply(perm_request(T0, "Bash", "git merge topic"));
    h.apply(hook(T0 + 2 * SEC, "PostToolBatch", json!({})));
    h.tick(T0 + 20 * SEC);
    assert!(h.open(T0 + 20 * SEC).is_empty());
}

// ───────────────────── 하위 에이전트(§13.2) ─────────────────────

/// 하위 에이전트 `agent`(없으면 주 에이전트)의 훅 이벤트.
fn by(agent: Option<&str>, ts: i64, name: &str, extra: Value) -> Event {
    let mut x = extra;
    if let Some(a) = agent {
        x["agent_id"] = json!(a);
    }
    hook(ts, name, x)
}

/// `PostToolBatch`: `tool_calls`에 도구 이름들(빈 목록이면 `tool_calls` 없음).
fn batch(agent: Option<&str>, ts: i64, tools: &[&str]) -> Event {
    let extra = if tools.is_empty() {
        json!({})
    } else {
        json!({"tool_calls": tools.iter().map(|t| json!({"tool_name": t, "tool_use_id": "toolu", "tool_input": {}, "tool_response": "r"})).collect::<Vec<_>>()})
    };
    by(agent, ts, "PostToolBatch", extra)
}

fn sub_perm_request(agent: Option<&str>, ts: i64, tool: &str, command: &str) -> Event {
    by(
        agent,
        ts,
        "PermissionRequest",
        json!({"tool_name": tool, "tool_input": {"command": command}}),
    )
}

/// R3 M3(계약 §13.2, G-034): 다른 하위 에이전트의 `PostToolBatch`는 진짜 승인 대기를 닫지 않는다.
#[test]
fn other_subagent_batch_does_not_close_approval() {
    let mut h = H::new();
    h.apply(sub_perm_request(Some("a1"), T0, "Bash", "npm install"));
    h.apply(perm_prompt(T0 + SEC)); // agent_id 없는 알림
    assert_eq!(h.kinds(T0 + SEC), vec![ItemKind::Approval]);
    let t = T0 + 2 * SEC;
    for (i, ev) in [
        batch(Some("a2"), t, &["Read"]),
        batch(Some("a2"), t, &["Bash"]),
        batch(Some("a2"), t, &[]),
        batch(None, t, &["Bash"]),
        batch(None, t, &[]),
        batch(Some("a1"), t, &["Read", "Grep"]),
        by(Some("a2"), t, "PostToolUse", json!({"tool_name": "Bash"})),
        by(None, t, "PostToolUse", json!({"tool_name": "Bash"})),
    ]
    .into_iter()
    .enumerate()
    {
        let fx = h.apply(ev);
        assert!(removed(&fx).is_empty(), "{i}: {fx:?}");
        assert_eq!(h.kinds(t), vec![ItemKind::Approval], "{i}");
    }
    // 같은 에이전트의 묶음에 그 도구가 있으면 닫는다
    let fx = h.apply(batch(Some("a1"), T0 + 3 * SEC, &["Read", "Bash"]));
    assert_eq!(removed(&fx), vec![item_id(S, ItemKind::Approval)]);
    assert!(h.open(T0 + 3 * SEC).is_empty());

    // 도구 목록이 없는 묶음은 '모르는 도구'라 같은 에이전트면 닫는다
    let mut h = H::new();
    h.apply(sub_perm_request(Some("a1"), T0, "Bash", "npm install"));
    h.apply(perm_prompt(T0 + SEC));
    h.apply(batch(Some("a2"), T0 + 2 * SEC, &[]));
    assert_eq!(h.kinds(T0 + 2 * SEC), vec![ItemKind::Approval]);
    h.apply(batch(Some("a1"), T0 + 3 * SEC, &[]));
    assert!(h.open(T0 + 3 * SEC).is_empty());

    // 주 에이전트(agent_id 없음)의 승인은 주 에이전트의 신호로 닫는다
    let mut h = H::new();
    h.apply(perm_request(T0, "Bash", "npm install"));
    h.apply(perm_prompt(T0 + SEC));
    h.apply(batch(Some("a1"), T0 + 2 * SEC, &["Bash"]));
    assert_eq!(h.kinds(T0 + 2 * SEC), vec![ItemKind::Approval]);
    h.apply(batch(None, T0 + 3 * SEC, &["Bash"]));
    assert!(h.open(T0 + 3 * SEC).is_empty());
}

/// `agent_id` 없는 `Notification(permission_prompt)`은 열린 승인에 붙고, 그 승인의 에이전트를 바꾸지 않는다.
#[test]
fn agentless_notification_keeps_approval_agent() {
    // 알림에 agent_id가 있으면 그 에이전트의 승인이다. 그 뒤 agent_id 없는 알림이 와도 에이전트는 그대로
    let mut h = H::new();
    h.apply(by(
        Some("a1"),
        T0,
        "Notification",
        json!({"notification_type": "permission_prompt", "message": "m"}),
    ));
    h.apply(perm_prompt(T0 + 30 * SEC));
    assert_eq!(h.kinds(T0 + 30 * SEC), vec![ItemKind::Approval]);
    h.apply(batch(None, T0 + 31 * SEC, &[]));
    assert_eq!(h.kinds(T0 + 31 * SEC), vec![ItemKind::Approval]);
    h.apply(batch(Some("a1"), T0 + 32 * SEC, &[]));
    assert!(h.open(T0 + 32 * SEC).is_empty());

    // 알림이 먼저(agent_id 없음), 10초 안의 요청(a1)이 보강하며 에이전트를 알려 준다
    let mut h = H::new();
    h.apply(perm_prompt(T0));
    h.apply(sub_perm_request(Some("a1"), T0 + 2 * SEC, "Bash", "ls"));
    assert_eq!(
        h.get(ItemKind::Approval, T0).unwrap().title,
        "권한 승인 대기 · Bash"
    );
    h.apply(batch(None, T0 + 3 * SEC, &[]));
    h.apply(batch(Some("a2"), T0 + 3 * SEC, &["Bash"]));
    assert_eq!(h.kinds(T0 + 3 * SEC), vec![ItemKind::Approval]);
    h.apply(batch(Some("a1"), T0 + 4 * SEC, &["Bash"]));
    assert!(h.open(T0 + 4 * SEC).is_empty());

    // 열린 승인(a1)에 다른 에이전트(a2)의 요청은 섞지 않는다: 보강하지 않고 보류로 둔다
    let mut h = H::new();
    h.apply(by(
        Some("a1"),
        T0,
        "Notification",
        json!({"notification_type": "permission_prompt", "message": "m"}),
    ));
    h.apply(sub_perm_request(Some("a2"), T0 + 2 * SEC, "Edit", "x"));
    let it = h.get(ItemKind::Approval, T0).unwrap();
    assert_eq!(it.title, "권한 승인 대기");
    // 그 뒤 agent_id 없는 알림도 a1의 승인에 붙으므로 a2의 요청으로 보강하지 않는다
    h.apply(perm_prompt(T0 + 3 * SEC));
    let it = h.get(ItemKind::Approval, T0 + 3 * SEC).unwrap();
    assert_eq!(it.title, "권한 승인 대기");
    h.apply(batch(Some("a1"), T0 + 4 * SEC, &["Read"]));
    assert!(
        h.open(T0 + 4 * SEC).is_empty(),
        "a1 승인은 도구 모름 → 닫힘"
    );
}

/// R3 L1(계약 §13.2): 같은 세션·같은 에이전트·같은 도구의 `PostToolUseFailure`도 승인 대기를 닫는다.
#[test]
fn tool_failure_closes_same_agent_same_tool() {
    let fail = |agent: Option<&str>, ts: i64, tool: &str| {
        by(
            agent,
            ts,
            "PostToolUseFailure",
            json!({"tool_name": tool, "error": "denied"}),
        )
    };
    let mut h = H::new();
    h.apply(sub_perm_request(Some("a1"), T0, "Bash", "npm install"));
    h.apply(perm_prompt(T0 + SEC));
    h.apply(fail(Some("a2"), T0 + 2 * SEC, "Bash"));
    h.apply(fail(None, T0 + 2 * SEC, "Bash"));
    h.apply(fail(Some("a1"), T0 + 2 * SEC, "Read"));
    assert_eq!(h.kinds(T0 + 2 * SEC), vec![ItemKind::Approval]);
    let fx = h.apply(fail(Some("a1"), T0 + 3 * SEC, "Bash"));
    assert_eq!(removed(&fx), vec![item_id(S, ItemKind::Approval)]);
    assert!(h.open(T0 + 3 * SEC).is_empty());

    // 주 에이전트도 같다(옛 설치: agent_id 없음)
    let mut h = H::new();
    h.apply(perm_request(T0, "Bash", "npm install"));
    h.apply(perm_prompt(T0 + SEC));
    h.apply(fail(None, T0 + 2 * SEC, "Bash"));
    assert!(h.open(T0 + 2 * SEC).is_empty());

    // 열린 병합 항목과 보류 중인 병합 요청도
    let mut h = H::new();
    h.apply(sub_perm_request(Some("a1"), T0, "Bash", "gh pr merge 3"));
    h.tick(T0 + 11 * SEC);
    assert_eq!(h.kinds(T0 + 11 * SEC), vec![ItemKind::Merge]);
    h.apply(fail(Some("a2"), T0 + 12 * SEC, "Bash"));
    assert_eq!(h.kinds(T0 + 12 * SEC), vec![ItemKind::Merge]);
    h.apply(fail(Some("a1"), T0 + 13 * SEC, "Bash"));
    assert!(h.open(T0 + 13 * SEC).is_empty());
    let mut h = H::new();
    h.apply(sub_perm_request(Some("a1"), T0, "Bash", "git merge topic"));
    h.apply(fail(Some("a1"), T0 + 2 * SEC, "Bash"));
    h.tick(T0 + 20 * SEC);
    assert!(h.open(T0 + 20 * SEC).is_empty());
}

/// 병합: 보류 창이 지나 열린 병합은 요청한 에이전트를 기억한다. 다른 에이전트의 신호는 보류도 지우지 않는다.
#[test]
fn merge_remembers_requesting_agent() {
    let mut h = H::new();
    h.apply(sub_perm_request(
        Some("a1"),
        T0,
        "Bash",
        "git push origin +main",
    ));
    // 보류 중 다른 에이전트의 묶음: 보류를 지우지 않는다
    h.apply(batch(Some("a2"), T0 + 2 * SEC, &[]));
    h.tick(T0 + 11 * SEC);
    assert_eq!(h.kinds(T0 + 11 * SEC), vec![ItemKind::Merge]);
    h.apply(batch(Some("a2"), T0 + 12 * SEC, &["Bash"]));
    assert_eq!(h.kinds(T0 + 12 * SEC), vec![ItemKind::Merge]);
    h.apply(batch(Some("a1"), T0 + 13 * SEC, &["Bash"]));
    assert!(h.open(T0 + 13 * SEC).is_empty());
}

/// R2 N3(계약 §13.6): 보강된 감지로 병합 항목이 열린다.
#[test]
fn n3_push_forms_open_merge() {
    for cmd in [
        "git push origin +main",
        "git push origin HEAD:refs/heads/main",
        "git push origin main;",
        "git push origin main&& echo",
        "git push origin main|cat",
        "git push origin \"main\"",
        "git push origin 'main'",
        "git push origin refs/heads/main",
    ] {
        let mut h = H::new();
        h.apply(perm_request(T0, "Bash", cmd));
        h.tick(T0 + 11 * SEC);
        assert_eq!(h.kinds(T0 + 11 * SEC), vec![ItemKind::Merge], "{cmd}");
    }
}

#[test]
fn delivery_ambient_when_user_present() {
    let mut h = H::new();
    h.apply(prompt(T0));
    let fx = h.apply(perm_prompt(T0 + 10 * SEC));
    let d = deliveries(&fx);
    assert_eq!(d[0].level, Level::Ambient);
    assert!(d[0].reasons.iter().any(|r| r.contains("1분 안에 입력")));
    // 60초가 지나면 다시 판단해 말풍선으로 올린다
    let fx = h.tick(T0 + 60 * SEC + 1);
    let d = deliveries(&fx);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].level, Level::Nudge);
}

#[test]
fn delivery_ambient_below_nudge_min() {
    let mut h = H::new();
    let fx = h.apply(stop(T0, "끝"));
    let d = deliveries(&fx);
    assert_eq!(d[0].level, Level::Ambient);
    assert!(d[0].reasons.iter().any(|r| r.contains("말풍선 기준 80")));
    assert_eq!(d[0].line, "proj에서 결과가 나왔어요");
    // failed(80)는 기준에 닿아 말풍선
    let fx = h.apply(hook(T0 + SEC, "StopFailure", json!({})));
    assert_eq!(deliveries(&fx)[0].level, Level::Nudge);
}

#[test]
fn stall_escalates_when_waiting_bonus_reaches_nudge_min() {
    let mut h = H::new();
    h.apply(tool(T0, "Bash"));
    let fx = h.tick(T0 + 31 * MIN);
    assert_eq!(deliveries(&fx)[0].level, Level::Ambient);
    let opened = h.get(ItemKind::Stall, 0).unwrap().updated_ms;
    let w = crate::model::wait_to_reach(ItemKind::Stall, 80.0).unwrap();
    assert!(deliveries(&h.tick(opened + w)).is_empty());
    let fx = h.tick(opened + w + 1000);
    let d = deliveries(&fx);
    assert_eq!(d.len(), 1, "{fx:?}");
    assert_eq!(d[0].level, Level::Nudge);
}

#[test]
fn delivery_in_focus_mode() {
    let mut h = loud();
    h.apply(tool(T0 - 10 * SEC, "Read"));
    h.focus(true, None, T0);
    assert_eq!(h.e.presence().focus_mode_until_ms, Some(T0 + 60 * MIN));
    let fx = h.apply(perm_prompt(T0 + SEC));
    let d = deliveries(&fx);
    assert_eq!(d[0].level, Level::Ambient);
    assert!(d[0].reasons.iter().any(|r| r == "집중 모드예요"));
    // 2분이 지나도 소리 없음
    assert!(deliveries(&h.tick(T0 + SEC + 2 * MIN + 1)).is_empty());
    // 3분 넘게 기다린 95점 이상 막힘만 말풍선
    assert!(deliveries(&h.tick(T0 + SEC + 3 * MIN)).is_empty());
    let d = deliveries(&h.tick(T0 + SEC + 3 * MIN + 1));
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].level, Level::Nudge);
    // 결정(85)은 집중 모드에서 표정만
    let fx = h.apply(hook_in(
        T0 + 5 * MIN,
        "s2",
        "/w/b",
        2,
        "Stop",
        json!({"last_assistant_message": "할까요?"}),
    ));
    assert_eq!(deliveries(&fx)[0].level, Level::Ambient);
    // 집중 모드를 끄면 다음 판단에서 올라간다
    let fx = h.focus(false, None, T0 + 10 * MIN);
    let d = deliveries(&fx);
    assert_eq!(d.len(), 2, "{d:?}");
    assert!(d.iter().any(|x| x.level == Level::Interrupt));
}

#[test]
fn delivery_in_quiet_hours() {
    let t = 1_764_162_000_000; // 22:00 KST
    let p = Policy {
        quiet_hours: Policy::parse_quiet_hours(&["22:00-08:00"]).unwrap(),
        utc_offset_minutes: 540,
        approval_interrupt: true,
        ..Policy::default()
    };
    let mut h = H::with(p);
    h.apply(hook(t, "PostToolUse", json!({"tool_name": "Read"})));
    let fx = h.apply(perm_prompt(t + MIN));
    let d = deliveries(&fx);
    assert_eq!(d[0].level, Level::Ambient);
    assert!(
        d[0].reasons
            .iter()
            .any(|r| r.contains("조용한 시간이에요(22:00-08:00)"))
    );
    let d = deliveries(&h.tick(t + 4 * MIN + 1));
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].level, Level::Nudge);
    // 조용한 시간이 끝나면 소리 후보
    let d = deliveries(&h.tick(t + 10 * 3_600_000));
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].level, Level::Interrupt);
    assert_eq!(d[0].at_ms, t + 10 * 3_600_000);
}

#[test]
fn nudge_budget_four_then_refill() {
    let mut h = H::new();
    let mut levels = Vec::new();
    for i in 0..5 {
        let ev = hook_in(
            T0 + i * SEC,
            &format!("s{i}"),
            &format!("/w/p{i}"),
            1,
            "StopFailure",
            json!({}),
        );
        levels.push(deliveries(&h.apply(ev))[0].clone());
    }
    let lv: Vec<Level> = levels.iter().map(|d| d.level).collect();
    assert_eq!(
        lv,
        vec![
            Level::Nudge,
            Level::Nudge,
            Level::Nudge,
            Level::Nudge,
            Level::Ambient
        ]
    );
    assert!(levels[4].reasons.iter().any(|r| r.contains("말풍선 예산")));
    assert_eq!(h.e.snapshot(T0 + 5 * SEC).budget.nudge, 0);
    // 15분마다 하나씩 돌아온다
    assert_eq!(h.e.snapshot(T0 + 15 * MIN).budget.nudge, 1);
    assert_eq!(h.e.snapshot(T0 + 60 * MIN).budget.nudge, 4);
    assert_eq!(h.e.snapshot(T0 + 600 * MIN).budget.nudge, 4);
    let ev = hook_in(T0 + 15 * MIN, "s9", "/w/p9", 1, "StopFailure", json!({}));
    assert_eq!(deliveries(&h.apply(ev))[0].level, Level::Nudge);
}

#[test]
fn interrupt_budget_downgrades() {
    let mut h = loud();
    h.apply(tool(T0 - 10 * SEC, "Read"));
    h.apply(hook_in(
        T0 - 10 * SEC,
        "s2",
        "/w/b",
        2,
        "PostToolUse",
        json!({"tool_name": "Read"}),
    ));
    h.apply(perm_prompt(T0));
    h.apply(hook_in(
        T0,
        "s2",
        "/w/b",
        2,
        "Notification",
        json!({"notification_type": "permission_prompt", "message": "m"}),
    ));
    let fx = h.tick(T0 + 2 * MIN + 1);
    let mut d = deliveries(&fx);
    d.sort_by_key(|x| x.level);
    assert_eq!(d.len(), 2);
    assert_eq!(d[1].level, Level::Interrupt);
    assert_eq!(d[0].level, Level::Nudge);
    assert!(d[0].reasons.iter().any(|r| r.contains("소리 예산")));
    assert_eq!(h.e.snapshot(T0 + 3 * MIN).budget.interrupt, 0);
    assert_eq!(
        h.e.snapshot(T0 + 2 * MIN + 1 + 30 * MIN).budget.interrupt,
        1
    );
}

#[test]
fn bucket_is_partition_invariant() {
    let mut a = Bucket::new(4, 100);
    let mut b = Bucket::new(4, 100);
    for t in [0, 10, 20, 30, 40] {
        assert_eq!(a.try_take(t), t < 40);
    }
    for t in [0, 5, 10, 15, 20, 25, 30, 35, 40] {
        let _ = b.tokens(t);
        if t % 10 == 0 {
            b.try_take(t);
        }
    }
    assert_eq!(a, b);
    assert_eq!(a.tokens(140), 1);
    assert_eq!(a.tokens(1000), 4);
}

// ───────────────────── 제안 ─────────────────────

fn dismiss_unreads(h: &mut H, n: i64, start: i64, reason: &str) -> Vec<Effect> {
    let mut all = Vec::new();
    for i in 0..n {
        let t = start + i * MIN;
        h.apply(stop(t, "끝"));
        let id = item_id(S, ItemKind::Unread);
        all.extend(h.act(&id, Action::Dismiss, Some(reason), None, t + SEC));
    }
    all
}

#[test]
fn proposal_opens_and_accept_makes_kind_quiet() {
    let mut h = H::new();
    let fx = dismiss_unreads(&mut h, 4, T0, "과민");
    assert!(!fx.iter().any(|e| matches!(e, Effect::Proposal(_))));
    let fx = dismiss_unreads(&mut h, 1, T0 + 10 * MIN, "과민");
    let p = fx
        .iter()
        .find_map(|e| match e {
            Effect::Proposal(p) => Some(p.clone()),
            _ => None,
        })
        .expect("proposal");
    assert_eq!(p.id, "pr_unread_quiet");
    assert_eq!(p.kind, ItemKind::Unread);
    assert_eq!(p.change, "quiet");
    assert_eq!(
        p.evidence,
        Evidence {
            dismissed: 5,
            total: 5,
            window_days: 14
        }
    );
    assert_eq!(
        p.line,
        "최근 ‘안 읽은 결과’ 알림을 5번 중 5번 넘기셨어요. 이 종류는 앞으로 조용히(표정만) 알릴까요?"
    );
    assert_eq!(h.e.snapshot(T0 + 20 * MIN).proposals, vec![p.clone()]);
    let (ev, fx) = h.e.respond_proposal(&p.id, true, T0 + 20 * MIN).unwrap();
    h.log.push(ev);
    assert!(fx.contains(&Effect::Resync));
    let snap = h.e.snapshot(T0 + 20 * MIN);
    assert!(snap.proposals.is_empty());
    assert_eq!(snap.settings.quiet_kinds, vec![ItemKind::Unread]);
    // 조용히 알리기로 한 종류는 표정만
    let fx = h.apply(stop(T0 + 30 * MIN, "끝"));
    let d = deliveries(&fx);
    assert_eq!(d[0].level, Level::Ambient);
    assert!(d[0].reasons.iter().any(|r| r.contains("조용히 알리기로")));
    // 되돌리기
    let (ev, fx) =
        h.e.act("unread", Action::Unquiet, None, None, T0 + 31 * MIN)
            .unwrap();
    h.log.push(ev);
    assert!(fx.contains(&Effect::Resync));
    assert!(h.e.snapshot(T0 + 31 * MIN).settings.quiet_kinds.is_empty());
}

#[test]
fn only_oversensitive_reason_counts() {
    let mut h = H::new();
    let fx = dismiss_unreads(&mut h, 8, T0, "이미 처리");
    assert!(!fx.iter().any(|e| matches!(e, Effect::Proposal(_))));
    let fx = dismiss_unreads(&mut h, 4, T0 + 20 * MIN, "과민");
    // 과민 4번뿐이라 아직 아니다
    assert!(!fx.iter().any(|e| matches!(e, Effect::Proposal(_))));
    let st = &h.e.learn().kinds[&ItemKind::Unread];
    assert_eq!(st.dismissed["이미 처리"], 8);
    assert_eq!(st.opened, 12);
}

#[test]
fn rejected_proposal_is_suppressed_for_fourteen_days() {
    let mut h = H::new();
    dismiss_unreads(&mut h, 5, T0, "과민");
    let id = proposal_id(ItemKind::Unread);
    let (ev, _) = h.e.respond_proposal(&id, false, T0 + HOUR_MS).unwrap();
    h.log.push(ev);
    assert!(h.e.snapshot(T0 + HOUR_MS).proposals.is_empty());
    assert!(h.e.snapshot(T0 + HOUR_MS).settings.quiet_kinds.is_empty());
    let fx = dismiss_unreads(&mut h, 5, T0 + 2 * HOUR_MS, "과민");
    assert!(!fx.iter().any(|e| matches!(e, Effect::Proposal(_))));
    let later = T0 + HOUR_MS + 14 * 86_400_000 + 1;
    let fx = dismiss_unreads(&mut h, 5, later, "과민");
    assert!(fx.iter().any(|e| matches!(e, Effect::Proposal(_))));
}

// ───────────────────── channel·결정 ─────────────────────

#[test]
fn channel_links_by_pid_then_cwd() {
    let mut h = H::new();
    h.apply(hook_in(
        T0,
        "a",
        "/w/a",
        100,
        "SessionStart",
        json!({"source": "startup"}),
    ));
    h.apply(hook_in(
        T0 + 1,
        "b",
        "/w/b",
        200,
        "SessionStart",
        json!({"source": "startup"}),
    ));
    // PID로 잇기
    h.apply(channel_open(T0 + 2, "ch_1", 200, "/w/somewhere"));
    assert_eq!(h.e.channel("ch_1").unwrap().session.as_deref(), Some("b"));
    assert_eq!(h.e.channel_of_session("b").as_deref(), Some("ch_1"));
    // PID가 안 맞으면 같은 작업 폴더의 세션
    h.apply(channel_open(T0 + 3, "ch_2", 999, "/w/a/"));
    assert_eq!(h.e.channel("ch_2").unwrap().session.as_deref(), Some("a"));
    // 맞는 것이 없으면 이어지지 않는다
    h.apply(channel_open(T0 + 4, "ch_3", 555, "/w/c"));
    assert_eq!(h.e.channel("ch_3").unwrap().session, None);
    // 나중에 그 PID의 세션 이벤트가 오면 잇는다
    h.apply(hook_in(
        T0 + 5,
        "c",
        "/w/zzz",
        555,
        "PreToolUse",
        json!({"tool_name": "Bash"}),
    ));
    assert_eq!(h.e.channel("ch_3").unwrap().session.as_deref(), Some("c"));
    let sessions = h.e.sessions();
    let c = sessions.iter().find(|s| s.id == "c").unwrap();
    assert_eq!(c.channel.as_deref(), Some("ch_3"));
    assert_eq!(
        h.e.latest_channel_session(),
        Some(("c".into(), "ch_3".into()))
    );
    // 같은 프로세스의 새 세션(/clear)으로 channel이 옮겨 간다
    h.apply(hook_in(
        T0 + 6,
        "c2",
        "/w/zzz",
        555,
        "SessionStart",
        json!({"source": "clear"}),
    ));
    assert_eq!(h.e.channel("ch_3").unwrap().session.as_deref(), Some("c2"));
    assert_eq!(h.e.channel_of_session("c"), None);
    // 닫으면 풀린다
    h.apply(Event {
        body: EventBody::ChannelClose {
            cid: "ch_3".into(),
            pid: 9000,
            ppid: 555,
            cwd: "/w/c".into(),
        },
        ..channel_open(T0 + 7, "ch_3", 555, "/w/c")
    });
    assert!(h.e.channel("ch_3").is_none());
    assert_eq!(h.e.channel_of_session("c2"), None);
}

#[test]
fn channel_decision_roundtrip() {
    let mut h = H::new();
    h.apply(tool(T0, "Bash"));
    h.apply(decision_open(T0 + SEC, S, "dc_1", &["A안", "B안"]));
    let it = h.get(ItemKind::Decision, T0 + SEC).unwrap();
    assert_eq!(it.title, "어느 쪽으로 갈까요?");
    assert_eq!(it.options, Some(vec!["A안".to_owned(), "B안".to_owned()]));
    assert_eq!(it.decision_id.as_deref(), Some("dc_1"));
    assert_eq!(it.body, "A안 · B안");
    // 턴 끝 질문이 channel 결정을 덮어쓰지 않고, 턴 끝이 닫지도 않는다
    h.apply(stop(T0 + 2 * SEC, "곁에 물어봤어요. 어느 쪽으로 할까요?"));
    let it = h.get(ItemKind::Decision, T0 + 2 * SEC).unwrap();
    assert_eq!(it.decision_id.as_deref(), Some("dc_1"));
    assert_eq!(it.state, ItemState::Open);
    let info = h.e.decision("dc_1").unwrap();
    assert!(info.open);
    assert_eq!(info.session, S);
    // 답하면 처리됨
    let (ev, fx) = h.e.decide("dc_1", T0 + 3 * SEC).unwrap();
    h.log.push(ev);
    assert_eq!(removed(&fx), vec![item_id(S, ItemKind::Decision)]);
    assert_eq!(h.state(ItemKind::Decision), Some(ItemState::Done));
    assert!(!h.e.decision("dc_1").unwrap().open);
}

#[test]
fn sessions_state_and_effects() {
    let mut h = H::new();
    let fx = h.apply(tool(T0, "Bash"));
    let s = fx
        .iter()
        .find_map(|e| match e {
            Effect::Sessions(s) => Some(s.clone()),
            _ => None,
        })
        .expect("sessions effect");
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].state, SessionState::Working);
    assert_eq!(s[0].project, "proj");
    // 같은 상태의 작업 신호는 세션 효과를 다시 내지 않는다
    let fx = h.apply(tool(T0 + SEC, "Bash"));
    assert!(!fx.iter().any(|e| matches!(e, Effect::Sessions(_))));
    h.apply(perm_prompt(T0 + 2 * SEC));
    assert_eq!(h.e.sessions()[0].state, SessionState::Waiting);
    h.apply(prompt(T0 + 3 * SEC));
    h.apply(stop(T0 + 4 * SEC, "끝"));
    assert_eq!(h.e.sessions()[0].state, SessionState::Idle);
    assert_eq!(h.e.sessions()[0].last_ms, T0 + 4 * SEC);
    h.apply(hook(T0 + 5 * SEC, "SessionEnd", json!({"reason": "exit"})));
    assert_eq!(h.e.sessions()[0].state, SessionState::Ended);
    // 끝난 세션은 1시간 뒤 목록에서 빠진다
    h.tick(T0 + 5 * SEC + HOUR_MS);
    assert_eq!(h.e.sessions().len(), 1);
    let fx = h.tick(T0 + 5 * SEC + HOUR_MS + 1);
    assert!(h.e.sessions().is_empty());
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::Sessions(v) if v.is_empty()))
    );
}

#[test]
fn idle_sessions_with_open_items_are_kept() {
    let mut h = H::new();
    h.apply(hook(T0, "StopFailure", json!({})));
    h.tick(T0 + 72 * HOUR_MS);
    assert_eq!(h.e.sessions().len(), 1);
    h.act(
        &item_id(S, ItemKind::Failed),
        Action::Done,
        None,
        None,
        T0 + 72 * HOUR_MS,
    );
    h.tick(T0 + 72 * HOUR_MS + 1);
    assert!(h.e.sessions().is_empty());
}

// ───────────────────── 재생 결정성 ─────────────────────

fn scenario(h: &mut H, tick_every: Option<i64>) {
    let mut t = T0;
    let step = |h: &mut H, ev: Event, t: &mut i64| {
        if let Some(dt) = tick_every {
            while *t + dt < ev.ts_ms {
                *t += dt;
                h.tick(*t);
            }
        }
        *t = ev.ts_ms;
        h.apply(ev);
    };
    step(
        h,
        hook(
            T0,
            "SessionStart",
            json!({"source": "startup", "model": "m"}),
        ),
        &mut t,
    );
    step(h, prompt(T0 + SEC), &mut t);
    step(h, tool(T0 + 2 * SEC, "Read"), &mut t);
    step(
        h,
        perm_request(T0 + 3 * SEC, "Bash", "git push origin main"),
        &mut t,
    );
    step(h, perm_prompt(T0 + 4 * SEC), &mut t);
    step(h, tool(T0 + 5 * MIN, "Bash"), &mut t);
    step(h, channel_open(T0 + 5 * MIN + 1, "ch_1", 4321, CWD), &mut t);
    step(
        h,
        decision_open(T0 + 6 * MIN, S, "dc_1", &["예", "아니오"]),
        &mut t,
    );
    step(
        h,
        stop(T0 + 7 * MIN, "PR을 열었어요. 리뷰는 직접 해 주세요."),
        &mut t,
    );
    step(
        h,
        gate(T0 + 8 * MIN, None, Some("c:/work/proj"), "block"),
        &mut t,
    );
    step(
        h,
        hook_in(
            T0 + 9 * MIN,
            "s2",
            "/w/b",
            77,
            "UserPromptSubmit",
            json!({"prompt": "x"}),
        ),
        &mut t,
    );
    step(
        h,
        hook_in(
            T0 + 9 * MIN + SEC,
            "s2",
            "/w/b",
            77,
            "PreToolUse",
            json!({"tool_name": "Bash"}),
        ),
        &mut t,
    );
    // s2는 그 뒤 조용해서 멈춤이 열린다
    step(
        h,
        hook(T0 + 50 * MIN, "PreCompact", json!({"trigger": "auto"})),
        &mut t,
    );
    step(
        h,
        gate(T0 + 51 * MIN, Some(S), None, "push-uncommitted"),
        &mut t,
    );
    step(
        h,
        hook_in(
            T0 + 52 * MIN,
            "s3",
            "/w/c",
            3,
            "StopFailure",
            json!({"error_type": "overloaded"}),
        ),
        &mut t,
    );
    if tick_every.is_some() {
        h.tick(T0 + 53 * MIN);
    }
    // 사람 동작
    let stall = item_id("s2", ItemKind::Stall);
    let fid = item_id("s3", ItemKind::Failed);
    h.act(&stall, Action::Snooze, None, Some(30), T0 + 54 * MIN);
    h.act(&fid, Action::Open, None, None, T0 + 55 * MIN);
    h.act(&fid, Action::Dismiss, Some("과민"), None, T0 + 56 * MIN);
    h.focus(true, Some(20), T0 + 57 * MIN);
    let (ev, _) = h.e.decide("dc_1", T0 + 58 * MIN).unwrap();
    h.log.push(ev);
    h.e.set_listening(true, T0 + 59 * MIN);
    if tick_every.is_some() {
        h.tick(T0 + 70 * MIN);
    }
    for i in 0..5 {
        let t2 = T0 + 100 * MIN + i * MIN;
        step(
            h,
            hook_in(
                t2,
                "s4",
                "/w/d",
                4,
                "Stop",
                json!({"last_assistant_message": "끝"}),
            ),
            &mut t,
        );
        let id = item_id("s4", ItemKind::Unread);
        h.act(&id, Action::Dismiss, Some("과민"), None, t2 + SEC);
    }
    step(
        h,
        hook(T0 + 120 * MIN, "SessionEnd", json!({"reason": "exit"})),
        &mut t,
    );
    // s6: 하위 에이전트 a1의 승인 대기를 a2의 묶음이 닫지 않는다(§13.2). 재생에도 agent_id·tools가 남아야 한다.
    let s6 = |ts: i64, name: &str, extra: Value| {
        let mut raw = hook_raw(name, extra);
        raw["session_id"] = json!("s6");
        raw["cwd"] = json!("/w/f");
        normalize(Source::ClaudeCode, &raw, ts, Some(6)).remove(0)
    };
    let evs = [
        s6(
            T0 + 122 * MIN,
            "PermissionRequest",
            json!({"agent_id": "a1", "tool_name": "Bash", "tool_input": {"command": "npm i"}}),
        ),
        s6(
            T0 + 122 * MIN + SEC,
            "Notification",
            json!({"notification_type": "permission_prompt", "message": "m"}),
        ),
        s6(
            T0 + 122 * MIN + 2 * SEC,
            "PostToolBatch",
            json!({"agent_id": "a2", "tool_calls": [{"tool_name": "Bash"}]}),
        ),
        s6(
            T0 + 122 * MIN + 3 * SEC,
            "PostToolBatch",
            json!({"agent_id": "a1", "tool_calls": [{"tool_name": "Read"}]}),
        ),
    ];
    for ev in evs {
        step(h, ev, &mut t);
    }
    // s5는 작업 신호 없이 승인만 기다린다(§12.3: 말풍선까지만)
    step(
        h,
        hook_in(
            T0 + 125 * MIN,
            "s5",
            "/w/e",
            5,
            "Notification",
            json!({"notification_type": "permission_prompt", "message": "m"}),
        ),
        &mut t,
    );
}

fn snapshot_json(e: &Engine, now: i64) -> Value {
    serde_json::to_value(e.snapshot(now)).unwrap()
}

#[test]
fn replay_reproduces_state_exactly() {
    // 기본 설정과, 승인 대기도 소리까지 올리는 설정(예산·승격이 더 많이 움직인다) 둘 다
    for p in [Policy::default(), loud_policy()] {
        replay_reproduces_state_with(p);
    }
}

fn replay_reproduces_state_with(p: Policy) {
    // 실제 운영처럼 1초마다 tick한 엔진
    let mut live = H::with(p.clone());
    scenario(&mut live, Some(SEC));
    // 이벤트 로그를 JSONL로 쓰고 읽어 처음부터 다시 만든다(tick은 마지막 한 번만)
    let jsonl: String = live
        .log
        .iter()
        .map(|e| serde_json::to_string(e).unwrap() + "\n")
        .collect();
    let mut rebuilt = Engine::new(p.clone());
    for line in jsonl.lines() {
        let ev: Event = serde_json::from_str(line).unwrap();
        rebuilt.apply(&ev);
    }
    let now = T0 + 200 * MIN;
    live.e.tick(now);
    rebuilt.tick(now);
    let a = snapshot_json(&live.e, now);
    let b = snapshot_json(&rebuilt, now);
    assert_eq!(a, b);
    assert_eq!(live.e.learn(), rebuilt.learn());
    // 시나리오가 실제로 여러 갈래를 지났는지 확인
    assert!(!live.e.learn().proposals.is_empty());
    let snap = live.e.snapshot(now);
    assert!(snap.items.iter().any(|i| i.kind == ItemKind::Stall));
    // S는 session_end로 항목이 모두 닫히고, 1시간 뒤 목록에서 빠졌다.
    assert!(snap.items.iter().all(|i| i.session != S));
    assert!(snap.sessions.iter().all(|s| s.id != S));
    assert!(snap.sessions.iter().any(|s| s.id == "s3"));
    assert!(
        snap.items
            .iter()
            .any(|i| i.session == "s5" && i.kind == ItemKind::Approval)
    );
    // 다른 에이전트의 묶음·다른 도구의 묶음이 닫지 않은 a1의 승인(재생에도 agent_id·tools가 남았다)
    assert!(
        snap.items
            .iter()
            .any(|i| i.session == "s6" && i.kind == ItemKind::Approval)
    );
    if p.approval_interrupt {
        // 작업 신호를 낸 s6의 오래 기다린 승인은 소리까지 올라갔다(예산 재생까지 같아야 한다)
        assert_eq!(snap.presence.level, Level::Interrupt);
    } else {
        assert_ne!(snap.presence.level, Level::Interrupt);
    }

    // tick 간격이 달라도 같다(분 단위 tick, tick 없음)
    let mut coarse = H::with(p.clone());
    scenario(&mut coarse, Some(MIN));
    coarse.e.tick(now);
    assert_eq!(snapshot_json(&coarse.e, now), a);
    let mut none = H::with(p);
    scenario(&mut none, None);
    none.e.tick(now);
    assert_eq!(snapshot_json(&none.e, now), a);
}

#[test]
fn replay_with_saved_learn_does_not_double_count() {
    let mut live = H::new();
    scenario(&mut live, Some(SEC));
    let now = T0 + 200 * MIN;
    live.e.tick(now);
    // learn.json을 저장했다가, 그것을 안고 같은 로그를 다시 재생한다
    let saved: Learn =
        serde_json::from_str(&serde_json::to_string(live.e.learn()).unwrap()).unwrap();
    let mut restarted = Engine::with_learn(Policy::default(), saved);
    for ev in &live.log {
        restarted.apply(ev);
    }
    restarted.tick(now);
    assert_eq!(restarted.learn(), live.e.learn());
    assert_eq!(snapshot_json(&restarted, now), snapshot_json(&live.e, now));
}

#[test]
fn effects_coalesce_per_item() {
    let mut fx = Fx::default();
    let it = Item {
        id: "it_1".into(),
        kind: ItemKind::Unread,
        session: "s".into(),
        agent: None,
        project: String::new(),
        cwd: String::new(),
        title: String::new(),
        body: String::new(),
        blocking: false,
        score: 55.0,
        reasons: vec![],
        created_ms: 0,
        updated_ms: 0,
        signature: "unread:0".into(),
        state: ItemState::Open,
        snooze_until_ms: None,
        options: None,
        decision_id: None,
    };
    fx.touch("it_1", Some(it.clone()));
    fx.touch("it_2", None);
    fx.touch("it_1", None);
    let mut e = Engine::new(Policy::default());
    let out = e.finish(fx);
    assert_eq!(
        out,
        vec![
            Effect::ItemRemoved { id: "it_1".into() },
            Effect::ItemRemoved { id: "it_2".into() }
        ]
    );
}

#[test]
fn action_names_roundtrip() {
    for a in Action::ALL {
        assert_eq!(Action::parse(a.as_str()), Some(a));
    }
    assert!(Action::Open.is_item_action());
    assert!(!Action::FocusOn.is_item_action());
    assert_eq!(Action::parse("x"), None);
}

#[test]
fn unknown_project_and_cwd_are_empty_strings_not_null() {
    let mut h = H::new();
    let raw = json!({"type": "approval-requested", "thread-id": "th", "command": ["ls"]});
    let ev = normalize(Source::CodexNotify, &raw, T0, None).remove(0);
    h.apply(ev);
    let snap = serde_json::to_value(h.e.snapshot(T0)).unwrap();
    assert_eq!(snap["items"][0]["project"], json!(""));
    assert_eq!(snap["items"][0]["cwd"], json!(""));
    assert_eq!(snap["sessions"][0]["project"], json!(""));
    assert_eq!(snap["sessions"][0]["cwd"], json!(""));
    // 말에는 짧은 세션 id를 쓴다
    assert_eq!(h.e.presence().line, "권한 승인 1건이 기다려요");
    let d = h.e.item(&item_id("th", ItemKind::Approval), T0).unwrap();
    assert_eq!(d.agent, Some(Agent::Codex));
}
