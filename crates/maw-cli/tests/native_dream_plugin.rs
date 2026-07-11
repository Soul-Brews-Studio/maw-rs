use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::Command;

fn dream_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn dream_write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent dir");
    }
    std::fs::write(path, text).expect("write file");
}

fn dream_git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Dream Test")
        .env("GIT_AUTHOR_EMAIL", "dream@example.invalid")
        .env("GIT_COMMITTER_NAME", "Dream Test")
        .env("GIT_COMMITTER_EMAIL", "dream@example.invalid")
        .env("GIT_AUTHOR_DATE", "2026-06-24T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-06-24T00:00:00Z")
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {:?}\nstdout={}\nstderr={}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn dream_seed_repo(root: &Path, slug: &str, message: &str, date: &str) -> PathBuf {
    let repo = root.join("ghq/github.com").join(slug);
    std::fs::create_dir_all(repo.join("ψ")).expect("psi dir");
    Command::new("git")
        .arg("init")
        .arg(&repo)
        .output()
        .expect("git init");
    dream_write(&repo.join("README.md"), message);
    dream_git(&repo, &["add", "README.md"]);
    let output = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["commit", "-m", message])
        .env("GIT_AUTHOR_NAME", "Dream Test")
        .env("GIT_AUTHOR_EMAIL", "dream@example.invalid")
        .env("GIT_COMMITTER_NAME", "Dream Test")
        .env("GIT_COMMITTER_EMAIL", "dream@example.invalid")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .expect("git commit");
    assert!(
        output.status.success(),
        "commit stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    repo
}

fn dream_seed(name: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root =
        std::env::temp_dir().join(format!("maw-rs-native-dream-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let config = root.join("config");
    let cache = root.join("cache");
    let alpha = dream_seed_repo(
        &root,
        "org/alpha-oracle",
        "recent alpha work",
        "2026-06-24T00:00:00Z",
    );
    let _sleepy = dream_seed_repo(
        &root,
        "org/sleepy-oracle",
        "sleepy old work",
        "2026-03-17T00:00:00Z",
    );
    for i in 0..6 {
        dream_write(&alpha.join(format!("dirty-{i}.txt")), "dirty\n");
    }
    dream_write(
        &alpha.join("ψ/inbox/handoff/2026-06-24.md"),
        "- [ ] finish native dream\n",
    );
    dream_write(
        &config.join("fleet/01-alpha.json"),
        r#"{"windows":[{"name":"alpha-oracle","repo":"org/alpha-oracle"},{"name":"sleepy-oracle","repo":"org/sleepy-oracle"}]}"#,
    );
    (root, home, config, cache)
}

fn dream_command(root: &Path, home: &Path, config: &Path, cache: &Path) -> Command {
    let plugins = root.join("plugins");
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-dream/dream-plugin");
    std::fs::create_dir_all(&plugins).expect("plugins dir");
    std::fs::create_dir_all(plugins.join("dream")).expect("dream plugin dir");
    std::fs::copy(
        fixture.join("plugin.json"),
        plugins.join("dream/plugin.json"),
    )
    .expect("manifest");
    std::fs::copy(
        fixture.join("plugin.wasm"),
        plugins.join("dream/plugin.wasm"),
    )
    .expect("wasm");
    let mut command = Command::new(dream_bin());
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", home)
        .env("MAW_CONFIG_DIR", config)
        .env("MAW_CACHE_DIR", cache)
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("GHQ_ROOT", root.join("ghq"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_PLUGINS_DIR", plugins)
        .env("MAW_DREAM_DATE", "2026-06-25")
        .env("MAW_DREAM_EPOCH", "1782345600")
        .env("PATH", std::env::var_os("PATH").unwrap_or_default());
    command
}

#[test]
fn dream_plugin_porcelain_golden_is_hermetic() {
    let (root, home, config, cache) = dream_seed("porcelain");
    let output = dream_command(&root, &home, &config, &cache)
        .args([
            "dream",
            "--porcelain",
            "--limit",
            "20",
            "--date",
            "2026-06-25",
        ])
        .output()
        .expect("run dream");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = include_str!("fixtures/native-dream/porcelain.stdout");
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), expected);
    assert_eq!(dispatcher_status("dream"), DispatchKind::NativeError);
    assert!(
        !root.join("ψ/writing/dreams").exists(),
        "porcelain should not write dream state"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn dream_plugin_writes_seeded_state_and_guards_values() {
    let (root, home, config, cache) = dream_seed("write");
    let output = dream_command(&root, &home, &config, &cache)
        .args([
            "dream",
            "--project",
            "alpha",
            "--between",
            "--all",
            "--date",
            "2026-06-25",
        ])
        .output()
        .expect("run dream");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("Dream"));
    assert!(stdout.contains("2026-06-25"));
    assert!(root.join("ψ/writing/dreams/2026-06-25_alpha.md").is_file());
    assert!(root
        .join("ψ/memory/morpheus/2026-06-25_speculations.md")
        .is_file());

    let guarded = dream_command(&root, &home, &config, &cache)
        .args(["dream", "--project", "-bad"])
        .output()
        .expect("guard");
    assert!(!guarded.status.success());
    assert_eq!(
        String::from_utf8(guarded.stderr).expect("stderr"),
        "dream: --project requires a value\n"
    );
    let _ = std::fs::remove_dir_all(root);
}
