//! 훅 심이 데몬에 보내거나 스풀에 쓰기 전에 페이로드를 줄인다(계약 §12.5).
//!
//! [`normalize`](crate::normalize::normalize)가 읽는 필드만 남기고, 글은 마스킹·자르기를 거친다.
//! 정규화 결과는 원본과 같아야 한다(시험이 지킨다).

use crate::event::Source;
use crate::normalize::PREVIEW_MAX;
use crate::text::{collapse_ws, mask, truncate_chars};
use serde_json::{Map, Value};

/// 남기는 문자열의 최대 글자 수.
pub const SANITIZED_STR_MAX: usize = 4000;

/// `tool_input`에서 미리보기로 고르는 필드(정규화의 `tool_preview`와 같은 순서).
const TOOL_PREVIEW_KEYS: [&str; 9] = [
    "command",
    "file_path",
    "notebook_path",
    "path",
    "url",
    "pattern",
    "query",
    "description",
    "prompt",
];

/// `source`의 원본 페이로드를 정규화에 필요한 만큼만 남긴다.
///
/// - 식별자(`session_id`·`cwd`·`hook_event_name`·`type`·`thread-id` 등)는 그대로 둔다.
/// - `prompt`는 글자 수가 같은 `x` 문자열로 바꾼다(정규화는 길이만 쓴다).
/// - 미리보기로만 쓰는 글은 마스킹 → 공백 접기 → 4,000자 자르기. 미리보기(≤300자)는 원본과 같다.
/// - 마지막 응답(`last_assistant_message`)은 줄 구조가 질문 판단에 쓰이므로 접지 않고 마스킹만 한다.
///   4,000자가 넘으면 앞 2,000자와 끝 1,999자를 줄바꿈으로 잇는다(미리보기는 앞, 질문 판단은 끝 줄).
/// - `tool_input`은 정규화가 미리보기로 고르는 필드 하나만 남긴다. 그런 필드가 없으면 원본도
///   `tool_input` 전체 JSON을 미리보기로 쓰므로, 그 JSON을 마스킹해 미리보기 길이(300자)로 자른 글로 바꾼다.
/// - 그 밖의 필드(`transcript_path`·`tool_response`·`content`·`old_string`·`input-messages` …)는 버린다.
/// - 모르는 source·정규화가 버리는 모양은 `null`이다.
pub fn sanitize_payload(source: Source, raw: &Value) -> Value {
    match source {
        Source::ClaudeCode | Source::Codex => raw.as_object().map_or(Value::Null, hook),
        Source::CodexNotify => codex_notify(raw),
        Source::AokHarness | Source::FsmapHarness => raw.as_object().map_or(Value::Null, harness),
        Source::Custom => raw.as_object().map_or(Value::Null, custom),
        Source::Gyeot => Value::Null,
    }
}

/// 미리보기로만 쓰는 글: 마스킹 → 공백 접기 → 자르기.
fn clean(s: &str, max: usize) -> String {
    truncate_chars(&collapse_ws(&mask(s)), max)
}

/// 줄 구조를 지켜야 하는 긴 글: 마스킹만 하고, 길면 앞과 끝을 남긴다.
fn clean_message(s: &str) -> String {
    let masked = mask(s);
    let n = masked.chars().count();
    if n <= SANITIZED_STR_MAX {
        return masked;
    }
    let head_n = SANITIZED_STR_MAX / 2;
    let tail_n = SANITIZED_STR_MAX - head_n - 1;
    let mut out: String = masked.chars().take(head_n).collect();
    out.push('\n');
    out.extend(masked.chars().skip(n - tail_n));
    out
}

/// 정규화의 `text_of`와 같다(오류 객체 등을 글로).
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

fn join_array(a: &[Value]) -> String {
    a.iter()
        .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 문자열이면 그대로 옮긴다(식별자).
fn copy_str(o: &Map<String, Value>, out: &mut Map<String, Value>, key: &str) {
    if let Some(v @ Value::String(_)) = o.get(key) {
        out.insert(key.to_owned(), v.clone());
    }
}

/// 문자열이면 미리보기용으로 줄여 옮긴다.
fn copy_clean(o: &Map<String, Value>, out: &mut Map<String, Value>, key: &str) {
    if let Some(Value::String(s)) = o.get(key) {
        out.insert(key.to_owned(), Value::String(clean(s, SANITIZED_STR_MAX)));
    }
}

/// `text_of`로 읽는 필드: 글이 있으면 그 글을 줄여 문자열로 옮긴다.
fn copy_text_of(o: &Map<String, Value>, out: &mut Map<String, Value>, key: &str) {
    if let Some(t) = o.get(key).and_then(text_of) {
        out.insert(key.to_owned(), Value::String(clean(&t, SANITIZED_STR_MAX)));
    }
}

/// 줄 구조를 지키는 긴 글 필드.
fn copy_message(o: &Map<String, Value>, out: &mut Map<String, Value>, key: &str) {
    if let Some(Value::String(s)) = o.get(key) {
        out.insert(key.to_owned(), Value::String(clean_message(s)));
    }
}

/// `PermissionRequest.tool_input`: 정규화가 미리보기로 고르는 것만 남긴다.
fn tool_input(v: &Value) -> Option<Value> {
    match v {
        Value::Null => None,
        Value::String(x) => Some(Value::String(clean(x, SANITIZED_STR_MAX))),
        Value::Object(o) => {
            let picked = TOOL_PREVIEW_KEYS.iter().find_map(|k| match o.get(*k) {
                Some(Value::String(x)) if !x.trim().is_empty() => {
                    Some((*k, Value::String(clean(x, SANITIZED_STR_MAX))))
                }
                Some(Value::Array(a)) if !a.is_empty() => Some((
                    *k,
                    Value::Array(vec![Value::String(clean(
                        &join_array(a),
                        SANITIZED_STR_MAX,
                    ))]),
                )),
                _ => None,
            });
            Some(match picked {
                Some((k, v)) => {
                    let mut m = Map::new();
                    m.insert(k.to_owned(), v);
                    Value::Object(m)
                }
                None => Value::String(clean(&v.to_string(), PREVIEW_MAX)),
            })
        }
        other => Some(Value::String(clean(&other.to_string(), PREVIEW_MAX))),
    }
}

/// Claude Code·Codex 훅(계약 §2.1): 이벤트마다 정규화가 읽는 필드만.
fn hook(o: &Map<String, Value>) -> Value {
    let mut out = Map::new();
    for k in ["session_id", "cwd", "hook_event_name"] {
        copy_str(o, &mut out, k);
    }
    let name = o
        .get("hook_event_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let out_ref = &mut out;
    match name {
        "SessionStart" => {
            copy_clean(o, out_ref, "source");
            copy_clean(o, out_ref, "title");
            copy_clean(o, out_ref, "session_title");
            match o.get("model") {
                Some(Value::String(m)) => {
                    out_ref.insert("model".into(), Value::String(clean(m, SANITIZED_STR_MAX)));
                }
                Some(Value::Object(m)) => {
                    let mut mm = Map::new();
                    for k in ["display_name", "id", "name"] {
                        copy_clean(m, &mut mm, k);
                    }
                    out_ref.insert("model".into(), Value::Object(mm));
                }
                _ => {}
            }
        }
        "SessionEnd" => copy_clean(o, out_ref, "reason"),
        "UserPromptSubmit" => {
            if let Some(Value::String(p)) = o.get("prompt") {
                out_ref.insert(
                    "prompt".into(),
                    Value::String("x".repeat(p.chars().count())),
                );
            }
        }
        "Notification" => {
            for k in ["notification_type", "message", "title"] {
                copy_clean(o, out_ref, k);
            }
        }
        "PermissionRequest" => {
            copy_clean(o, out_ref, "tool_name");
            if let Some(t) = o.get("tool_input").and_then(tool_input) {
                out_ref.insert("tool_input".into(), t);
            }
        }
        "Stop" => copy_message(o, out_ref, "last_assistant_message"),
        "SubagentStart" | "SubagentStop" => copy_clean(o, out_ref, "agent_type"),
        "StopFailure" => {
            for k in ["error_type", "error", "reason", "matcher"] {
                copy_text_of(o, out_ref, k);
            }
        }
        "PostToolUseFailure" => {
            copy_clean(o, out_ref, "tool_name");
            copy_text_of(o, out_ref, "error");
        }
        "PreToolUse" | "PostToolUse" | "PostToolBatch" | "UserPromptExpansion" => {
            copy_clean(o, out_ref, "tool_name");
        }
        "TaskCompleted" => {
            copy_clean(o, out_ref, "task_subject");
            copy_clean(o, out_ref, "subject");
        }
        "PreCompact" => {
            copy_clean(o, out_ref, "trigger");
            copy_clean(o, out_ref, "matcher");
        }
        _ => {}
    }
    Value::Object(out)
}

/// Codex `notify`(계약 §2.2). JSON 문자열로 감싸 온 것은 풀어서 객체로 남긴다.
fn codex_notify(raw: &Value) -> Value {
    let parsed;
    let raw = match raw {
        Value::String(x) => match serde_json::from_str::<Value>(x) {
            Ok(v) => {
                parsed = v;
                &parsed
            }
            Err(_) => return Value::Null,
        },
        other => other,
    };
    let Some(o) = raw.as_object() else {
        return Value::Null;
    };
    let mut out = Map::new();
    for k in ["type", "thread-id", "session_id", "turn-id", "cwd"] {
        copy_str(o, &mut out, k);
    }
    copy_message(o, &mut out, "last-assistant-message");
    copy_text_of(o, &mut out, "message");
    match o.get("command") {
        Some(Value::Array(a)) if a.is_empty() => {
            out.insert("command".into(), Value::Array(Vec::new()));
        }
        Some(Value::Array(a)) => {
            out.insert(
                "command".into(),
                Value::Array(vec![Value::String(clean(
                    &join_array(a),
                    SANITIZED_STR_MAX,
                ))]),
            );
        }
        Some(_) => copy_text_of(o, &mut out, "command"),
        None => {}
    }
    Value::Object(out)
}

/// 하네스 로그 줄(계약 §2.3).
fn harness(o: &Map<String, Value>) -> Value {
    let mut out = Map::new();
    // `event`는 원래 이름(raw_event)으로 그대로 남으므로 고치지 않는다.
    for k in ["event", "session", "cwd"] {
        copy_str(o, &mut out, k);
    }
    copy_clean(o, &mut out, "result");
    if let Some(t @ Value::Number(_)) = o.get("t") {
        out.insert("t".into(), t.clone());
    }
    Value::Object(out)
}

fn clean_deep(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(clean(s, SANITIZED_STR_MAX)),
        Value::Array(a) => Value::Array(a.iter().map(clean_deep).collect()),
        Value::Object(o) => {
            Value::Object(o.iter().map(|(k, x)| (k.clone(), clean_deep(x))).collect())
        }
        other => other.clone(),
    }
}

/// `custom`(이미 정규화된 꼴): 글자 필드는 정규화도 미리보기로만 쓴다.
fn custom(o: &Map<String, Value>) -> Value {
    let mut out = Map::new();
    for k in ["kind", "session", "cwd"] {
        copy_str(o, &mut out, k);
    }
    if let Some(a) = o.get("agent")
        && serde_json::from_value::<crate::event::Agent>(a.clone()).is_ok()
    {
        out.insert("agent".into(), a.clone());
    }
    if let Some(d) = o.get("detail") {
        out.insert("detail".into(), clean_deep(d));
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::normalize;
    use serde_json::json;

    const T: i64 = 1_764_126_720_123;

    fn hook(name: &str, extra: Value) -> Value {
        let mut v = json!({
            "session_id": "abc123",
            "transcript_path": "/home/me/.claude/projects/x/abc123.jsonl",
            "cwd": "C:\\work\\proj",
            "hook_event_name": name,
            "permission_mode": "default"
        });
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            for (k, x) in e {
                o.insert(k.clone(), x.clone());
            }
        }
        v
    }

    fn walk_strings<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
        match v {
            Value::String(s) => out.push(s),
            Value::Array(a) => a.iter().for_each(|x| walk_strings(x, out)),
            Value::Object(o) => o.values().for_each(|x| walk_strings(x, out)),
            _ => {}
        }
    }

    const SECRET: &str = "sk-ant-api03-SECRETSECRETSECRET123";
    const BEARER: &str = "Bearer abcdefghijklmnopqrstuvwxyz0123456789";

    fn representative() -> Vec<(Source, Value)> {
        let long_cmd = format!("curl -H 'Authorization: {BEARER}' {}", "x".repeat(6000));
        let long_msg = format!(
            "{}\n중간 {}\n\n이대로 병합할까요?",
            "사용자 확인 필요. 앞부분 설명 ".repeat(10),
            "가".repeat(9000)
        );
        let big = "본문 ".repeat(50_000);
        let cc = Source::ClaudeCode;
        let mut v = vec![
            (
                cc,
                hook(
                    "SessionStart",
                    json!({"source": "resume", "model": {"id": "m1", "display_name": "Opus"}, "title": "  제목\n  "}),
                ),
            ),
            (
                cc,
                hook("SessionStart", json!({"source": " ", "model": "  "})),
            ),
            (
                cc,
                hook(
                    "SessionStart",
                    json!({"model": {"display_name": "", "id": "x"}, "session_title": "t"}),
                ),
            ),
            (
                cc,
                hook("SessionEnd", json!({"reason": "prompt_input_exit"})),
            ),
            (cc, hook("SessionEnd", json!({"reason": {"x": 1}}))),
            (
                cc,
                hook(
                    "UserPromptSubmit",
                    json!({"prompt": format!("비밀 {SECRET} 내용\n둘째 줄")}),
                ),
            ),
            (cc, hook("UserPromptSubmit", json!({"prompt": 5}))),
            (
                cc,
                hook(
                    "Notification",
                    json!({"notification_type": "permission_prompt", "title": "Permission needed", "message": format!("Claude needs your permission {SECRET}")}),
                ),
            ),
            (
                cc,
                hook(
                    "Notification",
                    json!({"notification_type": "idle_prompt", "message": "Claude is waiting for your input"}),
                ),
            ),
            (
                cc,
                hook(
                    "Notification",
                    json!({"notification_type": "quota_warning", "message": "m"}),
                ),
            ),
            (
                cc,
                hook(
                    "Notification",
                    json!({"message": "Claude needs your permission to use Bash"}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "Bash", "tool_input": {"command": "npm   install\n --save-dev vitest", "description": "install"}}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "Bash", "tool_input": {"command": long_cmd, "description": "긴 명령", "timeout": 5}}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "Bash", "tool_input": {"command": "git -C . push origin main"}}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "Edit", "tool_input": {"file_path": "C:\\work\\proj\\a.rs", "old_string": "비밀 옛 코드", "new_string": "비밀 새 코드"}}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "Write", "tool_input": {"file_path": "/w/a.txt", "content": big}}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "Task", "tool_input": {"description": "탐색", "prompt": "비밀 하위 프롬프트", "subagent_type": "Explore"}}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "mcp__x__y", "tool_input": {"a": 1, "content": format!("내용 {SECRET}")}}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "mcp__x__z", "tool_input": {"blob": "y".repeat(9000)}}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "Bash", "tool_input": {"command": "", "file_path": "/w/b"}}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "T", "tool_input": {"command": [], "path": "/w/c"}}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "T", "tool_input": {"command": ["git", "push", 1]}}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "T", "tool_input": {"command": [""], "path": "/w/d"}}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "T", "tool_input": {"command": 5, "url": "https://user:pw@example.com/x"}}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "T", "tool_input": "rm -rf build"}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "T", "tool_input": [1, 2]}),
                ),
            ),
            (
                cc,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "T", "tool_input": null}),
                ),
            ),
            (cc, hook("PermissionRequest", json!({}))),
            (
                cc,
                hook(
                    "Stop",
                    json!({"last_assistant_message": "다 했어요.\n\n이대로 병합할까요?", "stop_hook_active": false}),
                ),
            ),
            (
                cc,
                hook(
                    "Stop",
                    json!({"last_assistant_message": format!("PR을 열었어요. 키 {SECRET}. 리뷰는 직접 해 주세요.")}),
                ),
            ),
            (
                cc,
                hook("Stop", json!({"last_assistant_message": long_msg})),
            ),
            (cc, hook("Stop", json!({"last_assistant_message": 3}))),
            (cc, hook("Stop", json!({}))),
            (cc, hook("SubagentStart", json!({"agent_type": "Explore"}))),
            (cc, hook("SubagentStop", json!({"agent_type": "  "}))),
            (cc, hook("StopFailure", json!({"error_type": "rate_limit"}))),
            (
                cc,
                hook(
                    "StopFailure",
                    json!({"error": {"type": "authentication_failed", "detail": SECRET}}),
                ),
            ),
            (
                cc,
                hook("StopFailure", json!({"error": {"detail": "no known key"}})),
            ),
            (
                cc,
                hook(
                    "StopFailure",
                    json!({"error_type": "  ", "reason": "overloaded"}),
                ),
            ),
            (cc, hook("StopFailure", json!({"matcher": "server_error"}))),
            (cc, hook("StopFailure", json!({"error": 500}))),
            (cc, hook("StopFailure", json!({}))),
            (
                cc,
                hook(
                    "PostToolUseFailure",
                    json!({"tool_name": "Bash", "error": format!("{} {SECRET}", "e".repeat(5000)), "tool_input": {"command": "x"}}),
                ),
            ),
            (
                cc,
                hook(
                    "PostToolUseFailure",
                    json!({"tool_name": "Bash", "error": {"message": "boom"}}),
                ),
            ),
            (
                cc,
                hook(
                    "PreToolUse",
                    json!({"tool_name": "Write", "tool_input": {"file_path": "/w/a", "content": "비밀 내용"}}),
                ),
            ),
            (
                cc,
                hook(
                    "PostToolUse",
                    json!({"tool_name": "Read", "tool_response": {"content": "비밀 파일 내용"}}),
                ),
            ),
            // 공식 hooks 레퍼런스의 모양: tool_calls[]에 도구마다 tool_input·tool_response가 온다.
            (
                cc,
                hook(
                    "PostToolBatch",
                    json!({"tool_calls": [
                        {"tool_name": "Read", "tool_use_id": "toolu_1",
                         "tool_input": {"file_path": "/w/.env"},
                         "tool_response": "1\t비밀 파일 내용"},
                        {"tool_name": "Bash", "tool_use_id": "toolu_2",
                         "tool_input": {"command": "echo 비밀"},
                         "tool_response": [{"type": "text", "text": "비밀 출력"}]}
                    ]}),
                ),
            ),
            (cc, hook("PostToolBatch", json!({}))),
            (
                cc,
                hook(
                    "UserPromptExpansion",
                    json!({"tool_name": "Skill", "prompt": "비밀"}),
                ),
            ),
            (
                cc,
                hook(
                    "TaskCompleted",
                    json!({"task_id": "1", "task_subject": "시험 고치기", "task_description": "비밀 설명"}),
                ),
            ),
            (cc, hook("TaskCompleted", json!({"subject": "다른 이름"}))),
            (
                cc,
                hook(
                    "PreCompact",
                    json!({"trigger": "auto", "custom_instructions": "비밀"}),
                ),
            ),
            (cc, hook("PreCompact", json!({"matcher": "manual"}))),
            (cc, hook("SomethingNew", json!({"x": "비밀"}))),
            (cc, json!({"session_id": "x", "prompt": "비밀"})),
            (cc, json!("not an object")),
            (cc, json!([1, 2])),
            (
                Source::Codex,
                hook("Stop", json!({"last_assistant_message": "끝"})),
            ),
            (
                Source::Codex,
                hook(
                    "PermissionRequest",
                    json!({"tool_name": "shell", "tool_input": {"command": ["git", "merge", "topic"]}}),
                ),
            ),
        ];
        let cn = Source::CodexNotify;
        v.extend([
            (cn, json!({"type": "agent-turn-complete", "thread-id": "th-1", "turn-id": "tu-1", "cwd": "/w/p", "input-messages": ["비밀 프롬프트"], "last-assistant-message": "어느 쪽으로 할까요?"})),
            (cn, json!({"type": "approval-requested", "session_id": "s-2", "turn-id": "t", "command": ["curl", "-H", BEARER, "https://x"]})),
            (cn, json!({"type": "approval-requested", "turn-id": "only-turn"})),
            (cn, json!({"type": "approval-requested", "thread-id": "c", "command": []})),
            (cn, json!({"type": "approval-requested", "thread-id": "c", "command": "git push"})),
            (cn, json!({"type": "approval-requested", "thread-id": "c", "command": null, "message": {"message": "승인해 주세요"}})),
            (cn, json!({"type": "approval-requested", "thread-id": "c", "message": "  "})),
            (cn, Value::String(r#"{"type":"agent-turn-complete","thread-id":"x","last-assistant-message":"ok","input-messages":["비밀"]}"#.into())),
            (cn, json!("not json")),
            (cn, json!({"type": "other", "x": "비밀"})),
            (Source::Custom, json!({"kind": "turn_failed", "session": "c1", "cwd": "/w", "agent": "codex", "detail": {"error_type": format!("boom {SECRET}")}})),
            (Source::Custom, json!({"kind": "turn_done", "session": "c1", "detail": {"last_message": "a\n\nb?", "asks_question": true, "human_followup": false}})),
            (Source::Custom, json!({"kind": "user_action", "detail": {"item": "x", "action": "done"}})),
            (Source::Custom, json!({"kind": "nope"})),
            (Source::AokHarness, json!({"t": 1764126700.5, "event": "stop", "result": "fail", "cwd": "C:/work/proj"})),
            (Source::FsmapHarness, json!({"t": 1764126700, "event": "gate", "session": "abc123", "result": "block", "ms": 812})),
            (Source::Gyeot, hook("Stop", json!({}))),
        ]);
        v
    }

    #[test]
    fn normalization_is_unchanged() {
        for (src, raw) in representative() {
            let clean = sanitize_payload(src, &raw);
            assert_eq!(
                normalize(src, &raw, T, Some(7)),
                normalize(src, &clean, T, Some(7)),
                "{src:?} {raw}\n→ {clean}"
            );
            // 두 번 줄여도 같다(스풀에서 다시 보내도 같은 모양)
            assert_eq!(sanitize_payload(src, &clean), clean, "{src:?} {raw}");
        }
    }

    #[test]
    fn keeps_secrets_and_bodies_out() {
        for (src, raw) in representative() {
            let clean = sanitize_payload(src, &raw);
            let text = clean.to_string();
            for bad in [
                "비밀",
                SECRET,
                "abcdefghijklmnopqrstuvwxyz0123456789",
                "transcript_path",
                "permission_mode",
                "user:pw@",
            ] {
                assert!(!text.contains(bad), "{bad} 남음: {src:?} {text}");
            }
            let mut strs = Vec::new();
            walk_strings(&clean, &mut strs);
            for s in strs {
                assert!(
                    s.chars().count() <= SANITIZED_STR_MAX,
                    "{} > 4000: {src:?}",
                    s.chars().count()
                );
            }
        }
    }

    #[test]
    fn prompt_becomes_x_of_same_length_and_tool_input_is_trimmed() {
        let raw = hook(
            "UserPromptSubmit",
            json!({"prompt": "비밀 프롬프트\n둘째 줄"}),
        );
        let clean = sanitize_payload(Source::ClaudeCode, &raw);
        assert_eq!(
            clean,
            json!({
                "session_id": "abc123",
                "cwd": "C:\\work\\proj",
                "hook_event_name": "UserPromptSubmit",
                "prompt": "x".repeat(12),
            })
        );
        // 쓰는 요약 필드 하나만 남기고(마스킹·자름) 나머지 tool_input은 버린다
        let raw = hook(
            "PermissionRequest",
            json!({"tool_name": "Edit", "tool_input": {"file_path": "/w/a.rs", "old_string": "x", "new_string": "y", "replace_all": true}}),
        );
        let clean = sanitize_payload(Source::ClaudeCode, &raw);
        assert_eq!(clean["tool_input"], json!({"file_path": "/w/a.rs"}));
        assert_eq!(clean["tool_name"], "Edit");
        let raw = hook(
            "PermissionRequest",
            json!({"tool_name": "Bash", "tool_input": {"command": format!("echo {BEARER}"), "description": "d"}}),
        );
        let clean = sanitize_payload(Source::ClaudeCode, &raw);
        assert_eq!(
            clean["tool_input"],
            json!({"command": "echo Bearer [가림]"})
        );
        // 작업 신호에서는 tool_input을 쓰지 않으므로 통째로 버린다
        let raw = hook(
            "PreToolUse",
            json!({"tool_name": "Bash", "tool_input": {"command": "ls"}}),
        );
        let clean = sanitize_payload(Source::ClaudeCode, &raw);
        assert!(clean.get("tool_input").is_none(), "{clean}");
        // 긴 마지막 응답은 앞과 끝을 남긴다(질문 판단은 마지막 줄)
        let long = format!("첫 줄\n{}\n어떻게 할까요?", "가".repeat(9000));
        let raw = hook("Stop", json!({ "last_assistant_message": long }));
        let clean = sanitize_payload(Source::ClaudeCode, &raw);
        let m = clean["last_assistant_message"].as_str().unwrap();
        assert_eq!(m.chars().count(), SANITIZED_STR_MAX);
        assert!(m.starts_with("첫 줄\n"));
        assert!(m.ends_with("\n어떻게 할까요?"));
    }
}
