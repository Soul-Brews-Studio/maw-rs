#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
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
    let path = std::env::temp_dir().join(format!("maw-rs-team-t4-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn seed(root: &Path) {
    fs::create_dir_all(root.join("ψ/teams")).expect("teams");
    fs::create_dir_all(root.join("home/.claude/teams/alpha/inboxes")).expect("inboxes");
    fs::create_dir_all(root.join("home/.claude/teams/alpha/builder")).expect("builder dir");
    fs::write(
        root.join("ψ/teams/alpha.yaml"),
        r"name: alpha
session: alpha
description: Alpha team
goal: Ship T4 down
members:
  - role: lead
    name: lead
    engine: claude
    cwd: agents/lead
  - role: builder
    name: builder
    engine: codex
    cwd: agents/builder
  - role: reviewer
    name: reviewer
    engine: claude
    cwd: agents/reviewer
",
    )
    .expect("charter");
    fs::write(
        root.join("home/.claude/teams/alpha/config.json"),
        r#"{"name":"alpha","members":[{"name":"lead"},{"name":"builder"},{"name":"reviewer"}],"createdAt":1}"#,
    )
    .expect("config");
    fs::write(
        root.join("home/.claude/teams/alpha/inboxes/builder.json"),
        r#"[{"message":"inbox"}]"#,
    )
    .expect("inbox");
    fs::write(
        root.join("home/.claude/teams/alpha/builder/builder_findings.md"),
        "finding\n",
    )
    .expect("findings");
}

fn run_with_panes(
    args: &[&str],
    root: &Path,
    panes: &str,
    fake_log: Option<&Path>,
) -> std::process::Output {
    let mut command = Command::new(bin());
    command
        .args(args)
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_RS_TEAM_PSI", root.join("ψ"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_TEAM_TMUX_PANES", panes);
    if let Some(log) = fake_log {
        command.env("MAW_RS_TEAM_DOWN_FAKE_LOG", log);
    }
    command.output().expect("run maw-rs")
}

fn run(args: &[&str], root: &Path, fake_log: Option<&Path>) -> std::process::Output {
    run_with_panes(
        args,
        root,
        "alpha|lead|claude|/repo/agents/lead|%0\nalpha|builder|codex|/repo/agents/builder|%1\nalpha|reviewer|bash|/repo/agents/reviewer|%2",
        fake_log,
    )
}

fn assert_stdout_golden(name: &str, root: &Path, args: &[&str], expected: &str) {
    let log = root.join("fake/down.log");
    let output = run(args, root, Some(&log));
    assert!(
        output.status.success(),
        "{name} stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        expected,
        "{name}"
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn team_t4_down_status_dryrun_live_goldens_are_hermetic() {
    let root = temp_dir("goldens");
    seed(&root);

    assert_stdout_golden(
        "status",
        &root,
        &["team", "down", "alpha", "--status"],
        include_str!("fixtures/native-team-t4-down/team-down-status.stdout"),
    );
    assert!(
        !root.join("fake/down.log").exists(),
        "status must not touch fake done log"
    );
    assert!(
        !root
            .join("ψ/memory/mailbox/builder/team-alpha-archive")
            .exists(),
        "status must not archive"
    );

    assert_stdout_golden(
        "dry-run",
        &root,
        &["team", "down", "alpha", "--dry-run"],
        include_str!("fixtures/native-team-t4-down/team-down-dry-run.stdout"),
    );
    assert!(
        !root.join("fake/down.log").exists(),
        "dry-run must not touch fake done log"
    );
    assert!(
        !root
            .join("ψ/memory/mailbox/builder/team-alpha-archive")
            .exists(),
        "dry-run must not archive"
    );

    assert_stdout_golden(
        "live-plan",
        &root,
        &["team", "down", "alpha"],
        include_str!("fixtures/native-team-t4-down/team-down-live.stdout"),
    );
    assert_eq!(
        fs::read_to_string(root.join("fake/down.log")).expect("fake log"),
        "archive\tbuilder\ndone\talpha:builder\n",
        "archive must be recorded before cmdDone"
    );
    assert!(root
        .join("ψ/memory/mailbox/builder/team-alpha-archive/inbox.json")
        .exists());
    assert!(root
        .join("ψ/memory/mailbox/builder/team-alpha-archive/builder_findings.md")
        .exists());
    assert!(root
        .join("ψ/memory/mailbox/teams/alpha/manifest.json")
        .exists());
    assert!(
        root.join("home/.claude/teams/alpha/config.json").exists(),
        "down must not rm team dir"
    );
}

#[test]
fn team_t4_down_validates_before_done_and_rejects_injection() {
    let root = temp_dir("guards");
    seed(&root);
    let log = root.join("fake/down.log");
    let duplicate = "alpha|lead|claude|/repo/agents/lead|%0\nalpha|builder|codex|/repo/agents/builder|%1\nalpha|builder|codex|/repo/agents/builder|%2";
    let ambiguous = run_with_panes(&["team", "down", "alpha"], &root, duplicate, Some(&log));
    assert!(!ambiguous.status.success());
    assert!(String::from_utf8_lossy(&ambiguous.stderr)
        .contains("refuse ambiguous target before teardown"));
    assert!(!log.exists(), "reject-before-kill must not call cmdDone");

    let missing_log = root.join("fake/missing.log");
    let missing = run_with_panes(
        &["team", "down", "alpha"],
        &root,
        "alpha|lead|claude|/repo/agents/lead|%0",
        Some(&missing_log),
    );
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("refuse missing target before teardown")
    );
    assert!(
        !missing_log.exists(),
        "missing-target reject must not call cmdDone"
    );

    let bad_team = run(&["team", "down", "-bad"], &root, Some(&log));
    assert!(!bad_team.status.success());
    assert!(String::from_utf8_lossy(&bad_team.stderr).contains("leading dash rejected"));

    let bad_keep = run(
        &["team", "down", "alpha", "--keep", "-lead"],
        &root,
        Some(&log),
    );
    assert!(!bad_keep.status.success());
    assert!(String::from_utf8_lossy(&bad_keep.stderr).contains("leading dash rejected"));
}

#[test]
fn team_t4_down_refuses_no_session_before_teardown() {
    let root = temp_dir("no-session");
    seed(&root);
    let charter = root.join("ψ/teams/alpha.yaml");
    let text = fs::read_to_string(&charter)
        .expect("charter")
        .replace("session: alpha\n", "");
    fs::write(&charter, text).expect("charter without session");
    let log = root.join("fake/down.log");
    let output = run(&["team", "down", "alpha"], &root, Some(&log));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("refuse no-session before teardown"));
    assert!(!log.exists(), "no-session reject must not call cmdDone");
}

#[test]
fn team_t4_down_finds_a_window_wake_named_with_the_oracle_suffix() {
    // #785 sub-bug B: `maw team up` can end up creating (or attaching to)
    // a window literally named `<role>-oracle` for a member whose charter
    // role/name is the bare `<role>` -- the resolved repo/window ends up
    // carrying the suffix even though nothing in the charter asked for it.
    // `team down` must still find and close that window without a manual
    // `tmux rename-window` first; both up's classification and down's
    // target-finding share `team_t3_classify`, so fixing the shared match
    // predicate closes the round trip for both directions at once.
    let root = temp_dir("oracle-suffix-window");
    fs::create_dir_all(root.join("ψ/teams")).expect("teams");
    fs::create_dir_all(root.join("home/.claude/teams/dashteam/inboxes")).expect("inboxes");
    // A `lead` member is kept automatically -- included only so killing
    // `worker-a` doesn't also read as "would kill the last window in the
    // session" and route through the unrelated teardown-guard branch.
    fs::write(
        root.join("ψ/teams/dashteam.yaml"),
        r"name: dashteam
session: dashteam
description: Dash test
goal: Ship T785b
members:
  - role: lead
    name: lead
    engine: claude
    cwd: agents/lead
  - role: worker-a
    name: worker-a
    engine: codex
    cwd: agents/worker-a
",
    )
    .expect("charter");
    fs::write(
        root.join("home/.claude/teams/dashteam/config.json"),
        r#"{"name":"dashteam","members":[{"name":"lead"},{"name":"worker-a"}],"createdAt":1}"#,
    )
    .expect("config");

    let log = root.join("fake/down.log");
    let output = run_with_panes(
        &["team", "down", "dashteam"],
        &root,
        "dashteam|lead|claude|/repo/agents/lead|%0\ndashteam|worker-a-oracle|codex|/repo/agents/worker-a|%1",
        Some(&log),
    );
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(
        stdout.contains("worker-a\tlive\tmaw done worker-a-oracle"),
        "{stdout}"
    );
    assert_eq!(
        fs::read_to_string(&log).expect("fake log"),
        "archive\tworker-a\ndone\tdashteam:worker-a-oracle\n"
    );
}

#[test]
fn team_t4_down_does_not_report_a_false_clean_teardown_over_a_live_engine_it_does_not_recognize() {
    // #785 sub-bug C: the liveness check used to be an allowlist of known
    // engine binary names (claude/codex/omx/node/bun) -- any engine outside
    // that closed list (verified real case: `opencode`) was silently
    // classified "dead" and `team down` reported a clean rc=0 table while
    // the pane was genuinely still running. An unrecognized-but-real
    // process must still be treated as live and actually torn down, not
    // silently skipped behind a false-positive "clean" table -- an unknown
    // state reported as success is worse than an error.
    let root = temp_dir("unknown-engine-live");
    seed(&root);
    let log = root.join("fake/down.log");
    let output = run_with_panes(
        &["team", "down", "alpha"],
        &root,
        "alpha|lead|claude|/repo/agents/lead|%0\nalpha|builder|opencode|/repo/agents/builder|%1\nalpha|reviewer|opencode|/repo/agents/reviewer|%2",
        Some(&log),
    );
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(
        stdout.contains("builder\tlive\tmaw done builder"),
        "{stdout}"
    );
    assert!(
        stdout.contains("reviewer\tlive\tmaw done reviewer"),
        "{stdout}"
    );
    assert!(!stdout.contains("\tdead\t"), "{stdout}");
    assert_eq!(
        fs::read_to_string(&log).expect("fake log"),
        "archive\tbuilder\ndone\talpha:builder\narchive\treviewer\ndone\talpha:reviewer\n",
        "an unrecognized-but-live engine must actually be torn down, not silently skipped"
    );
}

#[test]
fn team_t4_down_guard_window_and_keep_parity() {
    let root = temp_dir("guard-window");
    seed(&root);
    let all_log = root.join("fake/all.log");
    let all = run_with_panes(
        &["team", "down", "alpha", "--all", "--keep", "reviewer"],
        &root,
        "alpha|lead|claude|/repo/agents/lead|%0\nalpha|builder|codex|/repo/agents/builder|%1",
        Some(&all_log),
    );
    assert!(
        all.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&all.stderr)
    );
    assert!(String::from_utf8_lossy(&all.stdout)
        .contains("session\tguard\tcreate maw-team-lifecycle-guard"));
    assert_eq!(
        fs::read_to_string(&all_log).expect("all log"),
        "guard\talpha:maw-team-lifecycle-guard\narchive\tlead\ndone\talpha:lead\narchive\tbuilder\ndone\talpha:builder\n"
    );

    let keep = run(
        &["team", "down", "alpha", "--keep", "builder", "--dry-run"],
        &root,
        None,
    );
    assert!(keep.status.success());
    let keep_out = String::from_utf8(keep.stdout).expect("stdout");
    assert!(keep_out.contains("lead\tlive\tkeep (lead)"));
    assert!(keep_out.contains("builder\tlive\tkeep (--keep)"));
}
