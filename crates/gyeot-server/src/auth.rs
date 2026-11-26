//! 얼굴 열기(일회용 부트 코드 → 세션 열쇠)와 데몬 확인(계약 §7·§12.1·§13.1).
//!
//! 토큰을 가진 쪽(셸·`gyeotd --open`)이 `POST /v1/boot`로 코드를 받고, 얼굴 스크립트가
//! `POST /v1/session`으로 그 코드를 세션 열쇠로 바꾼다. 열쇠는 `Authorization: Bearer`로만 받는다.
//! 쿠키는 포트를 가리지 않아 같은 호스트의 다른 포트로 새므로 쓰지 않는다(G-030).
//! 세션은 **데몬 메모리에만** 있고 세션 값의 SHA-256만 든다. 디스크에 쓰지 않으므로 데몬이 다시 뜨면
//! 이전 열쇠는 모두 무효다(§14.1, G-036). 얼굴은 열쇠를 보내기 전에 `/v1/hello`의 `face` 목록
//! ([`Sessions::face_proofs`])으로 상대가 내 세션을 아는지 확인한다.

use crate::store::new_token;
use anyhow::Result;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;

/// 부트 코드가 살아 있는 시간.
pub const BOOT_TTL_MS: i64 = 120_000;
/// 동시에 열어 두는 부트 코드 수.
pub const BOOT_MAX: usize = 8;
/// 세션이 살아 있는 시간(만든 때부터 7일, 써도 늘지 않는다).
pub const SESSION_TTL_MS: i64 = 7 * 86_400_000;
/// 기억하는 세션 수.
pub const SESSION_MAX: usize = 20;
/// hello 증명에 묶는 글의 머리(계약 §13.1).
pub const HELLO_PREFIX: &str = "gyeot-hello/v1|";
/// hello의 얼굴 증명에 묶는 글의 머리(계약 §14.1).
pub const FACE_HELLO_PREFIX: &str = "gyeot-face-hello/v1|";

/// 32바이트 난수 hex(64자)인가.
fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    crate::http::ct_eq(a, b)
}

/// 세션 값(UTF-8)의 SHA-256 32바이트. 서버가 들고 있는 것은 이것뿐이다.
pub fn session_digest(id: &str) -> [u8; 32] {
    Sha256::digest(id.as_bytes()).into()
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

/// 계약 §14.1의 얼굴 증명: `hex(HMAC-SHA256(key = SHA-256(세션 값) 32바이트,
/// msg = "gyeot-face-hello/v1|<포트>|<n>"))`. 얼굴은 자기 열쇠로 같은 값을 계산해(WebCrypto) 목록에서 찾는다.
/// 열쇠도 해시도 내보이지 않고, `n`마다 달라 다시 쓸 수 없다.
pub fn face_proof(digest: &[u8; 32], port: u16, n: &str) -> String {
    let msg = format!("{FACE_HELLO_PREFIX}{port}|{n}");
    hmac_sha256_hex(digest, msg.as_bytes())
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

/// 세션 하나(메모리에만). 세션 값 자체는 들고 있지 않다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    /// 세션 값의 SHA-256.
    pub digest: [u8; 32],
    /// 만든 시각.
    pub created_ms: i64,
    /// 마지막으로 쓴 시각(개수 상한에서 버릴 순서에만 쓴다).
    pub last_ms: i64,
    /// 끝나는 시각(만든 시각 + 7일).
    pub expires_ms: i64,
}

/// 세션 목록. 데몬 메모리에만 있고 파일에 쓰지도 읽지도 않는다(§14.1, G-036).
#[derive(Debug, Default)]
pub struct Sessions {
    list: Vec<SessionRecord>,
}

impl Sessions {
    /// 빈 목록. 데몬이 뜰 때마다 빈 채로 시작한다.
    pub fn new() -> Sessions {
        Sessions::default()
    }

    /// 새 세션을 만들고 (세션 열쇠, 끝나는 시각)을 돌려준다. 열쇠 자체는 들고 있지 않는다.
    /// 20개가 넘으면 가장 오래 안 쓴 것부터 버린다.
    pub fn create(&mut self, now_ms: i64) -> Result<(String, i64)> {
        let id = new_token()?;
        self.insert(&id, now_ms);
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
            digest: session_digest(id),
            created_ms: now_ms,
            last_ms: now_ms,
            expires_ms: now_ms + SESSION_TTL_MS,
        });
    }

    fn purge(&mut self, now_ms: i64) {
        self.list.retain(|r| r.expires_ms > now_ms);
    }

    /// 세션 열쇠가 살아 있는 세션인가(해시를 상수 시간으로 대조). 맞으면 마지막 사용 시각을 갱신한다.
    pub fn validate(&mut self, id: &str, now_ms: i64) -> bool {
        if !is_hex64(id) {
            return false;
        }
        self.purge(now_ms);
        let h = session_digest(id);
        let mut found = false;
        for r in &mut self.list {
            if ct_eq(&r.digest, &h) {
                if now_ms > r.last_ms {
                    r.last_ms = now_ms;
                }
                found = true;
            }
        }
        found
    }

    /// hello의 `face` 목록(§14.1): 살아 있는 세션마다 [`face_proof`], 순서는 섞는다(최대 20개).
    pub fn face_proofs(&self, port: u16, n: &str, now_ms: i64) -> Vec<String> {
        let mut out: Vec<(u64, String)> = self
            .list
            .iter()
            .filter(|r| r.expires_ms > now_ms)
            .take(SESSION_MAX)
            .map(|r| (shuffle_key(), face_proof(&r.digest, port, n)))
            .collect();
        out.sort_unstable();
        out.into_iter().map(|(_, p)| p).collect()
    }

    /// 세션 기록(시험·진단용).
    pub fn records(&self) -> &[SessionRecord] {
        &self.list
    }
}

/// 순서 섞기용 난수(비밀이 아니다). 난수원이 없으면 0이라 만든 순서 그대로가 된다.
fn shuffle_key() -> u64 {
    let mut b = [0u8; 8];
    let _ = getrandom::fill(&mut b);
    u64::from_le_bytes(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 셸·channel 시험과 같은 공유 값(계약 §13.1). 부품끼리 식이 어긋나면 여기서 깨진다.
    #[test]
    fn hello_proof_matches_shared_vectors() {
        let token = "0123456789abcdef".repeat(4);
        let n = "ab".repeat(32);
        assert_eq!(
            hello_proof(&token, 47321, &n),
            "cb2a3490aa613ccf14c93af5162080aff69a29f105ef62e3ef4777d7f85bd4c5"
        );
        assert_eq!(
            hello_proof(&token, 47322, &n),
            "c909b0dde49a2587904b41e63cd46b3faa9674ea124aa03d002b4b1235a683a1"
        );
    }

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
        let mut s = Sessions::new();
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

        let mut s = Sessions::new();
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

    /// 얼굴 담당과 같은 공유 값(파이썬 hmac·hashlib으로 따로 계산, 계약 §14.1).
    #[test]
    fn face_proof_matches_shared_vectors() {
        let session = "0123456789abcdef".repeat(4);
        let n = "ab".repeat(32);
        let d = session_digest(&session);
        assert_eq!(
            crate::store::hex(&d),
            "a8ae6e6ee929abea3afcfc5258c8ccd6f85273e0d4626d26c7279f3250f77c8e"
        );
        assert_eq!(
            face_proof(&d, 47321, &n),
            "607b8f06e715b5b23515fbb572a63617cf35019b687b1444f4433e27a7d368e4"
        );
        assert_eq!(
            face_proof(&d, 47322, &n),
            "fcb875c58193eea573405310509b4ded38cafc4c0296e5189bad09d3a6d83f91"
        );
        // 열쇠는 해시 32바이트(hex 글자가 아니다), 머리는 얼굴용(토큰 증명과 섞이지 않는다)
        assert_eq!(
            face_proof(&d, 47321, &n),
            hmac_sha256_hex(&d, format!("gyeot-face-hello/v1|47321|{n}").as_bytes())
        );
        assert_ne!(
            face_proof(&d, 47321, &n),
            hmac_sha256_hex(
                crate::store::hex(&d).as_bytes(),
                format!("gyeot-face-hello/v1|47321|{n}").as_bytes()
            )
        );
        assert_ne!(
            face_proof(&d, 47321, &n),
            hello_proof(&crate::store::hex(&d), 47321, &n)
        );
        assert_ne!(
            face_proof(&d, 47321, &n),
            face_proof(&d, 47321, &"cd".repeat(32))
        );
    }

    /// §14.1: 살아 있는 세션만, 세션마다 하나, 순서는 섞는다. 지난 세션은 빠진다.
    #[test]
    fn face_proofs_list_live_sessions_shuffled() {
        let n = "ab".repeat(32);
        let mut s = Sessions::new();
        assert!(s.face_proofs(47321, &n, T).is_empty());
        let (a, _) = s.create(T).unwrap();
        let (b, _) = s.create(T + 10).unwrap();
        let mut got = s.face_proofs(47321, &n, T + 20);
        got.sort();
        let mut want = vec![
            face_proof(&session_digest(&a), 47321, &n),
            face_proof(&session_digest(&b), 47321, &n),
        ];
        want.sort();
        assert_eq!(got, want);
        // a는 만든 때부터 7일이 지나면 빠진다(목록을 바꾸지 않고 걸러도)
        assert_eq!(
            s.face_proofs(47321, &n, T + SESSION_TTL_MS),
            vec![face_proof(&session_digest(&b), 47321, &n)]
        );
        assert!(s.face_proofs(47321, &n, T + 10 + SESSION_TTL_MS).is_empty());
        // 20개를 여러 번 받으면 순서가 한 가지만 나오지 않는다
        let mut s = Sessions::new();
        for i in 0..SESSION_MAX as i64 {
            s.create(T + i).unwrap();
        }
        let orders: std::collections::HashSet<Vec<String>> =
            (0..6).map(|_| s.face_proofs(47321, &n, T + 100)).collect();
        assert!(orders.len() > 1, "순서를 섞는다");
        assert!(orders.iter().all(|o| o.len() == SESSION_MAX));
    }

    /// §14.1: 세션은 메모리에만. 새 목록은 늘 비어 있다(파일을 읽는 길이 없다).
    #[test]
    fn new_sessions_start_empty() {
        let s = Sessions::new();
        assert!(s.records().is_empty());
        let mut s2 = Sessions::new();
        assert!(!s2.validate(&hex('9'), T));
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
