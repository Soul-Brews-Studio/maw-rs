#[cfg(test)]
mod workon_kind_tests {
    use super::*;

    #[derive(Default)]
    struct WorkonKindMockTmux {
        calls: Vec<(String, Vec<String>)>,
        has_session: bool,
        windows: String,
    }

    impl maw_tmux::TmuxRunner for WorkonKindMockTmux {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "has-session" => {
                    if self.has_session { Ok(String::new()) } else { Err(maw_tmux::TmuxError::new("no session")) }
                }
                "list-windows" => Ok(self.windows.clone()),
                "display-message" | "new-session" | "new-window" | "send-keys" | "select-window" | "capture-pane" => Ok(String::new()),
                other => Err(maw_tmux::TmuxError::new(format!("unexpected {other}"))),
            }
        }
    }

    fn workon_kind_temp_root(label: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("maw-rs-workon-kind-{label}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp root");
        path
    }

    fn workon_kind_repo(root: &std::path::Path, name: &str) -> WorkonRepo {
        let repo_path = root.join("ghq/github.com/acme").join(name);
        std::fs::create_dir_all(&repo_path).expect("repo");
        WorkonRepo {
            parent_dir: repo_path.parent().expect("parent").to_path_buf(),
            repo_name: name.to_owned(),
            repo_path,
        }
    }

    #[test]
    fn taskless_workon_uses_declared_kind_before_suffix() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = EnvVarRestore::capture("HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let _tmux = EnvVarRestore::capture("TMUX");
        let root = workon_kind_temp_root("kind");
        std::fs::create_dir_all(root.join("config/fleet")).expect("fleet");
        std::fs::write(
            root.join("config/fleet/99-kind.json"),
            r#"{"name":"99-kind","windows":[{"name":"foo","repo":"acme/foo","kind":"oracle"},{"name":"bar-oracle","repo":"acme/bar-oracle","kind":"project"}]}"#,
        )
        .expect("fleet");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));
        std::env::remove_var("TMUX");

        let repo = workon_kind_repo(&root, "foo");
        let options = WorkonOptions { repo: "foo".to_owned(), task: None, layout: WorkonLayout::Nested };
        let mut runner = WorkonKindMockTmux::default();
        let (stdout, _attach) = workon_cmd_with_runner(&options, &repo, &mut runner).expect("oracle workon");
        assert!(stdout.contains("fleet registered foo:foo"), "{stdout}");
        assert!(root.join("state/fleet/foo.json").exists());

        let repo = workon_kind_repo(&root, "bar-oracle");
        let options = WorkonOptions { repo: "bar-oracle".to_owned(), task: None, layout: WorkonLayout::Nested };
        let mut runner = WorkonKindMockTmux::default();
        let (stdout, _attach) = workon_cmd_with_runner(&options, &repo, &mut runner).expect("project workon");
        assert!(!stdout.contains("fleet registered"), "{stdout}");
        assert!(!root.join("state/fleet/bar-oracle.json").exists());
    }
}
