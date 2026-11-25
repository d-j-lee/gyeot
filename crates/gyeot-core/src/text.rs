//! 문자열 도우미: 마스킹(계약 §2.4), 접기·자르기, 질문·사람 후속 감지(§2.1), 병합 명령 감지(§3), 경로.

use regex::Regex;
use std::sync::OnceLock;

/// 가린 자리에 넣는 표시.
pub const MASK: &str = "[가림]";

struct Masker {
    private_key: Regex,
    url_cred: Regex,
    bearer: Regex,
    tokens: Regex,
}

fn masker() -> &'static Masker {
    static M: OnceLock<Masker> = OnceLock::new();
    M.get_or_init(|| Masker {
        // 끝 줄이 잘려 나간 블록도 끝까지 가린다.
        private_key: Regex::new(
            r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?(?:-----END [A-Z0-9 ]*PRIVATE KEY-----|\z)",
        )
        .expect("private key regex"),
        url_cred: Regex::new(r"(?P<scheme>[A-Za-z][A-Za-z0-9+.\-]*://)[^\s/@:]+:[^\s/@]*@")
            .expect("url credential regex"),
        bearer: Regex::new(r"(?i)(?P<b>\bbearer\s+)[A-Za-z0-9\-._~+/]{16,}=*")
            .expect("bearer regex"),
        tokens: Regex::new(concat!(
            r"\bsk-(?:ant-)?[A-Za-z0-9_\-]{16,}",
            r"|\bgh[pousr]_[A-Za-z0-9]{20,}",
            r"|\bgithub_pat_[A-Za-z0-9_]{20,}",
            r"|\bxox[abpr]-[A-Za-z0-9\-]{10,}",
            r"|\bAKIA[0-9A-Z]{16}\b",
            r"|\bAIza[0-9A-Za-z_\-]{20,}",
        ))
        .expect("token regex"),
    })
}

/// 비밀처럼 보이는 것을 [`MASK`]로 바꾼다(계약 §2.4).
pub fn mask(s: &str) -> String {
    let m = masker();
    let s = m.private_key.replace_all(s, MASK);
    let s = m.url_cred.replace_all(&s, "${scheme}[가림]@");
    let s = m.bearer.replace_all(&s, "${b}[가림]");
    let s = m.tokens.replace_all(&s, MASK);
    s.into_owned()
}

/// 연속한 공백(줄바꿈 포함)을 한 칸으로 접고 앞뒤 공백을 지운다.
pub fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, part) in s.split_whitespace().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(part);
    }
    out
}

/// 글자(유니코드 스칼라) 수로 자른다. 잘렸으면 마지막 글자가 `…`이고 전체 길이는 `max`다.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let mut it = s.char_indices();
    match it.nth(max) {
        None => s.to_owned(),
        Some(_) => {
            let cut = s
                .char_indices()
                .nth(max - 1)
                .map(|(i, _)| i)
                .unwrap_or(s.len());
            let mut out = s[..cut].to_owned();
            out.push('…');
            out
        }
    }
}

/// 미리보기: 마스킹 → 공백 접기 → 자르기. 마스킹을 먼저 해서 잘린 비밀 조각이 남지 않게 한다.
pub fn preview(s: &str, max: usize) -> String {
    truncate_chars(&collapse_ws(&mask(s)), max)
}

const QUESTION_KEYS: [&str; 9] = [
    "할까요",
    "할지",
    "원하시",
    "선택해",
    "알려 주세요",
    "알려주세요",
    "which",
    "should i",
    "do you want",
];

/// 마지막 비어 있지 않은 줄이 질문인가(계약 §2.1 `asks_question`).
///
/// 그 줄이 `?`·`？`로 끝나거나(뒤따르는 마크다운 강조·괄호·따옴표는 무시) 질문 표현을 포함하면 참이다.
pub fn asks_question(text: &str) -> bool {
    let Some(last) = text.lines().map(str::trim).rev().find(|l| !l.is_empty()) else {
        return false;
    };
    let tail = last.trim_end_matches(|c: char| {
        matches!(
            c,
            '*' | '_' | '`' | ')' | '"' | '\'' | '」' | '』' | '”' | '’' | ' '
        )
    });
    if tail.ends_with('?') || tail.ends_with('？') {
        return true;
    }
    let lower = last.to_lowercase();
    QUESTION_KEYS.iter().any(|k| lower.contains(k))
}

const DONE_PHRASES: [&str; 7] = [
    "병합 완료",
    "nothing left",
    "모두 끝",
    "남은 작업 없",
    "남은 일 없",
    "남은 것 없",
    "남은 건 없",
];

const FOLLOWUP_KEYS: [&str; 9] = [
    "확인 필요",
    "남음",
    "남은",
    "직접",
    "설치해",
    "사용자 몫",
    "left to",
    "your review",
    "you need to",
];

fn decision_count_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"결정\s*\d+\s*건").expect("decision count regex"))
}

/// 사람이 해야 할 후속 일이 남았다는 표현이 있는가(계약 §2.1 `human_followup`).
///
/// 완료 표현(`병합 완료`, `nothing left`, `모두 끝`, `남은 작업 없…`)을 먼저 지운 뒤 찾는다.
pub fn human_followup(text: &str) -> bool {
    let mut t = text.to_lowercase();
    for p in DONE_PHRASES {
        t = t.replace(p, " ");
    }
    FOLLOWUP_KEYS.iter().any(|k| t.contains(k)) || decision_count_re().is_match(&t)
}

fn merge_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)\bgh\s+pr\s+merge\b|\bgit\s+merge(?:\s|$)|\bgit\s+push\b[^;&|\n]*\bmain\b")
            .expect("merge regex")
    })
}

/// 미리보기가 병합 명령(`gh pr merge`·`git merge`·`git push … main`)인가(계약 §3 `merge`).
pub fn is_merge_command(preview: &str) -> bool {
    merge_re().is_match(preview)
}

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// 표시용 프로젝트 이름: 작업 폴더의 마지막 경로 조각(`/`·`\` 둘 다 구분자로 본다).
pub fn project_name(cwd: &str) -> String {
    let trimmed = cwd.trim().trim_end_matches(is_sep);
    match trimmed.rsplit(is_sep).find(|p| !p.is_empty()) {
        Some(last) => last.to_owned(),
        None => cwd.trim().to_owned(),
    }
}

/// 작업 폴더 비교 열쇠: 구분자를 `/`로 맞추고 끝 구분자를 지운다. Windows 경로(드라이브 문자·UNC)는 소문자로 바꾼다.
pub fn cwd_key(cwd: &str) -> String {
    let s = cwd.trim().replace('\\', "/");
    let s = s.trim_end_matches('/');
    let bytes = s.as_bytes();
    let windows = (bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic())
        || s.starts_with("//");
    if windows {
        s.to_lowercase()
    } else if s.is_empty() {
        "/".to_owned()
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_every_pattern() {
        let cases = [
            ("key sk-abcdefghijklmnop1234 end", "key [가림] end"),
            ("sk-ant-api03-AbCdEfGhIjKlMnOpQrStUv-xyz", "[가림]"),
            ("tok ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123 x", "tok [가림] x"),
            ("gho_ABCDEFGHIJKLMNOPQRSTUVWX", "[가림]"),
            ("github_pat_11ABCDEFG0123456789_abcdefghijkl", "[가림]"),
            ("slack xoxb-1234567890-abcdef", "slack [가림]"),
            ("xoxp-123456789012-x", "[가림]"),
            ("aws AKIAIOSFODNN7EXAMPLE id", "aws [가림] id"),
            ("g AIzaSyA-1234567890abcdefghijklmnopqrs", "g [가림]"),
            (
                "Authorization: Bearer abcdefghijklmnopqrstuvwxyz012345",
                "Authorization: Bearer [가림]",
            ),
            (
                "git clone https://user:hunter2@github.com/x/y",
                "git clone https://[가림]@github.com/x/y",
            ),
            (
                "a\n-----BEGIN RSA PRIVATE KEY-----\nMIIE\nabc\n-----END RSA PRIVATE KEY-----\nb",
                "a\n[가림]\nb",
            ),
            (
                "x -----BEGIN OPENSSH PRIVATE KEY-----\nAAAA잘림",
                "x [가림]",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(mask(input), want, "input: {input}");
        }
    }

    #[test]
    fn does_not_mask_ordinary_text() {
        for s in [
            "task-runner and disk-usage are fine",
            "sk-learn",
            "https://github.com/x/y",
            "user@example.com",
            "Bearer short",
            "AKIA too short",
        ] {
            assert_eq!(mask(s), s);
        }
    }

    #[test]
    fn preview_masks_before_truncating() {
        let s = format!("{}sk-abcdefghijklmnopqrstuvwxyz", "a ".repeat(10));
        let p = preview(&s, 10);
        assert!(!p.contains("sk-"), "{p}");
        assert_eq!(p.chars().count(), 10);
        assert!(p.ends_with('…'));
    }

    #[test]
    fn collapse_and_truncate() {
        assert_eq!(collapse_ws("  a \n\t b   c "), "a b c");
        assert_eq!(truncate_chars("가나다라", 4), "가나다라");
        assert_eq!(truncate_chars("가나다라마", 4), "가나다…");
        assert_eq!(truncate_chars("abc", 0), "");
        assert_eq!(truncate_chars("ab", 1), "…");
        assert_eq!(preview("x\n\n  y", 300), "x y");
    }

    #[test]
    fn asks_question_korean_and_english() {
        let yes = [
            "작업을 마쳤습니다.\n이대로 병합할까요",
            "어느 쪽으로 할지 정해 주세요.",
            "다음 단계를 진행할까요?",
            "Done.\n\nShould I also update the docs",
            "Which option do you prefer",
            "Do you want me to continue",
            "괜찮을까요？",
            "**어떻게 진행하면 좋을까요?**",
            "원하시는 방식을 알려 주세요.",
            "둘 중 하나를 선택해 주세요",
            "Ready to go?  \n\n   ",
        ];
        for t in yes {
            assert!(asks_question(t), "should ask: {t:?}");
        }
        let no = [
            "",
            "   \n  ",
            "모든 시험을 통과했습니다.",
            "Should I? No — I already did it.\nAll tests pass.",
            "I fixed the bug.",
        ];
        for t in no {
            assert!(!asks_question(t), "should not ask: {t:?}");
        }
    }

    #[test]
    fn human_followup_detection() {
        let yes = [
            "PR을 열었습니다. 리뷰 확인 필요.",
            "시험 2개 남음",
            "남은 일: 배포",
            "토큰은 직접 넣어 주세요",
            "node를 설치해 주세요",
            "배포는 사용자 몫입니다",
            "결정 3건이 있습니다",
            "결정 2 건",
            "There is one step left to do",
            "Ready for your review",
            "You need to restart the server",
        ];
        for t in yes {
            assert!(human_followup(t), "should follow up: {t:?}");
        }
        let no = [
            "병합 완료",
            "모든 작업을 마쳤고 병합 완료했습니다.",
            "Nothing left to do.",
            "모두 끝났습니다",
            "남은 작업 없음",
            "All tests pass.",
        ];
        for t in no {
            assert!(!human_followup(t), "should not follow up: {t:?}");
        }
        // 완료 표현을 지운 뒤에도 다른 후속 표현이 있으면 참이다.
        assert!(human_followup("병합 완료. 배포는 직접 해 주세요"));
    }

    #[test]
    fn merge_commands() {
        for s in [
            "gh pr merge 12 --squash",
            "GH PR MERGE",
            "git merge feature",
            "cd x && git merge --no-ff topic",
            "git push origin main",
            "git push -u origin HEAD:main",
        ] {
            assert!(is_merge_command(s), "{s}");
        }
        for s in [
            "git merge-base a b",
            "git push origin feature",
            "git push origin maintenance",
            "echo main; git push origin dev",
            "npm install",
        ] {
            assert!(!is_merge_command(s), "{s}");
        }
    }

    #[test]
    fn project_names() {
        assert_eq!(project_name("C:\\work\\proj"), "proj");
        assert_eq!(project_name("C:\\work\\proj\\"), "proj");
        assert_eq!(project_name("/home/me/repo/"), "repo");
        assert_eq!(project_name("\\\\server\\share\\proj"), "proj");
        assert_eq!(project_name("C:/mixed\\sep/last"), "last");
        assert_eq!(project_name("proj"), "proj");
        assert_eq!(project_name("/"), "/");
    }

    #[test]
    fn cwd_keys() {
        assert_eq!(cwd_key("C:\\Work\\Proj\\"), "c:/work/proj");
        assert_eq!(cwd_key("c:/work/proj"), "c:/work/proj");
        assert_eq!(cwd_key("\\\\Server\\Share"), "//server/share");
        assert_eq!(cwd_key("/Home/Me/"), "/Home/Me");
        assert_eq!(cwd_key("/"), "/");
    }
}
