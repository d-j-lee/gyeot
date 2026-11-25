//! 얼굴 열기(일회용 부트 코드 → 세션 열쇠)와 데몬 확인(계약 §7·§12.1·§13.1).
//!
//! 토큰을 가진 쪽(셸·`gyeotd --open`)이 `POST /v1/boot`로 코드를 받고, 얼굴 스크립트가
//! `POST /v1/session`으로 그 코드를 세션 열쇠로 바꾼다. 열쇠는 `Authorization: Bearer`로만 받는다.
//! 쿠키는 포트를 가리지 않아 같은 호스트의 다른 포트로 새므로 쓰지 않는다(G-030).
//! 세션 값은 디스크에 SHA-256으로만 남는다.

use crate::store::{new_token, write_private};
use anyhow::Result;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// 부트 코드가 살아 있는 시간.
pub const BOOT_TTL_MS: i64 = 120_000;
/// 동시에 열어 두는 부트 코드 수.
pub const BOOT_MAX: usize = 8;
/// 세션이 살아 있는 시간(만든 때부터 7일, 써도 늘지 않는다).
pub const SESSION_TTL_MS: i64 = 7 * 86_400_000;
/// 기억하는 세션 수.
pub const SESSION_MAX: usize = 20;
/// 마지막 사용 시각만 바뀌었을 때 파일에 쓰는 간격.
const LAST_SAVE_EVERY_MS: i64 = 10 * 60_000;
/// hello 증명에 묶는 글의 머리(계약 §13.1).
pub const HELLO_PREFIX: &str = "gyeot-hello/v1|";

/// 32바이트 난수 hex(64자)인가.
fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    crate::http::ct_eq(a, b)
}

/// 세션 값의 SHA-256(소문자 hex).
pub fn session_hash(id: &str) -> String {
    crate::store::hex(&Sha256::digest(id.as_bytes()))
}

// ───────────────────────── 데몬 확인(hello) ─────────────────────────

/// `hex(HMAC-SHA256(key, msg))`(소문자).
pub fn hmac_sha256_hex(key: &[u8], msg: &[u8]) -> String {
    // HMAC은 어떤 길이의 열쇠든 받는다(긴 열쇠는 해시한다). 실패하지 않는다.
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key)
        .unwrap_or_else(|_| unreachable!("HMAC 열쇠 길이에는 제한이 없다"));
    mac.update(msg);
    crate::store::hex(&mac.finalize().into_bytes())
}

/// hello의 `n`: 16–128자 hex(대소문자 모두, 받은 글자 그대로 묶는다).
pub fn valid_nonce(n: &str) -> bool {
    (16..=128).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 계약 §13.1의 증명: `hex(HMAC-SHA256(key = 토큰 UTF-8, msg = "gyeot-hello/v1|<포트>|<n>"))`.
///
/// 서버는 **실제로 묶인 포트**를, 클라이언트는 **자기가 연결한 포트**를 넣는다. 그래서 다른 포트로
/// 중계하는 서버는 맞는 증명을 낼 수 없다. 클라이언트는 [`crate::http::ct_eq`]로 비교한다.
pub fn hello_proof(token: &str, port: u16, n: &str) -> String {
    let msg = format!("{HELLO_PREFIX}{port}|{n}");
    hmac_sha256_hex(token.as_bytes(), msg.as_bytes())
}

// ───────────────────────── 부트 코드 ─────────────────────────

/// 열린 부트 코드(코드, 끝나는 시각). 메모리에만 둔다.
#[derive(Debug, Default)]
pub struct BootCodes {
    codes: VecDeque<(String, i64)>,
}

impl BootCodes {
    fn purge(&mut self, now_ms: i64) {
        self.codes.retain(|(_, exp)| *exp > now_ms);
    }

    /// 새 코드를 낸다. (코드, 끝나는 시각). 8개가 넘으면 가장 오래된 것부터 버린다.
    pub fn issue(&mut self, now_ms: i64) -> Result<(String, i64)> {
        let code = new_token()?;
        Ok(self.insert(code, now_ms))
    }

    fn insert(&mut self, code: String, now_ms: i64) -> (String, i64) {
        self.purge(now_ms);
        while self.codes.len() >= BOOT_MAX {
            self.codes.pop_front();
        }
        let exp = now_ms + BOOT_TTL_MS;
        self.codes.push_back((code.clone(), exp));
        (code, exp)
    }

    /// 코드를 쓴다(한 번만). 맞고 아직 살아 있으면 `true`.
    pub fn redeem(&mut self, code: &str, now_ms: i64) -> bool {
        self.purge(now_ms);
        if !is_hex64(code) {
            return false;
        }
        let pos = self
            .codes
            .iter()
            .position(|(c, _)| ct_eq(c.as_bytes(), code.as_bytes()));
        match pos {
            Some(i) => {
                self.codes.remove(i);
                true
            }
            None => false,
        }
    }

    /// 열린 코드 수.
    pub fn len(&self) -> usize {
        self.codes.len()
    }

    /// 열린 코드가 없는가.
    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }
}

// ───────────────────────── 세션 ─────────────────────────

/// `sessions.json`의 세션 하나. 세션 값 자체는 저장하지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    /// 세션 값의 SHA-256(소문자 hex).
    pub sha256: String,
    /// 만든 시각.
    pub created_ms: i64,
    /// 마지막으로 쓴 시각.
    pub last_ms: i64,
    /// 끝나는 시각(만든 시각 + 7일).
    pub expires_ms: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionsFile {
    version: u32,
    sessions: Vec<SessionRecord>,
}

/// 세션 목록. `path`가 있으면 바뀔 때 파일에 쓴다(Unix 0600).
#[derive(Debug)]
pub struct Sessions {
    path: Option<PathBuf>,
    list: Vec<SessionRecord>,
    dirty: bool,
    saved_ms: i64,
}

impl Sessions {
    /// 파일 없이(시험용).
    pub fn in_memory() -> Sessions {
        Sessions {
            path: None,
            list: Vec::new(),
            dirty: false,
            saved_ms: 0,
        }
    }

    /// 파일에서 읽는다. 없거나 망가졌으면 빈 목록, 지난 것·이상한 것은 버린다.
    pub fn load(path: &Path, now_ms: i64) -> Sessions {
        let mut s = Sessions {
            path: Some(path.to_path_buf()),
            ..Sessions::in_memory()
        };
        s.saved_ms = now_ms;
        match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<SessionsFile>(&bytes) {
                Ok(f) => {
                    let before = f.sessions.len();
                    // 지난 것, 7일보다 길게 적힌 것(쿠키 시절의 30일 세션 포함)은 버린다.
                    s.list = f
                        .sessions
                        .into_iter()
                        .filter(|r| {
                            is_hex64(&r.sha256)
                                && r.expires_ms > now_ms
                                && r.expires_ms <= r.created_ms + SESSION_TTL_MS
                        })
                        .collect();
                    s.list.sort_by_key(|r| std::cmp::Reverse(r.last_ms));
                    s.list.truncate(SESSION_MAX);
                    s.dirty = s.list.len() != before;
                }
                Err(e) => {
                    tracing::warn!("{}을 읽지 못해 빈 세션으로 시작해요: {e}", path.display());
                    s.dirty = true;
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!("{}을 열지 못했어요: {e}", path.display()),
        }
        s
    }

    /// 새 세션을 만들고 (세션 열쇠, 끝나는 시각)을 돌려준다. 열쇠 자체는 저장하지 않는다.
    /// 20개가 넘으면 가장 오래 안 쓴 것부터 버린다.
    pub fn create(&mut self, now_ms: i64) -> Result<(String, i64)> {
        let id = new_token()?;
        self.insert(&id, now_ms);
        self.save();
        Ok((id, now_ms + SESSION_TTL_MS))
    }

    fn insert(&mut self, id: &str, now_ms: i64) {
        self.purge(now_ms);
        while self.list.len() >= SESSION_MAX {
            let oldest = self
                .list
                .iter()
                .enumerate()
                .min_by_key(|(_, r)| (r.last_ms, r.created_ms))
                .map(|(i, _)| i);
            match oldest {
                Some(i) => {
                    self.list.remove(i);
                }
                None => break,
            }
        }
        self.list.push(SessionRecord {
            sha256: session_hash(id),
            created_ms: now_ms,
            last_ms: now_ms,
            expires_ms: now_ms + SESSION_TTL_MS,
        });
        self.dirty = true;
    }

    fn purge(&mut self, now_ms: i64) {
        let before = self.list.len();
        self.list.retain(|r| r.expires_ms > now_ms);
        if self.list.len() != before {
            self.dirty = true;
        }
    }

    /// 세션 열쇠가 살아 있는 세션인가(해시를 상수 시간으로 대조). 맞으면 마지막 사용 시각을 갱신한다.
    pub fn validate(&mut self, id: &str, now_ms: i64) -> bool {
        if !is_hex64(id) {
            return false;
        }
        let before = self.list.len();
        self.purge(now_ms);
        let h = session_hash(id);
        let mut found = false;
        for r in &mut self.list {
            if ct_eq(r.sha256.as_bytes(), h.as_bytes()) {
                if now_ms > r.last_ms {
                    r.last_ms = now_ms;
                }
                found = true;
            }
        }
        if self.list.len() != before {
            self.save();
        } else if found {
            self.dirty = true;
            if now_ms - self.saved_ms >= LAST_SAVE_EVERY_MS {
                self.save();
            }
        }
        found
    }

    /// 바뀐 것이 있으면 파일에 쓴다(끌 때).
    pub fn flush(&mut self) {
        if self.dirty {
            self.save();
        }
    }

    fn save(&mut self) {
        let Some(path) = &self.path else {
            self.dirty = false;
            return;
        };
        let file = SessionsFile {
            version: 1,
            sessions: self.list.clone(),
        };
        match serde_json::to_vec_pretty(&file)
            .map_err(anyhow::Error::from)
            .and_then(|b| write_private(path, &b))
        {
            Ok(()) => {
                self.dirty = false;
                self.saved_ms = self.list.iter().map(|r| r.last_ms).max().unwrap_or(0);
            }
            Err(e) => tracing::error!("세션 파일을 쓰지 못했어요: {e:#}"),
        }
    }

    /// 세션 기록(시험·진단용).
    pub fn records(&self) -> &[SessionRecord] {
        &self.list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: i64 = 1_764_126_720_000;

    fn hex(c: char) -> String {
        c.to_string().repeat(64)
    }

    #[test]
    fn boot_codes_are_single_use_and_expire() {
        let mut b = BootCodes::default();
        let (c, exp) = b.insert(hex('a'), T);
        assert_eq!(exp, T + BOOT_TTL_MS);
        assert!(!b.redeem(&hex('b'), T));
        assert!(b.redeem(&c, T + 1));
        assert!(!b.redeem(&c, T + 2), "한 번만");
        let (c, _) = b.insert(hex('c'), T);
        assert!(!b.redeem(&c, T + BOOT_TTL_MS), "120초가 지나면 끝");
        let (c, _) = b.insert(hex('d'), T);
        assert!(b.redeem(&c, T + BOOT_TTL_MS - 1));
        assert!(!b.redeem("", T));
        assert!(!b.redeem("short", T));
    }

    #[test]
    fn at_most_eight_codes_oldest_dropped() {
        let mut b = BootCodes::default();
        let codes: Vec<String> = (0..9)
            .map(|i| b.insert(format!("{i:064x}"), T + i).0)
            .collect();
        assert_eq!(b.len(), BOOT_MAX);
        assert!(!b.redeem(&codes[0], T + 10));
        for c in &codes[1..] {
            assert!(b.redeem(c, T + 10));
        }
        assert!(b.is_empty());
        // 진짜 코드는 64자 hex이고 매번 다르다
        let (x, _) = b.issue(T).unwrap();
        let (y, _) = b.issue(T).unwrap();
        assert!(is_hex64(&x) && is_hex64(&y) && x != y);
    }

    #[test]
    fn sessions_expire_after_seven_days_and_evict_lru() {
        assert_eq!(
            SESSION_TTL_MS,
            7 * 86_400_000,
            "계약 §12.1: 만든 때부터 7일"
        );
        let mut s = Sessions::in_memory();
        let (a, exp) = s.create(T).unwrap();
        assert!(is_hex64(&a));
        assert_eq!(exp, T + 7 * 86_400_000);
        assert_eq!(s.records()[0].expires_ms, exp);
        assert!(s.validate(&a, T + SESSION_TTL_MS - 1));
        assert!(
            !s.validate(&a, T + SESSION_TTL_MS),
            "7일 뒤 끝(쓰고 있어도)"
        );
        assert!(s.records().is_empty());

        let mut s = Sessions::in_memory();
        let ids: Vec<String> = (0..SESSION_MAX as i64)
            .map(|i| {
                let id = format!("{i:064x}");
                s.insert(&id, T + i);
                id
            })
            .collect();
        // 첫 세션을 쓰면 두 번째가 가장 오래 안 쓴 것이 된다
        assert!(s.validate(&ids[0], T + 100));
        s.insert(&hex('e'), T + 200);
        assert_eq!(s.records().len(), SESSION_MAX);
        assert!(s.validate(&ids[0], T + 300));
        assert!(!s.validate(&ids[1], T + 300));
        assert!(s.validate(&hex('e'), T + 300));
        assert!(!s.validate("not-hex", T));
    }

    #[test]
    fn session_file_keeps_only_hashes() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sessions.json");
        let mut s = Sessions::load(&p, T);
        let (id, _) = s.create(T).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains(&id));
        assert!(text.contains(&session_hash(&id)));
        // 다시 읽어도 이어지고, 지난 것·망가진 것은 버린다
        let mut s2 = Sessions::load(&p, T + 1);
        assert!(s2.validate(&id, T + 2));
        let mut s3 = Sessions::load(&p, T + SESSION_TTL_MS);
        assert!(!s3.validate(&id, T + SESSION_TTL_MS));
        std::fs::write(&p, "{broken").unwrap();
        let mut s4 = Sessions::load(&p, T);
        assert!(!s4.validate(&id, T));
        // 마지막 사용 시각은 10분마다 쓴다
        let mut s = Sessions::load(&p, T);
        let (id, _) = s.create(T).unwrap();
        assert!(s.validate(&id, T + 60_000));
        let on_disk: SessionsFile = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(on_disk.sessions[0].last_ms, T);
        assert!(s.validate(&id, T + LAST_SAVE_EVERY_MS));
        let on_disk: SessionsFile = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(on_disk.sessions[0].last_ms, T + LAST_SAVE_EVERY_MS);
        assert!(s.validate(&id, T + LAST_SAVE_EVERY_MS + 5));
        s.flush();
        let on_disk: SessionsFile = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(on_disk.sessions[0].last_ms, T + LAST_SAVE_EVERY_MS + 5);
    }

    /// 쿠키 시절(30일)의 세션 기록은 읽을 때 버리고, 파일도 다시 쓴다(G-030).
    #[test]
    fn legacy_thirty_day_sessions_are_dropped_on_load() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sessions.json");
        let id = hex('9');
        let legacy = serde_json::json!({
            "version": 1,
            "sessions": [{
                "sha256": session_hash(&id),
                "created_ms": T,
                "last_ms": T,
                "expires_ms": T + 30 * 86_400_000_i64,
            }]
        });
        std::fs::write(&p, legacy.to_string()).unwrap();
        let mut s = Sessions::load(&p, T + 1);
        assert!(s.records().is_empty());
        assert!(!s.validate(&id, T + 2));
        s.flush();
        let on_disk: SessionsFile = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert!(on_disk.sessions.is_empty(), "버린 기록을 파일에서도 지운다");
    }

    /// RFC 4231 시험 벡터(HMAC-SHA256 1·2번)와 계약 §13.1의 묶음 글.
    #[test]
    fn hello_proof_is_hmac_sha256_over_port_and_nonce() {
        assert_eq!(
            hmac_sha256_hex(&[0x0b; 20], b"Hi There"),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_eq!(
            hmac_sha256_hex(b"Jefe", b"what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        let token = hex('a');
        let n = "0123456789abcdef";
        let p = hello_proof(&token, 47321, n);
        assert_eq!(
            p,
            hmac_sha256_hex(token.as_bytes(), b"gyeot-hello/v1|47321|0123456789abcdef")
        );
        assert!(is_hex64(&p));
        assert_ne!(p, hello_proof(&token, 47322, n), "포트");
        assert_ne!(p, hello_proof(&hex('b'), 47321, n), "토큰");
        assert_ne!(p, hello_proof(&token, 47321, "0123456789abcdee"), "n");
    }

    #[test]
    fn nonce_is_16_to_128_hex() {
        assert!(valid_nonce(&"a".repeat(16)));
        assert!(valid_nonce(&"F".repeat(128)));
        assert!(valid_nonce(&hex('c')));
        assert!(!valid_nonce(&"a".repeat(15)));
        assert!(!valid_nonce(&"a".repeat(129)));
        assert!(!valid_nonce(""));
        assert!(!valid_nonce(&"g".repeat(16)));
        assert!(!valid_nonce(&format!("{}%20", "a".repeat(16))));
        assert!(!valid_nonce(&format!("{} ", "a".repeat(16))));
    }
}
