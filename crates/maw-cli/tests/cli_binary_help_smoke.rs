use std::path::PathBuf;
use std::process::Command;

fn maw_rs_bin() -> PathBuf {
    let cargo_bin = PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"));
    if cargo_bin.exists() {
        return cargo_bin;
    }
    let mut current = std::env::current_exe().expect("current test exe");
    current.pop();
    if current.file_name().is_some_and(|name| name == "deps") {
        current.pop();
    }
    current.join("maw-rs")
}

#[test]
fn maw_rs_binary_smoke_runs_main_and_prints_help() {
    let output = Command::new(maw_rs_bin())
        .arg("--help")
        .output()
        .expect("run maw-rs binary");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    assert!(
        stdout.contains("usage: maw-rs <command> [args]"),
        "{stdout}"
    );
    assert!(output.stderr.is_empty());
}
