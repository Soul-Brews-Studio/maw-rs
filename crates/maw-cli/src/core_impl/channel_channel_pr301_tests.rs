#[cfg(test)]
mod channel_pr301_tests {
    use super::*;

    #[test]
    fn channel_plugin_schema_accepts_github_setup_fields() {
        let raw = r#"{
          "plugins": [
            {
              "id": "server:relay",
              "source": "github:ARRA-01/claude-channel-relay",
              "path": "/tmp/ghq/github.com/ARRA-01/claude-channel-relay",
              "mcp": { "command": "node", "args": ["server.js"] },
              "dev": true,
              "env": { "CHANNEL_MODE": "dev" }
            }
          ],
          "token_source": "pass:github/hermes-token"
        }"#;
        let config: ChannelConfig = serde_json::from_str(raw).expect("schema parses");
        let plugin = config.plugins.first().expect("plugin");
        assert_eq!(plugin.source.as_deref(), Some("github:ARRA-01/claude-channel-relay"));
        assert_eq!(plugin.path.as_deref(), Some("/tmp/ghq/github.com/ARRA-01/claude-channel-relay"));
        assert_eq!(plugin.dev, Some(true));
        assert_eq!(
            plugin.mcp.as_ref().expect("mcp"),
            &ChannelMcpConfig { command: "node".to_owned(), args: vec!["server.js".to_owned()], untrusted: None }
        );
        let serialized = serde_json::to_string(&config).expect("serialize");
        assert!(serialized.contains("\"source\""));
        assert!(serialized.contains("\"path\""));
        assert!(serialized.contains("\"mcp\""));
        assert!(serialized.contains("\"dev\""));
    }

    #[derive(Debug)]
    struct FakeGithubRunner {
        root: std::path::PathBuf,
        bun_fail: bool,
        calls: std::cell::RefCell<Vec<String>>,
    }

    impl FakeGithubRunner {
        fn new(root: std::path::PathBuf) -> Self {
            Self { root, bun_fail: false, calls: std::cell::RefCell::new(Vec::new()) }
        }

        fn with_bun_fail(root: std::path::PathBuf) -> Self {
            Self { root, bun_fail: true, calls: std::cell::RefCell::new(Vec::new()) }
        }
    }

    impl ChannelGithubRunner for FakeGithubRunner {
        fn ghq_root(&self) -> Result<std::path::PathBuf, (i32, String)> {
            self.calls.borrow_mut().push("ghq root".to_owned());
            Ok(self.root.clone())
        }

        fn repo_exists(&self, path: &std::path::Path) -> bool {
            self.calls.borrow_mut().push(format!("exists {}", path.display()));
            path.exists()
        }

        fn ghq_get(&self, repo: &str, url: &str, root: &std::path::Path) -> Result<(), (i32, String)> {
            self.calls.borrow_mut().push(format!("ghq get {url}"));
            std::fs::create_dir_all(channel_github_repo_path(root, repo)).expect("fake clone");
            Ok(())
        }

        fn file_exists(&self, path: &std::path::Path) -> bool {
            self.calls.borrow_mut().push(format!("file-exists {}", path.display()));
            path.exists()
        }

        fn read_to_string(&self, path: &std::path::Path) -> Result<Option<String>, (i32, String)> {
            self.calls.borrow_mut().push(format!("read {}", path.display()));
            match std::fs::read_to_string(path) {
                Ok(raw) => Ok(Some(raw)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err((1, error.to_string())),
            }
        }

        fn bun_install_stub(&self, repo: &std::path::Path) -> Result<(), (i32, String)> {
            self.calls.borrow_mut().push(format!("bun-stub {}", repo.display()));
            if self.bun_fail {
                Err((1, "channel setup: bun install failed".to_owned()))
            } else {
                Ok(())
            }
        }
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("maw-rs-channel-{name}-{stamp}"));
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }

    fn github_args() -> ChannelSetupArgs {
        let mut env = std::collections::BTreeMap::new();
        env.insert("CHANNEL_MODE".to_owned(), "dev".to_owned());
        ChannelSetupArgs {
            oracle: "relay-oracle".to_owned(),
            provider: ChannelSetupProvider::Github("ARRA-01/claude-channel-relay".to_owned()),
            pass_key: Some("github/relay-token".to_owned()),
            guild_id: None,
            env,
        }
    }

    fn seed_repo(root: &std::path::Path, mcp: Option<&str>, package_json: bool) -> std::path::PathBuf {
        let repo = channel_github_repo_path(root, "ARRA-01/claude-channel-relay");
        std::fs::create_dir_all(&repo).expect("repo");
        if let Some(mcp) = mcp {
            std::fs::write(repo.join(".mcp.json"), mcp).expect("mcp");
        }
        if package_json {
            std::fs::write(repo.join("package.json"), "{}\n").expect("package");
        }
        repo
    }

    #[test]
    fn channel_github_prb_records_untrusted_mcp_without_autospawn_or_token_value() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = super::EnvVarRestore::capture("HOME");
        let home = temp_path("home-record");
        let ghq = temp_path("ghq-record");
        std::env::set_var("HOME", &home);
        let repo = seed_repo(
            &ghq,
            Some(r#"{"mcpServers":{"relay":{"command":"bun","args":["run","start"]}}}"#),
            true,
        );
        let runner = FakeGithubRunner::new(ghq.clone());

        let stdout = channel_setup_github_with_runner(&github_args(), &runner).expect("setup");
        assert!(stdout.contains("setup records MCP only; it does not spawn"));
        assert!(stdout.contains("pass:github/relay-token (reference only)"));
        assert!(!stdout.contains("ghp_"));
        let calls = runner.calls.borrow().join("\n");
        assert!(calls.contains("bun-stub"));
        assert!(!calls.contains("bun run start"), "mcp command must not spawn at setup: {calls}");

        let config = channel_load_config_at(&channel_oracle_config_path("relay-oracle")).expect("config");
        assert_eq!(config.token_source.as_deref(), Some("pass:github/relay-token"));
        assert_eq!(config.plugins.len(), 1);
        let plugin = &config.plugins[0];
        assert_eq!(plugin.id, "server:relay");
        assert_eq!(plugin.source.as_deref(), Some("github:ARRA-01/claude-channel-relay"));
        assert_eq!(plugin.path.as_deref(), Some(repo.canonicalize().expect("repo canon").to_string_lossy().as_ref()));
        assert_eq!(plugin.dev, Some(true));
        let mcp = plugin.mcp.as_ref().expect("mcp");
        assert_eq!(mcp.command, "bun");
        assert_eq!(mcp.args, vec!["run".to_owned(), "start".to_owned()]);
        assert_eq!(mcp.untrusted, Some(true));
    }

    #[test]
    fn channel_github_prb_rejects_untrusted_mcp_leading_dash_before_config_write() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = super::EnvVarRestore::capture("HOME");
        let home = temp_path("home-reject");
        let ghq = temp_path("ghq-reject");
        std::env::set_var("HOME", &home);
        seed_repo(
            &ghq,
            Some(r#"{"mcpServers":{"relay":{"command":"bun","args":["--cwd","/tmp"]}}}"#),
            false,
        );
        let runner = FakeGithubRunner::new(ghq);

        let err = channel_setup_github_with_runner(&github_args(), &runner).expect_err("reject untrusted arg");
        assert_eq!(err.0, 2);
        assert!(err.1.contains("invalid .mcp.json args"));
        assert!(!channel_oracle_config_path("relay-oracle").exists());
        let calls = runner.calls.borrow().join("\n");
        assert!(!calls.contains("bun-stub"));
    }

    #[test]
    fn channel_github_prb_default_mcp_allows_internal_cwd_and_bun_failure_warns_continue() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = super::EnvVarRestore::capture("HOME");
        let home = temp_path("home-default");
        let ghq = temp_path("ghq-default");
        std::env::set_var("HOME", &home);
        let repo = seed_repo(&ghq, None, true);
        let runner = FakeGithubRunner::with_bun_fail(ghq);

        let stdout = channel_setup_github_with_runner(&github_args(), &runner).expect("setup");
        assert!(stdout.contains("bun install failed; continuing"));
        let config = channel_load_config_at(&channel_oracle_config_path("relay-oracle")).expect("config");
        let mcp = config.plugins[0].mcp.as_ref().expect("mcp");
        assert_eq!(mcp.untrusted, None);
        assert_eq!(
            mcp.args,
            vec!["run".to_owned(), "--cwd".to_owned(), repo.canonicalize().expect("repo").display().to_string(), "start".to_owned()]
        );
    }
}
