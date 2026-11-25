//! `[[tail]]` JSONL 따라 읽기: 위치를 기억하고, 잘림·교체를 알아챈다.

use crate::config::TailConfig;
use gyeot_core::Source;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// 한 번에 읽는 최대 바이트.
const READ_CHUNK: u64 = 4 * 1024 * 1024;
/// 줄 하나의 최대 길이.
const MAX_LINE: usize = 1024 * 1024;
/// 지문에 쓰는 첫 줄 최대 길이.
const FP_MAX: usize = 256;

/// 파일 하나의 따라 읽기 위치.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TailPos {
    /// 다음에 읽을 바이트 위치(늘 줄의 처음).
    pub offset: u64,
    /// 첫 줄의 FNV-1a 지문(첫 줄이 다 쓰였을 때만). 바뀌면 파일이 교체된 것이다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<u64>,
}

/// 모든 tail의 위치(`tails.json`). 열쇠는 설정의 경로 그대로.
pub type TailState = BTreeMap<String, TailPos>;

/// 따라 읽어 얻은 줄 하나(정규화 전).
#[derive(Debug, Clone, PartialEq)]
pub struct TailLine {
    /// source.
    pub source: Source,
    /// JSON 줄(설정의 `cwd`를 채워 넣은 것).
    pub value: Value,
}

fn key(t: &TailConfig) -> String {
    t.path.to_string_lossy().into_owned()
}

fn first_line_fingerprint(path: &Path) -> Option<u64> {
    let mut f = File::open(path).ok()?;
    let mut buf = vec![0u8; FP_MAX];
    let n = f.read(&mut buf).ok()?;
    let nl = buf[..n].iter().position(|b| *b == b'\n');
    match nl {
        Some(i) => Some(gyeot_core::model::fnv1a64(&buf[..i])),
        // 첫 줄이 아주 길면 앞부분만으로 지문을 만든다.
        None if n == FP_MAX => Some(gyeot_core::model::fnv1a64(&buf[..n])),
        None => None,
    }
}

/// 처음 보는 tail의 시작 위치: 파일이 이미 있으면 끝에서(옛 기록은 들이지 않는다), 없으면 처음부터.
pub fn init_state(tails: &[TailConfig], state: &mut TailState) {
    for t in tails {
        let k = key(t);
        if state.contains_key(&k) {
            continue;
        }
        let pos = match std::fs::metadata(&t.path) {
            Ok(m) => TailPos {
                offset: m.len(),
                fingerprint: first_line_fingerprint(&t.path),
            },
            Err(_) => TailPos::default(),
        };
        state.insert(k, pos);
    }
}

/// 모든 tail에서 새 줄을 읽는다. 위치가 바뀌었으면 `true`.
pub fn poll(tails: &[TailConfig], state: &mut TailState) -> (Vec<TailLine>, bool) {
    let mut out = Vec::new();
    let mut changed = false;
    for t in tails {
        let Some(source) = Source::parse(&t.source) else {
            continue;
        };
        let k = key(t);
        let pos = state.entry(k).or_default();
        let before = pos.clone();
        read_one(t, source, pos, &mut out);
        if *pos != before {
            changed = true;
        }
    }
    (out, changed)
}

fn read_one(t: &TailConfig, source: Source, pos: &mut TailPos, out: &mut Vec<TailLine>) {
    let Ok(meta) = std::fs::metadata(&t.path) else {
        // 파일이 사라졌다(교체 중). 다시 생기면 처음부터 읽는다.
        if pos.offset != 0 || pos.fingerprint.is_some() {
            *pos = TailPos::default();
        }
        return;
    };
    let len = meta.len();
    let fp = first_line_fingerprint(&t.path);
    let replaced = match (pos.fingerprint, fp) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    };
    if len < pos.offset || replaced {
        tracing::info!("{}이 잘리거나 바뀌어서 처음부터 읽어요", t.path.display());
        pos.offset = 0;
    }
    pos.fingerprint = fp;
    if len == pos.offset {
        return;
    }
    let Ok(mut f) = File::open(&t.path) else {
        return;
    };
    if f.seek(SeekFrom::Start(pos.offset)).is_err() {
        return;
    }
    let mut buf = Vec::new();
    if f.take(READ_CHUNK).read_to_end(&mut buf).is_err() {
        return;
    }
    let Some(last_nl) = buf.iter().rposition(|b| *b == b'\n') else {
        // 줄바꿈 없이 너무 길면 버리고 넘어간다.
        if buf.len() as u64 >= READ_CHUNK {
            pos.offset += buf.len() as u64;
        }
        return;
    };
    for line in buf[..last_nl].split(|b| *b == b'\n') {
        if line.len() > MAX_LINE {
            continue;
        }
        let text = String::from_utf8_lossy(line);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let Ok(mut v) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        if let (Some(o), Some(cwd)) = (v.as_object_mut(), t.cwd.as_ref())
            && !o.contains_key("cwd")
        {
            o.insert("cwd".into(), Value::String(cwd.clone()));
        }
        out.push(TailLine { source, value: v });
    }
    pos.offset += last_nl as u64 + 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn cfg(path: &Path) -> TailConfig {
        TailConfig {
            path: path.to_path_buf(),
            source: "fsmap-harness".into(),
            cwd: Some("C:/work/proj".into()),
        }
    }

    fn append(path: &Path, s: &str) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        f.write_all(s.as_bytes()).unwrap();
    }

    #[test]
    fn existing_file_starts_at_end_and_reads_new_lines() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("log.jsonl");
        append(&p, "{\"t\":1,\"event\":\"gate\",\"result\":\"fail\"}\n");
        let tails = vec![cfg(&p)];
        let mut st = TailState::new();
        init_state(&tails, &mut st);
        let (lines, _) = poll(&tails, &mut st);
        assert!(lines.is_empty());
        append(
            &p,
            "{\"t\":2,\"event\":\"gate\",\"result\":\"block\"}\n{\"t\":3,",
        );
        let (lines, changed) = poll(&tails, &mut st);
        assert!(changed);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].value["result"], "block");
        assert_eq!(lines[0].value["cwd"], "C:/work/proj");
        assert_eq!(lines[0].source, Source::FsmapHarness);
        // 덜 쓴 줄은 다 쓰인 뒤에 읽는다
        append(
            &p,
            "\"event\":\"gate\",\"result\":\"pass\",\"cwd\":\"/other\"}\nnot json\n\n",
        );
        let (lines, _) = poll(&tails, &mut st);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].value["result"], "pass");
        assert_eq!(lines[0].value["cwd"], "/other");
        let (lines, changed) = poll(&tails, &mut st);
        assert!(lines.is_empty());
        assert!(!changed);
    }

    #[test]
    fn missing_file_reads_from_start_when_created() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("later.jsonl");
        let tails = vec![cfg(&p)];
        let mut st = TailState::new();
        init_state(&tails, &mut st);
        assert!(poll(&tails, &mut st).0.is_empty());
        append(&p, "{\"event\":\"gate\",\"result\":\"fail\"}\n");
        assert_eq!(poll(&tails, &mut st).0.len(), 1);
    }

    #[test]
    fn truncation_and_rotation_restart_from_zero() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("log.jsonl");
        let tails = vec![cfg(&p)];
        let mut st = TailState::new();
        init_state(&tails, &mut st);
        append(
            &p,
            "{\"n\":1,\"result\":\"a\"}\n{\"n\":2,\"result\":\"b\"}\n",
        );
        assert_eq!(poll(&tails, &mut st).0.len(), 2);
        // 잘림
        std::fs::write(&p, "{\"n\":3,\"result\":\"c\"}\n").unwrap();
        let (lines, _) = poll(&tails, &mut st);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].value["n"], 3);
        // 교체(새 파일이 옛 위치보다 길어도 첫 줄 지문으로 안다)
        std::fs::remove_file(&p).unwrap();
        std::fs::write(
            &p,
            "{\"n\":4,\"result\":\"d\",\"pad\":\"xxxxxxxxxxxxxxxxxxxx\"}\n{\"n\":5,\"result\":\"e\"}\n",
        )
        .unwrap();
        let (lines, _) = poll(&tails, &mut st);
        assert_eq!(
            lines
                .iter()
                .map(|l| l.value["n"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            vec![4, 5]
        );
    }

    #[test]
    fn state_serializes() {
        let mut st = TailState::new();
        st.insert(
            "a".into(),
            TailPos {
                offset: 5,
                fingerprint: Some(9),
            },
        );
        let s = serde_json::to_string(&st).unwrap();
        let back: TailState = serde_json::from_str(&s).unwrap();
        assert_eq!(back, st);
    }
}
