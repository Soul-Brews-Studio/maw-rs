use maw_cli::{dispatcher_status, DispatchKind};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("maw-rs-team-resume-{name}-{stamp}"));
    fs::create_dir_all(root.join(".git")).expect("git marker");
    fs::create_dir_all(root.join("home/.claude/teams/phoenix")).expect("team config dir");
    fs::create_dir_all(root.join("maw-home")).expect("maw home");
    fs::create_dir_all(root.join("psi/memory/mailbox/teams/phoenix")).expect("vault team dir");
    fs::create_dir_all(root.join("fakebin")).expect("fakebin");
    root
}

fn seed_team(root: &Path) {
    fs::write(
        root.join("home/.claude/teams/phoenix/config.json"),
        r#"{"name":"phoenix","description":"Phoenix team","createdAt":1,"leadSessionId":"old-session-abcdef","members":[{"name":"team-lead","role":"lead","agentType":"team-lead"},{"name":"builder","model":"gpt-5.5"},{"name":"reviewer"}]}"#,
    ).expect("config");
    fs::write(
        root.join("psi/memory/mailbox/teams/phoenix/manifest.json"),
        r#"{"name":"phoenix","createdAt":1,"members":["builder","reviewer"],"description":"Phoenix team"}"#,
    ).expect("manifest");
    fs::write(
        root.join("fakebin/maw"),
        "#!/usr/bin/env bash\necho DELEGATED-MAW\n",
    )
    .expect("fake maw");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(root.join("fakebin/maw"), fs::Permissions::from_mode(0o755))
            .expect("chmod fake maw");
    }
}

fn normalize(root: &Path, bytes: Vec<u8>) -> String {
    let mut candidates = vec![root.display().to_string()];
    if let Ok(canonical) = root.canonicalize() {
        candidates.push(canonical.display().to_string());
    }
    if let Some(path) = root.to_str() {
        if path.starts_with("/var/") {
            candidates.push(format!("/private{path}"));
        } else if let Some(stripped) = path.strip_prefix("/private") {
            candidates.push(stripped.to_owned());
        }
    }
    candidates.sort_by_key(|path| std::cmp::Reverse(path.len()));
    candidates.dedup();
    let mut output = String::from_utf8(bytes).expect("utf8");
    for candidate in candidates {
        output = output.replace(&candidate, "<ROOT>");
    }
    output.replace(&bin().display().to_string(), "<BIN>")
}

#[test]
fn team_resume_golden_parity_and_fake_maw_no_delegate() {
    let root = temp_dir("golden");
    seed_team(&root);
    let path = format!(
        "{}:{}",
        root.join("fakebin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(bin())
        .args(["team", "resume", "phoenix", "--model", "gpt-5.5"])
        .current_dir(&root)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_RS_TEAM_PSI", root.join("psi"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("CODEX_THREAD_ID", "new-session-123456")
        .env("PATH", path)
        .output()
        .expect("run maw-rs");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = normalize(&root, output.stdout);
    assert_eq!(
        stdout,
        include_str!("fixtures/native-team-resume/team-resume.stdout")
    );
    assert!(!stdout.contains("DELEGATED-MAW"), "fake maw was delegated");
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert_eq!(dispatcher_status("team"), DispatchKind::Native);

    let config =
        fs::read_to_string(root.join("home/.claude/teams/phoenix/config.json")).expect("config");
    assert!(config.contains("\"leadSessionId\": \"new-session-123456\""));
    assert!(fs::read_to_string(
        root.join("psi/memory/mailbox/teams/phoenix/builder-spawn-prompt.md")
    )
    .expect("builder prompt")
    .contains("You are 'builder' on team 'phoenix'."));
    assert!(fs::read_to_string(
        root.join("psi/memory/mailbox/teams/phoenix/reviewer-spawn-prompt.md")
    )
    .expect("reviewer prompt")
    .contains("You are 'reviewer' on team 'phoenix'."));
}

#[test]
fn team_resume_input_guards_before_state_mutation() {
    let root = temp_dir("guards");
    seed_team(&root);
    let before =
        fs::read_to_string(root.join("home/.claude/teams/phoenix/config.json")).expect("before");
    let output = Command::new(bin())
        .args(["team", "resume", "-bad"])
        .current_dir(&root)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_RS_TEAM_PSI", root.join("psi"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("CODEX_THREAD_ID", "new-session-123456")
        .output()
        .expect("run maw-rs");
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .expect("stderr")
        .contains("leading dash rejected"));
    let after =
        fs::read_to_string(root.join("home/.claude/teams/phoenix/config.json")).expect("after");
    assert_eq!(before, after);
}
