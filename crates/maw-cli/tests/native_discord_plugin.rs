#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use maw_discord::{run_discord_command_with, DiscordEnv, DiscordHttpResponse, DiscordRest};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    process::Command,
    sync::{Mutex as StdMutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

struct MockRest {
    calls: StdMutex<Vec<String>>,
    posts: StdMutex<Vec<(String, serde_json::Value)>>,
    responses: BTreeMap<String, DiscordHttpResponse>,
}

impl DiscordRest for MockRest {
    fn get_json<'a>(
        &'a self,
        path: &'a str,
        _token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<DiscordHttpResponse, String>> + Send + 'a>> {
        Box::pin(async move {
            assert!(path.starts_with('/'));
            assert!(!path.contains("://"));
            self.calls.lock().expect("calls").push(path.to_owned());
            self.responses
                .get(path)
                .cloned()
                .ok_or_else(|| format!("missing mock response: {path}"))
        })
    }

    fn post_json<'a>(
        &'a self,
        path: &'a str,
        _token: &'a str,
        body: serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<DiscordHttpResponse, String>> + Send + 'a>> {
        Box::pin(async move {
            assert!(path.starts_with('/'));
            assert!(!path.contains("://"));
            self.posts
                .lock()
                .expect("posts")
                .push((path.to_owned(), body));
            self.responses
                .get(path)
                .cloned()
                .ok_or_else(|| "REST error bearer SECRET ghp_SECRET github_pat_SECRET https://user:pass@example.test".to_owned())
        })
    }
}

fn env_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-native-discord-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn env(root: &std::path::Path) -> DiscordEnv {
    DiscordEnv {
        home: root.join("home"),
        ghq_root: root.join("ghq"),
        hostname: "fleetbox.example".to_owned(),
    }
}

fn seed_bot(root: &std::path::Path, bot: &str) -> DiscordEnv {
    let env = env(root);
    fs::create_dir_all(env.home.join(".password-store/discord")).expect("pass dir");
    fs::write(
        env.home
            .join(".password-store/discord")
            .join(format!("{bot}-token.gpg")),
        "encrypted-placeholder\n",
    )
    .expect("token placeholder");
    let repo = env.ghq_root.join("github.com/Soul-Brews-Studio").join(bot);
    fs::create_dir_all(repo.join(".discord")).expect("discord dir");
    fs::write(
        repo.join(".discord/channel-map.json"),
        "{\n  \"general\": \"111\"\n}\n",
    )
    .expect("channel map");
    fs::write(
        repo.join(".discord/access.json"),
        "{\n  \"dmPolicy\": \"allowlist\",\n  \"allowFrom\": [],\n  \"groups\": {\n    \"111\": { \"requireMention\": true, \"allowFrom\": [\"42\"] }\n  },\n  \"pending\": {}\n}\n",
    )
    .expect("access");
    let oracle = env
        .ghq_root
        .join("github.com/Soul-Brews-Studio/discord-oracle/src");
    fs::create_dir_all(&oracle).expect("oracle");
    fs::write(
        oracle.join("state-dirs.ts"),
        format!(
            "export const STATE_DIRS = {{\n  \"{bot}\": \"/state/{bot}\",\n}};\nexport const ANCHORS = {{\n  \"{bot}\": \"fleetbox\",\n}};\n"
        ),
    )
    .expect("state dirs");
    env
}

#[tokio::test]
async fn discord_members_rejects_non_numeric_allow_from_without_rest_call() {
    let _guard = env_lock().lock().await;
    std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
    let root = temp_dir("invalid-user-id");
    let env = seed_bot(&root, "nova-oracle");
    std::env::set_var("DISCORD_BOT_TOKEN", "mock-token-never-logged");
    let access_path = env
        .ghq_root
        .join("github.com/Soul-Brews-Studio/nova-oracle/.discord/access.json");
    fs::write(
        access_path,
        "{\n  \"dmPolicy\": \"allowlist\",\n  \"allowFrom\": [],\n  \"groups\": {\n    \"111\": { \"requireMention\": true, \"allowFrom\": [\"@me/guilds\"] }\n  },\n  \"pending\": {}\n}\n",
    )
    .expect("access with invalid id");
    let rest = MockRest {
        calls: StdMutex::new(Vec::new()),
        posts: StdMutex::new(Vec::new()),
        responses: BTreeMap::new(),
    };

    let output = run_discord_command_with(
        &[
            "members".to_owned(),
            "nova-oracle".to_owned(),
            "general".to_owned(),
            "--json".to_owned(),
        ],
        &env,
        &rest,
    )
    .await;

    assert_eq!(output.code, 0);
    assert!(
        output.stdout.contains("\"invalid\": true"),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("@me/guilds"), "{}", output.stdout);
    assert!(
        rest.calls.lock().expect("calls").is_empty(),
        "invalid allow_from id must not be interpolated into a Discord REST path"
    );
    std::env::remove_var("DISCORD_BOT_TOKEN");
}

#[test]
fn discord_is_native_not_bun_fallback() {
    assert_eq!(dispatcher_status("discord"), DispatchKind::Native);
}

#[tokio::test]
async fn discord_version_committed_golden_without_ref_checkout() {
    let _guard = env_lock().lock().await;
    std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
    let root = temp_dir("version");
    let env = env(&root);
    let rest = MockRest {
        calls: StdMutex::new(Vec::new()),
        posts: StdMutex::new(Vec::new()),
        responses: BTreeMap::new(),
    };

    let output = run_discord_command_with(&["version".to_owned()], &env, &rest).await;

    assert_eq!(output.code, 0);
    assert_eq!(
        output.stdout,
        include_str!("fixtures/native-discord/version.stdout")
    );
    assert_eq!(output.stderr, "");
}

#[tokio::test]
async fn discord_access_list_and_inventory_use_mocked_rest_only() {
    let _guard = env_lock().lock().await;
    std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
    let root = temp_dir("inventory");
    let env = seed_bot(&root, "nova-oracle");
    std::env::set_var("DISCORD_BOT_TOKEN", "mock-token-never-logged");
    let mut responses = BTreeMap::new();
    responses.insert(
        "/users/@me/guilds".to_owned(),
        DiscordHttpResponse {
            status: 200,
            body: json!([{ "id": "999", "name": "Guild One" }]),
            retry_after: None,
        },
    );
    responses.insert(
        "/guilds/999/channels".to_owned(),
        DiscordHttpResponse {
            status: 200,
            body: json!([{ "id": "111", "name": "general", "type": 0, "guild_id": "999" }]),
            retry_after: None,
        },
    );
    responses.insert(
        "/users/42".to_owned(),
        DiscordHttpResponse {
            status: 200,
            body: json!({ "id": "42", "username": "tk" }),
            retry_after: None,
        },
    );
    let rest = MockRest {
        calls: StdMutex::new(Vec::new()),
        posts: StdMutex::new(Vec::new()),
        responses,
    };

    let listed = run_discord_command_with(
        &[
            "access".to_owned(),
            "nova-oracle".to_owned(),
            "list".to_owned(),
        ],
        &env,
        &rest,
    )
    .await;
    assert!(listed
        .stdout
        .contains("🪪 maw discord access nova-oracle list"));
    assert!(listed.stdout.contains("general"));

    let inventory = run_discord_command_with(
        &["inventory".to_owned(), "nova-oracle".to_owned()],
        &env,
        &rest,
    )
    .await;
    assert!(
        inventory.stdout.contains("📋 nova-oracle — full inventory"),
        "{}",
        inventory.stdout
    );
    assert!(inventory.stdout.contains("✓ #general"));
    let calls = rest.calls.lock().expect("calls");
    assert!(calls.iter().all(|path| path.starts_with('/')));
    assert!(calls.contains(&"/users/@me/guilds".to_owned()));
    assert!(calls.contains(&"/guilds/999/channels".to_owned()));
    std::env::remove_var("DISCORD_BOT_TOKEN");
}

fn empty_rest() -> MockRest {
    MockRest {
        calls: StdMutex::new(Vec::new()),
        posts: StdMutex::new(Vec::new()),
        responses: BTreeMap::new(),
    }
}

#[tokio::test]
async fn discord_pair_route_and_serve_are_native_and_hermetic() {
    let _guard = env_lock().lock().await;
    std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
    std::env::set_var("DISCORD_BOT_TOKEN", "mock-token-never-logged");
    let root = temp_dir("pair-route-serve");
    let env = seed_bot(&root, "nova-oracle");
    let mut responses = BTreeMap::new();
    responses.insert(
        "/channels/222/messages".to_owned(),
        DiscordHttpResponse {
            status: 200,
            body: json!({"id": "m1"}),
            retry_after: None,
        },
    );
    let rest = MockRest {
        calls: StdMutex::new(Vec::new()),
        posts: StdMutex::new(Vec::new()),
        responses,
    };

    let pair = run_discord_command_with(
        &[
            "pair".to_owned(),
            "nova-oracle".to_owned(),
            "222".to_owned(),
            "--allow".to_owned(),
            "42".to_owned(),
        ],
        &env,
        &rest,
    )
    .await;
    assert_eq!(pair.code, 0, "{}", pair.stdout);
    assert!(pair.stdout.contains("✓ paired nova-oracle → 222"));
    assert!(!pair.stdout.contains("mock-token-never-logged"));

    let route = run_discord_command_with(
        &[
            "route".to_owned(),
            "nova-oracle".to_owned(),
            "ops".to_owned(),
            "222".to_owned(),
        ],
        &env,
        &rest,
    )
    .await;
    assert_eq!(route.code, 0, "{}", route.stdout);
    assert!(route.stdout.contains("✓ route ops → 222"));

    let posted = run_discord_command_with(
        &[
            "serve".to_owned(),
            "nova-oracle".to_owned(),
            "--channel".to_owned(),
            "ops".to_owned(),
            "--message".to_owned(),
            "hello discord".to_owned(),
        ],
        &env,
        &rest,
    )
    .await;
    assert_eq!(posted.code, 0, "{}", posted.stdout);
    assert!(posted.stdout.contains("✓ posted Discord message to 222"));
    let posts = rest.posts.lock().expect("posts");
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].0, "/channels/222/messages");
    assert_eq!(posts[0].1["content"], "hello discord");
    std::env::remove_var("DISCORD_BOT_TOKEN");
}

#[tokio::test]
async fn discord_security_guards_reject_bad_inputs_and_non_loopback() {
    let _guard = env_lock().lock().await;
    let root = temp_dir("guards");
    let env = seed_bot(&root, "nova-oracle");
    let rest = empty_rest();

    let bad_pair = run_discord_command_with(
        &["pair".to_owned(), "../bad".to_owned(), "111".to_owned()],
        &env,
        &rest,
    )
    .await;
    assert_ne!(bad_pair.code, 0);
    assert!(bad_pair.stdout.contains("#67 guard"));

    let bad_serve = run_discord_command_with(
        &[
            "serve".to_owned(),
            "--host".to_owned(),
            "0.0.0.0".to_owned(),
            "--dry-run".to_owned(),
        ],
        &env,
        &rest,
    )
    .await;
    assert_ne!(bad_serve.code, 0);
    assert!(bad_serve.stdout.contains("refuses non-loopback"));

    let dry =
        run_discord_command_with(&["serve".to_owned(), "--dry-run".to_owned()], &env, &rest).await;
    assert_eq!(dry.code, 0);
    assert_eq!(
        dry.stdout,
        include_str!("fixtures/native-discord/serve-dry-run.stdout")
    );
    assert!(!dry.stdout.contains("0.0.0.0"));
}

#[tokio::test]
async fn discord_serve_rest_errors_are_redacted() {
    let _guard = env_lock().lock().await;
    std::env::set_var("DISCORD_BOT_TOKEN", "mock-token-never-logged");
    let root = temp_dir("redact");
    let env = seed_bot(&root, "nova-oracle");
    let rest = empty_rest();
    let out = run_discord_command_with(
        &[
            "serve".to_owned(),
            "nova-oracle".to_owned(),
            "--channel".to_owned(),
            "general".to_owned(),
            "--message".to_owned(),
            "hello".to_owned(),
        ],
        &env,
        &rest,
    )
    .await;
    assert_ne!(out.code, 0);
    assert!(!out.stdout.contains("bearer SECRET"), "{}", out.stdout);
    assert!(!out.stdout.contains("ghp_SECRET"), "{}", out.stdout);
    assert!(!out.stdout.contains("github_pat_SECRET"), "{}", out.stdout);
    assert!(!out.stdout.contains("user:pass"), "{}", out.stdout);
    assert!(
        !out.stdout.contains("mock-token-never-logged"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("[REDACTED]"), "{}", out.stdout);
    std::env::remove_var("DISCORD_BOT_TOKEN");
}

fn discord_write_fake_marker(dir: &Path, name: &str, marker: &str) {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\necho {marker}\nexit 77\n")).expect("fake marker");
    #[cfg(unix)]
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod fake marker");
}

#[test]
fn discord_runtime_fake_maw_no_delegate_proof() {
    let root = temp_dir("runtime-proof");
    let bin = root.join("bin");
    fs::create_dir_all(&bin).expect("bin");
    discord_write_fake_marker(&bin, "maw", "DELEGATED-MAW");
    discord_write_fake_marker(&bin, "bun", "DELEGATED-BUN");
    let output = Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .arg("discord")
        .arg("serve")
        .arg("--dry-run")
        .env("PATH", &bin)
        .env("HOME", root.join("home"))
        .env("GHQ_ROOT", root.join("ghq"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs discord");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stdout={stdout}\nstderr={stderr}");
    assert!(stdout.contains("127.0.0.1"), "stdout={stdout}");
    assert!(!stdout.contains("DELEGATED-MAW"), "stdout={stdout}");
    assert!(!stderr.contains("DELEGATED-MAW"), "stderr={stderr}");
    assert!(!stdout.contains("DELEGATED-BUN"), "stdout={stdout}");
    assert!(!stderr.contains("DELEGATED-BUN"), "stderr={stderr}");
}
