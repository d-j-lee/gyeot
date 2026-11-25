//! 훅을 띄운 에이전트의 PID(`pid_hint`). 부모가 셸이면 그 위를 본다(최대 3단계).

/// 부모 사이에 끼어 있을 수 있는 셸 이름(소문자, 확장자 없이).
const SHELLS: [&str; 9] = [
    "sh",
    "bash",
    "dash",
    "zsh",
    "fish",
    "cmd",
    "powershell",
    "pwsh",
    "conhost",
];
const MAX_HOPS: usize = 3;

fn is_shell(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    let n = n.strip_suffix(".exe").unwrap_or(&n);
    SHELLS.contains(&n)
}

/// 에이전트 PID.
pub fn agent_pid() -> Option<u32> {
    imp::agent_pid()
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{MAX_HOPS, is_shell};

    fn stat(pid: u32) -> Option<(String, u32)> {
        let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // pid (comm) state ppid ...  — comm에 괄호·공백이 있을 수 있어 마지막 ')'로 자른다.
        let open = s.find('(')?;
        let close = s.rfind(')')?;
        let comm = s.get(open + 1..close)?.to_owned();
        let ppid = s
            .get(close + 2..)?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()?;
        Some((comm, ppid))
    }

    pub fn agent_pid() -> Option<u32> {
        let mut pid = std::os::unix::process::parent_id();
        for _ in 0..MAX_HOPS {
            match stat(pid) {
                Some((comm, ppid)) if is_shell(&comm) && ppid > 1 => pid = ppid,
                _ => break,
            }
        }
        Some(pid)
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
mod imp {
    pub fn agent_pid() -> Option<u32> {
        Some(std::os::unix::process::parent_id())
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use super::{MAX_HOPS, is_shell};
    use std::collections::HashMap;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;

    /// PID → (부모 PID, 실행 파일 이름).
    fn table() -> Option<HashMap<u32, (u32, String)>> {
        // SAFETY: Toolhelp32 스냅숏 API를 문서대로 쓴다. 구조체 크기를 채우고, 핸들은 끝에 닫는다.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut map = HashMap::new();
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if Process32FirstW(snap, &mut e) != 0 {
                loop {
                    let len = e
                        .szExeFile
                        .iter()
                        .position(|c| *c == 0)
                        .unwrap_or(e.szExeFile.len());
                    let name = String::from_utf16_lossy(&e.szExeFile[..len]);
                    map.insert(e.th32ProcessID, (e.th32ParentProcessID, name));
                    if Process32NextW(snap, &mut e) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
            Some(map)
        }
    }

    pub fn agent_pid() -> Option<u32> {
        let map = table()?;
        // SAFETY: 인자 없는 단순 조회.
        let me = unsafe { GetCurrentProcessId() };
        let mut pid = map.get(&me)?.0;
        for _ in 0..MAX_HOPS {
            match map.get(&pid) {
                Some((pp, name)) if is_shell(name) && *pp != 0 && map.contains_key(pp) => pid = *pp,
                _ => break,
            }
        }
        Some(pid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_names() {
        for s in [
            "sh",
            "bash",
            "BASH.EXE",
            "cmd.exe",
            "powershell.exe",
            "pwsh",
            "zsh",
        ] {
            assert!(is_shell(s), "{s}");
        }
        for s in ["node", "node.exe", "claude", "codex.exe", "python3"] {
            assert!(!is_shell(s), "{s}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_walks_up_through_shells() {
        // sh -c 'gyeot 시험 실행 파일' 아래에서 돌면 sh를 건너뛴다.
        let pid = agent_pid().unwrap();
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        assert!(!is_shell(&comm) || pid <= 1, "{comm}");
    }
}
