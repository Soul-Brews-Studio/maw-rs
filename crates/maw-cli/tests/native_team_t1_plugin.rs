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
    let path = std::env::temp_dir().join(format!("maw-rs-team-t1-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn write_charter(root: &Path) -> PathBuf {
    let builder = root.join("agents/builder");
    let reviewer = root.join("agents/reviewer");
    for worktree in [&builder, &reviewer] {
        fs::create_dir_all(worktree.join(".codex")).expect("worktree codex");
        fs::write(
            worktree.join(".codex/config.toml"),
            format!(
                "[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
                worktree.display()
            ),
        )
        .expect("trust config");
    }
    fs::create_dir_all(root.join("maw-home/config")).expect("maw config");
    fs::write(
        root.join("maw-home/config/maw.config.50.json"),
        r#"{"commands":{"local-builder":"CODEX_HOME=$PWD/.codex codex --model gpt-5.5","local-reviewer":"CODEX_HOME=$PWD/.codex codex --model gpt-5.5"}}"#,
    )
    .expect("maw commands");
    let path = root.join("team.json");
    fs::write(
        &path,
        format!(
            r#"{{"name":"beta","project":"acme/repo","description":"Beta team","goal":"Ship T1","session":"beta-session","members":[{{"role":"builder","model":"gpt-5.5","target":"auto","engine":"local-builder","worktree":"{}","branch":"agents/builder"}},{{"role":"reviewer","target":"auto","engine":"local-reviewer","worktree":"{}","branch":"agents/reviewer"}}]}}"#,
            builder.display(),
            reviewer.display()
        ),
    )
    .expect("charter");
    path
}

fn run(args: &[&str], root: &Path) -> std::process::Output {
    let home = root.join("home");
    let maw_home = root.join("maw-home");
    let psi = root.join("psi");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&maw_home).expect("maw home");
    fs::create_dir_all(&psi).expect("psi");
    Command::new(bin())
        .args(args)
        .env("HOME", &home)
        .env("MAW_HOME", &maw_home)
        .env("MAW_RS_TEAM_PSI", &psi)
        .env(
            "MAW_RS_TEAM_TMUX_PANES",
            "beta-session|lead|codex|/tmp|%1\n",
        )
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs")
}

fn normalize(root: &Path, bytes: Vec<u8>) -> String {
    String::from_utf8(bytes)
        .expect("stdout utf8")
        .replace(&root.display().to_string(), "<ROOT>")
}

fn assert_stdout_golden(name: &str, root: &Path, args: &[&str], expected: &str) {
    let output = run(args, root);
    assert!(
        output.status.success(),
        "{name} stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(normalize(root, output.stdout), expected, "{name}");
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}

#[test]
fn team_t1_committed_goldens_are_hermetic_without_js_ref() {
    let root = temp_dir("goldens");
    let charter = write_charter(&root);
    let charter_s = charter.to_string_lossy().into_owned();

    assert_stdout_golden(
        "create",
        &root,
        &["team", "create", "alpha", "--description", "Alpha", "team"],
        include_str!("fixtures/native-team-t1/team-create.stdout"),
    );
    assert_stdout_golden(
        "list",
        &root,
        &["team", "list"],
        include_str!("fixtures/native-team-t1/team-list.stdout"),
    );
    assert_stdout_golden(
        "status",
        &root,
        &["team", "status", "alpha"],
        include_str!("fixtures/native-team-t1/team-status.stdout"),
    );
    assert_stdout_golden(
        "tasks",
        &root,
        &["team", "tasks", "--team", "alpha"],
        include_str!("fixtures/native-team-t1/team-tasks.stdout"),
    );
    assert_stdout_golden(
        "members",
        &root,
        &["team", "members", "--team", "alpha"],
        include_str!("fixtures/native-team-t1/team-members.stdout"),
    );
    assert_stdout_golden(
        "lives",
        &root,
        &["team", "lives", "scout"],
        include_str!("fixtures/native-team-t1/team-lives.stdout"),
    );
    assert_stdout_golden(
        "plan",
        &root,
        &["team", "plan", &charter_s],
        include_str!("fixtures/native-team-t1/team-plan.stdout"),
    );
    assert_stdout_golden(
        "preflight",
        &root,
        &["team", "preflight", &charter_s],
        include_str!("fixtures/native-team-t1/team-preflight.stdout"),
    );
    assert_stdout_golden(
        "load",
        &root,
        &["team", "load", &charter_s, "--no-spawn"],
        include_str!("fixtures/native-team-t1/team-load.stdout"),
    );

    #[cfg(unix)]
    {
        assert_eq!(
            mode(&root.join("psi/memory/mailbox/teams/alpha/manifest.json")),
            0o600
        );
        assert_eq!(
            mode(&root.join("home/.claude/teams/alpha/config.json")),
            0o600
        );
        assert_eq!(
            mode(&root.join("psi/memory/mailbox/teams/beta/manifest.json")),
            0o600
        );
    }
}

#[test]
fn team_t1_rejects_unsafe_team_name_before_state_write() {
    let root = temp_dir("guard");
    let output = run(&["team", "create", "../bad"], &root);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("path traversal rejected"));
    assert!(!root.join("psi/memory/mailbox/teams").exists());
}

#[test]
fn team_t1_load_requires_no_spawn_and_never_executes() {
    let root = temp_dir("nospawn");
    let charter = write_charter(&root);
    let output = run(&["team", "load", &charter.to_string_lossy()], &root);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--no-spawn"));
    assert!(!root.join("home/.claude/teams/beta/config.json").exists());
}
