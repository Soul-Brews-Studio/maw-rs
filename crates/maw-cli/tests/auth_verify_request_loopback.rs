use std::{path::PathBuf, process::Command};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

#[test]
fn auth_verify_request_d2_loopback_matches_committed_golden_without_ref_checkout() {
    let output = Command::new(bin())
        .args([
            "auth",
            "verify-request",
            "--peer-ip",
            "127.0.0.1",
            "--now",
            "1700000000",
            "--plan-json",
        ])
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs auth verify-request d2");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/auth302/verify-request-d2-loopback.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}
