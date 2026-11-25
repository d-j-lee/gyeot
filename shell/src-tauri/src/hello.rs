//! 데몬 확인 — 토큰을 보내기 전에 (docs/contracts.md §13.1, G-032).
//!
//! `47321`은 모든 OS 사용자가 같이 쓰는 포트라서, 내 데몬이 없을 때 다른 사용자가 먼저 잡을 수 있다.
//! 셸은 토큰을 보내기 전에 32바이트 난수 `n`으로 `GET /v1/hello?n=<n>`(인증 머리 없음)을 묻고,
//! 답의 `proof`가 `hex(HMAC-SHA256(key = 토큰 문자열의 UTF-8 바이트,
//! msg = "gyeot-hello/v1|" + <셸이 연결한 포트> + "|" + n))`와 같은지 상수 시간으로 비교한다.
//! 포트를 넣었으므로 다른 포트의 곁 데몬으로 중계하는 서버도 걸러진다.

use hmac::{Hmac, Mac};
use sha2::Sha256;

/// 증명 메시지의 머리.
pub const PREFIX: &str = "gyeot-hello/v1";

/// 확인에 실패했을 때 사람에게 보이는 이유의 머리(대체 화면·`shell.log`).
pub const NOT_MINE: &str =
    "이 포트의 서버는 내 곁 데몬이 아니에요(다른 사용자나 프로그램일 수 있어요)";

/// 32바이트 난수의 소문자 hex 64자. 미리 짐작할 수 있으면 다른 사용자가 진짜 데몬에 같은 `n`을
/// 먼저 물어 증명을 모아 둘 수 있으므로 운영체제 난수를 쓴다.
pub fn new_nonce() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("난수를 만들지 못했어요: {e}"))?;
    Ok(to_hex(&bytes))
}

/// 확인 요청 경로. `n`은 hex만 받는다(주소에 그대로 들어간다).
pub fn path(nonce: &str) -> String {
    format!("/v1/hello?n={nonce}")
}

fn mac(token: &str, port: u16, nonce: &str) -> Hmac<Sha256> {
    // HMAC은 어떤 길이의 열쇠든 받는다(new_from_slice는 실패하지 않는다).
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(token.as_bytes())
        .expect("HMAC 열쇠는 길이 제한이 없다");
    mac.update(format!("{PREFIX}|{port}|{nonce}").as_bytes());
    mac
}

/// 이 토큰·포트·난수에 대한 증명(소문자 hex). 시험과 기록용이다. 비교는 [`check`]가 상수 시간으로 한다.
#[cfg(test)]
pub fn proof_hex(token: &str, port: u16, nonce: &str) -> String {
    to_hex(&mac(token, port, nonce).finalize().into_bytes())
}

/// `GET /v1/hello` 응답이 내 토큰과 **내가 연결한 포트**로 계산한 증명인지 본다. 아니면 이유.
pub fn check(status: u16, body: &[u8], token: &str, port: u16, nonce: &str) -> Result<(), String> {
    if status != 200 {
        return Err(format!(
            "`/v1/hello`가 HTTP {status}를 돌려줬어요(곁 데몬이 아니거나 확인을 모르는 옛 판)"
        ));
    }
    let v: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| "확인 응답이 JSON이 아니에요".to_string())?;
    if v.get("v").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err("확인 응답의 판(v)이 1이 아니에요".into());
    }
    let proof = v
        .get("proof")
        .and_then(serde_json::Value::as_str)
        .and_then(decode_hex32)
        .ok_or_else(|| "확인 응답에 64자 hex 증명이 없어요".to_string())?;
    // verify_slice는 상수 시간으로 비교한다(subtle).
    mac(token, port, nonce)
        .verify_slice(&proof)
        .map_err(|_| "확인 값이 맞지 않아요(이 서버는 내 토큰을 몰라요)".to_string())
}

fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(DIGITS[usize::from(b >> 4)]));
        s.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    s
}

fn decode_hex32(s: &str) -> Option<[u8; 32]> {
    let raw = s.as_bytes();
    if raw.len() != 64 {
        return None;
    }
    let nib = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    let mut out = [0u8; 32];
    for (i, pair) in raw.chunks_exact(2).enumerate() {
        out[i] = (nib(pair[0])? << 4) | nib(pair[1])?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn nonce() -> String {
        "ab".repeat(32)
    }

    fn body(proof: &str) -> Vec<u8> {
        format!(r#"{{"v":1,"proof":"{proof}"}}"#).into_bytes()
    }

    #[test]
    fn proof_matches_the_shared_test_vector() {
        // 다른 구현(서버·channel·Python hmac)과 같은 값. docs/contracts.md §13.1의 식 그대로.
        assert_eq!(
            proof_hex(TOKEN, 47321, &nonce()),
            "cb2a3490aa613ccf14c93af5162080aff69a29f105ef62e3ef4777d7f85bd4c5"
        );
        assert_eq!(
            proof_hex(TOKEN, 47322, &nonce()),
            "c909b0dde49a2587904b41e63cd46b3faa9674ea124aa03d002b4b1235a683a1"
        );
    }

    #[test]
    fn check_accepts_only_my_token_and_my_port() {
        let n = nonce();
        let good = proof_hex(TOKEN, 47321, &n);
        assert_eq!(check(200, &body(&good), TOKEN, 47321, &n), Ok(()));
        // 대문자 hex도 같은 값이다
        assert_eq!(
            check(200, &body(&good.to_uppercase()), TOKEN, 47321, &n),
            Ok(())
        );
        // 다른 포트로 계산한 증명(다른 포트의 데몬으로 중계)
        let other_port = proof_hex(TOKEN, 47322, &n);
        assert!(check(200, &body(&other_port), TOKEN, 47321, &n).is_err());
        // 다른 토큰(다른 사용자의 곁 데몬)
        let other_token = proof_hex(&"f".repeat(64), 47321, &n);
        assert!(check(200, &body(&other_token), TOKEN, 47321, &n).is_err());
        // 다른 난수(옛 응답 되풀이)
        let replay = proof_hex(TOKEN, 47321, &"cd".repeat(32));
        assert!(check(200, &body(&replay), TOKEN, 47321, &n).is_err());
        // 모양이 틀린 답
        for bad in [
            format!(r#"{{"v":2,"proof":"{good}"}}"#),
            format!(r#"{{"proof":"{good}"}}"#),
            r#"{"v":1}"#.to_string(),
            format!(r#"{{"v":1,"proof":"{}"}}"#, &good[..63]),
            format!(r#"{{"v":1,"proof":"{good}0"}}"#),
            format!(r#"{{"v":1,"proof":"{}g"}}"#, &good[..63]),
            r#"{"ok":true,"version":"0.1.0"}"#.to_string(),
            "<html>".to_string(),
        ] {
            assert!(
                check(200, bad.as_bytes(), TOKEN, 47321, &n).is_err(),
                "{bad}"
            );
        }
        // 200이 아니면 본문을 보지 않는다
        assert!(check(404, &body(&good), TOKEN, 47321, &n).is_err());
    }

    #[test]
    fn nonce_is_fresh_64_lowercase_hex() {
        let a = new_nonce().unwrap();
        let b = new_nonce().unwrap();
        assert_eq!(a.len(), 64);
        assert!(a
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
        assert_ne!(a, b, "난수는 매번 달라야 한다");
        assert_eq!(path(&a), format!("/v1/hello?n={a}"));
    }
}
