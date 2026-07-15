use super::*;
use std::{
    ffi::OsString,
    sync::{Mutex, OnceLock},
};

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

struct EnvRestore {
    key: &'static str,
    value: Option<OsString>,
}

impl EnvRestore {
    fn capture(key: &'static str) -> Self {
        Self {
            key,
            value: env::var_os(key),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            env::set_var(self.key, value);
        } else {
            env::remove_var(self.key);
        }
    }
}

#[test]
fn discord_bot_token_env_fast_path_still_wins() {
    let _guard = env_lock();
    let _restore = EnvRestore::capture("DISCORD_BOT_TOKEN");
    env::set_var("DISCORD_BOT_TOKEN", "  env-token  ");

    let token = decrypt_token_result("--invalid-name").expect("env token");

    assert_eq!(token, "env-token");
}

#[cfg(unix)]
#[test]
fn pass_command_success_returns_stdout_token() {
    let _guard = env_lock();
    let _restore = EnvRestore::capture("DISCORD_BOT_TOKEN");
    env::remove_var("DISCORD_BOT_TOKEN");

    let token =
        decrypt_token_with_command("bot-token", "/bin/echo", std::time::Duration::from_secs(1))
            .expect("echo token");

    assert_eq!(token, "show discord/bot-token");
}

#[cfg(unix)]
#[test]
fn slow_pass_command_times_out() {
    let _guard = env_lock();
    let _restore = EnvRestore::capture("DISCORD_BOT_TOKEN");
    env::remove_var("DISCORD_BOT_TOKEN");
    let script = write_pass_script("slow", "#!/bin/sh\nsleep 2\necho too-late\n");
    let started = std::time::Instant::now();

    let err = decrypt_token_with_command(
        "bot-token",
        script.to_str().expect("script path"),
        std::time::Duration::from_millis(80),
    )
    .expect_err("timeout");

    assert_eq!(err, TokenDecryptError::TimedOut);
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(
        err.to_string(),
        "token decrypt timed out — set DISCORD_BOT_TOKEN or unlock gpg-agent"
    );
}

fn write_pass_script(name: &str, body: &str) -> PathBuf {
    let root = env::temp_dir().join(format!(
        "maw-discord-token-test-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("temp dir");
    let path = root.join("fake-pass");
    fs::write(&path, body).expect("script write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).expect("chmod");
    }
    path
}
