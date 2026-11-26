//! 정규화 이벤트 모델(계약 §2).
//!
//! 바깥 JSON 모양은 `{"id","ts_ms","source","agent","session","cwd","pid_hint","kind","raw_event","detail"}`이다.
//! 안에서는 `kind`와 `detail`을 한 열거형 [`EventBody`]로 다룬다.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// 이벤트가 온 곳.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// Claude Code 훅.
    ClaudeCode,
    /// Codex 훅.
    Codex,
    /// Codex `notify` 명령.
    CodexNotify,
    /// AOK 하네스 로그.
    AokHarness,
    /// fsmap 하네스 로그.
    FsmapHarness,
    /// 곁 자신(사람 동작·channel).
    Gyeot,
    /// 그 밖의 도구가 정규화된 모양으로 보낸 것.
    Custom,
}

impl Source {
    /// 모든 값.
    pub const ALL: [Source; 7] = [
        Source::ClaudeCode,
        Source::Codex,
        Source::CodexNotify,
        Source::AokHarness,
        Source::FsmapHarness,
        Source::Gyeot,
        Source::Custom,
    ];

    /// 계약에 적힌 이름.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::ClaudeCode => "claude-code",
            Source::Codex => "codex",
            Source::CodexNotify => "codex-notify",
            Source::AokHarness => "aok-harness",
            Source::FsmapHarness => "fsmap-harness",
            Source::Gyeot => "gyeot",
            Source::Custom => "custom",
        }
    }

    /// 이름으로 찾는다.
    pub fn parse(s: &str) -> Option<Source> {
        Source::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// 바깥에서 `POST /v1/ingest/{source}`로 받을 수 있는 source인가(`gyeot`는 내부 전용).
    pub fn is_ingestible(self) -> bool {
        self != Source::Gyeot
    }

    /// 에이전트 자신이 낸 신호인가(세션의 활동 시각·작업 상태를 바꾼다).
    pub fn is_agent(self) -> bool {
        matches!(
            self,
            Source::ClaudeCode | Source::Codex | Source::CodexNotify | Source::Custom
        )
    }
}

/// 에이전트 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Agent {
    /// Claude Code.
    ClaudeCode,
    /// Codex.
    Codex,
}

impl Agent {
    /// 계약에 적힌 이름.
    pub fn as_str(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "claude-code",
            Agent::Codex => "codex",
        }
    }
}

/// `kind`와 `detail`(계약 §2 표).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum EventBody {
    /// 세션 시작·재개.
    SessionStart {
        /// startup·resume·clear·compact·fork
        source_kind: String,
        /// 모델 이름.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// 세션 제목.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    /// 세션 종료.
    SessionEnd {
        /// 종료 이유.
        #[serde(default)]
        reason: String,
    },
    /// 사람이 그 세션에 입력함(길이만).
    Prompt {
        /// 글자 수.
        #[serde(default)]
        chars: u64,
    },
    /// 권한 승인 대기 알림.
    WaitingPermission {
        /// 알림 제목.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// 알림 문구(마스킹).
        #[serde(default)]
        message: String,
    },
    /// 승인 요청의 내용.
    PermissionRequest {
        /// 도구 이름.
        #[serde(default)]
        tool: String,
        /// 명령·경로 요약(마스킹·300자).
        #[serde(default)]
        preview: String,
    },
    /// 입력·선택 대기.
    WaitingInput {
        /// elicitation_dialog·agent_needs_input·idle_prompt …
        #[serde(default)]
        notification_type: String,
        /// 알림 문구.
        #[serde(default)]
        message: String,
    },
    /// 턴 끝남.
    TurnDone {
        /// 마지막 응답(마스킹·280자).
        #[serde(default)]
        last_message: String,
        /// 질문으로 끝나는가.
        #[serde(default)]
        asks_question: bool,
        /// 사람 후속 표현이 있는가.
        #[serde(default)]
        human_followup: bool,
    },
    /// 턴이 API 오류로 끝남.
    TurnFailed {
        /// 오류 종류.
        #[serde(default)]
        error_type: String,
    },
    /// 도구 실패.
    ToolFailed {
        /// 도구 이름.
        #[serde(default)]
        tool: String,
        /// 오류(120자).
        #[serde(default)]
        error: String,
        /// 실패한 호출의 미리보기(`tool_input`, 마스킹·300자, §14.2). 승인 요청과 맞춰 볼 때만 쓴다.
        /// 비었으면 JSON에 쓰지 않는다(옛 로그와 같은 모양).
        #[serde(default, skip_serializing_if = "String::is_empty")]
        preview: String,
    },
    /// 작업 중 신호.
    Activity {
        /// 도구 이름.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool: Option<String>,
        /// `PostToolBatch`의 도구 이름 목록(`tool_calls[].tool_name`, 앞 64개, §13.2). 이름마다 요청 하나를
        /// 지우므로 겹친 이름도 그대로 둔다(§14.2). 비었으면 JSON에 쓰지 않는다(옛 로그와 같은 모양).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tools: Vec<String>,
        /// 심이 잘린 입력에서 건진 페이로드인가(`gyeot_partial`, §14.2). 그렇다면 요청을 지우지 않고
        /// 작업 신호로만 센다. 거짓이면 JSON에 쓰지 않는다.
        #[serde(default, skip_serializing_if = "is_false")]
        partial: bool,
        /// `PostToolUse`가 끝낸 호출의 미리보기(`tool_input`, 승인 요청과 같은 식, §15.3). 요청을 지울 때 둘 다
        /// 있으면 같아야 한다. 비었으면 JSON에 쓰지 않는다(옛 로그와 같은 모양).
        #[serde(default, skip_serializing_if = "String::is_empty")]
        preview: String,
    },
    /// 하위 에이전트 시작.
    SubagentStart {
        /// 하위 에이전트 종류.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_type: Option<String>,
    },
    /// 하위 에이전트 끝.
    SubagentStop {
        /// 하위 에이전트 종류.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_type: Option<String>,
    },
    /// 할 일 완료.
    TaskDone {
        /// 할 일 제목(120자).
        #[serde(default)]
        subject: String,
    },
    /// 맥락 압축 직전.
    ContextCompact {
        /// manual·auto
        #[serde(default)]
        trigger: String,
    },
    /// 하네스 게이트 결과.
    Gate {
        /// 결과(pass·fail·block …).
        #[serde(default)]
        result: String,
        /// fsmap·aok
        #[serde(default)]
        harness: String,
        /// 원래 이벤트 이름.
        #[serde(default)]
        event: String,
    },
    /// 그 밖의 알림.
    Notice {
        /// 알림 종류.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notification_type: Option<String>,
        /// 알림 문구.
        #[serde(default)]
        message: String,
    },
    /// 사람이 곁에서 한 일(재생용).
    UserAction {
        /// 대상(항목 id, 제안 id, decision_id, 종류 이름 등).
        #[serde(default)]
        item: String,
        /// 동작 이름([`crate::Action`]).
        #[serde(default)]
        action: String,
        /// 넘기기 이유.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        /// 분(미루기·집중 모드).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        minutes: Option<i64>,
    },
    /// channel 연결.
    ChannelOpen {
        /// channel id.
        #[serde(default)]
        cid: String,
        /// channel 프로세스 PID.
        #[serde(default)]
        pid: u32,
        /// channel의 부모 PID.
        #[serde(default)]
        ppid: u32,
        /// channel의 작업 폴더.
        #[serde(default)]
        cwd: String,
    },
    /// channel 끊김.
    ChannelClose {
        /// channel id.
        #[serde(default)]
        cid: String,
        /// channel 프로세스 PID.
        #[serde(default)]
        pid: u32,
        /// channel의 부모 PID.
        #[serde(default)]
        ppid: u32,
        /// channel의 작업 폴더.
        #[serde(default)]
        cwd: String,
    },
    /// Claude가 곁에 결정을 요청함.
    DecisionOpen {
        /// 결정 id.
        #[serde(default)]
        decision_id: String,
        /// 질문.
        #[serde(default)]
        question: String,
        /// 선택지(2–4개).
        #[serde(default)]
        options: Vec<String>,
    },
}

impl EventBody {
    /// 계약의 `kind` 이름.
    pub fn kind(&self) -> &'static str {
        match self {
            EventBody::SessionStart { .. } => "session_start",
            EventBody::SessionEnd { .. } => "session_end",
            EventBody::Prompt { .. } => "prompt",
            EventBody::WaitingPermission { .. } => "waiting_permission",
            EventBody::PermissionRequest { .. } => "permission_request",
            EventBody::WaitingInput { .. } => "waiting_input",
            EventBody::TurnDone { .. } => "turn_done",
            EventBody::TurnFailed { .. } => "turn_failed",
            EventBody::ToolFailed { .. } => "tool_failed",
            EventBody::Activity { .. } => "activity",
            EventBody::SubagentStart { .. } => "subagent_start",
            EventBody::SubagentStop { .. } => "subagent_stop",
            EventBody::TaskDone { .. } => "task_done",
            EventBody::ContextCompact { .. } => "context_compact",
            EventBody::Gate { .. } => "gate",
            EventBody::Notice { .. } => "notice",
            EventBody::UserAction { .. } => "user_action",
            EventBody::ChannelOpen { .. } => "channel_open",
            EventBody::ChannelClose { .. } => "channel_close",
            EventBody::DecisionOpen { .. } => "decision_open",
        }
    }

    /// 곁 내부에서만 만드는 종류인가(`custom` 입력으로 받지 않는다).
    pub fn is_internal(&self) -> bool {
        matches!(
            self,
            EventBody::UserAction { .. }
                | EventBody::ChannelOpen { .. }
                | EventBody::ChannelClose { .. }
                | EventBody::DecisionOpen { .. }
        )
    }

    /// 세션이 "작업 중"임을 뜻하는 신호인가(activity·prompt·subagent_*).
    pub fn is_work_signal(&self) -> bool {
        matches!(
            self,
            EventBody::Activity { .. }
                | EventBody::Prompt { .. }
                | EventBody::SubagentStart { .. }
                | EventBody::SubagentStop { .. }
        )
    }

    /// 사람에게 보일 수 있는 문자열 필드를 모두 마스킹하고 자른다(`custom` 입력용).
    pub fn sanitize(&mut self) {
        use crate::text::preview;
        fn p(s: &mut String, max: usize) {
            *s = preview(s, max);
        }
        fn po(s: &mut Option<String>, max: usize) {
            if let Some(v) = s {
                *v = preview(v, max);
            }
        }
        match self {
            EventBody::SessionStart {
                source_kind,
                model,
                title,
            } => {
                p(source_kind, 40);
                po(model, 80);
                po(title, 120);
            }
            EventBody::SessionEnd { reason } => p(reason, 80),
            EventBody::Prompt { .. } => {}
            EventBody::WaitingPermission { title, message } => {
                po(title, 120);
                p(message, 300);
            }
            EventBody::PermissionRequest { tool, preview: pv } => {
                p(tool, 80);
                p(pv, 300);
            }
            EventBody::WaitingInput {
                notification_type,
                message,
            } => {
                p(notification_type, 60);
                p(message, 300);
            }
            EventBody::TurnDone { last_message, .. } => p(last_message, 280),
            EventBody::TurnFailed { error_type } => p(error_type, 120),
            EventBody::ToolFailed {
                tool,
                error,
                preview: pv,
            } => {
                p(tool, 80);
                p(error, 120);
                p(pv, 300);
            }
            EventBody::Activity {
                tool,
                tools,
                preview: pv,
                ..
            } => {
                po(tool, 80);
                p(pv, 300);
                *tools = crate::normalize::tool_list(tools.iter().map(String::as_str));
            }
            EventBody::SubagentStart { agent_type } | EventBody::SubagentStop { agent_type } => {
                po(agent_type, 80)
            }
            EventBody::TaskDone { subject } => p(subject, 120),
            EventBody::ContextCompact { trigger } => p(trigger, 40),
            EventBody::Gate {
                result,
                harness,
                event,
            } => {
                p(result, 60);
                p(harness, 40);
                p(event, 60);
            }
            EventBody::Notice {
                notification_type,
                message,
            } => {
                po(notification_type, 60);
                p(message, 300);
            }
            EventBody::UserAction { .. }
            | EventBody::ChannelOpen { .. }
            | EventBody::ChannelClose { .. } => {}
            EventBody::DecisionOpen {
                question, options, ..
            } => {
                p(question, 300);
                for o in options {
                    p(o, 120);
                }
            }
        }
    }
}

/// 정규화 이벤트(계약 §2).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// `"<ts_ms>-<n>"`. 서버가 붙인다.
    pub id: String,
    /// UTC 밀리초.
    pub ts_ms: i64,
    /// 온 곳.
    pub source: Source,
    /// 에이전트.
    pub agent: Option<Agent>,
    /// 하위 에이전트 id(Claude Code 훅 입력의 `agent_id`, 128자까지, §13.2). 주 에이전트면 `None`.
    /// JSON에는 값이 있을 때만 쓴다(옛 로그와 같은 모양).
    pub agent_id: Option<String>,
    /// 세션 id.
    pub session: Option<String>,
    /// 작업 폴더.
    pub cwd: Option<String>,
    /// 훅을 띄운 프로세스의 PID.
    pub pid_hint: Option<u32>,
    /// 원래 이벤트 이름(`hook_event_name` 등).
    pub raw_event: Option<String>,
    /// 종류와 내용.
    pub body: EventBody,
}

impl Event {
    /// 계약의 `kind` 이름.
    pub fn kind(&self) -> &'static str {
        self.body.kind()
    }
}

#[derive(Serialize, Deserialize)]
struct EventWire {
    #[serde(default)]
    id: String,
    ts_ms: i64,
    source: Source,
    #[serde(default)]
    agent: Option<Agent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent_id: Option<String>,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    pid_hint: Option<u32>,
    kind: String,
    #[serde(default)]
    raw_event: Option<String>,
    #[serde(default)]
    detail: Value,
}

// serde의 `skip_serializing_if`가 `&bool`을 넘긴다.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

/// `kind`와 `detail` 값으로 [`EventBody`]를 만든다. `detail`이 없거나 `null`이면 빈 객체로 본다.
pub fn body_from_parts(kind: &str, detail: Value) -> Result<EventBody, serde_json::Error> {
    let detail = match detail {
        Value::Null => Value::Object(Map::new()),
        other => other,
    };
    let mut m = Map::new();
    m.insert("kind".into(), Value::String(kind.to_owned()));
    m.insert("detail".into(), detail);
    serde_json::from_value(Value::Object(m))
}

impl Serialize for Event {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error as _;
        let v = serde_json::to_value(&self.body).map_err(S::Error::custom)?;
        let detail = v
            .get("detail")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()));
        EventWire {
            id: self.id.clone(),
            ts_ms: self.ts_ms,
            source: self.source,
            agent: self.agent,
            agent_id: self.agent_id.clone(),
            session: self.session.clone(),
            cwd: self.cwd.clone(),
            pid_hint: self.pid_hint,
            kind: self.body.kind().to_owned(),
            raw_event: self.raw_event.clone(),
            detail,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Event {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let w = EventWire::deserialize(deserializer)?;
        let body = body_from_parts(&w.kind, w.detail).map_err(D::Error::custom)?;
        Ok(Event {
            id: w.id,
            ts_ms: w.ts_ms,
            source: w.source,
            agent: w.agent,
            agent_id: w.agent_id,
            session: w.session,
            cwd: w.cwd,
            pid_hint: w.pid_hint,
            raw_event: w.raw_event,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_matches_contract_shape() {
        let json = r#"{
          "id": "1764126720123-7",
          "ts_ms": 1764126720123,
          "source": "claude-code",
          "agent": "claude-code",
          "session": "abc123",
          "cwd": "C:\\work\\proj",
          "pid_hint": 4321,
          "kind": "waiting_permission",
          "raw_event": "Notification",
          "detail": { "title": "Permission needed", "message": "Claude needs your permission" }
        }"#;
        let ev: Event = serde_json::from_str(json).unwrap();
        assert_eq!(ev.kind(), "waiting_permission");
        assert_eq!(ev.pid_hint, Some(4321));
        assert_eq!(ev.cwd.as_deref(), Some("C:\\work\\proj"));
        let back = serde_json::to_value(&ev).unwrap();
        let orig: Value = serde_json::from_str(json).unwrap();
        assert_eq!(back, orig);
        let again: Event = serde_json::from_value(back).unwrap();
        assert_eq!(again, ev);
    }

    #[test]
    fn missing_detail_is_empty_object() {
        let ev: Event = serde_json::from_str(
            r#"{"ts_ms":1,"source":"codex","kind":"activity","agent":null,"session":"s"}"#,
        )
        .unwrap();
        assert_eq!(
            ev.body,
            EventBody::Activity {
                tool: None,
                tools: vec![],
                partial: false,
                preview: String::new(),
            }
        );
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["detail"], serde_json::json!({}));
        assert_eq!(v["agent"], Value::Null);
        assert!(v.get("agent_id").is_none(), "{v}");
    }

    /// 계약 §13.2: `agent_id`와 activity의 `tools`가 없던 옛 로그 줄도 그대로 재생된다.
    #[test]
    fn old_lines_without_agent_id_or_tools_replay() {
        let old = r#"{"id":"1-0","ts_ms":1,"source":"claude-code","agent":"claude-code","session":"s","cwd":"/w","pid_hint":3,"kind":"activity","raw_event":"PostToolBatch","detail":{}}"#;
        let ev: Event = serde_json::from_str(old).unwrap();
        assert_eq!(ev.agent_id, None);
        assert_eq!(
            ev.body,
            EventBody::Activity {
                tool: None,
                tools: vec![],
                partial: false,
                preview: String::new(),
            }
        );
        // 옛 줄은 다시 써도 같은 모양(새 필드는 값이 있을 때만 쓴다)
        assert_eq!(
            serde_json::to_value(&ev).unwrap(),
            serde_json::from_str::<Value>(old).unwrap()
        );
        let new = r#"{"id":"1-0","ts_ms":1,"source":"claude-code","agent":"claude-code","agent_id":"a1","session":"s","cwd":null,"pid_hint":null,"kind":"activity","raw_event":"PostToolBatch","detail":{"tools":["Read","Bash"]}}"#;
        let ev: Event = serde_json::from_str(new).unwrap();
        assert_eq!(ev.agent_id.as_deref(), Some("a1"));
        assert_eq!(
            ev.body,
            EventBody::Activity {
                tool: None,
                tools: vec!["Read".into(), "Bash".into()],
                partial: false,
                preview: String::new(),
            }
        );
        assert_eq!(
            serde_json::to_value(&ev).unwrap(),
            serde_json::from_str::<Value>(new).unwrap()
        );
    }

    #[test]
    fn unknown_kind_is_error() {
        let r: Result<Event, _> =
            serde_json::from_str(r#"{"ts_ms":1,"source":"codex","kind":"nope","detail":{}}"#);
        assert!(r.is_err());
    }

    #[test]
    fn source_names() {
        for s in Source::ALL {
            assert_eq!(Source::parse(s.as_str()), Some(s));
            let j = serde_json::to_string(&s).unwrap();
            assert_eq!(j, format!("\"{}\"", s.as_str()));
        }
        assert_eq!(Source::parse("nope"), None);
        assert!(!Source::Gyeot.is_ingestible());
        assert!(Source::Custom.is_ingestible());
    }

    #[test]
    fn every_kind_roundtrips() {
        let bodies = vec![
            EventBody::SessionStart {
                source_kind: "startup".into(),
                model: Some("m".into()),
                title: None,
            },
            EventBody::SessionEnd {
                reason: "exit".into(),
            },
            EventBody::Prompt { chars: 3 },
            EventBody::WaitingPermission {
                title: None,
                message: "m".into(),
            },
            EventBody::PermissionRequest {
                tool: "Bash".into(),
                preview: "ls".into(),
            },
            EventBody::WaitingInput {
                notification_type: "idle_prompt".into(),
                message: "m".into(),
            },
            EventBody::TurnDone {
                last_message: "x".into(),
                asks_question: true,
                human_followup: false,
            },
            EventBody::TurnFailed {
                error_type: "rate_limit".into(),
            },
            EventBody::ToolFailed {
                tool: "Bash".into(),
                error: "e".into(),
                preview: String::new(),
            },
            EventBody::ToolFailed {
                tool: "Bash".into(),
                error: "e".into(),
                preview: "ls".into(),
            },
            EventBody::Activity {
                tool: Some("Read".into()),
                tools: vec![],
                partial: false,
                preview: String::new(),
            },
            EventBody::Activity {
                tool: None,
                tools: vec!["Read".into(), "Bash".into(), "Read".into()],
                partial: true,
                preview: String::new(),
            },
            EventBody::Activity {
                tool: Some("Bash".into()),
                tools: vec![],
                partial: false,
                preview: "npm test".into(),
            },
            EventBody::SubagentStart { agent_type: None },
            EventBody::SubagentStop {
                agent_type: Some("x".into()),
            },
            EventBody::TaskDone {
                subject: "s".into(),
            },
            EventBody::ContextCompact {
                trigger: "auto".into(),
            },
            EventBody::Gate {
                result: "pass".into(),
                harness: "fsmap".into(),
                event: "gate".into(),
            },
            EventBody::Notice {
                notification_type: None,
                message: "m".into(),
            },
            EventBody::UserAction {
                item: "it_1".into(),
                action: "dismiss".into(),
                reason: Some("과민".into()),
                minutes: None,
            },
            EventBody::ChannelOpen {
                cid: "ch_1".into(),
                pid: 1,
                ppid: 2,
                cwd: "/w".into(),
            },
            EventBody::ChannelClose {
                cid: "ch_1".into(),
                pid: 1,
                ppid: 2,
                cwd: "/w".into(),
            },
            EventBody::DecisionOpen {
                decision_id: "dc_1".into(),
                question: "q".into(),
                options: vec!["a".into(), "b".into()],
            },
        ];
        for (i, body) in bodies.into_iter().enumerate() {
            let ev = Event {
                id: "1-0".into(),
                ts_ms: 1,
                source: Source::Gyeot,
                agent: None,
                agent_id: (i % 2 == 0).then(|| format!("a{i}")),
                session: Some("s".into()),
                cwd: None,
                pid_hint: None,
                raw_event: None,
                body,
            };
            let s = serde_json::to_string(&ev).unwrap();
            let back: Event = serde_json::from_str(&s).unwrap();
            assert_eq!(back, ev, "{s}");
            let v: Value = serde_json::from_str(&s).unwrap();
            assert_eq!(v["kind"], ev.kind());
        }
    }
}
