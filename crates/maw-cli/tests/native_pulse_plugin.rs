use maw_cli::{dispatcher_status, DispatchKind};
use maw_plugin_manifest::{
    invoke_plugin, load_manifest_from_dir, ExtismWasmInvokeRuntime, InvokeContext, InvokeSource,
    MawWasmHost,
};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-pulse/pulse-plugin")
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-pulse-plugin-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn exec_input(cmd: &str, args: &[&str], allow_non_zero: bool) -> String {
    json!({
        "cmd": cmd,
        "args": args,
        "timeoutMs": 10_000,
        "allowNonZero": allow_non_zero
    })
    .to_string()
}

fn exec_ok(stdout: &str) -> String {
    json!({
        "ok": true,
        "value": {"status": 0, "stdout": stdout, "stderr": "", "durationMs": 0}
    })
    .to_string()
}

fn invoke(args: &[&str], repos: &Path, host: MawWasmHost) -> maw_plugin_manifest::InvokeResult {
    let plugin = load_manifest_from_dir(&fixture())
        .expect("load pulse fixture")
        .expect("pulse fixture");
    let host = host.with_fs_root("repos", repos);
    let mut runtime = ExtismWasmInvokeRuntime::default().with_host("pulse", host);
    let context = InvokeContext::new(
        InvokeSource::Cli,
        args.iter().map(|arg| (*arg).to_owned()).collect(),
    );
    invoke_plugin(&plugin, &context, &mut runtime)
}

fn plugin_host() -> MawWasmHost {
    let plugin = load_manifest_from_dir(&fixture())
        .expect("load pulse fixture")
        .expect("pulse fixture");
    MawWasmHost::new(&plugin)
}

fn issue_list_args(repo: &str) -> [&str; 10] {
    [
        "issue",
        "list",
        "--repo",
        repo,
        "--state",
        "open",
        "--json",
        "number,title,labels",
        "--limit",
        "50",
    ]
}

const ISSUES: &str = r#"[{"number":20,"title":"📅 2026-06-25 Daily Thread","labels":[{"name":"daily-thread"}]},{"number":21,"title":"P001 launch board","labels":[{"name":"oracle:nova"}]},{"number":19,"title":"registry cleanup","labels":[]},{"number":22,"title":"ship pulse native","labels":[{"name":"oracle:pulse"}]}]"#;

#[test]
fn pulse_plugin_list_matches_committed_native_golden() {
    let _guard = env_lock();
    let previous = std::env::var_os("MAW_PULSE_REPO");
    std::env::remove_var("MAW_PULSE_REPO");
    let root = temp_dir("list");
    let repos = root.join("repos");
    fs::create_dir_all(&repos).expect("repos");
    let args = issue_list_args("laris-co/pulse-oracle");
    let host = plugin_host().with_fake_response(
        "maw.exec.run",
        exec_input("gh", &args, false),
        exec_ok(ISSUES),
    );

    let result = invoke(&["list"], &repos, host);

    match previous {
        Some(value) => std::env::set_var("MAW_PULSE_REPO", value),
        None => std::env::remove_var("MAW_PULSE_REPO"),
    }
    assert!(result.ok, "{:?}", result.error);
    let expected = format!("{}\n", include_str!("fixtures/native-pulse/list.stdout"));
    assert_eq!(result.output.as_deref(), Some(expected.as_str()));
}

#[test]
fn pulse_plugin_preserves_maw_pulse_repo_override() {
    let _guard = env_lock();
    let previous = std::env::var_os("MAW_PULSE_REPO");
    std::env::set_var("MAW_PULSE_REPO", "acme/pulse-board");
    let root = temp_dir("override");
    let repos = root.join("repos");
    fs::create_dir_all(&repos).expect("repos");
    let args = issue_list_args("acme/pulse-board");
    let host = plugin_host()
        .with_fake_response(
            "maw.exec.run",
            exec_input("gh", &args, false),
            exec_ok(ISSUES),
        )
        .with_fake_response(
            "maw.exec.run",
            exec_input("gh", &issue_list_args("laris-co/pulse-oracle"), false),
            exec_ok("[]"),
        );

    let result = invoke(&["list"], &repos, host);

    match previous {
        Some(value) => std::env::set_var("MAW_PULSE_REPO", value),
        None => std::env::remove_var("MAW_PULSE_REPO"),
    }
    assert!(result.ok, "{:?}", result.error);
    assert!(result
        .output
        .as_deref()
        .is_some_and(|output| output.contains("3 open")));
}

#[test]
fn pulse_plugin_cleanup_uses_typed_tmux_abi_and_matches_golden() {
    let _guard = env_lock();
    let root = temp_dir("cleanup");
    let repos = root.join("repos");
    let worktree = repos.join("acme/widgets/agents/1-old");
    fs::create_dir_all(&worktree).expect("worktree");
    fs::write(
        worktree.join(".git"),
        "gitdir: ../../../.git/worktrees/1-old\n",
    )
    .expect("git marker");
    let canonical_worktree = fs::canonicalize(&worktree).expect("canonical worktree");
    let worktree_text = canonical_worktree.to_string_lossy();
    let main = repos.join("acme/widgets");
    let main_text = main.to_string_lossy();
    let branch_args = [
        "-C",
        worktree_text.as_ref(),
        "rev-parse",
        "--abbrev-ref",
        "HEAD",
    ];
    let list_args = ["-C", main_text.as_ref(), "worktree", "list", "--porcelain"];
    let host = plugin_host()
        .with_fake_response(
            "maw.exec.run",
            exec_input("git", &branch_args, true),
            exec_ok("agents/1-old\n"),
        )
        .with_fake_response(
            "maw.exec.run",
            exec_input("git", &list_args, true),
            exec_ok(""),
        )
        .with_fake_response(
            "maw.tmux.list_sessions",
            "{}",
            r#"{"ok":true,"value":{"sessions":[{"name":"fleet","windows":[{"index":0,"name":"1-active","active":true}]}]}}"#,
        );

    let result = invoke(&["cleanup", "--dry-run"], &repos, host);

    assert!(result.ok, "{:?}", result.error);
    let expected = format!(
        "{}\n",
        include_str!("fixtures/native-pulse/cleanup-dry-run.stdout")
    );
    assert_eq!(result.output.as_deref(), Some(expected.as_str()));
}

#[test]
fn pulse_dispatcher_registration_is_removed_for_plugin_fallthrough() {
    assert_eq!(dispatcher_status("pulse"), DispatchKind::NativeError);
}
