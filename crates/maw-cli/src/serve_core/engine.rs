use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use super::ServecoreEngine;

const SERVEENGINE_CHILD_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub struct ServecoreNativeEngine;

impl ServecoreEngine for ServecoreNativeEngine {
    fn servecore_engine_name(&self) -> &'static str {
        "maw-rs"
    }
}

pub trait ServecoreExecRunner: Send + Sync {
    /// Runs a controlled maw child process for serve orchestration.
    ///
    /// # Errors
    ///
    /// Returns an error when the runner cannot spawn, wait for, or complete the
    /// child process within its bounded timeout.
    fn servecore_run(&self, argv: &[String], cwd: &Path) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct ServecoreProcessRunner;

impl ServecoreExecRunner for ServecoreProcessRunner {
    fn servecore_run(&self, argv: &[String], cwd: &Path) -> Result<(), String> {
        serveengine_run_with_timeout(
            &serveengine_self_bin()?,
            argv,
            cwd,
            SERVEENGINE_CHILD_TIMEOUT,
        )
    }
}

pub(crate) fn serveengine_self_bin() -> Result<PathBuf, String> {
    std::env::var_os("MAW_RS_SELF_BIN")
        .map(PathBuf::from)
        .map_or_else(
            || {
                std::env::current_exe()
                    .map_err(|error| format!("serve-orchestration: current_exe failed: {error}"))
            },
            Ok,
        )
}

pub(crate) fn serveengine_run_with_timeout(
    program: &Path,
    argv: &[String],
    cwd: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .current_dir(cwd)
        .spawn()
        .map_err(|error| format!("serve-orchestration: spawn failed: {error}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("serve-orchestration: wait failed: {error}"))?
        {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("serve-orchestration: workon exited with {status}"))
            };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("serve-orchestration: workon timed out".to_owned());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    struct EnvGuard {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let old = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(old) = &self.old {
                std::env::set_var(self.key, old);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("maw-rs-serveengine-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp");
        path
    }

    #[test]
    fn serveengine_self_bin_uses_env() {
        let root = temp_dir("self-bin");
        let fake = root.join("maw-self");
        fs::write(&fake, "#!/bin/sh\nexit 0\n").expect("fake");
        let _guard = EnvGuard::set("MAW_RS_SELF_BIN", &fake);
        assert_eq!(serveengine_self_bin().expect("self bin"), fake);
    }

    #[test]
    fn serveengine_runner_reaches_marker_with_argv_and_cwd() {
        let root = temp_dir("marker");
        let bin = root.join("maw-marker");
        let marker = root.join("marker.json");
        fs::write(
            &bin,
            format!(
                r#"#!/bin/sh
printf '{{"cwd":"%s","argv":["%s","%s","%s","%s"]}}' "$(pwd)" "$1" "$2" "$3" "$4" > '{}'
"#,
                marker.display()
            ),
        )
        .expect("script");
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o700)).expect("chmod");
        serveengine_run_with_timeout(
            &bin,
            &[
                "workon".to_owned(),
                "demo".to_owned(),
                "--layout".to_owned(),
                "nested".to_owned(),
            ],
            &root,
            Duration::from_secs(10),
        )
        .expect("run");
        let body = fs::read_to_string(marker).expect("marker");
        assert!(body.contains("\"cwd\""));
        assert!(body.contains("\"workon\""));
        assert!(body.contains("\"--layout\""));
    }

    #[test]
    fn serveengine_timeout_is_generic() {
        let root = temp_dir("timeout");
        let bin = root.join("maw-sleep");
        fs::write(&bin, "#!/bin/sh\n/bin/sleep 2\n").expect("script");
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o700)).expect("chmod");
        let err = serveengine_run_with_timeout(&bin, &[], &root, Duration::from_millis(10))
            .expect_err("timeout");
        assert_eq!(err, "serve-orchestration: workon timed out");
    }
}
