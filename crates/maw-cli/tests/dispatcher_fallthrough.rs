use maw_cli::{dispatcher_status, native_dispatch_commands, run_cli, DispatchKind};
use std::ffi::OsString;
use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    env_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct EnvRestore {
    maw_from_rs: Option<OsString>,
    home: Option<OsString>,
    maw_home: Option<OsString>,
    maw_plugins_dir: Option<OsString>,
    maw_rs_hey_fallback: Option<OsString>,
    maw_rs_notify_fallback: Option<OsString>,
    maw_rs_send_fallback: Option<OsString>,
    maw_rs_talkto_fallback: Option<OsString>,
    maw_rs_talkto_no_thread: Option<OsString>,
    path: Option<OsString>,
    check_tool_probes: Vec<(&'static str, Option<OsString>)>,
}

impl EnvRestore {
    fn capture() -> Self {
        const CHECK_TOOL_PROBES: &[&str] = &[
            "MAW_RS_CHECK_TOOL_PROBE_BUN",
            "MAW_RS_CHECK_TOOL_PROBE_GH",
            "MAW_RS_CHECK_TOOL_PROBE_GHQ",
            "MAW_RS_CHECK_TOOL_PROBE_GIT",
            "MAW_RS_CHECK_TOOL_PROBE_TMUX",
            "MAW_RS_CHECK_TOOL_PROBE_UV",
            "MAW_RS_CHECK_TOOL_PROBE_UVX",
            "MAW_RS_CHECK_TOOL_PROBE_WHICH",
        ];
        Self {
            maw_from_rs: std::env::var_os("MAW_FROM_RS"),
            home: std::env::var_os("HOME"),
            maw_home: std::env::var_os("MAW_HOME"),
            maw_plugins_dir: std::env::var_os("MAW_PLUGINS_DIR"),
            maw_rs_hey_fallback: std::env::var_os("MAW_RS_HEY_FALLBACK"),
            maw_rs_notify_fallback: std::env::var_os("MAW_RS_NOTIFY_FALLBACK"),
            maw_rs_send_fallback: std::env::var_os("MAW_RS_SEND_FALLBACK"),
            maw_rs_talkto_fallback: std::env::var_os("MAW_RS_TALKTO_FALLBACK"),
            maw_rs_talkto_no_thread: std::env::var_os("MAW_RS_TALKTO_NO_THREAD"),
            path: std::env::var_os("PATH"),
            check_tool_probes: CHECK_TOOL_PROBES
                .iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        restore_env("MAW_FROM_RS", self.maw_from_rs.take());
        restore_env("HOME", self.home.take());
        restore_env("MAW_HOME", self.maw_home.take());
        restore_env("MAW_PLUGINS_DIR", self.maw_plugins_dir.take());
        restore_env("MAW_RS_HEY_FALLBACK", self.maw_rs_hey_fallback.take());
        restore_env("MAW_RS_NOTIFY_FALLBACK", self.maw_rs_notify_fallback.take());
        restore_env("MAW_RS_SEND_FALLBACK", self.maw_rs_send_fallback.take());
        restore_env("MAW_RS_TALKTO_FALLBACK", self.maw_rs_talkto_fallback.take());
        restore_env(
            "MAW_RS_TALKTO_NO_THREAD",
            self.maw_rs_talkto_no_thread.take(),
        );
        restore_env("PATH", self.path.take());
        for (key, value) in self.check_tool_probes.drain(..) {
            restore_env(key, value);
        }
    }
}

fn restore_env(key: &str, value: Option<OsString>) {
    if let Some(value) = value {
        std::env::set_var(key, value);
    } else {
        std::env::remove_var(key);
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-dispatcher-fallthrough-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn write_maw_shim(dir: &Path, exit_code: u8) {
    let shim = dir.join("maw");
    write(
        &shim,
        format!(
            "#!/bin/sh\nprintf 'DELEGATED-MAW\\n'\nprintf 'MAW_FROM_RS=%s\\n' \"$MAW_FROM_RS\"\nprintf 'args=%s\\n' \"$*\"\nexit {exit_code}\n"
        ),
    )
    .expect("write maw shim");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&shim)
            .expect("shim metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&shim, permissions).expect("chmod maw shim");
    }
}

fn write_tool_shim(dir: &Path, name: &str, body: &str) {
    let shim = dir.join(name);
    write(&shim, body).expect("write tool shim");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&shim)
            .expect("tool shim metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&shim, permissions).expect("chmod tool shim");
    }
}

#[test]
fn dispatcher_table_marks_native_and_unknown_commands() {
    assert_eq!(dispatcher_status("ls"), DispatchKind::Native);
    assert_eq!(dispatcher_status("hey"), DispatchKind::Native);
    assert_eq!(dispatcher_status("check"), DispatchKind::Native);
    assert!(native_dispatch_commands().contains(&"ls"));
    assert!(native_dispatch_commands().contains(&"hey"));
    assert!(native_dispatch_commands().contains(&"check"));
}

#[test]
fn native_command_stays_native_without_invoking_maw_fallback() {
    let _guard = lock_env();
    let _restore = EnvRestore::capture();
    let root = temp_dir("native");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    create_dir_all(&bin_dir).expect("bin dir");
    create_dir_all(&plugins_dir).expect("plugins dir");
    write_maw_shim(&bin_dir, 42);
    std::env::set_var("PATH", &bin_dir);
    std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);
    std::env::remove_var("MAW_FROM_RS");

    let output = run_cli(&args(&["ls", "--help"]));

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stderr.is_empty(), "{}", output.stderr);
    assert!(output.stdout.contains("maw ls"), "{}", output.stdout);
    assert!(!output.stdout.contains("MAW_FROM_RS"), "{}", output.stdout);

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn unknown_command_is_native_error_and_never_invokes_path_maw() {
    let _guard = lock_env();
    let _restore = EnvRestore::capture();
    let root = temp_dir("unknown-native");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    create_dir_all(&bin_dir).expect("bin dir");
    create_dir_all(&plugins_dir).expect("plugins dir");
    write_maw_shim(&bin_dir, 37);
    std::env::set_var("PATH", &bin_dir);
    std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);
    std::env::remove_var("MAW_FROM_RS");
    std::env::remove_var("MAW_RS_HEY_FALLBACK");

    let output = run_cli(&args(&["__definitely_unknown_xyz__"]));

    assert_eq!(
        dispatcher_status("__definitely_unknown_xyz__"),
        DispatchKind::NativeError
    );
    assert_eq!(output.code, 2, "{}", output.stdout);
    assert!(output.stdout.is_empty(), "{}", output.stdout);
    assert_eq!(
        output.stderr,
        include_str!("fixtures/zero-bun/unknown-command.stderr")
    );
    assert!(
        !output.stderr.contains("DELEGATED-MAW"),
        "{}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("failed to run maw fallback"),
        "{}",
        output.stderr
    );

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn env_fallback_hatches_are_ignored_and_never_invoke_path_maw() {
    let _guard = lock_env();
    let _restore = EnvRestore::capture();
    let root = temp_dir("fallback-hatches-removed");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    let home_dir = root.join("home");
    let maw_home = root.join("maw-home");
    create_dir_all(&bin_dir).expect("bin dir");
    create_dir_all(&plugins_dir).expect("plugins dir");
    create_dir_all(&home_dir).expect("home dir");
    create_dir_all(&maw_home).expect("maw home dir");
    write_maw_shim(&bin_dir, 37);
    write_tool_shim(&bin_dir, "bun", "#!/bin/sh\necho DELEGATED-BUN\nexit 37\n");
    write_tool_shim(&bin_dir, "tmux", "#!/bin/sh\necho FAKE-TMUX >&2\nexit 1\n");
    std::env::set_var("PATH", &bin_dir);
    std::env::set_var("HOME", &home_dir);
    std::env::set_var("MAW_HOME", &maw_home);
    std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);
    std::env::remove_var("MAW_FROM_RS");
    std::env::set_var("MAW_RS_HEY_FALLBACK", "1");
    std::env::set_var("MAW_RS_SEND_FALLBACK", "1");
    std::env::set_var("MAW_RS_NOTIFY_FALLBACK", "1");
    std::env::set_var("MAW_RS_TALKTO_FALLBACK", "1");
    std::env::set_var("MAW_RS_TALKTO_NO_THREAD", "1");

    let mut proof = String::new();
    for (command, argv) in [
        ("hey", args(&["hey", "ghost", "ping"])),
        ("send", args(&["send", "ghost", "ping"])),
        ("notify", args(&["notify", "ghost", "ping"])),
        ("talk-to", args(&["talk-to", "ghost", "ping"])),
    ] {
        let output = run_cli(&argv);
        assert_ne!(output.code, 37, "{argv:?} delegated to fake maw/bun");
        assert!(
            output.stdout.is_empty(),
            "{argv:?} stdout={}",
            output.stdout
        );
        assert!(
            !output.stdout.contains("DELEGATED-MAW") && !output.stderr.contains("DELEGATED-MAW"),
            "{argv:?} stdout={} stderr={}",
            output.stdout,
            output.stderr
        );
        assert!(
            !output.stdout.contains("DELEGATED-BUN") && !output.stderr.contains("DELEGATED-BUN"),
            "{argv:?} stdout={} stderr={}",
            output.stdout,
            output.stderr
        );
        assert!(
            !output.stderr.contains("failed to run maw fallback"),
            "{argv:?} stderr={}",
            output.stderr
        );
        proof.push_str(command);
        proof.push_str(" env fallback ignored: native/fail-closed\n");
    }
    assert_eq!(
        proof,
        include_str!("fixtures/zero-bun/env-fallback-hatches-removed.stdout")
    );

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn check_tools_is_native_and_never_invokes_path_maw() {
    let _guard = lock_env();
    let _restore = EnvRestore::capture();
    let root = temp_dir("check-tools-native");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    create_dir_all(&bin_dir).expect("bin dir");
    create_dir_all(&plugins_dir).expect("plugins dir");
    write_maw_shim(&bin_dir, 37);
    std::env::set_var("PATH", &bin_dir);
    std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);
    std::env::remove_var("MAW_FROM_RS");
    std::env::remove_var("MAW_RS_HEY_FALLBACK");

    let output = run_cli(&args(&["check", "tools"]));

    assert_eq!(dispatcher_status("check"), DispatchKind::Native);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stderr.is_empty(), "{}", output.stderr);
    assert_eq!(
        output.stdout,
        include_str!("fixtures/zero-bun/check-tools-missing.stdout")
    );
    assert!(
        !output.stdout.contains("DELEGATED-MAW"),
        "{}",
        output.stdout
    );
    assert!(
        !output.stderr.contains("DELEGATED-MAW"),
        "{}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("failed to run maw fallback"),
        "{}",
        output.stderr
    );

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn check_tools_extracts_versions_with_argv_only_tool_probes() {
    let _guard = lock_env();
    let _restore = EnvRestore::capture();
    let root = temp_dir("check-tools-present");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    create_dir_all(&bin_dir).expect("bin dir");
    create_dir_all(&plugins_dir).expect("plugins dir");
    write_maw_shim(&bin_dir, 37);
    write_tool_shim(&bin_dir, "bun", "#!/bin/sh\nprintf 'bun 1.2.3\\n'\n");
    write_tool_shim(&bin_dir, "gh", "#!/bin/sh\nprintf 'gh version 2.3.4\\n'\n");
    write_tool_shim(&bin_dir, "ghq", "#!/bin/sh\nprintf 'ghq 3.4.5\\n'\n");
    write_tool_shim(
        &bin_dir,
        "git",
        "#!/bin/sh\nprintf 'git version 4.5.6\\n'\n",
    );
    write_tool_shim(&bin_dir, "tmux", "#!/bin/sh\nprintf 'tmux 5.6\\n'\n");
    write_tool_shim(&bin_dir, "uv", "#!/bin/sh\nprintf 'uv 6.7.8\\n'\n");
    write_tool_shim(
        &bin_dir,
        "uvx",
        "#!/bin/sh\nprintf 'uvx should-not-run\\n'\n",
    );
    write_tool_shim(
        &bin_dir,
        "which",
        "#!/bin/sh\nif [ \"$1\" = uvx ]; then printf 'uvx\\n'; exit 0; fi\nexit 1\n",
    );
    std::env::set_var("MAW_RS_CHECK_TOOL_PROBE_BUN", "bun 1.2.3\n");
    std::env::set_var("MAW_RS_CHECK_TOOL_PROBE_GH", "gh version 2.3.4\n");
    std::env::set_var("MAW_RS_CHECK_TOOL_PROBE_GHQ", "ghq 3.4.5\n");
    std::env::set_var("MAW_RS_CHECK_TOOL_PROBE_GIT", "git version 4.5.6\n");
    std::env::set_var("MAW_RS_CHECK_TOOL_PROBE_TMUX", "tmux 5.6\n");
    std::env::set_var("MAW_RS_CHECK_TOOL_PROBE_UV", "uv 6.7.8\n");
    std::env::set_var("MAW_RS_CHECK_TOOL_PROBE_WHICH", "uvx\n");
    std::env::set_var("PATH", &bin_dir);
    std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);
    std::env::remove_var("MAW_FROM_RS");

    let output = run_cli(&args(&["check"]));

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stderr.is_empty(), "{}", output.stderr);
    assert!(
        output.stdout.contains("maw check tools"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("bun       1.2.3"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("gh        2.3.4"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("ghq       3.4.5"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("git       4.5.6"),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("tmux      5.6"), "{}", output.stdout);
    assert!(
        output.stdout.contains("uv        6.7.8"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("uvx       6.7.8"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("(provided by uv)"),
        "{}",
        output.stdout
    );
    assert!(
        output
            .stdout
            .contains("5 required ✓  ·  2 optional ✓  ·  0 missing"),
        "{}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("DELEGATED-MAW"),
        "{}",
        output.stdout
    );

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn check_unknown_subcommand_matches_maw_js_usage_without_tool_exec() {
    let _guard = lock_env();
    let _restore = EnvRestore::capture();
    let root = temp_dir("check-unknown-subcommand");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    create_dir_all(&bin_dir).expect("bin dir");
    create_dir_all(&plugins_dir).expect("plugins dir");
    write_maw_shim(&bin_dir, 37);
    std::env::set_var("PATH", &bin_dir);
    std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);

    let output = run_cli(&args(&["check", "status"]));

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stderr.is_empty(), "{}", output.stderr);
    assert_eq!(
        output.stdout,
        "unknown subcommand: status\nusage: maw check [tools]\n"
    );
    assert!(
        !output.stdout.contains("DELEGATED-MAW"),
        "{}",
        output.stdout
    );

    remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn sync_async_handler_guard_refuses_to_block_inside_runtime() {
    let _guard = lock_env();
    let output = run_cli(&args(&["hey", "local:nova:claude", "ping"]));

    assert_ne!(output.code, 0);
    assert!(output.stdout.is_empty(), "{}", output.stdout);
    assert!(
        output.stderr.contains("cannot block_on inside runtime"),
        "{}",
        output.stderr
    );
}

#[test]
fn maw_from_rs_loop_guard_env_no_longer_reenables_hey_fallback() {
    let _guard = lock_env();
    let _restore = EnvRestore::capture();
    let root = temp_dir("loop-guard-cutover");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    let maw_home = root.join("maw-home");
    create_dir_all(&bin_dir).expect("bin dir");
    create_dir_all(&plugins_dir).expect("plugins dir");
    create_dir_all(&maw_home).expect("maw home dir");
    write_maw_shim(&bin_dir, 37);
    write_tool_shim(&bin_dir, "tmux", "#!/bin/sh\necho FAKE-TMUX >&2\nexit 1\n");
    std::env::set_var("PATH", &bin_dir);
    std::env::set_var("MAW_HOME", &maw_home);
    std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);
    std::env::set_var("MAW_FROM_RS", "1");
    std::env::set_var("MAW_RS_HEY_FALLBACK", "1");

    let output = run_cli(&args(&["hey", "ghost", "ping"]));

    assert_ne!(output.code, 37, "delegated to fake maw");
    assert!(output.stdout.is_empty(), "{}", output.stdout);
    assert!(
        !output.stdout.contains("DELEGATED-MAW") && !output.stderr.contains("DELEGATED-MAW"),
        "stdout={} stderr={}",
        output.stdout,
        output.stderr
    );
    assert!(
        !output.stderr.contains("failed to run maw fallback"),
        "{}",
        output.stderr
    );

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn plugin_manifest_invoke_is_native_and_never_invokes_path_maw() {
    let _guard = lock_env();
    let _restore = EnvRestore::capture();
    let root = temp_dir("plugin-manifest-native");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    let manifest_dir = plugins_dir.join("ts-proof");
    create_dir_all(&bin_dir).expect("bin dir");
    create_dir_all(&manifest_dir).expect("plugin dir");
    write_maw_shim(&bin_dir, 37);
    write(
        manifest_dir.join("plugin.json"),
        r#"{"name":"ts-proof","version":"1.0.0","sdk":"*","target":"js","entry":"index.ts"}"#,
    )
    .expect("manifest");
    write(
        manifest_dir.join("index.ts"),
        b"export default () => ({ ok: true });\n",
    )
    .expect("entry");
    std::env::set_var("PATH", &bin_dir);
    std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);
    std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
    std::env::remove_var("MAW_FROM_RS");
    std::env::remove_var("MAW_RS_HEY_FALLBACK");

    let output = run_cli(&args(&[
        "plugin-manifest",
        "invoke",
        "--scan-dir",
        plugins_dir.to_str().expect("plugins path"),
        "--plugin",
        "ts-proof",
    ]));

    assert_eq!(dispatcher_status("plugin-manifest"), DispatchKind::Native);
    assert_eq!(output.code, 2, "{}", output.stdout);
    assert!(output.stdout.is_empty(), "{}", output.stdout);
    assert!(
        output
            .stderr
            .contains("No Bun/JS subprocess fallback is available"),
        "{}",
        output.stderr
    );
    assert!(
        !output.stdout.contains("DELEGATED-MAW"),
        "{}",
        output.stdout
    );
    assert!(
        !output.stderr.contains("DELEGATED-MAW"),
        "{}",
        output.stderr
    );

    remove_dir_all(root).expect("cleanup");
}
