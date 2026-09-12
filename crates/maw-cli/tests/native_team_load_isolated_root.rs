#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
                                                    // #826: `team create` writes its vault manifest into the REAL repo's `ψ/` even
                                                    // when the caller isolated everything else — MAW_HOME, XDG_CONFIG_HOME,
                                                    // MAW_STATE_DIR all pointed at a fresh temp dir, and the vault write still
                                                    // landed beside the actual source tree. Reproduced and reported by
                                                    // maw-rs@white on white.local.
                                                    //
                                                    // This test deliberately does NOT set `MAW_RS_TEAM_PSI` (the private,
                                                    // undocumented override that predates this fix) -- the whole point is that
                                                    // isolating via the SAME variables every other test in this crate uses
                                                    // (MAW_HOME / MAW_STATE_DIR) must isolate the vault too, without the caller
                                                    // needing to know about a second, team-specific variable.
use std::{
    fs,
    path::PathBuf,
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
    let path = std::env::temp_dir().join(format!("maw-rs-team-826-load-{stamp}-{name}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

#[test]
fn team_create_honours_maw_state_dir_instead_of_the_real_repo_vault() {
    let root = temp_dir("isolated-root");
    let home = root.join("home");
    let maw_home = root.join("maw-home");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&maw_home).expect("maw home");

    // The repo's REAL ψ, as it exists on disk right now -- this must not gain
    // a new file as a side effect of running this test.
    let repo_root = std::env::var("CARGO_MANIFEST_DIR").map_or_else(
        |_| PathBuf::from("../.."),
        |dir| PathBuf::from(dir).join("../.."),
    );
    let real_vault_teams = repo_root.join("ψ/memory/mailbox/teams/team-826-isolation-canary");
    let _ = fs::remove_dir_all(&real_vault_teams); // pre-clean in case a prior failed run left it

    let output = Command::new(bin())
        .args([
            "team",
            "create",
            "team-826-isolation-canary",
            "--description",
            "isolation probe",
        ])
        .env("HOME", &home)
        .env("MAW_HOME", &maw_home)
        .env("MAW_STATE_DIR", &maw_home)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .current_dir(&repo_root)
        .output()
        .expect("run maw-rs team create");

    assert!(
        output.status.success(),
        "team create failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        !real_vault_teams.exists(),
        "isolated team create leaked into the real repo vault at {}",
        real_vault_teams.display()
    );

    // And it DID write somewhere -- under the isolated root, not nowhere.
    let isolated_manifest =
        maw_home.join("team-vault/memory/mailbox/teams/team-826-isolation-canary/manifest.json");
    assert!(
        isolated_manifest.exists(),
        "expected the isolated vault manifest at {}",
        isolated_manifest.display()
    );

    let _ = fs::remove_dir_all(&real_vault_teams);
}
