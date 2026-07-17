#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, run_cli, DispatchKind};
use std::ffi::OsString;
use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvRestore {
    maw_js_ref_dir: Option<OsString>,
    path: Option<OsString>,
    tmux: Option<OsString>,
}

impl EnvRestore {
    fn capture() -> Self {
        Self {
            maw_js_ref_dir: std::env::var_os("MAW_JS_REF_DIR"),
            path: std::env::var_os("PATH"),
            tmux: std::env::var_os("TMUX"),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        restore_env("MAW_JS_REF_DIR", self.maw_js_ref_dir.take());
        restore_env("PATH", self.path.take());
        restore_env("TMUX", self.tmux.take());
    }
}

fn restore_env(key: &str, value: Option<OsString>) {
    if let Some(value) = value {
        std::env::set_var(key, value);
    } else {
        std::env::remove_var(key);
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-whoami-native-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_tmux_shim(dir: &Path) {
    let shim = dir.join("tmux");
    write(
        &shim,
        "#!/bin/sh\nif [ \"$1\" != \"display-message\" ] || [ \"$2\" != \"-p\" ]; then\n  echo unexpected tmux argv: \"$@\" >&2\n  exit 7\nfi\ncase \"$3\" in\n  '#S') echo fleet ;;\n  '#S\t#W\t#{window_id}\t#{pane_title}\t#{pane_id}') printf '%s\\n' 'fleet\tops\t@9\tguardian pane\t%42'\n  ;;\n  *) echo unexpected format: \"$3\" >&2; exit 8 ;;\nesac\n",
    )
    .expect("write tmux shim");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&shim)
            .expect("shim metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&shim, permissions).expect("chmod tmux shim");
    }
}

fn with_fake_tmux<T>(test: impl FnOnce() -> T) -> T {
    let _guard = env_lock().lock().expect("env lock");
    let _restore = EnvRestore::capture();
    let root = temp_dir("tmux");
    let bin = root.join("bin");
    create_dir_all(&bin).expect("create bin dir");
    write_tmux_shim(&bin);
    std::env::set_var("PATH", &bin);
    std::env::set_var("TMUX", "/tmp/tmux-1000/default,123,0");
    std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");

    let result = test();

    remove_dir_all(root).expect("cleanup");
    result
}

#[test]
fn whoami_is_registered_native_and_does_not_need_maw_js_ref() {
    let _guard = env_lock().lock().expect("env lock");
    let _restore = EnvRestore::capture();
    std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");

    assert_eq!(dispatcher_status("whoami"), DispatchKind::Native);
}

#[test]
fn whoami_requires_tmux_like_maw_js() {
    let _guard = env_lock().lock().expect("env lock");
    let _restore = EnvRestore::capture();
    std::env::remove_var("TMUX");

    let output = run_cli(&args(&["whoami"]));

    assert_eq!(output.code, 1);
    assert_eq!(output.stdout, "");
    assert_eq!(
        output.stderr,
        "maw whoami requires an active tmux session — run 'maw wake <oracle>' or attach to tmux first\n"
    );
}

#[test]
fn whoami_default_output_matches_maw_js_golden() {
    with_fake_tmux(|| {
        let output = run_cli(&args(&["whoami"]));

        assert_eq!(output.code, 0, "{}", output.stderr);
        assert_eq!(
            output.stdout,
            "session  fleet\nwindow   ops  \u{1b}[90m(@9)\u{1b}[0m\npane     guardian pane  \u{1b}[90m(%42)\u{1b}[0m\ntarget   \u{1b}[36mfleet:ops\u{1b}[0m  (or %42 for the exact pane)\n"
        );
        assert_eq!(output.stderr, "");
    });
}

#[test]
fn whoami_json_output_matches_maw_js_golden() {
    with_fake_tmux(|| {
        let output = run_cli(&args(&["whoami", "--json"]));

        assert_eq!(output.code, 0, "{}", output.stderr);
        assert_eq!(
            output.stdout,
            "{\"session\":\"fleet\",\"window\":\"ops\",\"window_id\":\"@9\",\"pane_title\":\"guardian pane\",\"pane_id\":\"%42\",\"target\":\"fleet:ops.42\"}\n"
        );
        assert_eq!(output.stderr, "");
    });
}

#[test]
fn whoami_short_output_matches_maw_js_golden() {
    with_fake_tmux(|| {
        let output = run_cli(&args(&["whoami", "--short"]));

        assert_eq!(output.code, 0, "{}", output.stderr);
        assert_eq!(output.stdout, "fleet\n");
        assert_eq!(output.stderr, "");
    });
}

#[test]
fn whoami_short_alias_matches_maw_js_golden() {
    with_fake_tmux(|| {
        let output = run_cli(&args(&["whoami", "-s"]));

        assert_eq!(output.code, 0, "{}", output.stderr);
        assert_eq!(output.stdout, "fleet\n");
        assert_eq!(output.stderr, "");
    });
}
