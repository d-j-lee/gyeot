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
    slack_hook: Regex,
    auth_header: Regex,
    cookie_header: Regex,
    secret_flag: Regex,
    mysql_p: Regex,
    user_secret: Regex,
    secret_kv: Regex,
    secret_word: Regex,
    not_secret: Regex,
}

/// 값 한 낱말: 따옴표 묶음(닫는 따옴표가 잘려 없으면 끝까지) 또는 공백·따옴표 전까지(닫는 따옴표는 남긴다).
const VALUE: &str = r#"(?P<v>"(?:[^"\\]|\\.)*"?|'(?:[^'\\]|\\.)*'?|[^\s"']+)"#;

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
            // §14.5 접두어: npm·Docker·GitLab·Stripe, JWT
            r"|\bnpm_[A-Za-z0-9]{20,}",
            r"|\bdckr_pat_[A-Za-z0-9_\-]{20,}",
            r"|\bglpat-[A-Za-z0-9_\-]{20,}",
            r"|\b[sr]k_(?:live|test)_[A-Za-z0-9]{10,}",
            r"|\beyJ[A-Za-z0-9_\-]{5,}\.eyJ[A-Za-z0-9_\-]{5,}\.[A-Za-z0-9_\-]*",
        ))
        .expect("token regex"),
        slack_hook: Regex::new(r"(?i)(?P<h>\bhooks\.slack\.com/services/)[A-Za-z0-9/_\-]+")
            .expect("slack webhook regex"),
        // `Authorization: [방식] <값>`(Basic·Bearer 등). 닫는 따옴표는 남긴다.
        auth_header: Regex::new(&format!(
            r#"(?i)(?P<h>\b(?:proxy-)?authorization["']?\s*:\s*["']?(?:(?:basic|bearer|digest|token|negotiate|ntlm)\s+)?){VALUE}"#
        ))
        .expect("authorization header regex"),
        // `Cookie: …`·`Set-Cookie: …`: 값에 공백·`;`가 들어 있으므로 따옴표나 끝까지.
        cookie_header: Regex::new(r#"(?i)(?P<h>\b(?:set-)?cookie["']?\s*:\s*["']?)(?P<v>[^\s"'][^"']*)"#)
            .expect("cookie header regex"),
        // 값을 공백 뒤에 받는 플래그(`--password SECRET`).
        secret_flag: Regex::new(&format!(
            r"(?i)(?P<h>(?:^|\s)--(?:password|passwd|token|secret|api-key|access-token|auth)\s+){VALUE}"
        ))
        .expect("secret flag regex"),
        // mysql·mysqldump·mysqladmin의 붙여 쓴 `-p<값>`(`-P`는 포트라 대소문자를 가린다).
        mysql_p: Regex::new(&format!(
            r"(?P<h>(?i:\bmysql(?:dump|admin)?\b)[^;&|]*?\s-p){VALUE}"
        ))
        .expect("mysql password regex"),
        // `-u`·`--user` 다음 `이름:비밀`.
        user_secret: Regex::new(
            r#"(?P<h>(?:^|\s)(?:-u|--user)(?:\s+|=)["']?(?P<n>[^\s:"']+):)(?P<v>[^\s"']+)"#,
        )
        .expect("user secret regex"),
        // 계약 §13.4·§14.5: 키 이름에 비밀 낱말이 든 `키=값`·`키: 값`. 공백은 모두 `\s`로 본다(공백 접기 전후가 같게).
        secret_kv: Regex::new(&format!(
            r#"(?i)(?P<k>[A-Za-z0-9_\-]*(?:pass|secret|token|api[_\-]?key|access[_\-]?key|private[_\-]?key|auth[_\-]?key|credential|pw)[A-Za-z0-9_\-]*)(?P<sep>["']?\s*[:=]\s*){VALUE}"#
        ))
        .expect("secret key-value regex"),
        secret_word: Regex::new(
            r"(?i)pass|secret|token|api[_\-]?key|access[_\-]?key|private[_\-]?key|auth[_\-]?key|credential",
        )
        .expect("secret word regex"),
        // 비밀 낱말을 품었지만 비밀이 아닌 흔한 이름(셈·한도·우회 등).
        not_secret: Regex::new(concat!(
            r"(?i)(?:by|com|sur|tres|over|under)pass",
            r"|pass(?:ed|es|ing|age|through|thru)",
            r"|tokens|tokeni[sz]",
            r"|token[_\-]?(?:count|limit|budget|usage|type)",
        ))
        .expect("not-secret regex"),
    })
}

/// 비밀처럼 보이는 것을 [`MASK`]로 바꾼다(계약 §2.4, §13.4, §14.5). 두 번 가려도 같다.
pub fn mask(s: &str) -> String {
    let m = masker();
    let s = m.private_key.replace_all(s, MASK);
    let s = m.url_cred.replace_all(&s, "${scheme}[가림]@");
    let s = m.bearer.replace_all(&s, "${b}[가림]");
    let s = m.tokens.replace_all(&s, MASK);
    let s = m.slack_hook.replace_all(&s, "${h}[가림]");
    let s = m.auth_header.replace_all(&s, mask_value);
    let s = m.cookie_header.replace_all(&s, "${h}[가림]");
    let s = m.secret_flag.replace_all(&s, mask_value);
    let s = m.mysql_p.replace_all(&s, mask_value);
    let s = m.user_secret.replace_all(&s, |c: &regex::Captures<'_>| {
        let (n, v) = (&c["n"], &c["v"]);
        // `-u 1000:1000`(uid:gid)은 비밀이 아니다.
        if n.bytes().all(|b| b.is_ascii_digit()) && v.bytes().all(|b| b.is_ascii_digit()) {
            c[0].to_owned()
        } else {
            format!("{}{MASK}", &c["h"])
        }
    });
    mask_secret_kv(m, &s)
}

/// `h`는 두고 값 `v`만 가린다. 따옴표 묶음은 따옴표째 남긴다.
fn mask_value(c: &regex::Captures<'_>) -> String {
    format!("{}{}", &c["h"], masked_value(&c["v"]))
}

fn masked_value(v: &str) -> &'static str {
    match v.chars().next() {
        Some('"') => "\"[가림]\"",
        Some('\'') => "'[가림]'",
        _ => MASK,
    }
}

/// 키 이름이 비밀 낱말을 품었는가(흔한 거짓 양성 이름을 지운 뒤에도 남는가). `pw`·`pwd`는 키가 정확히 그것일 때만.
fn is_secret_key(m: &Masker, key: &str) -> bool {
    let bare = key.trim_start_matches('-').to_ascii_lowercase();
    if bare == "pw" || bare == "pwd" {
        return true;
    }
    m.secret_word
        .is_match(&m.not_secret.replace_all(key, "\u{1f}"))
}

/// `키=값`·`키: 값`의 값을 가린다. 비밀 키가 아니면(거짓 양성) 값 자리부터 다시 찾는다.
fn mask_secret_kv(m: &Masker, s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut copied = 0;
    let mut at = 0;
    while at <= s.len() {
        let Some(c) = m.secret_kv.captures_at(s, at) else {
            break;
        };
        let (Some(k), Some(sep), Some(v)) = (c.name("k"), c.name("sep"), c.name("v")) else {
            break;
        };
        // `Token::new` 같은 경로 표기는 키=값이 아니다.
        let path_sep = sep.as_str().trim_end().ends_with(':') && v.as_str().starts_with(':');
        if path_sep || !is_secret_key(m, k.as_str()) {
            at = v.start();
            continue;
        }
        out.push_str(&s[copied..v.start()]);
        out.push_str(masked_value(v.as_str()));
        copied = v.end();
        at = v.end();
    }
    out.push_str(&s[copied..]);
    out
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

/// `git`과 하위 명령 사이의 앞 옵션: 값을 받는 옵션(`-C <경로>`·`-c <키=값>` 등, 따옴표 경로 포함)과 홀로 쓰는 옵션.
const GIT_GLOBAL_OPTS: &str = r#"(?:\s+(?:-C|-c|--git-dir|--work-tree|--namespace|--exec-path|--config-env)\s+(?:"[^"]*"|'[^']*'|\S+)|\s+-\S+)*"#;

fn merge_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        let opts = GIT_GLOBAL_OPTS;
        Regex::new(&format!(
            concat!(
                r"(?i)\bgh\s+pr\s+merge\b",
                r"|\bgit{opts}\s+merge(?:\s|$)",
                // 브랜치 자리는 정확히 main·master(§12.6·§13.6): 앞은 공백·`:`·`+`·따옴표(그 뒤 `refs/heads/` 허용),
                // 뒤는 공백·끝·`:`·`;`·`&`·`|`·따옴표·`)`.
                r#"|\bgit{opts}\s+push\b[^;&|\n]*?[\s:+"'](?:refs/heads/)?(?:main|master)(?:[\s:;&|"')]|$)"#,
            ),
            opts = opts
        ))
        .expect("merge regex")
    })
}

/// 미리보기가 병합 명령(`gh pr merge`·`git merge`·`git push … main|master`)인가(계약 §3 `merge`, §12.6).
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

    /// R3 L3(계약 §13.4): 키 이름에 비밀 낱말이 든 `키=값`·`키: 값`의 값을 가린다.
    #[test]
    fn masks_secret_key_values() {
        let cases = [
            ("password=hunter2", "password=[가림]"),
            (
                "DB_PASSWORD=abc123 npm start",
                "DB_PASSWORD=[가림] npm start",
            ),
            ("token: tok_live_abcdef", "token: [가림]"),
            ("PGPASSWORD=x psql -h db", "PGPASSWORD=[가림] psql -h db"),
            ("passwd: s3cr3t", "passwd: [가림]"),
            ("export API_KEY=abc", "export API_KEY=[가림]"),
            ("apikey=abc&x=1", "apikey=[가림]"),
            ("X-Api-Key: abc", "X-Api-Key: [가림]"),
            ("aws_access_key_id = AKIAxx", "aws_access_key_id = [가림]"),
            ("private_key: /k/id_rsa", "private_key: [가림]"),
            ("client_secret=xyz end", "client_secret=[가림] end"),
            ("--password=p4ss", "--password=[가림]"),
            // 따옴표 값은 따옴표째로(공백이 있어도 묶음 전체)
            (r#"password="two words" next"#, r#"password="[가림]" next"#),
            ("secret='a b c' x", "secret='[가림]' x"),
            (
                r#"{"api_key":"abc","n":1}"#,
                r#"{"api_key":"[가림]","n":1}"#,
            ),
            (r#"{"password": "a\"b"}"#, r#"{"password": "[가림]"}"#),
            // 잘려서 닫는 따옴표가 없어도 끝까지 가린다
            (r#"token="abcdef"#, r#"token="[가림]""#),
            // 대소문자, 줄이 바뀐 값
            ("Password:\nhunter2", "Password:\n[가림]"),
            // 앞 글자가 한글이어도
            ("비밀password=abc", "비밀password=[가림]"),
        ];
        for (input, want) in cases {
            assert_eq!(mask(input), want, "input: {input}");
            // 두 번 가려도 같다
            assert_eq!(mask(want), want, "idempotent: {want}");
        }
    }

    /// 거짓 양성: 값이 없는 플래그 이름, 셈 결과, 경로 표기 같은 것은 그대로 둔다.
    #[test]
    fn secret_key_rule_keeps_ordinary_text() {
        for s in [
            "npm test --token-limit",
            "npm test --token-limit 5",
            "tests passed: 12, failed: 0",
            "max_tokens=4096",
            "input tokens: 5000",
            "token_count: 12",
            "bypass: true",
            "let t = AccessToken::new(x);",
            "PasswordHasher::hash(&p)",
            "the password is set",
            "password",
            "token:",
            "result: pass",
        ] {
            assert_eq!(mask(s), s, "should stay: {s}");
        }
        // 거짓 양성으로 넘긴 값 안의 진짜 키=값은 다시 본다
        assert_eq!(mask("bypass: token=abc"), "bypass: token=[가림]");
        assert_eq!(mask("passed_token=abc"), "passed_token=[가림]");
    }

    /// R3 회귀 N5·R5 L2(계약 §14.5): 남아 있던 비밀 꼴.
    #[test]
    fn masks_r5_secret_forms() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let cases = [
            // 공백 뒤에 값을 받는 플래그
            (
                "psql --password SECRET -h db",
                "psql --password [가림] -h db",
            ),
            ("vercel deploy --token Xy9", "vercel deploy --token [가림]"),
            (
                "az login --password 'pw' -u me",
                "az login --password '[가림]' -u me",
            ),
            (
                "x --passwd p --secret s",
                "x --passwd [가림] --secret [가림]",
            ),
            (
                "gh --api-key k1 --access-token t1",
                "gh --api-key [가림] --access-token [가림]",
            ),
            ("tool --auth user:pw", "tool --auth [가림]"),
            // mysql 계열의 붙여 쓴 -p
            ("mysql -u root -pS3cret db", "mysql -u root -p[가림] db"),
            ("mysqldump -pS3cret --all", "mysqldump -p[가림] --all"),
            (
                "MYSQLADMIN -h x -pabc status",
                "MYSQLADMIN -h x -p[가림] status",
            ),
            // -u·--user 다음 이름:비밀
            (
                "curl -u admin:Hunter2 https://x",
                "curl -u admin:[가림] https://x",
            ),
            ("curl --user admin:Hunter2", "curl --user admin:[가림]"),
            ("curl --user=admin:Hunter2", "curl --user=admin:[가림]"),
            ("curl -u 'admin:Hunter2' x", "curl -u 'admin:[가림]' x"),
            // 머리 값(닫는 따옴표는 남긴다)
            (
                "curl -H 'Authorization: Basic dXNlcjpwYXNz' x",
                "curl -H 'Authorization: Basic [가림]' x",
            ),
            ("Authorization: abc123", "Authorization: [가림]"),
            (
                "curl -H 'Cookie: session=abc; theme=dark' x",
                "curl -H 'Cookie: [가림]' x",
            ),
            ("Set-Cookie: sid=1; HttpOnly", "Set-Cookie: [가림]"),
            (r#"{"cookie":"a=b","n":1}"#, r#"{"cookie":"[가림]","n":1}"#),
            (
                "curl -H 'X-Api-Key: abc' x",
                "curl -H 'X-Api-Key: [가림]' x",
            ),
            (
                "curl -H \"X-Api-Key: abc\" x",
                "curl -H \"X-Api-Key: [가림]\" x",
            ),
            // 접두어
            ("npm_abcdefghijklmnopqrstuvwxyz0123456789", MASK),
            ("t dckr_pat_abcdefghijklmnopqrstuvwx", "t [가림]"),
            ("glpat-abcdefghijklmnopqrst", MASK),
            ("sk_live_abcdefghij0123 x", "[가림] x"),
            ("rk_live_abcdefghij0123", MASK),
            ("xoxb-1234567890-abcdefghij", MASK),
            (
                "curl https://hooks.slack.com/services/T000/B000/XXXXyyyy -d x",
                "curl https://hooks.slack.com/services/[가림] -d x",
            ),
            // 키 이름 규칙: pw·pwd(정확히), auth_key, credential(s)
            ("PW=hunter2 run", "PW=[가림] run"),
            ("pwd=hunter2", "pwd=[가림]"),
            ("--pw=hunter2", "--pw=[가림]"),
            ("auth_key=abc", "auth_key=[가림]"),
            ("credentials=abc", "credentials=[가림]"),
            ("aws_credential: abc", "aws_credential: [가림]"),
        ];
        for (input, want) in cases {
            assert_eq!(mask(input), want, "input: {input}");
            assert_eq!(mask(want), want, "idempotent: {want}");
        }
        let jwt_line = format!("Bearer? no: token {jwt} end");
        assert_eq!(mask(&jwt_line), "Bearer? no: token [가림] end");
        assert_eq!(mask(jwt), MASK);
    }

    /// §14.5의 평범한 글은 그대로다.
    #[test]
    fn r5_rules_keep_ordinary_text() {
        for s in [
            "mkdir -p dir",
            "cp -pr a b",
            "npm test --token-limit 5",
            "git commit --author \"Me <me@x>\"",
            "mysql -p",
            "mysql -P 3306 db",
            "ssh -p 2222 host",
            "curl -u admin https://x",
            "docker run -u 1000:1000 img",
            "npm_config_registry=https://r",
            "the pwd command prints",
            "upward=3",
            "cookies: 3",
            "Authorization:",
            "eyJ only",
        ] {
            assert_eq!(mask(s), s, "should stay: {s}");
        }
    }

    /// §14.5 규칙도 두 번 가려도 같고 공백 접기 전후에 같다.
    #[test]
    fn r5_rules_are_idempotent_and_whitespace_stable() {
        let parts = [
            "--password",
            "--token",
            " ",
            "\n",
            "x",
            "'",
            "\"",
            "mysql",
            "-pS",
            "-u",
            "a:b",
            "Cookie: ",
            "Authorization: Basic ",
            "eyJabcdef.eyJabcdef.zz",
            "pw=",
            ";",
        ];
        for a in parts {
            for b in parts {
                for c in parts {
                    for d in parts {
                        let s = format!("{a}{b}{c}{d}");
                        let once = mask(&s);
                        assert_eq!(mask(&once), once, "idempotent: {s:?}");
                        assert_eq!(
                            collapse_ws(&mask(&collapse_ws(&s))),
                            collapse_ws(&once),
                            "collapse-stable: {s:?}"
                        );
                    }
                }
            }
        }
    }

    /// 조각을 이어 붙인 글 모두에서: 두 번 가려도 같고, 공백 접기 전후에 가린 결과가 같다
    /// (심이 줄인 글을 정규화가 다시 가려도 원본과 같아야 한다, §12.5).
    #[test]
    fn masking_is_idempotent_and_whitespace_stable() {
        let parts = [
            "password",
            "=",
            ": ",
            "\n",
            " ",
            "\"",
            "'",
            "x y",
            "token",
            "::",
            "passed",
            "sk-abcdefghijklmnopqrstu",
            "Bearer abcdefghijklmnopqrstuvwxyz",
            "https://u:p@h/",
            "가",
            "\\",
            "api-key",
        ];
        let mut n = 0;
        for a in parts {
            for b in parts {
                for c in parts {
                    for d in parts {
                        let s = format!("{a}{b}{c}{d}");
                        let once = mask(&s);
                        assert_eq!(mask(&once), once, "idempotent: {s:?}");
                        assert_eq!(
                            collapse_ws(&mask(&collapse_ws(&s))),
                            collapse_ws(&once),
                            "collapse-stable: {s:?}"
                        );
                        n += 1;
                    }
                }
            }
        }
        assert_eq!(n, parts.len().pow(4));
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

    /// R1 L5(계약 §12.6): 앞 옵션을 건너뛰고, 브랜치 자리는 정확히 `main`·`master`만 본다.
    #[test]
    fn merge_detection_r1() {
        for s in [
            "git -C . push origin main",
            "git -C /w/proj push origin main",
            "git -C \"C:/my repo\" push -u origin main",
            "git push origin HEAD:main",
            "git push origin main:main",
            "git push origin master",
            "git push origin master:master",
            "git -C . merge topic",
            "git -C 'my dir' merge --no-ff topic",
            "git --no-pager -c core.editor=true merge topic",
            "cd x && git -C . push origin main && echo done",
        ] {
            assert!(is_merge_command(s), "병합이어야 함: {s}");
        }
        for s in [
            "git push origin feature/main-fix",
            "git push origin main-fix",
            "git push origin feature:main-fix",
            "git push origin mainline",
            "git push origin my_main",
            "git push origin remaster",
            "git merge-base main topic",
            "git -C . merge-base main topic",
            "git -C main status",
            "git log main",
            "git push origin dev; echo main",
        ] {
            assert!(!is_merge_command(s), "병합이 아니어야 함: {s}");
        }
    }

    /// R2 N3(계약 §13.6): 브랜치 자리 앞 `+`·따옴표·`refs/heads/`, 뒤 `;`·`&`·`|`·따옴표·`)`.
    #[test]
    fn merge_detection_n3() {
        for s in [
            "git push origin +main",
            "git push origin refs/heads/main",
            "git push origin HEAD:refs/heads/main",
            "git push origin main;",
            "git push origin main&& echo done",
            "git push origin main|cat",
            "git push origin \"main\"",
            "git push origin 'main'",
            "(git push origin main)",
            "git push origin +refs/heads/master",
            "git push origin HEAD:'master'",
        ] {
            assert!(is_merge_command(s), "병합이어야 함: {s}");
        }
        for s in [
            "git push origin feature/main-fix",
            "git push origin main-fix",
            "git push origin mainline",
            "git merge-base main topic",
            "git push origin refs/heads/main-fix",
            "git push origin +mainline",
            "git push origin \"main-fix\"",
            "git push origin refs/tags/main2",
            "git push origin dev; git log main",
        ] {
            assert!(!is_merge_command(s), "병합이 아니어야 함: {s}");
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
