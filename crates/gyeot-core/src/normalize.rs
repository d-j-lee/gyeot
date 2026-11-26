//! 원본 페이로드 → 정규화 이벤트(계약 §2.1–§2.3). 모르는 이벤트는 버린다.

use crate::event::{Agent, Event, EventBody, Source, body_from_parts};
use crate::text::{asks_question, collapse_ws, human_followup, mask, preview, truncate_chars};
use serde_json::{Map, Value};

/// 미리보기 길이(계약 §2).
pub const PREVIEW_MAX: usize = 300;
/// `turn_done.last_message` 길이.
pub const LAST_MESSAGE_MAX: usize = 280;
/// 짧은 필드(오류·할 일 제목) 길이.
pub const SHORT_MAX: usize = 120;
/// 하위 에이전트 id 길이(계약 §13.2).
pub const AGENT_ID_MAX: usize = 128;
/// `PostToolBatch`에서 보는 도구 호출 수(계약 §13.2).
pub const TOOL_CALLS_MAX: usize = 64;
/// 도구 이름 길이.
const TOOL_NAME_MAX: usize = 80;

/// 훅 입력의 `agent_id`: 앞뒤 공백을 지우고 128자로 자른다(다시 해도 같다). 비었거나 글이 아니면 `None`.
pub fn agent_id_of(o: &Map<String, Value>) -> Option<String> {
    let t = o.get("agent_id")?.as_str()?.trim();
    let cut: String = t.chars().take(AGENT_ID_MAX).collect();
    let cut = cut.trim_end();
    (!cut.is_empty()).then(|| cut.to_owned())
}

/// `tool_calls[]`에서 도구 이름이 있는(비지 않은 글) 호출만, 앞에서 64개까지의 이름.
pub fn batch_tool_names(o: &Map<String, Value>) -> Vec<&str> {
    match o.get("tool_calls") {
        Some(Value::Array(calls)) => calls
            .iter()
            .filter_map(|c| c.get("tool_name").and_then(Value::as_str))
            .filter(|n| !n.trim().is_empty())
            .take(TOOL_CALLS_MAX)
            .collect(),
        _ => Vec::new(),
    }
}

/// 도구 이름 목록: 비지 않은 이름 앞 64개를 미리보기(80자)로 바꾼다. 이름마다 요청 하나를 지우므로(§14.2)
/// 겹친 이름도 그대로 둔다(처음 순서).
pub fn tool_list<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    names
        .filter(|n| !n.trim().is_empty())
        .take(TOOL_CALLS_MAX)
        .map(|n| preview(n, TOOL_NAME_MAX))
        .collect()
}

/// 심이 잘린 입력에서 건진 페이로드인가(`"gyeot_partial": true`, §14.2).
pub fn is_partial(o: &Map<String, Value>) -> bool {
    o.get("gyeot_partial").and_then(Value::as_bool) == Some(true)
}

/// 원본 페이로드를 정규화한다. 버릴 것이면 빈 목록이다.
///
/// - `received_ms`: 데몬이 받은 시각(심의 봉투에 있으면 그 값). 하네스 로그는 줄의 `t`가 더 이르면 그것을 쓴다.
/// - `pid_hint`: 훅을 띄운 프로세스의 PID.
/// - 하네스 로그 줄에는 세션이 없을 수 있다. 따라 읽는 쪽이 `cwd` 필드를 넣어 주면 그 저장소로 세션을 찾는다.
pub fn normalize(
    source: Source,
    raw: &Value,
    received_ms: i64,
    pid_hint: Option<u32>,
) -> Vec<Event> {
    let ev = match source {
        Source::ClaudeCode => hook_event(raw, Source::ClaudeCode, Agent::ClaudeCode),
        Source::Codex => hook_event(raw, Source::Codex, Agent::Codex),
        Source::CodexNotify => codex_notify(raw),
        Source::AokHarness => harness(raw, Source::AokHarness, "aok", received_ms),
        Source::FsmapHarness => harness(raw, Source::FsmapHarness, "fsmap", received_ms),
        Source::Custom => custom(raw),
        Source::Gyeot => None,
    };
    match ev {
        Some(mut e) => {
            if e.ts_ms == 0 {
                e.ts_ms = received_ms;
            }
            e.id = format!("{}-0", e.ts_ms);
            e.pid_hint = pid_hint;
            vec![e]
        }
        None => Vec::new(),
    }
}

fn s<'a>(o: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    o.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

/// 문자열이 아닌 값도 글로 바꾼다(오류 객체 등).
fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(x) if x.trim().is_empty() => None,
        Value::String(x) => Some(x.clone()),
        Value::Object(o) => ["type", "code", "message", "error"]
            .iter()
            .find_map(|k| o.get(*k).and_then(text_of))
            .or_else(|| Some(v.to_string())),
        other => Some(other.to_string()),
    }
}

fn base_event(source: Source, agent: Option<Agent>, raw_event: &str, body: EventBody) -> Event {
    Event {
        id: String::new(),
        ts_ms: 0,
        source,
        agent,
        agent_id: None,
        session: None,
        cwd: None,
        pid_hint: None,
        raw_event: Some(raw_event.to_owned()),
        body,
    }
}

/// `tool_input`에서 사람이 알아볼 요약을 고른다(명령 → 경로 → URL → …, 없으면 JSON).
fn tool_preview(input: Option<&Value>) -> String {
    let text = match input {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(x)) => x.clone(),
        Some(Value::Object(o)) => [
            "command",
            "file_path",
            "notebook_path",
            "path",
            "url",
            "pattern",
            "query",
            "description",
            "prompt",
        ]
        .iter()
        .find_map(|k| match o.get(*k) {
            Some(Value::String(x)) if !x.trim().is_empty() => Some(x.clone()),
            Some(Value::Array(a)) if !a.is_empty() => Some(
                a.iter()
                    .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        })
        .unwrap_or_else(|| Value::Object(o.clone()).to_string()),
        Some(other) => other.to_string(),
    };
    preview(&text, PREVIEW_MAX)
}

/// 턴 끝 이벤트: 마스킹한 전문으로 질문·후속을 판단한 뒤 미리보기로 접는다.
fn turn_done(message: &str) -> EventBody {
    let masked = mask(message);
    EventBody::TurnDone {
        last_message: truncate_chars(&collapse_ws(&masked), LAST_MESSAGE_MAX),
        asks_question: asks_question(&masked),
        human_followup: human_followup(&masked),
    }
}

fn notification(o: &Map<String, Value>) -> EventBody {
    let message = preview(s(o, "message").unwrap_or(""), PREVIEW_MAX);
    let title = s(o, "title").map(|t| preview(t, SHORT_MAX));
    match s(o, "notification_type") {
        Some("permission_prompt") => EventBody::WaitingPermission { title, message },
        Some(
            t @ ("idle_prompt"
            | "agent_needs_input"
            | "elicitation_dialog"
            | "elicitation_url_dialog"),
        ) => EventBody::WaitingInput {
            notification_type: t.to_owned(),
            message,
        },
        Some(t) => EventBody::Notice {
            notification_type: Some(preview(t, 60)),
            message,
        },
        None => {
            // 옛 판(notification_type 없음): 문구로 짐작한다.
            let lower = message.to_lowercase();
            if lower.contains("permission") {
                EventBody::WaitingPermission { title, message }
            } else if lower.contains("waiting for your input") {
                EventBody::WaitingInput {
                    notification_type: "idle_prompt".to_owned(),
                    message,
                }
            } else {
                EventBody::Notice {
                    notification_type: None,
                    message,
                }
            }
        }
    }
}

fn model_of(o: &Map<String, Value>) -> Option<String> {
    match o.get("model") {
        Some(Value::String(m)) if !m.trim().is_empty() => Some(preview(m, 80)),
        Some(Value::Object(m)) => ["display_name", "id", "name"]
            .iter()
            .find_map(|k| m.get(*k).and_then(Value::as_str))
            .map(|x| preview(x, 80)),
        _ => None,
    }
}

/// Claude Code·Codex 훅(계약 §2.1 표).
fn hook_event(raw: &Value, source: Source, agent: Agent) -> Option<Event> {
    let o = raw.as_object()?;
    let name = s(o, "hook_event_name")?;
    let body = match name {
        "SessionStart" => EventBody::SessionStart {
            source_kind: preview(s(o, "source").unwrap_or("startup"), 40),
            model: model_of(o),
            title: s(o, "title")
                .or_else(|| s(o, "session_title"))
                .map(|t| preview(t, SHORT_MAX)),
        },
        "SessionEnd" => EventBody::SessionEnd {
            reason: preview(s(o, "reason").unwrap_or("other"), 80),
        },
        "UserPromptSubmit" => EventBody::Prompt {
            chars: o
                .get("prompt")
                .and_then(Value::as_str)
                .map_or(0, |p| p.chars().count() as u64),
        },
        "Notification" => notification(o),
        "PermissionRequest" => EventBody::PermissionRequest {
            tool: preview(s(o, "tool_name").unwrap_or("도구"), 80),
            preview: tool_preview(o.get("tool_input")),
        },
        "Stop" => turn_done(
            o.get("last_assistant_message")
                .and_then(Value::as_str)
                .unwrap_or(""),
        ),
        "SubagentStop" => EventBody::SubagentStop {
            agent_type: s(o, "agent_type").map(|t| preview(t, 80)),
        },
        "SubagentStart" => EventBody::SubagentStart {
            agent_type: s(o, "agent_type").map(|t| preview(t, 80)),
        },
        "StopFailure" => EventBody::TurnFailed {
            error_type: preview(
                &["error_type", "error", "reason", "matcher"]
                    .iter()
                    .find_map(|k| o.get(*k).and_then(text_of))
                    .unwrap_or_else(|| "unknown".to_owned()),
                SHORT_MAX,
            ),
        },
        "PostToolUseFailure" => EventBody::ToolFailed {
            tool: preview(s(o, "tool_name").unwrap_or("도구"), 80),
            error: preview(
                &o.get("error").and_then(text_of).unwrap_or_default(),
                SHORT_MAX,
            ),
            // 승인 요청과 같은 식의 미리보기(같은 호출인지 맞춰 본다, §14.2).
            preview: tool_preview(o.get("tool_input")),
        },
        "PreToolUse" | "UserPromptExpansion" => EventBody::Activity {
            tool: s(o, "tool_name").map(|t| preview(t, TOOL_NAME_MAX)),
            tools: Vec::new(),
            partial: is_partial(o),
            preview: String::new(),
        },
        // 끝낸 호출의 미리보기도 싣는다(같은 이름의 다른 호출과 가린다, §15.3).
        "PostToolUse" => EventBody::Activity {
            tool: s(o, "tool_name").map(|t| preview(t, TOOL_NAME_MAX)),
            tools: Vec::new(),
            partial: is_partial(o),
            preview: tool_preview(o.get("tool_input")),
        },
        // 묶음의 도구 이름들(§13.2). 목록이 없으면 '모르는 도구'다(G-016).
        "PostToolBatch" => EventBody::Activity {
            tool: s(o, "tool_name").map(|t| preview(t, TOOL_NAME_MAX)),
            tools: tool_list(batch_tool_names(o).into_iter()),
            partial: is_partial(o),
            preview: String::new(),
        },
        "TaskCompleted" => EventBody::TaskDone {
            subject: preview(
                s(o, "task_subject")
                    .or_else(|| s(o, "subject"))
                    .unwrap_or(""),
                SHORT_MAX,
            ),
        },
        "PreCompact" => EventBody::ContextCompact {
            trigger: preview(
                s(o, "trigger")
                    .or_else(|| s(o, "matcher"))
                    .unwrap_or("manual"),
                40,
            ),
        },
        _ => return None,
    };
    let mut ev = base_event(source, Some(agent), name, body);
    ev.agent_id = agent_id_of(o);
    ev.session = s(o, "session_id").map(str::to_owned);
    ev.cwd = s(o, "cwd").map(str::to_owned);
    Some(ev)
}

/// Codex `notify`(계약 §2.2). JSON 문자열로 감싸 온 것도 받는다.
fn codex_notify(raw: &Value) -> Option<Event> {
    let parsed;
    let raw = match raw {
        Value::String(x) => {
            parsed = serde_json::from_str::<Value>(x).ok()?;
            &parsed
        }
        other => other,
    };
    let o = raw.as_object()?;
    let ty = s(o, "type")?;
    let body = match ty {
        "agent-turn-complete" => turn_done(
            o.get("last-assistant-message")
                .and_then(Value::as_str)
                .unwrap_or(""),
        ),
        "approval-requested" => {
            let msg = o
                .get("message")
                .and_then(text_of)
                .or_else(|| match o.get("command") {
                    Some(Value::Array(a)) => Some(
                        a.iter()
                            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    Some(v) => text_of(v),
                    None => None,
                })
                .unwrap_or_else(|| "Codex가 승인을 기다려요".to_owned());
            EventBody::WaitingPermission {
                title: None,
                message: preview(&msg, PREVIEW_MAX),
            }
        }
        _ => return None,
    };
    let mut ev = base_event(Source::CodexNotify, Some(Agent::Codex), ty, body);
    ev.session = s(o, "thread-id")
        .or_else(|| s(o, "session_id"))
        .or_else(|| s(o, "turn-id"))
        .map(str::to_owned);
    ev.cwd = s(o, "cwd").map(str::to_owned);
    Some(ev)
}

/// AOK·fsmap 하네스 로그 줄(계약 §2.3) → `gate`.
fn harness(raw: &Value, source: Source, name: &str, received_ms: i64) -> Option<Event> {
    let o = raw.as_object()?;
    let result = s(o, "result")?;
    let event = s(o, "event").unwrap_or("gate");
    let mut ev = base_event(
        source,
        None,
        event,
        EventBody::Gate {
            result: preview(result, 60),
            harness: name.to_owned(),
            event: preview(event, 60),
        },
    );
    // 줄의 t(초)가 받은 시각보다 이르면 그것이 실제 시각이다. 미래 시각은 믿지 않는다.
    let t_ms = o
        .get("t")
        .and_then(Value::as_f64)
        .filter(|t| t.is_finite() && *t > 0.0 && *t < 1e13)
        .map(|t| (t * 1000.0).round());
    ev.ts_ms = match t_ms {
        // 범위를 위에서 확인했다.
        #[allow(clippy::cast_possible_truncation)]
        Some(t) => (t as i64).min(received_ms),
        None => received_ms,
    };
    ev.session = s(o, "session").map(str::to_owned);
    ev.cwd = s(o, "cwd").map(str::to_owned);
    Some(ev)
}

/// `custom`: 이미 정규화된 모양(`kind`, `detail`, `session?`, `cwd?`, `agent?`, `agent_id?`)을 받는다.
/// 내부 전용 종류는 받지 않고, 글자 필드는 모두 마스킹하고 자른다.
fn custom(raw: &Value) -> Option<Event> {
    let o = raw.as_object()?;
    let kind = s(o, "kind")?;
    let mut body = body_from_parts(kind, o.get("detail").cloned().unwrap_or(Value::Null)).ok()?;
    if body.is_internal() {
        return None;
    }
    body.sanitize();
    let agent = o
        .get("agent")
        .cloned()
        .and_then(|a| serde_json::from_value::<Agent>(a).ok());
    let mut ev = base_event(Source::Custom, agent, kind, body);
    ev.agent_id = agent_id_of(o);
    ev.session = s(o, "session").map(str::to_owned);
    ev.cwd = s(o, "cwd").map(str::to_owned);
    Some(ev)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const T: i64 = 1_764_126_720_123;

    fn cc(v: Value) -> Vec<Event> {
        normalize(Source::ClaudeCode, &v, T, Some(4321))
    }

    fn one(v: Value) -> Event {
        let mut evs = cc(v);
        assert_eq!(evs.len(), 1, "expected one event");
        evs.remove(0)
    }

    fn hook(name: &str, extra: Value) -> Value {
        let mut base = json!({
            "session_id": "abc123",
            "transcript_path": "/tmp/t.jsonl",
            "cwd": "C:\\work\\proj",
            "hook_event_name": name,
            "permission_mode": "default"
        });
        if let (Some(b), Some(e)) = (base.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                b.insert(k.clone(), v.clone());
            }
        }
        base
    }

    #[test]
    fn common_fields() {
        let ev = one(hook("PreToolUse", json!({"tool_name": "Bash"})));
        assert_eq!(ev.source, Source::ClaudeCode);
        assert_eq!(ev.agent, Some(Agent::ClaudeCode));
        assert_eq!(ev.session.as_deref(), Some("abc123"));
        assert_eq!(ev.cwd.as_deref(), Some("C:\\work\\proj"));
        assert_eq!(ev.pid_hint, Some(4321));
        assert_eq!(ev.ts_ms, T);
        assert_eq!(ev.id, format!("{T}-0"));
        assert_eq!(ev.raw_event.as_deref(), Some("PreToolUse"));
    }

    #[test]
    fn session_start_and_end() {
        let ev = one(hook(
            "SessionStart",
            json!({"source": "resume", "model": "claude-opus"}),
        ));
        assert_eq!(
            ev.body,
            EventBody::SessionStart {
                source_kind: "resume".into(),
                model: Some("claude-opus".into()),
                title: None
            }
        );
        let ev = one(hook(
            "SessionStart",
            json!({"source": "fork", "model": {"id": "m1", "display_name": "Opus"}}),
        ));
        assert!(
            matches!(ev.body, EventBody::SessionStart { ref source_kind, ref model, .. }
            if source_kind == "fork" && model.as_deref() == Some("Opus"))
        );
        let ev = one(hook("SessionEnd", json!({"reason": "prompt_input_exit"})));
        assert_eq!(
            ev.body,
            EventBody::SessionEnd {
                reason: "prompt_input_exit".into()
            }
        );
    }

    #[test]
    fn prompt_keeps_only_length() {
        let ev = one(hook(
            "UserPromptSubmit",
            json!({"prompt": "비밀 sk-abcdefghijklmnopqrstu 내용"}),
        ));
        assert_eq!(ev.body, EventBody::Prompt { chars: 30 });
        let s = serde_json::to_string(&ev).unwrap();
        assert!(!s.contains("비밀"));
        assert!(!s.contains("sk-"));
    }

    #[test]
    fn notification_types() {
        let ev = one(hook(
            "Notification",
            json!({"notification_type": "permission_prompt", "title": "Permission needed", "message": "Claude needs your permission to use Bash"}),
        ));
        assert_eq!(
            ev.body,
            EventBody::WaitingPermission {
                title: Some("Permission needed".into()),
                message: "Claude needs your permission to use Bash".into()
            }
        );
        for t in [
            "idle_prompt",
            "agent_needs_input",
            "elicitation_dialog",
            "elicitation_url_dialog",
        ] {
            let ev = one(hook(
                "Notification",
                json!({"notification_type": t, "message": "m"}),
            ));
            assert_eq!(
                ev.body,
                EventBody::WaitingInput {
                    notification_type: t.into(),
                    message: "m".into()
                },
                "{t}"
            );
        }
        for t in [
            "auth_success",
            "agent_completed",
            "quota_warning",
            "quota_exceeded",
            "plugin_notification",
        ] {
            let ev = one(hook(
                "Notification",
                json!({"notification_type": t, "message": "m"}),
            ));
            assert_eq!(
                ev.body,
                EventBody::Notice {
                    notification_type: Some(t.into()),
                    message: "m".into()
                },
                "{t}"
            );
        }
    }

    #[test]
    fn notification_without_type_is_inferred() {
        let ev = one(hook(
            "Notification",
            json!({"message": "Claude needs your permission to use Bash"}),
        ));
        assert!(matches!(ev.body, EventBody::WaitingPermission { .. }));
        let ev = one(hook(
            "Notification",
            json!({"message": "Claude is waiting for your input"}),
        ));
        assert!(
            matches!(ev.body, EventBody::WaitingInput { ref notification_type, .. } if notification_type == "idle_prompt")
        );
        let ev = one(hook("Notification", json!({"message": "hello"})));
        assert!(matches!(
            ev.body,
            EventBody::Notice {
                notification_type: None,
                ..
            }
        ));
    }

    #[test]
    fn permission_request_preview() {
        let ev = one(hook(
            "PermissionRequest",
            json!({"tool_name": "Bash", "tool_input": {"command": "npm   install\n --save-dev vitest", "description": "install"}}),
        ));
        assert_eq!(
            ev.body,
            EventBody::PermissionRequest {
                tool: "Bash".into(),
                preview: "npm install --save-dev vitest".into()
            }
        );
        let ev = one(hook(
            "PermissionRequest",
            json!({"tool_name": "Edit", "tool_input": {"file_path": "C:\\work\\proj\\a.rs", "old_string": "x"}}),
        ));
        assert!(
            matches!(ev.body, EventBody::PermissionRequest { ref preview, .. } if preview == "C:\\work\\proj\\a.rs")
        );
        let long = "x".repeat(500);
        let ev = one(hook(
            "PermissionRequest",
            json!({"tool_name": "Bash", "tool_input": {"command": format!("curl -H 'Authorization: Bearer abcdefghijklmnopqrstuvwxyz' {long}")}}),
        ));
        let EventBody::PermissionRequest { preview, .. } = ev.body else {
            panic!()
        };
        assert_eq!(preview.chars().count(), 300);
        assert!(preview.contains("Bearer [가림]"), "{preview}");
        let ev = one(hook(
            "PermissionRequest",
            json!({"tool_name": "mcp__x__y", "tool_input": {"a": 1}}),
        ));
        assert!(
            matches!(ev.body, EventBody::PermissionRequest { ref preview, .. } if preview == "{\"a\":1}")
        );
    }

    #[test]
    fn stop_becomes_turn_done() {
        let ev = one(hook(
            "Stop",
            json!({"last_assistant_message": "다 했어요.\n\n이대로 병합할까요?", "stop_hook_active": false}),
        ));
        assert_eq!(
            ev.body,
            EventBody::TurnDone {
                last_message: "다 했어요. 이대로 병합할까요?".into(),
                asks_question: true,
                human_followup: false
            }
        );
        let ev = one(hook(
            "Stop",
            json!({"last_assistant_message": "PR을 열었어요. 리뷰는 직접 해 주세요."}),
        ));
        assert!(matches!(
            ev.body,
            EventBody::TurnDone {
                asks_question: false,
                human_followup: true,
                ..
            }
        ));
        let ev = one(hook("Stop", json!({})));
        assert!(
            matches!(ev.body, EventBody::TurnDone { ref last_message, asks_question: false, human_followup: false } if last_message.is_empty())
        );
        let long = "가".repeat(400);
        let ev = one(hook("Stop", json!({ "last_assistant_message": long })));
        let EventBody::TurnDone { last_message, .. } = ev.body else {
            panic!()
        };
        assert_eq!(last_message.chars().count(), 280);
    }

    #[test]
    fn subagents() {
        let ev = one(hook("SubagentStart", json!({"agent_type": "Explore"})));
        assert_eq!(
            ev.body,
            EventBody::SubagentStart {
                agent_type: Some("Explore".into())
            }
        );
        let ev = one(hook("SubagentStop", json!({})));
        assert_eq!(ev.body, EventBody::SubagentStop { agent_type: None });
    }

    #[test]
    fn stop_failure_error_field() {
        for (field, val, want) in [
            ("error_type", json!("rate_limit"), "rate_limit"),
            ("error", json!("server_error"), "server_error"),
            ("reason", json!("overloaded"), "overloaded"),
            (
                "error",
                json!({"type": "authentication_failed"}),
                "authentication_failed",
            ),
        ] {
            let ev = one(hook("StopFailure", json!({ field: val })));
            assert_eq!(
                ev.body,
                EventBody::TurnFailed {
                    error_type: want.into()
                },
                "{field}"
            );
        }
        let ev = one(hook("StopFailure", json!({})));
        assert_eq!(
            ev.body,
            EventBody::TurnFailed {
                error_type: "unknown".into()
            }
        );
    }

    #[test]
    fn tool_failure() {
        let err = "e".repeat(200);
        let ev = one(hook(
            "PostToolUseFailure",
            json!({"tool_name": "Bash", "error": err, "tool_input": {"command": "x"}}),
        ));
        let EventBody::ToolFailed {
            tool,
            error,
            preview,
        } = ev.body
        else {
            panic!()
        };
        assert_eq!(tool, "Bash");
        assert_eq!(error.chars().count(), 120);
        // §14.2: 실패한 호출의 미리보기(승인 요청과 맞춰 본다)
        assert_eq!(preview, "x");
        let ev = one(hook(
            "PostToolUseFailure",
            json!({"tool_name": "WebFetch", "error": "e", "tool_input": {"url": "https://b.example/x", "prompt": "p"}}),
        ));
        assert!(
            matches!(ev.body, EventBody::ToolFailed { ref preview, .. } if preview == "https://b.example/x")
        );
        // 미리보기가 없으면 JSON에 키가 없다(옛 로그와 같은 모양)
        let ev = one(hook("PostToolUseFailure", json!({"tool_name": "Bash"})));
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["detail"], json!({"tool": "Bash", "error": ""}));
    }

    /// §15.3: `PostToolUse`의 activity는 요청과 같은 식의 미리보기를 싣는다(다른 activity는 싣지 않는다).
    #[test]
    fn post_tool_use_carries_preview() {
        let ev = one(hook(
            "PostToolUse",
            json!({"tool_name": "WebFetch", "tool_input": {"url": "https://b.example/", "prompt": "p"}, "tool_response": "r"}),
        ));
        assert_eq!(
            ev.body,
            EventBody::Activity {
                tool: Some("WebFetch".into()),
                tools: vec![],
                partial: false,
                preview: "https://b.example/".into(),
            }
        );
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(
            v["detail"],
            json!({"tool": "WebFetch", "preview": "https://b.example/"})
        );
        // 요청의 미리보기와 같은 식(명령·마스킹·300자)
        let input = json!({"command": format!("curl -H 'Authorization: Bearer abcdefghijklmnopqrstuvwxyz' {}", "x".repeat(500))});
        let post = one(hook(
            "PostToolUse",
            json!({"tool_name": "Bash", "tool_input": input}),
        ));
        let req = one(hook(
            "PermissionRequest",
            json!({"tool_name": "Bash", "tool_input": input}),
        ));
        let (
            EventBody::Activity { preview: a, .. },
            EventBody::PermissionRequest { preview: b, .. },
        ) = (post.body, req.body)
        else {
            panic!()
        };
        assert_eq!(a, b);
        assert_eq!(a.chars().count(), 300);
        // PreToolUse·묶음에는 싣지 않는다, 미리보기가 없으면 키가 없다
        for name in ["PreToolUse", "PostToolBatch", "UserPromptExpansion"] {
            let ev = one(hook(
                name,
                json!({"tool_name": "Bash", "tool_input": {"command": "ls"}}),
            ));
            assert!(
                matches!(ev.body, EventBody::Activity { ref preview, .. } if preview.is_empty()),
                "{name}"
            );
        }
        let ev = one(hook("PostToolUse", json!({"tool_name": "Read"})));
        assert_eq!(
            serde_json::to_value(&ev).unwrap()["detail"],
            json!({"tool": "Read"})
        );
    }

    /// §14.2: 심이 잘린 입력에서 건진 페이로드(`gyeot_partial`)의 activity는 `partial`이다.
    #[test]
    fn partial_payload_marks_activity() {
        let ev = one(hook(
            "PostToolBatch",
            json!({"gyeot_partial": true, "tool_name": "Bash"}),
        ));
        assert_eq!(
            ev.body,
            EventBody::Activity {
                tool: Some("Bash".into()),
                tools: vec![],
                partial: true,
                preview: String::new(),
            }
        );
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["detail"], json!({"tool": "Bash", "partial": true}));
        for extra in [
            json!({}),
            json!({"gyeot_partial": false}),
            json!({"gyeot_partial": "true"}),
        ] {
            let ev = one(hook("PostToolBatch", extra));
            assert!(matches!(
                ev.body,
                EventBody::Activity { partial: false, .. }
            ));
        }
    }

    #[test]
    fn activity_events() {
        for name in [
            "PreToolUse",
            "PostToolUse",
            "PostToolBatch",
            "UserPromptExpansion",
        ] {
            let ev = one(hook(name, json!({"tool_name": "Read"})));
            assert_eq!(
                ev.body,
                EventBody::Activity {
                    tool: Some("Read".into()),
                    tools: vec![],
                    partial: false,
                    preview: String::new(),
                },
                "{name}"
            );
        }
        let ev = one(hook("PostToolBatch", json!({})));
        assert_eq!(
            ev.body,
            EventBody::Activity {
                tool: None,
                tools: vec![],
                partial: false,
                preview: String::new(),
            }
        );
    }

    /// 계약 §13.2: 하위 에이전트의 `agent_id`를 이벤트에 싣는다(128자까지). 없으면 `None`.
    #[test]
    fn agent_id_is_carried() {
        let ev = one(hook(
            "PostToolUse",
            json!({"tool_name": "Read", "agent_id": " agent-a1 ", "agent_type": "Explore"}),
        ));
        assert_eq!(ev.agent_id.as_deref(), Some("agent-a1"));
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["agent_id"], "agent-a1");
        assert_eq!(serde_json::from_value::<Event>(v).unwrap(), ev);
        // 모든 Claude Code 이벤트에서
        for (name, extra) in [
            (
                "Notification",
                json!({"notification_type": "permission_prompt", "message": "m"}),
            ),
            (
                "PermissionRequest",
                json!({"tool_name": "Bash", "tool_input": {"command": "ls"}}),
            ),
            ("PostToolUseFailure", json!({"tool_name": "Bash"})),
            ("PostToolBatch", json!({})),
            ("SubagentStop", json!({})),
            ("Stop", json!({})),
        ] {
            let mut x = extra;
            x["agent_id"] = json!("a2");
            assert_eq!(one(hook(name, x)).agent_id.as_deref(), Some("a2"), "{name}");
        }
        // 128자까지
        let long = format!("{}  {}", "a".repeat(127), "b".repeat(50));
        let ev = one(hook("PostToolUse", json!({ "agent_id": long })));
        let id = ev.agent_id.unwrap();
        assert_eq!(id, "a".repeat(127));
        // 없거나 비었거나 글이 아니면 None, JSON에도 없다
        for extra in [json!({}), json!({"agent_id": "  "}), json!({"agent_id": 7})] {
            let ev = one(hook("PostToolUse", extra));
            assert_eq!(ev.agent_id, None);
            let v = serde_json::to_value(&ev).unwrap();
            assert!(v.get("agent_id").is_none(), "{v}");
        }
        // Codex 훅도 같은 표
        let ev = &normalize(
            Source::Codex,
            &hook("PostToolUse", json!({"agent_id": "c1"})),
            T,
            None,
        )[0];
        assert_eq!(ev.agent_id.as_deref(), Some("c1"));
    }

    /// 계약 §13.2: `PostToolBatch`의 activity는 `tool_calls[]`의 도구 이름 목록(64개까지)을 싣는다.
    #[test]
    fn post_tool_batch_lists_tools() {
        let ev = one(hook(
            "PostToolBatch",
            json!({"tool_calls": [
                {"tool_name": "Read", "tool_use_id": "t1", "tool_input": {"file_path": "/w/a"}, "tool_response": "x"},
                {"tool_name": "Bash", "tool_use_id": "t2"},
                {"tool_name": "Read", "tool_use_id": "t3"},
                {"tool_use_id": "t4"},
                {"tool_name": "  "},
                "junk"
            ]}),
        ));
        assert_eq!(
            ev.body,
            EventBody::Activity {
                tool: None,
                tools: vec!["Read".into(), "Bash".into(), "Read".into()],
                partial: false,
                preview: String::new(),
            }
        );
        // 이름마다 요청 하나를 지우므로 겹친 이름도 그대로 센다(§14.2)
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["detail"], json!({"tools": ["Read", "Bash", "Read"]}));
        // 64개까지만 본다
        let calls: Vec<Value> = (0..100)
            .map(|i| json!({"tool_name": format!("T{i}")}))
            .collect();
        let ev = one(hook("PostToolBatch", json!({ "tool_calls": calls })));
        let EventBody::Activity { tools, .. } = ev.body else {
            panic!()
        };
        assert_eq!(tools.len(), 64);
        assert_eq!(tools[63], "T63");
        // 다른 이벤트의 tool_calls는 보지 않는다
        let ev = one(hook(
            "PostToolUse",
            json!({"tool_name": "Read", "tool_calls": [{"tool_name": "Bash"}]}),
        ));
        assert_eq!(
            ev.body,
            EventBody::Activity {
                tool: Some("Read".into()),
                tools: vec![],
                partial: false,
                preview: String::new(),
            }
        );
    }

    #[test]
    fn task_completed_and_precompact() {
        let ev = one(hook(
            "TaskCompleted",
            json!({"task_id": "1", "task_subject": "시험 고치기"}),
        ));
        assert_eq!(
            ev.body,
            EventBody::TaskDone {
                subject: "시험 고치기".into()
            }
        );
        let ev = one(hook("PreCompact", json!({"trigger": "auto"})));
        assert_eq!(
            ev.body,
            EventBody::ContextCompact {
                trigger: "auto".into()
            }
        );
        let ev = one(hook("PreCompact", json!({"matcher": "manual"})));
        assert_eq!(
            ev.body,
            EventBody::ContextCompact {
                trigger: "manual".into()
            }
        );
    }

    #[test]
    fn unknown_and_malformed_are_dropped() {
        assert!(cc(hook("SomethingNew", json!({}))).is_empty());
        assert!(cc(json!({"session_id": "x"})).is_empty());
        assert!(cc(json!("not an object")).is_empty());
        assert!(cc(json!([1, 2])).is_empty());
        assert!(normalize(Source::Gyeot, &hook("Stop", json!({})), T, None).is_empty());
    }

    #[test]
    fn codex_hooks_use_same_table() {
        let v = hook("Stop", json!({"last_assistant_message": "끝"}));
        let evs = normalize(Source::Codex, &v, T, None);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].source, Source::Codex);
        assert_eq!(evs[0].agent, Some(Agent::Codex));
        assert_eq!(evs[0].kind(), "turn_done");
        let v = hook(
            "Notification",
            json!({"notification_type": "permission_prompt", "message": "m"}),
        );
        assert_eq!(
            normalize(Source::Codex, &v, T, None)[0].kind(),
            "waiting_permission"
        );
    }

    #[test]
    fn codex_notify_payloads() {
        let v = json!({
            "type": "agent-turn-complete",
            "thread-id": "th-1",
            "turn-id": "tu-1",
            "cwd": "/w/p",
            "input-messages": ["비밀 프롬프트"],
            "last-assistant-message": "어느 쪽으로 할까요?"
        });
        let evs = normalize(Source::CodexNotify, &v, T, Some(7));
        assert_eq!(evs.len(), 1);
        let ev = &evs[0];
        assert_eq!(ev.session.as_deref(), Some("th-1"));
        assert_eq!(ev.agent, Some(Agent::Codex));
        assert_eq!(ev.cwd.as_deref(), Some("/w/p"));
        assert_eq!(ev.raw_event.as_deref(), Some("agent-turn-complete"));
        assert!(matches!(
            ev.body,
            EventBody::TurnDone {
                asks_question: true,
                ..
            }
        ));
        assert!(!serde_json::to_string(ev).unwrap().contains("비밀"));

        let v = json!({"type": "approval-requested", "session_id": "s-2", "turn-id": "t", "command": ["rm", "-rf", "build"]});
        let ev = &normalize(Source::CodexNotify, &v, T, None)[0];
        assert_eq!(ev.session.as_deref(), Some("s-2"));
        assert_eq!(
            ev.body,
            EventBody::WaitingPermission {
                title: None,
                message: "rm -rf build".into()
            }
        );

        let v = json!({"type": "approval-requested", "turn-id": "only-turn"});
        let ev = &normalize(Source::CodexNotify, &v, T, None)[0];
        assert_eq!(ev.session.as_deref(), Some("only-turn"));

        // 인자를 JSON으로 읽지 못해 문자열로 감싸 온 경우
        let v = Value::String(
            r#"{"type":"agent-turn-complete","thread-id":"x","last-assistant-message":"ok"}"#
                .into(),
        );
        assert_eq!(normalize(Source::CodexNotify, &v, T, None).len(), 1);

        assert!(normalize(Source::CodexNotify, &json!({"type": "other"}), T, None).is_empty());
        assert!(normalize(Source::CodexNotify, &json!("not json"), T, None).is_empty());
    }

    #[test]
    fn aok_harness_lines() {
        let v =
            json!({"t": 1764126700.5, "event": "stop", "result": "fail", "cwd": "C:/work/proj"});
        let evs = normalize(Source::AokHarness, &v, T, None);
        assert_eq!(evs.len(), 1);
        let ev = &evs[0];
        assert_eq!(ev.ts_ms, 1_764_126_700_500);
        assert_eq!(ev.agent, None);
        assert_eq!(ev.session, None);
        assert_eq!(ev.cwd.as_deref(), Some("C:/work/proj"));
        assert_eq!(
            ev.body,
            EventBody::Gate {
                result: "fail".into(),
                harness: "aok".into(),
                event: "stop".into()
            }
        );
        for r in ["pass", "cannot", "skip-docs", "released"] {
            let v = json!({"t": 1.0, "event": "stop", "result": r});
            assert_eq!(normalize(Source::AokHarness, &v, T, None).len(), 1, "{r}");
        }
        // 결과가 없는 줄은 버린다.
        let v = json!({"t": 1.0, "event": "session-start"});
        assert!(normalize(Source::AokHarness, &v, T, None).is_empty());
        // 미래 시각은 받은 시각으로 묶는다.
        let v = json!({"t": 9_999_999_999.0, "event": "stop", "result": "pass"});
        assert_eq!(normalize(Source::AokHarness, &v, T, None)[0].ts_ms, T);
    }

    #[test]
    fn fsmap_harness_lines() {
        let v = json!({"t": 1764126700, "event": "gate", "session": "abc123", "result": "block", "ms": 812});
        let evs = normalize(Source::FsmapHarness, &v, T, None);
        assert_eq!(evs.len(), 1);
        let ev = &evs[0];
        assert_eq!(ev.ts_ms, 1_764_126_700_000);
        assert_eq!(ev.session.as_deref(), Some("abc123"));
        assert_eq!(ev.source, Source::FsmapHarness);
        assert_eq!(
            ev.body,
            EventBody::Gate {
                result: "block".into(),
                harness: "fsmap".into(),
                event: "gate".into()
            }
        );
        for r in [
            "pass",
            "released",
            "unchanged",
            "skip-background",
            "push-error",
            "push-uncommitted",
            "push-bypass",
        ] {
            let v = json!({"t": 1, "event": "gate", "session": "s", "result": r, "ms": 1});
            assert_eq!(normalize(Source::FsmapHarness, &v, T, None).len(), 1, "{r}");
        }
    }

    #[test]
    fn custom_events() {
        let v = json!({"kind": "turn_failed", "session": "c1", "cwd": "/w", "agent": "codex", "detail": {"error_type": "boom sk-abcdefghijklmnopqrstu"}});
        let evs = normalize(Source::Custom, &v, T, None);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].agent, Some(Agent::Codex));
        assert_eq!(
            evs[0].body,
            EventBody::TurnFailed {
                error_type: "boom [가림]".into()
            }
        );
        let v = json!({"kind": "user_action", "detail": {"item": "x", "action": "done"}});
        assert!(normalize(Source::Custom, &v, T, None).is_empty());
        let v = json!({"kind": "nope"});
        assert!(normalize(Source::Custom, &v, T, None).is_empty());
    }
}
