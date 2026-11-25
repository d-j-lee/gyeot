//! 실행 파일 시험: `--version`, `--init`, `--print-config`.

use std::process::Command;

fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_gyeotd"));
    c.env_remove("GYEOT_PORT").env_remove("GYEOT_FACE_DIR");
    c
}

#[test]
fn version() {
    let out = bin().arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("gyeotd {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn init_creates_home_and_token() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let out = bin()
        .arg("--init")
        .env("GYEOT_HOME", &home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("데이터 폴더:"), "{stdout}");
    assert!(stdout.contains(&home.display().to_string()), "{stdout}");
    let token = std::fs::read_to_string(home.join("token")).unwrap();
    assert_eq!(token.trim().len(), 64);
    assert!(home.join("events").is_dir());
    assert!(home.join("spool").is_dir());
    // 두 번 해도 토큰은 그대로
    bin()
        .arg("--init")
        .env("GYEOT_HOME", &home)
        .output()
        .unwrap();
    assert_eq!(std::fs::read_to_string(home.join("token")).unwrap(), token);
}

#[test]
fn print_config_shows_effective_values() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("config.toml"),
        "port = 5555\nquiet_hours = [\"23:00-07:00\"]\nutc_offset_minutes = 540\n",
    )
    .unwrap();
    let out = bin()
        .arg("--print-config")
        .env("GYEOT_HOME", d.path())
        .env("GYEOT_PORT", "6000")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("port = 6000"), "{s}");
    assert!(s.contains("23:00-07:00"), "{s}");
    assert!(s.contains("utc_offset_minutes = 540"), "{s}");
}

#[test]
fn bad_config_fails_with_message() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("config.toml"),
        "quiet_hours = [\"99:00-01:00\"]",
    )
    .unwrap();
    let out = bin()
        .arg("--print-config")
        .env("GYEOT_HOME", d.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("곁 데몬 오류"));
}

#[test]
fn unknown_argument_exits_2() {
    let out = bin().arg("--nope").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}
