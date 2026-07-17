#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-native-park-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn chmod_exec(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod");
    }
}

/// Write a fake tmux that returns deterministic values.
/// `MAW_PARK_CWD` env var controls the `pane_current_path` response.
fn write_fake_tmux(bin_dir: &Path) {
    let tmux = bin_dir.join("tmux");
    // Fake: session=test-session, current window=win-a, windows=[win-a, win-b]
    // pane_current_path uses MAW_PARK_CWD.
    // dispatch on $1 (subcommand) then sniff remaining args.
    fs::write(
        &tmux,
        r#"#!/bin/sh
CMD="$1"
case "$CMD" in
  display-message)
    # Look for the format string in the arguments
    LAST=""
    for arg in "$@"; do LAST="$arg"; done
    case "$LAST" in
      '#S') printf 'test-session\n' ;;
      '#W') printf 'win-a\n' ;;
      '#{pane_current_path}') printf '%s\n' "${MAW_PARK_CWD:-/tmp/test-repo}" ;;
      *) printf 'unknown\n' ;;
    esac
    ;;
  list-windows)
    # Returns two windows
    printf '0:win-a\n1:win-b\n'
    ;;
  *)
    exit 0
    ;;
esac
exit 0
"#,
    )
    .expect("write fake tmux");
    chmod_exec(&tmux);
}

/// Write a fake git that returns deterministic values.
/// Dispatches on the git subcommand (after the -C <dir> prefix).
fn write_fake_git(bin_dir: &Path) {
    let git = bin_dir.join("git");
    fs::write(
        &git,
        r#"#!/bin/sh
# Skip -C <dir> args to find the git subcommand
SUBCMD=""
skip_next=0
for arg in "$@"; do
  if [ "$skip_next" = "1" ]; then skip_next=0; continue; fi
  if [ "$arg" = "-C" ]; then skip_next=1; continue; fi
  SUBCMD="$arg"
  break
done
case "$SUBCMD" in
  branch)  printf 'feat/x\n' ;;
  log)     printf 'abc123 msg\n' ;;
  status)  printf ' M file.rs\n' ;;
  *)       exit 1 ;;
esac
exit 0
"#,
    )
    .expect("write fake git");
    chmod_exec(&git);
}

fn write_fake_maw_delegated(bin_dir: &Path) {
    let maw = bin_dir.join("maw");
    fs::write(
        &maw,
        "#!/bin/sh\nprintf 'DELEGATED-MAW %s\\n' \"$*\"\nprintf 'bun\\n'\nexit 42\n",
    )
    .expect("write fake maw");
    chmod_exec(&maw);
}

fn run(root: &Path, args: &[&str], cwd: Option<&str>, with_tmux: bool) -> std::process::Output {
    let bin_dir = root.join("bin");
    let home = root.join("home");
    let xdg_config = root.join("xdg-config");
    let xdg_data = root.join("xdg-data");
    let xdg_state = root.join("xdg-state");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&xdg_data).expect("xdg data");
    fs::create_dir_all(&xdg_state).expect("xdg state");

    let mut cmd = Command::new(bin());
    cmd.args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", &bin_dir)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &xdg_config)
        .env("XDG_DATA_HOME", &xdg_data)
        .env("XDG_STATE_HOME", &xdg_state)
        .env("MAW_JS_REF_DIR", "/nonexistent");
    if let Some(park_cwd) = cwd {
        cmd.env("MAW_PARK_CWD", park_cwd);
    }
    if with_tmux {
        cmd.env("TMUX", root.join("tmux-socket"));
    }
    cmd.output().expect("run maw-rs")
}

/// Build an ISO 8601 timestamp for "now" (used for ls golden where we want "0m ago").
fn iso_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    // Gregorian conversion (same algorithm as park_iso_now in part291)
    let sec = secs % 60;
    let total_min = secs / 60;
    let minute = total_min % 60;
    let total_hours = total_min / 60;
    let hour = total_hours % 24;
    let total_days = total_hours / 24;
    let zday = total_days + 719_468;
    let era = zday / 146_097;
    let doe = zday - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{sec:02}.000Z")
}

/// Resolve where park stores its JSON files, given the test root.
/// Without `MAW_XDG` set, `maw_state_dir` = HOME/.maw, so parked dir = HOME/.maw/parked.
fn parked_dir_for(root: &Path) -> PathBuf {
    root.join("home").join(".maw").join("parked")
}

/// Seed a parked JSON file in the resolved parked directory.
#[allow(clippy::too_many_arguments)]
fn seed_parked_json(
    root: &Path,
    window: &str,
    session: &str,
    branch: &str,
    repo_cwd: &str,
    last_commit: &str,
    dirty_files: &[&str],
    note: &str,
    parked_at: &str,
) {
    let parked_dir = parked_dir_for(root);
    fs::create_dir_all(&parked_dir).expect("parked dir");
    let dirty_json: String = dirty_files
        .iter()
        .map(|f| format!("\"{}\"", f.replace('\"', "\\\"")))
        .collect::<Vec<_>>()
        .join(",");
    let content = format!(
        "{{\n  \"window\": \"{window}\",\n  \"session\": \"{session}\",\n  \
         \"branch\": \"{branch}\",\n  \"cwd\": \"{repo_cwd}\",\n  \
         \"lastCommit\": \"{last_commit}\",\n  \
         \"dirtyFiles\": [{dirty_json}],\n  \
         \"note\": \"{note}\",\n  \
         \"parkedAt\": \"{parked_at}\"\n}}\n"
    );
    fs::write(parked_dir.join(format!("{window}.json")), content).expect("write parked json");
}

#[test]
fn native_park_with_note_writes_file_and_stdout_golden() {
    let root = temp_dir("park-note");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_tmux(&bin_dir);
    write_fake_git(&bin_dir);
    write_fake_maw_delegated(&bin_dir);

    // A fake repo dir for the pane cwd
    let repo_dir = root.join("my-repo");
    fs::create_dir_all(&repo_dir).expect("repo dir");

    let out = run(
        &root,
        &["park", "a note"],
        Some(repo_dir.to_str().expect("path")),
        false,
    );
    assert!(
        out.status.success(),
        "exit={} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("stdout");
    let stderr = String::from_utf8(out.stderr).expect("stderr");

    // Stdout should be the park-ok golden format.
    assert!(
        stdout.contains("\x1b[32m\u{2713}\x1b[0m parked \x1b[33mwin-a\x1b[0m"),
        "stdout={stdout}"
    );
    assert!(stdout.contains("\"a note\""), "stdout={stdout}");
    assert_eq!(stderr, "");

    // The JSON file must exist and contain the right fields.
    // Without MAW_XDG, parked dir = HOME/.maw/parked
    let json_path = parked_dir_for(&root).join("win-a.json");
    assert!(
        json_path.exists(),
        "JSON file not created at {}",
        json_path.display()
    );
    let json_text = fs::read_to_string(&json_path).expect("read json");
    let parsed: serde_json::Value = serde_json::from_str(&json_text).expect("parse json");
    assert_eq!(parsed["window"], "win-a");
    assert_eq!(parsed["session"], "test-session");
    assert_eq!(parsed["note"], "a note");
    assert_eq!(parsed["branch"], "feat/x");
    assert_eq!(parsed["lastCommit"], "abc123 msg");
    // git status --short lines are trimmed (leading space stripped)
    assert_eq!(parsed["dirtyFiles"], serde_json::json!(["M file.rs"]));

    // No delegation to PATH maw.
    assert!(!stdout.contains("DELEGATED-MAW"), "stdout={stdout}");
    assert!(!stderr.contains("bun"), "stderr={stderr}");

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_park_win_b_as_target_window() {
    let root = temp_dir("park-win-b");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_tmux(&bin_dir);
    write_fake_git(&bin_dir);
    write_fake_maw_delegated(&bin_dir);

    let repo_dir = root.join("my-repo");
    fs::create_dir_all(&repo_dir).expect("repo dir");

    // "win-b" is a known window, so args=["win-b", "extra note"] → target=win-b, note="extra note"
    let out = run(
        &root,
        &["park", "win-b", "extra note"],
        Some(repo_dir.to_str().expect("path")),
        false,
    );
    assert!(
        out.status.success(),
        "exit={} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("stdout");

    assert!(
        stdout.contains("\x1b[33mwin-b\x1b[0m"),
        "stdout should mention win-b: {stdout}"
    );
    assert!(stdout.contains("\"extra note\""), "stdout={stdout}");

    // win-b.json must exist
    let json_path = parked_dir_for(&root).join("win-b.json");
    assert!(json_path.exists(), "win-b.json not created");
    let parsed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json_path).expect("read")).expect("parse");
    assert_eq!(parsed["window"], "win-b");
    assert_eq!(parsed["note"], "extra note");

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_park_ls_empty_shows_no_parked_tabs() {
    let root = temp_dir("ls-empty");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_tmux(&bin_dir);
    write_fake_git(&bin_dir);
    write_fake_maw_delegated(&bin_dir);

    let out = run(&root, &["park", "ls"], None, false);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("stdout");
    // Empty → dim "no parked tabs"
    assert!(stdout.contains("no parked tabs"), "stdout={stdout}");
    assert!(stdout.contains("\x1b[90m"), "should be dim: {stdout}");

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_park_ls_with_entries_shows_header_and_rows() {
    let root = temp_dir("ls-entries");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_tmux(&bin_dir);
    write_fake_git(&bin_dir);
    write_fake_maw_delegated(&bin_dir);

    // Seed two parked entries.
    let now = iso_now();
    seed_parked_json(
        &root,
        "win-a",
        "test-session",
        "feat/nova",
        "/repo/acme/app",
        "abc1234 add nova",
        &["M src/main.rs", "?? notes.md"],
        "handoff note",
        &now,
    );
    seed_parked_json(
        &root,
        "win-b",
        "test-session",
        "",
        "/repo/acme/app",
        "",
        &[],
        "",
        &now,
    );

    let out = run(&root, &["park", "ls"], None, false);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("stdout");

    // Header line
    assert!(
        stdout.contains("\x1b[36mPARKED\x1b[0m (2):"),
        "stdout={stdout}"
    );
    // win-a row
    assert!(stdout.contains("win-a"), "stdout={stdout}");
    assert!(stdout.contains("\"handoff note\""), "stdout={stdout}");
    assert!(stdout.contains("feat/nova"), "stdout={stdout}");
    assert!(stdout.contains("2 dirty"), "stdout={stdout}");
    // win-b row — no note, no branch, clean
    assert!(stdout.contains("win-b"), "stdout={stdout}");
    assert!(stdout.contains("(no note)"), "stdout={stdout}");
    assert!(stdout.contains("no branch"), "stdout={stdout}");
    assert!(stdout.contains("clean"), "stdout={stdout}");
    // Trailing: exactly one newline after the last row (maw-js parity, not a blank line)
    assert!(
        stdout.ends_with('\n') && !stdout.ends_with("\n\n"),
        "should end with exactly one trailing newline (maw-js parity): {stdout:?}"
    );

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_park_no_delegated_maw_no_bun() {
    let root = temp_dir("zero-bun");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_tmux(&bin_dir);
    write_fake_git(&bin_dir);
    write_fake_maw_delegated(&bin_dir);

    // park ls is the safest (no tmux write side-effect needed for this check)
    let out = run(&root, &["park", "ls"], None, false);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!combined.contains("DELEGATED-MAW"), "combined={combined}");
    assert!(!combined.contains("bun"), "combined={combined}");

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_park_dispatcher_status_is_native() {
    assert_eq!(dispatcher_status("park"), DispatchKind::Native);
}

#[test]
fn native_park_no_tmux_returns_error_not_delegation() {
    // When tmux is absent (PATH has only maw, no tmux), park should fail cleanly
    // without delegating to the fake maw.
    let root = temp_dir("no-tmux");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_maw_delegated(&bin_dir); // maw prints DELEGATED-MAW, but no tmux binary

    let repo_dir = root.join("my-repo");
    fs::create_dir_all(&repo_dir).expect("repo dir");

    let out = run(
        &root,
        &["park", "a note"],
        Some(repo_dir.to_str().expect("path")),
        false,
    );
    // Should exit non-zero (tmux failed).
    assert!(!out.status.success(), "should fail when no tmux");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // Must NOT contain delegation strings.
    assert!(!combined.contains("DELEGATED-MAW"), "combined={combined}");
    assert!(!combined.contains("bun"), "combined={combined}");

    fs::remove_dir_all(root).expect("cleanup");
}
