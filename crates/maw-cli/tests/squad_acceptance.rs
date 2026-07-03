//! Acceptance harness for the squad fleet plugin (#72).
//!
//! Proves a maw-rs port of squad preserves the golden behavioral contract —
//! `crates/maw-cli/tests/fixtures/squad-contract/CONTRACT.md`, extracted from
//! `laris-co/athena-oracle/.maw/plugins/squad/impl.ts` @ v2.0.0.
//!
//! The tests drive the REAL user-facing CLI dispatch path (`run_cli(["squad", …])`
//! → `dispatch_cli_plugin`) against the ported plugin at repo-root
//! `fleet-plugins/squad/` (sibling agent's surface, on a parallel branch). Until that
//! directory exists every test SKIPS with a clear message, so this file is green
//! standalone and lights up automatically when the port merges.
//!
//! Isolation: squad writes under `~/.claude/teams/` (`~` = `os.homedir()` = `HOME` on
//! POSIX) and derives the team name from the *lead repo* — the directory the user runs
//! `maw squad` from, `basename` minus `-oracle`. The bun-dev runtime forces the plugin's
//! cwd to the plugin dir, so the port recovers the lead repo from the invoking shell's
//! `PWD` env var (not cwd). The harness therefore points `HOME` at a tempdir and `PWD`
//! at a git-init'd lead repo it names `athena-oracle` (→ team `athena`, exercising the
//! `-oracle` strip). Everything lands inside that tempdir; nothing touches the real
//! `~/.claude`.
//!
//! Tiers, detected from the port's `plugin.json`: the bun-dev tier is a TS entry with
//! `"runtime":"bun-dev"` and needs `bun` on PATH (gated — skips if absent); the ship
//! tier is a WASM artifact that runs on the Extism runtime with no toolchain. A port
//! that declares neither is an incomplete port and the tests skip loudly.
//!
//! Guards are asserted tier-agnostically as: process exits non-zero (loud) AND zero
//! bytes are written under the teams root. The single live-tmux path (join's
//! "one oracle, one session" pre-check) is `#[ignore]`-gated.

use maw_cli::run_cli;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{create_dir_all, read_to_string, write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// Repo-root `fleet-plugins/squad` — the sibling agent's ported plugin. `None` until
/// that branch merges, which is the harness's skip signal.
fn squad_source_dir() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fleet-plugins/squad")
        .canonicalize()
        .ok()?;
    dir.join("plugin.json").is_file().then_some(dir)
}

fn bun_available() -> bool {
    std::process::Command::new("bun")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn manifest_declares_wasm_artifact(manifest: &Value) -> bool {
    let ends_wasm = |value: &Value| {
        value
            .as_str()
            .is_some_and(|text| Path::new(text).extension().is_some_and(|ext| ext == "wasm"))
    };
    manifest.get("wasm").is_some()
        || manifest.get("entry").is_some_and(ends_wasm)
        || manifest
            .get("artifact")
            .and_then(|artifact| artifact.get("path"))
            .is_some_and(ends_wasm)
}

fn manifest_opts_into_bun_dev(manifest: &Value) -> bool {
    manifest.get("runtime").and_then(Value::as_str) == Some("bun-dev")
}

/// Why a behavioral test cannot run right now. Printed, then the test returns (green).
enum Skip {
    NoPort,
    BadManifest,
    IncompletePort,
    NoBun,
}

impl Skip {
    fn announce(&self, test: &str) {
        let why = match self {
            Skip::NoPort => "fleet-plugins/squad not present — sq-port's port not merged yet",
            Skip::BadManifest => "fleet-plugins/squad/plugin.json is unreadable/not JSON",
            Skip::IncompletePort => {
                "fleet-plugins/squad declares neither a WASM artifact nor runtime=bun-dev"
            }
            Skip::NoBun => "bun-dev tier port but `bun` is not on PATH",
        };
        eprintln!("SKIP {test}: {why}");
    }
}

fn temp_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-squad-acceptance-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("temp root");
    dir
}

fn copy_tree(src: &Path, dst: &Path) {
    create_dir_all(dst).expect("dst dir");
    for entry in std::fs::read_dir(src).expect("read_dir") {
        let entry = entry.expect("entry");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            std::fs::copy(&from, &to).expect("copy file");
        }
    }
}

fn walk(base: &Path, dir: &Path, out: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(base, &path, out);
        } else if let Ok(rel) = path.strip_prefix(base) {
            out.insert(rel.to_path_buf());
        }
    }
}

/// Relative paths of every file under `dir`, for zero-writes assertions.
fn snapshot(dir: &Path) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    walk(dir, dir, &mut out);
    out
}

struct EnvRestore {
    home: Option<OsString>,
    maw_home: Option<OsString>,
    maw_plugins_dir: Option<OsString>,
    pwd: Option<OsString>,
    cwd: Option<PathBuf>,
}

impl EnvRestore {
    fn capture() -> Self {
        Self {
            home: std::env::var_os("HOME"),
            maw_home: std::env::var_os("MAW_HOME"),
            maw_plugins_dir: std::env::var_os("MAW_PLUGINS_DIR"),
            pwd: std::env::var_os("PWD"),
            cwd: std::env::current_dir().ok(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        restore("HOME", self.home.take());
        restore("MAW_HOME", self.maw_home.take());
        restore("MAW_PLUGINS_DIR", self.maw_plugins_dir.take());
        restore("PWD", self.pwd.take());
        if let Some(cwd) = self.cwd.take() {
            std::env::set_current_dir(cwd).ok();
        }
    }
}

fn restore(key: &str, value: Option<OsString>) {
    match value {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

/// A staged, isolated squad plugin ready to drive via `run_cli`. Holds the env lock and
/// env restore for its whole lifetime and cleans the tempdir on drop.
struct Harness {
    root: PathBuf,
    team: String,
    team_dir: PathBuf,
    teams_root: PathBuf,
    // Dropped after `root` cleanup, in declaration order: restore env, then release lock.
    _restore: EnvRestore,
    _guard: MutexGuard<'static, ()>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

impl Harness {
    /// Locks env, gates on the port's presence/tier, stages it into an isolated
    /// tempdir, and points `HOME` + `MAW_PLUGINS_DIR` + `PWD` + the process cwd at
    /// it. Returns `None` (after printing a skip reason) when a precondition is
    /// missing, so callers early-return green.
    fn try_new(label: &str) -> Option<Self> {
        // Poison-tolerant: a panic in one test must not cascade PoisonError into the
        // rest — the serialized section only mutates env/cwd, which EnvRestore repairs.
        let guard = env_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let Some(source) = squad_source_dir() else {
            Skip::NoPort.announce(label);
            return None;
        };
        let Ok(manifest_text) = read_to_string(source.join("plugin.json")) else {
            Skip::BadManifest.announce(label);
            return None;
        };
        let Ok(manifest) = serde_json::from_str::<Value>(&manifest_text) else {
            Skip::BadManifest.announce(label);
            return None;
        };
        let is_wasm = manifest_declares_wasm_artifact(&manifest);
        let is_bun_dev = manifest_opts_into_bun_dev(&manifest);
        if !is_wasm && !is_bun_dev {
            Skip::IncompletePort.announce(label);
            return None;
        }
        if !is_wasm && is_bun_dev && !bun_available() {
            Skip::NoBun.announce(label);
            return None;
        }

        let root = temp_root(label);
        let home = root.join("home");
        let plugins_dir = root.join("plugins");
        copy_tree(&source, &plugins_dir.join("squad"));
        // The lead repo: the directory the user "runs maw squad from". The bun-dev
        // runtime forces the plugin's cwd to the plugin dir, so the port recovers the
        // lead repo from the invoking shell's PWD (env), not cwd — the harness must set
        // PWD to this dir. Its basename minus `-oracle` is the team, so `athena-oracle`
        // pins team=athena and exercises the `-oracle` strip. git-init'd so
        // `git -C <lead> rev-parse --show-toplevel` resolves here deterministically.
        let lead_repo = root.join("lead").join("athena-oracle");
        create_dir_all(&lead_repo).expect("lead repo dir");
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&lead_repo)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok();
        create_dir_all(&home).expect("home dir");

        let restore = EnvRestore::capture();
        std::env::set_var("HOME", &home);
        std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);
        std::env::set_var("PWD", &lead_repo);
        std::env::remove_var("MAW_HOME");
        // The wasm tier gets its cwd from the process (InvokeContext), not $PWD —
        // the harness must actually chdir, or the team derives from the cargo test
        // cwd (`crates/maw-cli` → team "maw-cli"). Serialized by the env lock;
        // EnvRestore puts it back.
        std::env::set_current_dir(&lead_repo).expect("chdir into lead repo");

        let team = "athena".to_owned();
        let teams_root = home.join(".claude").join("teams");
        let team_dir = teams_root.join(&team);
        Some(Self {
            root,
            team,
            team_dir,
            teams_root,
            _restore: restore,
            _guard: guard,
        })
    }

    /// Pre-seed a started team with the given `(name, color)` roster and an empty
    /// `inboxes/` dir — simulates a prior `start`/`join` for `say`/`ls`/guard tests.
    fn seed_team(&self, members: &[(&str, &str)]) {
        create_dir_all(self.team_dir.join("inboxes")).expect("inboxes dir");
        let roster: Vec<Value> = members
            .iter()
            .map(|(name, color)| {
                json!({
                    "agentId": format!("{name}@{}", self.team),
                    "name": name,
                    "color": color,
                    "repo": "/seed/repo",
                    "joinedAt": 1_720_000_000_000_i64,
                })
            })
            .collect();
        let config = json!({
            "name": self.team,
            "members": roster,
            "createdAt": 1_720_000_000_000_i64,
            "leadSessionId": "seed-session",
            "leadRepo": "/seed/lead-repo",
        });
        write(
            self.team_dir.join("config.json"),
            serde_json::to_string_pretty(&config).expect("config json") + "\n",
        )
        .expect("write seed config");
    }

    fn config(&self) -> Value {
        let text = read_to_string(self.team_dir.join("config.json")).expect("read config.json");
        serde_json::from_str(&text).expect("config.json is JSON")
    }

    fn inbox(&self, member: &str) -> Value {
        let text = read_to_string(self.team_dir.join("inboxes").join(format!("{member}.json")))
            .expect("read inbox");
        serde_json::from_str(&text).expect("inbox is JSON")
    }
}

/// Drive the real CLI dispatch path for `maw squad <argv…>`.
fn run_squad(argv: &[&str]) -> maw_cli::CliOutput {
    let mut full = vec!["squad"];
    full.extend_from_slice(argv);
    run_cli(&args(&full))
}

// ---------------------------------------------------------------------------
// start
// ---------------------------------------------------------------------------

#[test]
fn start_creates_team_structure() {
    let Some(h) = Harness::try_new("start-create") else {
        return;
    };

    let out = run_squad(&["start"]);
    assert_eq!(out.code, 0, "start failed: {}\n{}", out.stderr, out.stdout);

    let config = h.config();
    assert_eq!(
        config["name"],
        json!(h.team),
        "team name in config: {config}"
    );
    assert_eq!(
        config["members"],
        json!([]),
        "fresh team has no members: {config}"
    );
    assert!(
        config["leadSessionId"]
            .as_str()
            .is_some_and(|id| !id.is_empty()),
        "leadSessionId must be non-empty after start: {config}"
    );
    assert!(
        config["leadRepo"]
            .as_str()
            .is_some_and(|repo| repo.ends_with("athena-oracle")),
        "leadRepo points at the lead repo: {config}"
    );

    let lead_inbox = h.team_dir.join("inboxes").join("team-lead.json");
    assert_eq!(
        read_to_string(&lead_inbox).expect("team-lead.json"),
        "[]\n",
        "member->lead inbox seeded empty"
    );
}

#[test]
fn start_adopts_existing_without_clobber() {
    let Some(h) = Harness::try_new("start-adopt") else {
        return;
    };
    // A team that already has a member and a createdAt, as if joined earlier.
    h.seed_team(&[("digger", "cyan")]);
    let created_at = h.config()["createdAt"].clone();

    let out = run_squad(&["start"]);
    assert_eq!(out.code, 0, "start (adopt) failed: {}", out.stderr);

    let config = h.config();
    let members = config["members"].as_array().expect("members array");
    assert_eq!(
        members.len(),
        1,
        "existing member must survive start: {config}"
    );
    assert_eq!(
        members[0]["name"],
        json!("digger"),
        "roster preserved: {config}"
    );
    assert_eq!(
        config["createdAt"], created_at,
        "createdAt must not be reset: {config}"
    );
    assert!(
        config["leadRepo"]
            .as_str()
            .is_some_and(|repo| repo.ends_with("athena-oracle")),
        "leadRepo refreshed on adopt: {config}"
    );
}

// ---------------------------------------------------------------------------
// say
// ---------------------------------------------------------------------------

#[test]
fn say_appends_and_never_clobbers() {
    let Some(h) = Harness::try_new("say-append") else {
        return;
    };
    h.seed_team(&[("digger", "cyan")]);

    let first = run_squad(&["say", "digger", "hello", "one"]);
    assert_eq!(first.code, 0, "first say failed: {}", first.stderr);
    let second = run_squad(&["say", "digger", "hello", "two"]);
    assert_eq!(second.code, 0, "second say failed: {}", second.stderr);

    let inbox = h.inbox("digger");
    let msgs = inbox.as_array().expect("inbox is an array");
    assert_eq!(
        msgs.len(),
        2,
        "two says must append, never clobber: {inbox}"
    );
    assert_eq!(
        msgs[0]["text"],
        json!("hello one"),
        "first message text: {inbox}"
    );
    assert_eq!(
        msgs[1]["text"],
        json!("hello two"),
        "second message text: {inbox}"
    );
    for msg in msgs {
        assert_eq!(msg["from"], json!("team-lead"), "message from lead: {msg}");
        assert_eq!(msg["type"], json!("message"), "message type: {msg}");
        assert_eq!(msg["read"], json!(false), "message starts unread: {msg}");
        assert!(
            msg["timestamp"].as_str().is_some_and(|ts| !ts.is_empty()),
            "message carries a timestamp: {msg}"
        );
    }
}

#[test]
fn say_to_non_member_is_loud_and_writes_nothing() {
    let Some(h) = Harness::try_new("say-non-member") else {
        return;
    };
    h.seed_team(&[]); // started, empty roster
    let before = snapshot(&h.teams_root);

    let out = run_squad(&["say", "ghost", "hi"]);

    assert_ne!(
        out.code, 0,
        "non-member say must fail loudly: {}",
        out.stdout
    );
    assert!(
        !h.team_dir.join("inboxes").join("ghost.json").exists(),
        "non-member say must not create an inbox"
    );
    assert_eq!(
        before,
        snapshot(&h.teams_root),
        "non-member say must write nothing"
    );
}

#[test]
fn say_rejects_path_traversal_member() {
    let Some(h) = Harness::try_new("say-traversal") else {
        return;
    };
    h.seed_team(&[]);
    let before = snapshot(&h.teams_root);

    let out = run_squad(&["say", "../evil", "hi"]);

    assert_ne!(
        out.code, 0,
        "traversal member name must be rejected: {}",
        out.stdout
    );
    // The would-be traversal target ~/.claude/teams/athena/inboxes/../evil.json
    // resolves to ~/.claude/teams/athena/evil.json — assert it (and everything) is
    // untouched: NAME_RE rejects before any path is built.
    assert!(
        !h.team_dir.join("evil.json").exists(),
        "no traversal write escaped"
    );
    assert_eq!(
        before,
        snapshot(&h.teams_root),
        "rejected say must write nothing"
    );
}

// ---------------------------------------------------------------------------
// ls
// ---------------------------------------------------------------------------

#[test]
fn ls_reflects_roster_and_unread() {
    let Some(h) = Harness::try_new("ls-roster") else {
        return;
    };
    h.seed_team(&[("digger", "cyan"), ("scout", "blue")]);
    // one unread message in digger's inbox
    write(
        h.team_dir.join("inboxes").join("digger.json"),
        serde_json::to_string_pretty(&json!([{
            "from": "team-lead", "text": "hi", "timestamp": "2026-07-03T00:00:00.000Z",
            "color": "cyan", "type": "message", "read": false
        }]))
        .expect("inbox json")
            + "\n",
    )
    .expect("seed inbox");

    let out = run_squad(&["ls"]);
    assert_eq!(out.code, 0, "ls failed: {}", out.stderr);
    assert!(
        out.stdout.contains("squad: athena"),
        "ls names the team: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("member: digger (cyan)"),
        "ls lists digger with color: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("member: scout (blue)"),
        "ls lists scout with color: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("digger (1 unread)"),
        "ls reports the unread count: {}",
        out.stdout
    );
}

// ---------------------------------------------------------------------------
// join guards (spawn path stays on mawjs; only the pre-spawn guards are asserted)
// ---------------------------------------------------------------------------

#[test]
fn join_rejects_invalid_color() {
    let Some(h) = Harness::try_new("join-color") else {
        return;
    };
    h.seed_team(&[]);
    let before = snapshot(&h.teams_root);

    // `orange` is invalid — the guard throws before any tmux/mawjs spawn.
    let out = run_squad(&["join", "digger", "orange"]);

    assert_ne!(
        out.code, 0,
        "invalid color must be rejected: {}",
        out.stdout
    );
    assert_eq!(
        h.config()["members"],
        json!([]),
        "rejected join must not add a member"
    );
    assert_eq!(
        before,
        snapshot(&h.teams_root),
        "rejected join must write nothing"
    );
}

#[test]
fn join_rejects_path_traversal_name() {
    let Some(h) = Harness::try_new("join-traversal") else {
        return;
    };
    h.seed_team(&[]);
    let before = snapshot(&h.teams_root);

    let out = run_squad(&["join", "../evil", "cyan"]);

    assert_ne!(
        out.code, 0,
        "traversal oracle name must be rejected: {}",
        out.stdout
    );
    assert_eq!(
        before,
        snapshot(&h.teams_root),
        "rejected join must write nothing"
    );
}

// ---------------------------------------------------------------------------
// session pre-check — the one live-tmux path
// ---------------------------------------------------------------------------

#[test]
#[ignore = "needs a live tmux server: exercises join's one-oracle-one-session pre-check"]
fn join_refuses_when_session_already_live() {
    let Some(h) = Harness::try_new("join-precheck") else {
        return;
    };
    // Guard against being run (via --ignored) without tmux available.
    if std::process::Command::new("tmux")
        .arg("-V")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_err()
    {
        eprintln!("SKIP join_refuses_when_session_already_live: tmux not installed");
        return;
    }
    h.seed_team(&[]);

    let session = format!("sqacc-{}", std::process::id());
    std::process::Command::new("tmux")
        .args(["new-session", "-d", "-s", &session])
        .status()
        .expect("spawn tmux session");

    // A member named after the live session must be refused before any spawn.
    let out = run_squad(&["join", &session, "cyan"]);

    std::process::Command::new("tmux")
        .args(["kill-session", "-t", &session])
        .status()
        .ok();

    assert_ne!(
        out.code, 0,
        "join must refuse when a same-named session is already live: {}",
        out.stdout
    );
    assert_eq!(
        h.config()["members"],
        json!([]),
        "refused join must not add a member"
    );
}
