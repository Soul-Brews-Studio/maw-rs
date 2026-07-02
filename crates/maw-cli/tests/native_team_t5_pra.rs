use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-team-t5-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    fs::create_dir_all(path.join(".git")).expect("git marker");
    fs::create_dir_all(path.join("home")).expect("home");
    fs::create_dir_all(path.join("maw-home")).expect("maw home");
    fs::create_dir_all(path.join("psi")).expect("psi");
    path
}

fn run(args: &[&str], root: &Path) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_RS_TEAM_PSI", root.join("psi"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs")
}

fn run_fake_exec(args: &[&str], root: &Path, log: &Path) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_RS_TEAM_PSI", root.join("psi"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_TEAM_FAKE_SPAWN_LOG", log)
        .env("MAW_RS_SELF_BIN", "/fake/maw")
        .output()
        .expect("run maw-rs")
}

fn normalize(root: &Path, bytes: Vec<u8>) -> String {
    let mut text = String::from_utf8(bytes).expect("utf8");
    if let Ok(canonical_root) = root.canonicalize() {
        text = text.replace(&canonical_root.display().to_string(), "<ROOT>");
    }
    text.replace(&root.display().to_string(), "<ROOT>")
        .replace(&bin().display().to_string(), "<BIN>")
}

fn create_team(root: &Path, team: &str) {
    let output = run(&["team", "create", team], root);
    assert!(
        output.status.success(),
        "create stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_charter(root: &Path) -> PathBuf {
    fs::create_dir_all(root.join("agents/builder")).expect("builder dir");
    fs::create_dir_all(root.join("agents/reviewer")).expect("reviewer dir");
    let path = root.join("team.json");
    fs::write(
        &path,
        r#"{"name":"beta","description":"Beta team","goal":"Ship T5","governance":{"requires_human_approval":true},"members":[{"role":"builder","model":"gpt-5.5","cwd":"agents/builder","prompt":"Build"},{"role":"reviewer","engine":"codex","cwd":"agents/reviewer"}]}"#,
    )
    .expect("charter");
    path
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}

#[test]
fn team_t5_spawn_print_only_golden_and_atomic_files() {
    let root = temp_dir("spawn-print");
    fs::create_dir_all(root.join("agents/builder")).expect("builder dir");
    create_team(&root, "alpha");
    let output = run(
        &[
            "team",
            "spawn",
            "alpha",
            "builder",
            "--engine",
            "codex",
            "--model",
            "gpt-5.5",
            "--cwd",
            "agents/builder",
            "--prompt",
            "Ship",
            "now",
        ],
        &root,
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        normalize(&root, output.stdout),
        include_str!("fixtures/native-team-t5/team-spawn-print.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    let prompt = root.join("psi/memory/mailbox/teams/alpha/builder-spawn-prompt.md");
    assert!(fs::read_to_string(&prompt)
        .expect("prompt")
        .contains("Ship now"));
    let manifest = fs::read_to_string(root.join("psi/memory/mailbox/teams/alpha/manifest.json"))
        .expect("manifest");
    assert!(manifest.contains("builder"));
    #[cfg(unix)]
    assert_eq!(mode(&prompt), 0o600);
}

#[test]
fn team_t5_spawn_exec_uses_fake_spawn_runner_no_shell_shape() {
    let root = temp_dir("spawn-exec");
    fs::create_dir_all(root.join("agents/builder")).expect("builder dir");
    create_team(&root, "alpha");
    let log = root.join("spawn.jsonl");
    let output = run_fake_exec(
        &[
            "team",
            "spawn",
            "alpha",
            "builder",
            "--engine",
            "codex",
            "--cwd",
            "agents/builder",
            "--exec",
        ],
        &root,
        &log,
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        normalize(&root, output.stdout),
        include_str!("fixtures/native-team-t5/team-spawn-exec.stdout")
    );
    let line = fs::read_to_string(&log).expect("spawn log");
    let json: serde_json::Value = serde_json::from_str(line.trim()).expect("json log");
    assert_eq!(json["program"], "/fake/maw");
    assert_eq!(json["args"].as_array().expect("args")[0], "wake");
    assert!(json["args"]
        .as_array()
        .expect("args")
        .iter()
        .any(|v| v == "--repo-path"));
    assert!(json["args"]
        .as_array()
        .expect("args")
        .iter()
        .all(|v| v.as_str() != Some("-c")));
}

#[test]
fn team_t5_spawn_from_governance_blocks_then_approved_execs() {
    let root = temp_dir("spawn-from");
    let charter = write_charter(&root);
    let charter_s = charter.to_string_lossy().into_owned();
    let blocked = run(&["team", "spawn-from", &charter_s], &root);
    assert!(!blocked.status.success());
    assert_eq!(
        String::from_utf8(blocked.stderr).expect("stderr"),
        include_str!("fixtures/native-team-t5/team-spawn-from-approval.stderr")
    );
    assert!(!root
        .join("psi/memory/mailbox/teams/beta/manifest.json")
        .exists());

    let log = root.join("spawn.jsonl");
    let approved = run_fake_exec(
        &["team", "spawn-from", &charter_s, "--approve", "--exec"],
        &root,
        &log,
    );
    assert!(
        approved.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&approved.stderr)
    );
    assert_eq!(
        normalize(&root, approved.stdout),
        include_str!("fixtures/native-team-t5/team-spawn-from-approved.stdout")
    );
    let logs = fs::read_to_string(&log).expect("spawn log");
    assert_eq!(logs.lines().count(), 2);
    assert!(logs.contains("builder"));
    assert!(logs.contains("reviewer"));
}

#[test]
fn team_t5_rejects_injection_before_writes_or_spawn() {
    let root = temp_dir("guards");
    fs::create_dir_all(root.join("agents/builder")).expect("builder dir");
    create_team(&root, "alpha");
    for args in [
        vec!["team", "spawn", "alpha", "-bad"],
        vec!["team", "spawn", "alpha", "builder", "--engine", "-bad"],
        vec!["team", "spawn", "alpha", "builder", "--cwd", "../outside"],
        vec![
            "team",
            "spawn",
            "alpha",
            "builder",
            "--session-id",
            "bad/session",
        ],
    ] {
        let output = run(&args, &root);
        assert!(!output.status.success(), "{args:?}");
    }
    assert!(!root
        .join("psi/memory/mailbox/teams/alpha/-bad-spawn-prompt.md")
        .exists());
}
