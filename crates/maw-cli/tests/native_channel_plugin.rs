#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

const SECRET_VALUE: &str = "super-secret-channel-token";

fn channel_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn channel_temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-channel-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn channel_seed(root: &Path) -> PathBuf {
    let home = root.join("home");
    let repo = root.join("repo");
    fs::create_dir_all(home.join(".claude/channels/hermes-discord")).expect("channel dir");
    fs::create_dir_all(home.join(".claude/plugins/cache/claude-plugins-official/discord"))
        .expect("plugin cache");
    fs::create_dir_all(home.join("channels-state/hermes")).expect("state dir");
    fs::create_dir_all(&repo).expect("repo");
    fs::write(
        home.join(".claude/channels/hermes-discord/config.json"),
        format!(
            r#"{{
  "plugins": [
    {{
      "id": "plugin:discord@claude-plugins-official",
      "env": {{
        "DISCORD_STATE_DIR": "~/channels-state/hermes",
        "DISCORD_BOT_TOKEN": "{SECRET_VALUE}"
      }}
    }}
  ],
  "token_source": "pass:discord/hermes-token",
  "permissionMode": "relay"
}}
"#
        ),
    )
    .expect("channel config");
    fs::write(
        repo.join(".mcp.json"),
        r#"{"mcpServers":{"relay":{"command":"node","args":["server.js"]}}}"#,
    )
    .expect("mcp json");
    repo
}

fn channel_empty_repo(root: &Path) -> PathBuf {
    let repo = root.join("repo");
    fs::create_dir_all(&repo).expect("repo");
    repo
}

fn channel_install_official_plugins(root: &Path) {
    for provider in ["discord", "telegram", "imessage"] {
        fs::create_dir_all(
            root.join("home/.claude/plugins/cache/claude-plugins-official")
                .join(provider),
        )
        .expect("plugin cache");
    }
}

fn channel_seed_global_config(root: &Path, oracle: &str, plugin: &str) -> PathBuf {
    let dir = root.join("home/.claude/channels").join(oracle);
    fs::create_dir_all(&dir).expect("channel dir");
    fs::write(dir.join("state.txt"), "keep-state").expect("state");
    fs::write(
        dir.join("access.json"),
        r#"{
  "dmPolicy": "allowlist",
  "allowFrom": ["111111111111111111"],
  "groups": {},
  "pending": {}
}
"#,
    )
    .expect("access");
    fs::write(
        dir.join("config.json"),
        format!(
            r#"{{
  "plugins": [
    {{
      "id": "plugin:{plugin}@claude-plugins-official"
    }}
  ],
  "token_source": "pass:{plugin}/{oracle}-token"
}}
"#
        ),
    )
    .expect("config");
    dir
}

fn channel_seed_fake_repo(root: &Path, name: &str) -> PathBuf {
    let repo = root.join("repo/ghq").join(name);
    fs::create_dir_all(&repo).expect("fake repo");
    repo
}

fn channel_command(root: &Path, cwd: &Path, args: &[&str]) -> Output {
    fs::create_dir_all(root.join("maw-home")).expect("maw home");
    fs::create_dir_all(root.join("maw-plugins")).expect("maw plugins");
    Command::new(channel_bin())
        .args(args)
        .current_dir(cwd)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_PLUGINS_DIR", root.join("maw-plugins"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs")
}

fn channel_command_with_env(
    root: &Path,
    cwd: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Output {
    let mut command = Command::new(channel_bin());
    command
        .args(args)
        .current_dir(cwd)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_PLUGINS_DIR", root.join("maw-plugins"))
        .env("MAW_JS_REF_DIR", "/nonexistent");
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("run maw-rs")
}

fn channel_assert_golden_with_env(
    root: &Path,
    cwd: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
    golden: &str,
) {
    let output = channel_command_with_env(root, cwd, args, envs);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert_eq!(stdout, golden);
    assert!(!stdout.contains(SECRET_VALUE));
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

fn channel_assert_golden(root: &Path, cwd: &Path, args: &[&str], golden: &str) {
    let output = channel_command(root, cwd, args);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert_eq!(stdout, golden);
    assert!(!stdout.contains(SECRET_VALUE));
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn channel_help_matches_committed_golden_without_ref_checkout() {
    let root = channel_temp_dir("help");
    let repo = channel_seed(&root);
    channel_assert_golden(
        &root,
        &repo,
        &["channel", "help"],
        include_str!("fixtures/native-channel/channel-help.stdout"),
    );
}

#[test]
fn channel_ls_and_list_json_match_committed_goldens_and_redact_tokens() {
    let root = channel_temp_dir("list");
    let repo = channel_seed(&root);
    channel_assert_golden(
        &root,
        &repo,
        &["channel", "ls", "-v"],
        include_str!("fixtures/native-channel/channel-ls.stdout"),
    );
    channel_assert_golden(
        &root,
        &repo,
        &["channel", "list", "hermes-discord", "--json"],
        include_str!("fixtures/native-channel/channel-list-json.stdout"),
    );
}

#[test]
fn channel_providers_matches_committed_golden_with_custom_mcp_provider() {
    let root = channel_temp_dir("providers");
    let repo = channel_seed(&root);
    channel_assert_golden(
        &root,
        &repo,
        &["channel", "providers"],
        include_str!("fixtures/native-channel/channel-providers.stdout"),
    );
}

#[test]
fn channel_test_matches_committed_golden_without_real_discord_or_pass() {
    let root = channel_temp_dir("test");
    let repo = channel_seed(&root);
    channel_assert_golden(
        &root,
        &repo,
        &["channel", "test", "hermes-discord"],
        include_str!("fixtures/native-channel/channel-test.stdout"),
    );
}

#[test]
fn channel_add_and_rm_plugin_match_committed_goldens_and_archive_previous_config() {
    let root = channel_temp_dir("add-rm");
    let repo = channel_empty_repo(&root);
    channel_assert_golden(
        &root,
        &repo,
        &[
            "channel",
            "add",
            "hermes-discord",
            "discord",
            "--env",
            "DISCORD_BOT_TOKEN=super-secret-channel-token",
            "--pass",
            "discord/hermes-token",
        ],
        include_str!("fixtures/native-channel/channel-add.stdout"),
    );
    let config_path = root.join("home/.claude/channels/hermes-discord/config.json");
    let config = fs::read_to_string(&config_path).expect("config");
    assert!(
        config.contains(SECRET_VALUE),
        "secret is stored only in hermetic temp config"
    );

    channel_assert_golden(
        &root,
        &repo,
        &["channel", "rm", "hermes-discord", "discord"],
        include_str!("fixtures/native-channel/channel-rm-plugin.stdout"),
    );
    let after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&config_path).expect("config after rm"))
            .expect("json");
    assert_eq!(after["plugins"].as_array().expect("plugins").len(), 0);
    let archives = fs::read_dir(root.join("home/.claude/channels/hermes-discord/archive"))
        .expect("archive dir")
        .count();
    assert!(archives >= 1, "rm preserves previous config in archive");
}

#[test]
fn channel_add_multiple_then_rm_all_preserves_maw_js_no_confirm_parity() {
    let root = channel_temp_dir("rm-all");
    let repo = channel_empty_repo(&root);
    channel_assert_golden(
        &root,
        &repo,
        &["channel", "add", "hermes-discord", "fakechat"],
        include_str!("fixtures/native-channel/channel-add-fakechat.stdout"),
    );
    channel_assert_golden(
        &root,
        &repo,
        &["channel", "add", "hermes-discord", "telegram"],
        include_str!("fixtures/native-channel/channel-add-telegram.stdout"),
    );
    channel_assert_golden(
        &root,
        &repo,
        &["channel", "remove", "hermes-discord"],
        include_str!("fixtures/native-channel/channel-rm-all.stdout"),
    );
    let after: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("home/.claude/channels/hermes-discord/config.json"))
            .expect("config after rm all"),
    )
    .expect("json");
    assert_eq!(after["plugins"].as_array().expect("plugins").len(), 0);
}

#[test]
fn channel_add_repo_mode_writes_repo_config_and_gitignore_only() {
    let root = channel_temp_dir("repo-mode");
    let repo = channel_empty_repo(&root);
    let repo_target = repo.join("repo-target");
    fs::create_dir_all(&repo_target).expect("repo target");

    let output = channel_command(
        &root,
        &repo,
        &[
            "channel",
            "add",
            "hermes-discord",
            "discord",
            "--repo",
            "repo-target",
            "--env",
            "DISCORD_BOT_TOKEN=super-secret-channel-token",
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("repo mode"));
    assert!(!stdout.contains(SECRET_VALUE));
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert!(repo_target.join(".claude/channel.json").exists());
    assert!(fs::read_to_string(repo_target.join(".gitignore"))
        .expect("gitignore")
        .contains(".claude/.env"));
    assert!(!root
        .join("home/.claude/channels/hermes-discord/config.json")
        .exists());
}

#[test]
fn channel_setup_discord_pass_matches_golden_and_preserves_valid_access() {
    let root = channel_temp_dir("setup-discord");
    let repo = channel_empty_repo(&root);
    channel_install_official_plugins(&root);
    let state = root.join("home/.claude/channels/hermes-discord");
    fs::create_dir_all(&state).expect("state");
    fs::write(
        state.join("access.json"),
        r#"{
  "dmPolicy": "allowlist",
  "allowFrom": ["111111111111111111", "222222222222222222"],
  "groups": {"humans":["111111111111111111"]},
  "pending": {"keep":"human-approved"}
}
"#,
    )
    .expect("access");

    channel_assert_golden_with_env(
        &root,
        &repo,
        &[
            "channel",
            "setup",
            "hermes-discord",
            "discord",
            "--pass",
            "discord/hermes-token",
            "--guild",
            "999999999999999999",
        ],
        &[
            (
                "MAW_RS_CHANNEL_FAKE_PASS_TOKEN",
                "MTIzNDU2Nzg5MDEyMzQ1Njc4.secret.tail",
            ),
            (
                "MAW_RS_CHANNEL_FAKE_DISCORD_GUILDS",
                "999999999999999999:Ops Guild;888888888888888888:Other Guild",
            ),
        ],
        include_str!("fixtures/native-channel/channel-setup-discord.stdout"),
    );
    let access = fs::read_to_string(state.join("access.json")).expect("access after");
    assert!(access.contains("111111111111111111"));
    assert!(access.contains("human-approved"));
    assert!(!access.contains("691531480689541170"));
    let config = fs::read_to_string(state.join("config.json")).expect("config");
    assert!(config.contains("pass:discord/hermes-token"));
    assert!(config.contains("DISCORD_STATE_DIR"));
}

#[test]
fn channel_setup_discord_wrong_guild_matches_maw_js_soft_parity_and_resets_malformed_access() {
    let root = channel_temp_dir("setup-discord-wrong-guild");
    let repo = channel_empty_repo(&root);
    channel_install_official_plugins(&root);
    let state = root.join("home/.claude/channels/hermes-discord");
    fs::create_dir_all(&state).expect("state");
    fs::write(state.join("access.json"), "not-json-secret-free").expect("malformed access");

    channel_assert_golden_with_env(
        &root,
        &repo,
        &[
            "channel",
            "setup",
            "hermes-discord",
            "discord",
            "--pass",
            "discord/hermes-token",
            "--guild",
            "777777777777777777",
        ],
        &[
            (
                "MAW_RS_CHANNEL_FAKE_PASS_TOKEN",
                "MTIzNDU2Nzg5MDEyMzQ1Njc4.secret.tail",
            ),
            (
                "MAW_RS_CHANNEL_FAKE_DISCORD_GUILDS",
                "999999999999999999:Ops Guild",
            ),
        ],
        include_str!("fixtures/native-channel/channel-setup-discord-wrong-guild.stdout"),
    );
    let access: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(state.join("access.json")).expect("access after reset"),
    )
    .expect("json");
    assert_eq!(access["dmPolicy"], "allowlist");
    assert_eq!(access["allowFrom"][0], "691531480689541170");
    assert!(
        fs::read_dir(state.join("archive"))
            .expect("archive")
            .count()
            >= 1
    );
}

#[test]
fn channel_setup_telegram_pass_matches_golden_without_real_rest() {
    let root = channel_temp_dir("setup-telegram");
    let repo = channel_empty_repo(&root);
    channel_install_official_plugins(&root);
    channel_assert_golden_with_env(
        &root,
        &repo,
        &[
            "channel",
            "setup",
            "hermes-telegram",
            "telegram",
            "--pass",
            "telegram/hermes-token",
        ],
        &[("MAW_RS_CHANNEL_FAKE_PASS_TOKEN", SECRET_VALUE)],
        include_str!("fixtures/native-channel/channel-setup-telegram.stdout"),
    );
}

#[test]
fn channel_setup_imessage_matches_golden_with_fake_darwin() {
    let root = channel_temp_dir("setup-imessage");
    let repo = channel_empty_repo(&root);
    channel_install_official_plugins(&root);
    channel_assert_golden_with_env(
        &root,
        &repo,
        &["channel", "setup", "hermes-imessage", "imessage"],
        &[("MAW_RS_CHANNEL_FAKE_PLATFORM", "darwin")],
        include_str!("fixtures/native-channel/channel-setup-imessage.stdout"),
    );
}

#[test]
fn channel_setup_github_clones_with_fake_ghq_and_registers_config() {
    let root = channel_temp_dir("setup-github");
    let repo = channel_empty_repo(&root);
    let ghq_root = root.join("ghq");
    fs::create_dir_all(&ghq_root).expect("ghq root");
    let ghq_log = root.join("ghq.log");
    let output = channel_command_with_env(
        &root,
        &repo,
        &[
            "channel",
            "setup",
            "hermes-git",
            "github:ARRA-01/claude-channel-relay",
            "--pass",
            "github/hermes-token",
        ],
        &[
            (
                "MAW_RS_CHANNEL_FAKE_GHQ_ROOT",
                ghq_root.to_str().expect("ghq root utf8"),
            ),
            (
                "MAW_RS_CHANNEL_FAKE_GHQ_GET_LOG",
                ghq_log.to_str().expect("ghq log utf8"),
            ),
        ],
    );
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let ghq_display = ghq_root.display().to_string();
    let normalized = stdout
        .replace(&format!("/private{ghq_display}"), "<GHQ_ROOT>")
        .replace(&ghq_display, "<GHQ_ROOT>");
    assert_eq!(
        normalized,
        include_str!("fixtures/native-channel/channel-setup-github-clone.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert_eq!(
        fs::read_to_string(&ghq_log).expect("ghq log"),
        "ghq get https://github.com/ARRA-01/claude-channel-relay\n"
    );
    assert!(ghq_root
        .join("github.com/ARRA-01/claude-channel-relay")
        .is_dir());
    let config_path = root.join("home/.claude/channels/hermes-git/config.json");
    assert!(config_path.exists());
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&config_path).expect("config")).expect("json");
    assert_eq!(config["token_source"], "pass:github/hermes-token");
    assert_eq!(config["plugins"][0]["id"], "server:relay");
    assert_eq!(
        config["plugins"][0]["source"],
        "github:ARRA-01/claude-channel-relay"
    );
    assert_eq!(config["plugins"][0]["dev"], true);
    assert_eq!(config["plugins"][0]["mcp"]["command"], "bun");
    assert!(!fs::read_to_string(&config_path)
        .expect("config text")
        .contains(SECRET_VALUE));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            fs::metadata(&config_path)
                .expect("config metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn channel_setup_github_locate_existing_skips_clone() {
    let root = channel_temp_dir("setup-github-existing");
    let repo = channel_empty_repo(&root);
    let ghq_root = root.join("ghq");
    fs::create_dir_all(ghq_root.join("github.com/ARRA-01/claude-channel-relay"))
        .expect("existing repo");
    let ghq_log = root.join("ghq.log");
    let output = channel_command_with_env(
        &root,
        &repo,
        &[
            "channel",
            "setup",
            "hermes-git",
            "github:ARRA-01/claude-channel-relay",
        ],
        &[
            (
                "MAW_RS_CHANNEL_FAKE_GHQ_ROOT",
                ghq_root.to_str().expect("ghq root utf8"),
            ),
            (
                "MAW_RS_CHANNEL_FAKE_GHQ_GET_LOG",
                ghq_log.to_str().expect("ghq log utf8"),
            ),
        ],
    );
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("repo already present"));
    assert!(!ghq_log.exists(), "existing repo must not run ghq get");
}

#[test]
fn channel_setup_github_clone_failure_writes_no_config() {
    let root = channel_temp_dir("setup-github-fail");
    let repo = channel_empty_repo(&root);
    let ghq_root = root.join("ghq");
    fs::create_dir_all(&ghq_root).expect("ghq root");
    let output = channel_command_with_env(
        &root,
        &repo,
        &[
            "channel",
            "setup",
            "hermes-git",
            "github:ARRA-01/claude-channel-relay",
        ],
        &[
            (
                "MAW_RS_CHANNEL_FAKE_GHQ_ROOT",
                ghq_root.to_str().expect("ghq root utf8"),
            ),
            ("MAW_RS_CHANNEL_FAKE_GHQ_GET_FAIL", "1"),
        ],
    );
    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), "");
    assert!(String::from_utf8(output.stderr)
        .expect("stderr")
        .contains("ghq get failed"));
    assert!(!root
        .join("home/.claude/channels/hermes-git/config.json")
        .exists());
}

#[test]
fn channel_setup_secret_and_snowflake_guards_fail_before_io() {
    let root = channel_temp_dir("setup-guards");
    let repo = channel_empty_repo(&root);
    channel_install_official_plugins(&root);
    let output = channel_command_with_env(
        &root,
        &repo,
        &[
            "channel",
            "setup",
            "hermes-discord",
            "discord",
            "--pass",
            "discord/hermes-token",
        ],
        &[("MAW_RS_CHANNEL_FAKE_PASS_TOKEN", "")],
    );
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("pass key 'discord/hermes-token' not found"));
    assert!(!stdout.contains(SECRET_VALUE));
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");

    for args in [
        ["channel", "setup", "../secret", "discord"].as_slice(),
        [
            "channel",
            "setup",
            "hermes-discord",
            "discord",
            "--guild",
            "abc",
        ]
        .as_slice(),
        ["channel", "setup", "hermes-discord", "github:../bad"].as_slice(),
        ["channel", "setup", "hermes-discord", "unknown"].as_slice(),
    ] {
        let output = channel_command(&root, &repo, args);
        assert!(!output.status.success(), "args={args:?}");
    }
}

#[test]
fn channel_migrate_missing_to_repo_matches_usage_golden() {
    let root = channel_temp_dir("migrate-usage");
    let repo = channel_empty_repo(&root);
    let output = channel_command(&root, &repo, &["channel", "migrate"]);
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).expect("stderr"),
        include_str!("fixtures/native-channel/channel-migrate-usage.stderr")
    );
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), "");
}

#[test]
fn channel_migrate_dry_run_all_is_zero_write_even_with_remove_global() {
    let root = channel_temp_dir("migrate-dry-run");
    let cwd = channel_empty_repo(&root);
    let state = channel_seed_global_config(&root, "hermes-discord", "discord");
    let repo = channel_seed_fake_repo(&root, "hermes-discord");
    let before = fs::read_to_string(state.join("config.json")).expect("before config");

    channel_assert_golden_with_env(
        &root,
        &cwd,
        &[
            "channel",
            "migrate",
            "--to-repo",
            "--dry-run",
            "--remove-global",
        ],
        &[("MAW_RS_CHANNEL_FAKE_GHQ_ROOT", "ghq")],
        include_str!("fixtures/native-channel/channel-migrate-dry-run.stdout"),
    );
    assert_eq!(
        fs::read_to_string(state.join("config.json")).expect("after config"),
        before
    );
    assert!(!state.join("archive").exists(), "dry-run must not archive");
    assert!(
        !repo.join(".claude/channel.json").exists(),
        "dry-run must not copy"
    );
}

#[test]
fn channel_migrate_one_copies_repo_config_and_prints_tip_without_delete() {
    let root = channel_temp_dir("migrate-one");
    let cwd = channel_empty_repo(&root);
    let state = channel_seed_global_config(&root, "hermes-discord", "discord");
    let repo = channel_seed_fake_repo(&root, "hermes-discord");

    channel_assert_golden_with_env(
        &root,
        &cwd,
        &["channel", "migrate", "--to-repo", "hermes-discord"],
        &[("MAW_RS_CHANNEL_FAKE_GHQ_ROOT", "ghq")],
        include_str!("fixtures/native-channel/channel-migrate-one.stdout"),
    );
    assert!(state.join("config.json").exists());
    assert!(repo.join(".claude/channel.json").exists());
    assert!(fs::read_to_string(repo.join(".gitignore"))
        .expect("gitignore")
        .contains(".claude/.env"));
}

#[test]
fn channel_migrate_remove_global_archives_before_delete_and_preserves_state_files() {
    let root = channel_temp_dir("migrate-remove-global");
    let cwd = channel_empty_repo(&root);
    let state = channel_seed_global_config(&root, "hermes-discord", "discord");
    let repo = channel_seed_fake_repo(&root, "hermes-discord");

    channel_assert_golden_with_env(
        &root,
        &cwd,
        &[
            "channel",
            "migrate",
            "--to-repo",
            "hermes-discord",
            "--remove-global",
        ],
        &[("MAW_RS_CHANNEL_FAKE_GHQ_ROOT", "ghq")],
        include_str!("fixtures/native-channel/channel-migrate-remove-global.stdout"),
    );
    assert!(
        repo.join(".claude/channel.json").exists(),
        "copy succeeded first"
    );
    assert!(
        !state.join("config.json").exists(),
        "global config removed after copy"
    );
    assert!(
        state.join("state.txt").exists(),
        "non-config state survives"
    );
    assert!(state.join("access.json").exists(), "access survives");
    assert!(
        fs::read_dir(state.join("archive"))
            .expect("archive")
            .count()
            >= 1
    );
}

#[test]
fn channel_migrate_copy_fail_never_deletes_global_config() {
    let root = channel_temp_dir("migrate-copy-fail");
    let cwd = channel_empty_repo(&root);
    let state = channel_seed_global_config(&root, "hermes-discord", "discord");

    channel_assert_golden_with_env(
        &root,
        &cwd,
        &[
            "channel",
            "migrate",
            "--to-repo",
            "hermes-discord",
            "--remove-global",
        ],
        &[("MAW_RS_CHANNEL_FAKE_GHQ_ROOT", "ghq")],
        include_str!("fixtures/native-channel/channel-migrate-no-repo.stdout"),
    );
    assert!(
        state.join("config.json").exists(),
        "no repo means no delete"
    );
    assert!(!state.join("archive").exists(), "no copy means no archive");
}

#[test]
fn channel_migrate_skips_existing_repo_config_without_overwrite() {
    let root = channel_temp_dir("migrate-existing");
    let cwd = channel_empty_repo(&root);
    channel_seed_global_config(&root, "hermes-discord", "discord");
    let repo = channel_seed_fake_repo(&root, "hermes-discord");
    fs::create_dir_all(repo.join(".claude")).expect("repo claude");
    fs::write(
        repo.join(".claude/channel.json"),
        r#"{
  "plugins": []
}
"#,
    )
    .expect("existing");

    channel_assert_golden_with_env(
        &root,
        &cwd,
        &["channel", "migrate", "--to-repo", "hermes-discord"],
        &[("MAW_RS_CHANNEL_FAKE_GHQ_ROOT", "ghq")],
        include_str!("fixtures/native-channel/channel-migrate-existing.stdout"),
    );
    assert_eq!(
        fs::read_to_string(repo.join(".claude/channel.json")).expect("existing after"),
        r#"{
  "plugins": []
}
"#
    );
}

#[test]
fn channel_migrate_missing_global_skip_and_empty_all_match_goldens() {
    let root = channel_temp_dir("migrate-missing-empty");
    let cwd = channel_empty_repo(&root);
    channel_assert_golden_with_env(
        &root,
        &cwd,
        &["channel", "migrate", "--to-repo", "ghost"],
        &[("MAW_RS_CHANNEL_FAKE_GHQ_ROOT", "ghq")],
        include_str!("fixtures/native-channel/channel-migrate-no-global.stdout"),
    );
    channel_assert_golden_with_env(
        &root,
        &cwd,
        &["channel", "migrate", "--to-repo"],
        &[("MAW_RS_CHANNEL_FAKE_GHQ_ROOT", "ghq")],
        include_str!("fixtures/native-channel/channel-migrate-empty.stdout"),
    );
}

#[test]
fn channel_number_67_guards_reject_traversal_and_flag_values_before_io() {
    let root = channel_temp_dir("guards");
    let repo = channel_seed(&root);

    for args in [
        ["channel", "test", "../secret"].as_slice(),
        ["channel", "test", "-bad"].as_slice(),
        ["channel", "ls", "hermes/discord"].as_slice(),
        ["channel", "providers", "extra"].as_slice(),
        ["channel", "add", "-bad", "discord"].as_slice(),
        ["channel", "add", "hermes-discord", "../discord"].as_slice(),
        [
            "channel",
            "add",
            "hermes-discord",
            "discord",
            "--repo",
            "../repo",
        ]
        .as_slice(),
        ["channel", "rm", "hermes/discord"].as_slice(),
        ["channel", "migrate", "--to-repo", "../secret"].as_slice(),
        ["channel", "migrate", "--to-repo", "-bad"].as_slice(),
        ["channel", "migrate", "--to-repo", "hermes/discord"].as_slice(),
    ] {
        let output = channel_command(&root, &repo, args);
        assert!(!output.status.success(), "args={args:?}");
    }
}

#[test]
fn channel_dispatch_registers_part120_native() {
    assert_eq!(
        maw_cli::dispatcher_status("channel"),
        maw_cli::DispatchKind::Native
    );
}
