use std::fs::create_dir_all;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{
    hash_file, invoke_plugin, load_manifest_from_dir, ExtismWasmInvokeRuntime, InvokeContext,
    InvokeResult, InvokeSource, LoadedPlugin, LoadedPluginKind, MawWasmHost, PluginManifest,
};
use serde_json::Value;

static ENV_LOCK: Mutex<()> = Mutex::new(());

const PROFILE_CURRENT_TRANSCRIPT: &[ExpectedHostCall] = &[ExpectedHostCall::new(
    "maw.fs.read",
    "fs:read:config",
    "/config/profile-active",
)];
const PROFILE_LIST_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.fs.read", "fs:read:config", "/config/profile-active"),
    ExpectedHostCall::new("maw.fs.read", "fs:read:config", "/config/profiles/all.json"),
    ExpectedHostCall::new(
        "maw.fs.read",
        "fs:read:config",
        "/config/profiles/minimal.json",
    ),
];
const PROFILE_SHOW_MINIMAL_TRANSCRIPT: &[ExpectedHostCall] = &[ExpectedHostCall::new(
    "maw.fs.read",
    "fs:read:config",
    "/config/profiles/minimal.json",
)];
const PROFILE_USE_ALL_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.fs.read", "fs:read:config", "/config/profiles/all.json"),
    ExpectedHostCall::new("maw.fs.write", "fs:write:config", "/config/profile-active"),
];

const CONFIG_SET_NODE_TRANSCRIPT: &[ExpectedHostCall] = &[ExpectedHostCall::new(
    "maw.config.set",
    "sdk:config:write",
    "config:node",
)];
const CONFIG_SET_PORT_JSON_TRANSCRIPT: &[ExpectedHostCall] = &[ExpectedHostCall::new(
    "maw.config.set",
    "sdk:config:write",
    "config:port",
)];
const CONSENT_LIST_TRANSCRIPT: &[ExpectedHostCall] = &[ExpectedHostCall::new(
    "maw.consent.read",
    "sdk:consent:read",
    "consent:pending",
)];
const CONSENT_LIST_TRUST_TRANSCRIPT: &[ExpectedHostCall] = &[ExpectedHostCall::new(
    "maw.consent.read",
    "sdk:consent:read",
    "consent:trust",
)];

const PEEK_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.tmux.list_sessions", "tmux:read", "tmux://sessions"),
    ExpectedHostCall::new("maw.tmux.capture", "tmux:read", "wasm-parity-peek:0"),
];
const WORKSPACE_LS_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.fs.list", "fs:read:data", "/data/workspaces"),
    ExpectedHostCall::new("maw.fs.read", "fs:read:data", "/data/workspaces/alpha.json"),
    ExpectedHostCall::new("maw.fs.read", "fs:read:data", "/data/workspaces/beta.json"),
];
const SERVE_PEER_STARTUP_WARNINGS_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.fs.read", "fs:read:config", "/config/maw.config.json"),
    ExpectedHostCall::new("maw.fs.read", "fs:read:state", "/state/peers.json"),
];
const SEND_TRANSCRIPT: &[ExpectedHostCall] = &[ExpectedHostCall::new(
    "maw.tmux.send_keys",
    "tmux:send",
    "mawjs:codex-5.pane",
)];
const CLEANUP_WORKTREES_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.fs.list", "fs:read:data", "/data/worktrees"),
    ExpectedHostCall::new("maw.fs.read", "fs:read:data", "/data/worktrees/clean.json"),
    ExpectedHostCall::new("maw.fs.read", "fs:read:data", "/data/worktrees/ask.json"),
];
const CONTACTS_LIST_TRANSCRIPT: &[ExpectedHostCall] = &[ExpectedHostCall::new(
    "maw.fs.read",
    "fs:read:data",
    "/data/contacts.json",
)];
const SIGNALS_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.fs.list", "fs:read:data", "/data/ψ/memory/signals"),
    ExpectedHostCall::new(
        "maw.fs.read",
        "fs:read:data",
        "/data/ψ/memory/signals/alpha.json",
    ),
    ExpectedHostCall::new(
        "maw.fs.read",
        "fs:read:data",
        "/data/ψ/memory/signals/beta.json",
    ),
    ExpectedHostCall::new(
        "maw.fs.read",
        "fs:read:data",
        "/data/ψ/memory/signals/old.json",
    ),
];
const COSTS_SUMMARY_TRANSCRIPT: &[ExpectedHostCall] = &[ExpectedHostCall::new(
    "maw.localserver.request",
    "sdk:localserver",
    "localserver:/api/costs",
)];
const COSTS_DAILY_TRANSCRIPT: &[ExpectedHostCall] = &[ExpectedHostCall::new(
    "maw.localserver.request",
    "sdk:localserver",
    "localserver:/api/costs/daily",
)];
const ABOUT_ATHENA_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.fs.read", "fs:read:data", "/data/oracles/athena.json"),
    ExpectedHostCall::new("maw.tmux.list_sessions", "tmux:read", "tmux://sessions"),
    ExpectedHostCall::new("maw.fs.list", "fs:read:config", "/config/fleet"),
    ExpectedHostCall::new("maw.fs.read", "fs:read:config", "/config/fleet/alpha.json"),
    ExpectedHostCall::new("maw.tmux.capture", "tmux:read", "athena:0"),
    ExpectedHostCall::new("maw.tmux.capture", "tmux:read", "athena:1"),
    ExpectedHostCall::new(
        "maw.fs.list",
        "fs:read:data",
        "/data/about/athena/worktrees",
    ),
    ExpectedHostCall::new(
        "maw.fs.read",
        "fs:read:data",
        "/data/about/athena/worktrees/main.json",
    ),
    ExpectedHostCall::new(
        "maw.fs.read",
        "fs:read:data",
        "/data/about/athena/worktrees/agent.json",
    ),
];
const ACTIVITY_ATHENA_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.tmux.capture", "tmux:read", "athena:0"),
    ExpectedHostCall::new("maw.tmux.capture", "tmux:read", "athena:0"),
    ExpectedHostCall::new("maw.tmux.capture", "tmux:read", "athena:0"),
];
const DISCOVER_CONFIG_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.config.get", "sdk:config:read", "config"),
    ExpectedHostCall::new("maw.fs.list", "fs:read:config", "/config/fleet"),
    ExpectedHostCall::new(
        "maw.fs.read",
        "fs:read:config",
        "/config/fleet/50-mawjs.json",
    ),
];
const DISCOVER_CONFIG_JSON_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.config.get", "sdk:config:read", "config"),
    ExpectedHostCall::new("maw.fs.list", "fs:read:config", "/config/fleet"),
    ExpectedHostCall::new(
        "maw.fs.read",
        "fs:read:config",
        "/config/fleet/50-mawjs.json",
    ),
    ExpectedHostCall::new("maw.tmux.list_sessions", "tmux:read", "tmux://sessions"),
];
const DISCOVER_AWAKE_TRANSCRIPT: &[ExpectedHostCall] = &[ExpectedHostCall::new(
    "maw.tmux.list_sessions",
    "tmux:read",
    "tmux://sessions",
)];

const FEDERATION_STATUS_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.config.get", "sdk:config:read", "config"),
    ExpectedHostCall::new(
        "maw.http.request",
        "net:https:alpha.example.test",
        "alpha.example.test",
    ),
    ExpectedHostCall::new(
        "maw.http.request",
        "net:https:alpha.example.test",
        "alpha.example.test",
    ),
    ExpectedHostCall::new(
        "maw.http.request",
        "net:https:beta.example.test",
        "beta.example.test",
    ),
    ExpectedHostCall::new(
        "maw.http.request",
        "net:https:beta.example.test",
        "beta.example.test",
    ),
];
const FEDERATION_SYNC_JSON_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.config.get", "sdk:config:read", "config"),
    ExpectedHostCall::new(
        "maw.http.request",
        "net:https:alpha.example.test",
        "alpha.example.test",
    ),
    ExpectedHostCall::new(
        "maw.http.request",
        "net:https:beta.example.test",
        "beta.example.test",
    ),
];
const PARK_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.tmux.list_sessions", "tmux:read", "tmux://sessions"),
    ExpectedHostCall::new("maw.tmux.capture", "tmux:read", "codex-1"),
    ExpectedHostCall::new("maw.exec.run", "proc:exec:git", "git"),
    ExpectedHostCall::new("maw.exec.run", "proc:exec:git", "git"),
    ExpectedHostCall::new("maw.exec.run", "proc:exec:git", "git"),
    ExpectedHostCall::new(
        "maw.fs.write",
        "fs:write:state",
        "/state/parked/codex-1.json",
    ),
];
const PARK_LS_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.fs.list", "fs:read:state", "/state/parked"),
    ExpectedHostCall::new("maw.fs.read", "fs:read:state", "/state/parked/codex-1.json"),
];

const PING_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.config.get", "sdk:config:read", "config"),
    ExpectedHostCall::new(
        "maw.http.request",
        "net:https:alpha.example.test",
        "alpha.example.test",
    ),
    ExpectedHostCall::new(
        "maw.http.request",
        "net:https:beta.example.test",
        "beta.example.test",
    ),
];
const PING_ALPHA_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.config.get", "sdk:config:read", "config"),
    ExpectedHostCall::new(
        "maw.http.request",
        "net:https:alpha.example.test",
        "alpha.example.test",
    ),
];

const CHECK_TOOLS_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.exec.run", "proc:exec:bun", "bun"),
    ExpectedHostCall::new("maw.exec.run", "proc:exec:gh", "gh"),
    ExpectedHostCall::new("maw.exec.run", "proc:exec:ghq", "ghq"),
    ExpectedHostCall::new("maw.exec.run", "proc:exec:git", "git"),
    ExpectedHostCall::new("maw.exec.run", "proc:exec:tmux", "tmux"),
    ExpectedHostCall::new("maw.exec.run", "proc:exec:uv", "uv"),
    ExpectedHostCall::new("maw.exec.run", "proc:exec:uvx", "uvx"),
];

const CLEANUP_WORKTREES_YES_TRANSCRIPT: &[ExpectedHostCall] = &[
    ExpectedHostCall::new("maw.fs.list", "fs:read:data", "/data/worktrees"),
    ExpectedHostCall::new("maw.fs.read", "fs:read:data", "/data/worktrees/clean.json"),
    ExpectedHostCall::new("maw.fs.read", "fs:read:data", "/data/worktrees/ask.json"),
    ExpectedHostCall::new(
        "maw.fs.remove",
        "fs:write:data",
        "/data/repos/acme/app/agents/cleanup-clean",
    ),
];

#[derive(Clone, Copy)]
struct ExpectedHostCall<'a> {
    host_fn: &'a str,
    capability: &'a str,
    resource: &'a str,
}

impl<'a> ExpectedHostCall<'a> {
    const fn new(host_fn: &'a str, capability: &'a str, resource: &'a str) -> Self {
        Self {
            host_fn,
            capability,
            resource,
        }
    }
}

#[derive(Clone, Copy)]
struct ParityCase<'a> {
    plugin: &'a str,
    manifest_name: &'a str,
    args: &'a [&'a str],
    expected_host_calls: Option<usize>,
    expected_host_transcript: Option<&'a [ExpectedHostCall<'a>]>,
}

#[test]
fn golden_parity_trivial_committed_golden_and_wasm_outputs_match_in_isolated_maw_home() {
    run_parity_case(ParityCase {
        plugin: "trivial",
        manifest_name: "trivial-parity",
        args: &["alpha", "beta"],
        expected_host_calls: None,
        expected_host_transcript: None,
    });
}

#[test]
fn golden_parity_shellenv_committed_golden_and_wasm_outputs_match_in_isolated_maw_home() {
    for args in [&["zsh"][..], &["bash"][..], &["fish"][..], &[][..]] {
        run_parity_case(ParityCase {
            plugin: "shellenv",
            manifest_name: "shellenv-parity",
            args,
            expected_host_calls: Some(0),
            expected_host_transcript: None,
        });
    }
}

#[test]
fn golden_parity_learn_committed_golden_and_wasm_outputs_match_in_isolated_maw_home() {
    for args in [
        &["Soul-Brews-Studio/maw-js"][..],
        &["Soul-Brews-Studio/maw-js", "--fast"][..],
        &["Soul-Brews-Studio/maw-js", "--deep"][..],
        &["repo", "--fast", "--deep"][..],
        &["repo", "--turbo"][..],
        &[][..],
    ] {
        run_parity_case(ParityCase {
            plugin: "learn",
            manifest_name: "learn-parity",
            args,
            expected_host_calls: Some(0),
            expected_host_transcript: None,
        });
    }
}

#[test]
fn golden_parity_cross_team_queue_committed_golden_and_wasm_outputs_match_in_isolated_maw_home() {
    run_parity_case(ParityCase {
        plugin: "cross-team-queue",
        manifest_name: "cross-team-queue-parity",
        args: &[],
        expected_host_calls: Some(0),
        expected_host_transcript: None,
    });
}

#[test]
fn golden_parity_project_committed_golden_and_wasm_outputs_match_in_isolated_maw_home() {
    for args in [
        &[][..],
        &["learn", "https://github.com/Soul-Brews-Studio/maw-js"][..],
        &["incubate", "https://github.com/Soul-Brews-Studio/maw-rs"][..],
        &["find", "oracle"][..],
        &["search", "ψ"][..],
        &["list"][..],
        &["bogus"][..],
        &["learn"][..],
    ] {
        run_parity_case(ParityCase {
            plugin: "project",
            manifest_name: "project-parity",
            args,
            expected_host_calls: Some(0),
            expected_host_transcript: None,
        });
    }
}

#[test]
fn golden_parity_triggers_committed_golden_and_wasm_outputs_match_in_isolated_maw_home() {
    run_parity_case(ParityCase {
        plugin: "triggers",
        manifest_name: "triggers-parity",
        args: &[],
        expected_host_calls: Some(0),
        expected_host_transcript: None,
    });
}

#[test]
fn golden_parity_profile_committed_golden_and_wasm_outputs_match_seeded_host() {
    for (args, expected_host_transcript) in [
        (&["current"][..], PROFILE_CURRENT_TRANSCRIPT),
        (&["list"][..], PROFILE_LIST_TRANSCRIPT),
        (&["show", "minimal"][..], PROFILE_SHOW_MINIMAL_TRANSCRIPT),
        (&["use", "all"][..], PROFILE_USE_ALL_TRANSCRIPT),
    ] {
        run_parity_case(ParityCase {
            plugin: "profile",
            manifest_name: "profile-parity",
            args,
            expected_host_calls: Some(expected_host_transcript.len()),
            expected_host_transcript: Some(expected_host_transcript),
        });
    }
}

#[test]
fn golden_parity_config_set_committed_golden_and_wasm_outputs_match_seeded_host() {
    for (args, expected_host_transcript) in [
        (
            &["set", "node", "nova-node"][..],
            CONFIG_SET_NODE_TRANSCRIPT,
        ),
        (
            &["set", "port", "4567", "--json"][..],
            CONFIG_SET_PORT_JSON_TRANSCRIPT,
        ),
    ] {
        run_parity_case(ParityCase {
            plugin: "config",
            manifest_name: "config-parity",
            args,
            expected_host_calls: Some(expected_host_transcript.len()),
            expected_host_transcript: Some(expected_host_transcript),
        });
    }
}

#[test]
fn golden_parity_consent_read_only_committed_golden_and_wasm_outputs_match_seeded_host() {
    for (args, expected_host_transcript) in [
        (&[][..], CONSENT_LIST_TRANSCRIPT),
        (&["list"][..], CONSENT_LIST_TRANSCRIPT),
        (&["list-trust"][..], CONSENT_LIST_TRUST_TRANSCRIPT),
    ] {
        run_parity_case(ParityCase {
            plugin: "consent",
            manifest_name: "consent-parity",
            args,
            expected_host_calls: Some(expected_host_transcript.len()),
            expected_host_transcript: Some(expected_host_transcript),
        });
    }
}

#[test]
fn golden_parity_contacts_committed_golden_and_wasm_outputs_match_seeded_host() {
    for args in [&[][..], &["ls"][..]] {
        run_parity_case(ParityCase {
            plugin: "contacts",
            manifest_name: "contacts-parity",
            args,
            expected_host_calls: Some(CONTACTS_LIST_TRANSCRIPT.len()),
            expected_host_transcript: Some(CONTACTS_LIST_TRANSCRIPT),
        });
    }
    run_parity_case(ParityCase {
        plugin: "contacts",
        manifest_name: "contacts-parity",
        args: &["rm"],
        expected_host_calls: Some(0),
        expected_host_transcript: None,
    });
}

#[test]
fn golden_parity_signals_committed_golden_and_wasm_outputs_match_seeded_host() {
    for args in [&[][..], &["--days", "3", "--json"][..]] {
        run_parity_case(ParityCase {
            plugin: "signals",
            manifest_name: "signals-parity",
            args,
            expected_host_calls: Some(SIGNALS_TRANSCRIPT.len()),
            expected_host_transcript: Some(SIGNALS_TRANSCRIPT),
        });
    }
}

#[test]
fn golden_parity_costs_committed_golden_and_wasm_outputs_match_seeded_host() {
    for (args, expected_host_transcript) in [
        (&[][..], COSTS_SUMMARY_TRANSCRIPT),
        (&["--daily", "--json"][..], COSTS_DAILY_TRANSCRIPT),
    ] {
        run_parity_case(ParityCase {
            plugin: "costs",
            manifest_name: "costs-parity",
            args,
            expected_host_calls: Some(expected_host_transcript.len()),
            expected_host_transcript: Some(expected_host_transcript),
        });
    }
}

#[test]
fn golden_parity_about_committed_golden_and_wasm_outputs_match_seeded_host() {
    run_parity_case(ParityCase {
        plugin: "about",
        manifest_name: "about-parity",
        args: &["athena"],
        expected_host_calls: Some(ABOUT_ATHENA_TRANSCRIPT.len()),
        expected_host_transcript: Some(ABOUT_ATHENA_TRANSCRIPT),
    });
    run_parity_case(ParityCase {
        plugin: "about",
        manifest_name: "about-parity",
        args: &[],
        expected_host_calls: Some(0),
        expected_host_transcript: None,
    });
}

#[test]
fn golden_parity_activity_committed_golden_and_wasm_outputs_match_seeded_host() {
    for args in [&["athena:0"][..], &["athena:0", "--json"][..]] {
        run_parity_case(ParityCase {
            plugin: "activity",
            manifest_name: "activity-parity",
            args,
            expected_host_calls: Some(ACTIVITY_ATHENA_TRANSCRIPT.len()),
            expected_host_transcript: Some(ACTIVITY_ATHENA_TRANSCRIPT),
        });
    }
    run_parity_case(ParityCase {
        plugin: "activity",
        manifest_name: "activity-parity",
        args: &[],
        expected_host_calls: Some(0),
        expected_host_transcript: None,
    });
}

#[test]
fn golden_parity_discover_committed_golden_and_wasm_outputs_match_seeded_host() {
    for (args, expected_host_transcript) in [
        (&["--peers", "config"][..], DISCOVER_CONFIG_TRANSCRIPT),
        (
            &["--peers", "config", "--json"][..],
            DISCOVER_CONFIG_JSON_TRANSCRIPT,
        ),
        (&["--awake"][..], DISCOVER_AWAKE_TRANSCRIPT),
    ] {
        run_parity_case(ParityCase {
            plugin: "discover",
            manifest_name: "discover-parity",
            args,
            expected_host_calls: Some(expected_host_transcript.len()),
            expected_host_transcript: Some(expected_host_transcript),
        });
    }
    run_parity_case(ParityCase {
        plugin: "discover",
        manifest_name: "discover-parity",
        args: &["--peers", "bogus"],
        expected_host_calls: Some(0),
        expected_host_transcript: None,
    });
}

#[test]
fn config_wasm_denies_secret_like_set_without_host_call() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm-parity/config");
    let wasm_plugin = load_wasm_fixture(&fixture, "config-parity");
    assert_eq!(
        wasm_plugin.manifest.capabilities.as_deref(),
        Some(&["sdk:config:write".to_owned()][..]),
        "config-write must declare only sdk:config:write"
    );
    let host = seeded_host(&fixture, &wasm_plugin);
    let host_audit = host.clone();
    let mut wasm_runtime =
        ExtismWasmInvokeRuntime::default().with_host(wasm_plugin.manifest.name.clone(), host);
    let ctx = InvokeContext {
        source: InvokeSource::Cli,
        args: ["set", "federationToken", "secret-value"]
            .iter()
            .map(|arg| (*arg).to_owned())
            .collect(),
        cwd: None,
        home: None,
    };

    let result = invoke_plugin(&wasm_plugin, &ctx, &mut wasm_runtime);

    assert!(!result.ok, "secret-like config write must fail");
    assert_eq!(
        result.error.as_deref(),
        Some("maw config set: secret-like keys are host-gated and cannot be written from WASM")
    );
    assert_eq!(
        host_audit.audit_json_lines(),
        "",
        "secret write reached host"
    );
}

#[test]
fn golden_parity_peek_committed_golden_and_wasm_outputs_match_seeded_host() {
    run_parity_case(ParityCase {
        plugin: "peek",
        manifest_name: "peek-parity",
        args: &["wasm-parity-peek:0"],
        expected_host_calls: Some(PEEK_TRANSCRIPT.len()),
        expected_host_transcript: Some(PEEK_TRANSCRIPT),
    });
}

#[test]
fn golden_parity_workspace_ls_committed_golden_and_wasm_outputs_match_seeded_host() {
    for args in [&[][..], &["ls"][..], &["list"][..]] {
        run_parity_case(ParityCase {
            plugin: "workspace",
            manifest_name: "workspace-parity",
            args,
            expected_host_calls: Some(WORKSPACE_LS_TRANSCRIPT.len()),
            expected_host_transcript: Some(WORKSPACE_LS_TRANSCRIPT),
        });
    }
}

#[test]
fn golden_parity_serve_peer_startup_warnings_committed_golden_and_wasm_outputs_match_seeded_host() {
    run_parity_case(ParityCase {
        plugin: "serve-peer-startup-warnings",
        manifest_name: "serve-peer-startup-warnings-parity",
        args: &[],
        expected_host_calls: Some(SERVE_PEER_STARTUP_WARNINGS_TRANSCRIPT.len()),
        expected_host_transcript: Some(SERVE_PEER_STARTUP_WARNINGS_TRANSCRIPT),
    });
}

#[test]
fn golden_parity_send_committed_golden_and_wasm_outputs_match_seeded_host() {
    run_parity_case(ParityCase {
        plugin: "send",
        manifest_name: "send-parity",
        args: &["mawjs:codex-5.pane", "hello", "world"],
        expected_host_calls: Some(SEND_TRANSCRIPT.len()),
        expected_host_transcript: Some(SEND_TRANSCRIPT),
    });
}

#[test]
fn golden_parity_cleanup_worktrees_committed_golden_and_wasm_outputs_match_seeded_host() {
    for (args, expected_host_transcript) in [
        (&["--worktrees", "--json"][..], CLEANUP_WORKTREES_TRANSCRIPT),
        (
            &["--worktrees", "--yes", "--json"][..],
            CLEANUP_WORKTREES_YES_TRANSCRIPT,
        ),
    ] {
        run_parity_case(ParityCase {
            plugin: "cleanup",
            manifest_name: "cleanup-parity",
            args,
            expected_host_calls: Some(expected_host_transcript.len()),
            expected_host_transcript: Some(expected_host_transcript),
        });
    }
}

#[test]
fn golden_parity_federation_net_committed_golden_and_wasm_outputs_match_seeded_host() {
    for (args, expected_host_transcript) in [
        (&["status"][..], FEDERATION_STATUS_TRANSCRIPT),
        (&["sync", "--json"][..], FEDERATION_SYNC_JSON_TRANSCRIPT),
    ] {
        run_parity_case(ParityCase {
            plugin: "federation",
            manifest_name: "federation-parity",
            args,
            expected_host_calls: Some(expected_host_transcript.len()),
            expected_host_transcript: Some(expected_host_transcript),
        });
    }
}

#[test]
fn golden_parity_park_git_committed_golden_and_wasm_outputs_match_seeded_host() {
    for (args, expected_host_transcript) in [
        (&["codex-1", "handoff", "note"][..], PARK_TRANSCRIPT),
        (&["ls"][..], PARK_LS_TRANSCRIPT),
    ] {
        run_parity_case(ParityCase {
            plugin: "park",
            manifest_name: "park-parity",
            args,
            expected_host_calls: Some(expected_host_transcript.len()),
            expected_host_transcript: Some(expected_host_transcript),
        });
    }
}

#[test]
fn golden_parity_check_exec_committed_golden_and_wasm_outputs_match_seeded_host() {
    run_parity_case(ParityCase {
        plugin: "check",
        manifest_name: "check-parity",
        args: &["tools"],
        expected_host_calls: Some(CHECK_TOOLS_TRANSCRIPT.len()),
        expected_host_transcript: Some(CHECK_TOOLS_TRANSCRIPT),
    });
}

#[test]
fn golden_parity_ping_net_committed_golden_and_wasm_outputs_match_seeded_host() {
    for (args, expected_host_transcript) in [
        (&[][..], PING_TRANSCRIPT),
        (&["alpha"][..], PING_ALPHA_TRANSCRIPT),
    ] {
        run_parity_case(ParityCase {
            plugin: "ping",
            manifest_name: "ping-parity",
            args,
            expected_host_calls: Some(expected_host_transcript.len()),
            expected_host_transcript: Some(expected_host_transcript),
        });
    }
}

#[test]
fn batch3_wasm_declares_exact_net_exec_git_caps_only() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm-parity");

    let ping = load_wasm_fixture(&root.join("ping"), "ping-parity");
    assert_eq!(
        ping.manifest.capabilities.as_deref(),
        Some(
            &[
                "sdk:config:read".to_owned(),
                "net:https:alpha.example.test".to_owned(),
                "net:https:beta.example.test".to_owned(),
            ][..]
        ),
        "ping must declare exact peer hosts only; no wildcard network cap"
    );
    let federation = load_wasm_fixture(&root.join("federation"), "federation-parity");
    assert_eq!(
        federation.manifest.capabilities.as_deref(),
        Some(&[
            "sdk:config:read".to_owned(),
            "net:https:alpha.example.test".to_owned(),
            "net:https:beta.example.test".to_owned(),
        ][..]),
        "federation must declare exact peer hosts only; DNS-rebind protection remains tracked by #35"
    );
    let park = load_wasm_fixture(&root.join("park"), "park-parity");
    assert_eq!(
        park.manifest.capabilities.as_deref(),
        Some(
            &[
                "tmux:read".to_owned(),
                "proc:exec:git".to_owned(),
                "fs:read:state".to_owned(),
                "fs:write:state".to_owned(),
            ][..]
        ),
        "park git context must use only bounded git argv plus state/tmux caps"
    );
    let check = load_wasm_fixture(&root.join("check"), "check-parity");
    assert_eq!(
        check.manifest.capabilities.as_deref(),
        Some(
            &[
                "proc:exec:bun".to_owned(),
                "proc:exec:gh".to_owned(),
                "proc:exec:ghq".to_owned(),
                "proc:exec:git".to_owned(),
                "proc:exec:tmux".to_owned(),
                "proc:exec:uv".to_owned(),
                "proc:exec:uvx".to_owned(),
            ][..]
        ),
        "check tools must enumerate exact commands; no shell wildcard cap"
    );
}

#[test]
fn cleanup_wasm_declares_only_bounded_fs_caps() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm-parity/cleanup");
    let wasm_plugin = load_wasm_fixture(&fixture, "cleanup-parity");
    assert_eq!(
        wasm_plugin.manifest.capabilities.as_deref(),
        Some(&["fs:read:data".to_owned(), "fs:write:data".to_owned()][..]),
        "cleanup fixture must declare only bounded data read/write caps"
    );
}

#[test]
fn contacts_wasm_declares_only_bounded_data_caps() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm-parity/contacts");
    let wasm_plugin = load_wasm_fixture(&fixture, "contacts-parity");
    assert_eq!(
        wasm_plugin.manifest.capabilities.as_deref(),
        Some(&["fs:read:data".to_owned(), "fs:write:data".to_owned()][..]),
        "contacts fixture must declare only bounded data read/write caps"
    );
}

#[test]
fn signals_wasm_declares_only_bounded_data_read_caps() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm-parity/signals");
    let wasm_plugin = load_wasm_fixture(&fixture, "signals-parity");
    assert_eq!(
        wasm_plugin.manifest.capabilities.as_deref(),
        Some(&["fs:read:data".to_owned()][..]),
        "signals fixture must declare only bounded data read caps"
    );
}

#[test]
fn costs_wasm_declares_only_localserver_caps() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm-parity/costs");
    let wasm_plugin = load_wasm_fixture(&fixture, "costs-parity");
    assert_eq!(
        wasm_plugin.manifest.capabilities.as_deref(),
        Some(&["sdk:localserver".to_owned()][..]),
        "costs fixture must use the host-pinned localserver cap only"
    );
}

#[test]
fn about_wasm_declares_only_read_caps() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm-parity/about");
    let wasm_plugin = load_wasm_fixture(&fixture, "about-parity");
    assert_eq!(
        wasm_plugin.manifest.capabilities.as_deref(),
        Some(
            &[
                "fs:read:data".to_owned(),
                "fs:read:config".to_owned(),
                "tmux:read".to_owned(),
            ][..]
        ),
        "about fixture must declare only bounded read caps"
    );
}

#[test]
fn activity_wasm_declares_only_tmux_read_caps() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm-parity/activity");
    let wasm_plugin = load_wasm_fixture(&fixture, "activity-parity");
    assert_eq!(
        wasm_plugin.manifest.capabilities.as_deref(),
        Some(&["tmux:read".to_owned()][..]),
        "activity fixture must declare only tmux read caps"
    );
}

#[test]
fn discover_wasm_declares_only_read_caps() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm-parity/discover");
    let wasm_plugin = load_wasm_fixture(&fixture, "discover-parity");
    assert_eq!(
        wasm_plugin.manifest.capabilities.as_deref(),
        Some(
            &[
                "sdk:config:read".to_owned(),
                "fs:read:config".to_owned(),
                "tmux:read".to_owned(),
            ][..]
        ),
        "discover fixture must declare only bounded read caps"
    );
}

#[test]
fn send_wasm_declares_plain_send_only_for_non_destructive_fixture() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm-parity/send");
    let wasm_plugin = load_wasm_fixture(&fixture, "send-parity");
    assert_eq!(
        wasm_plugin.manifest.capabilities.as_deref(),
        Some(&["tmux:send".to_owned()][..]),
        "non-destructive send fixture must not over-grant tmux:send:force"
    );
}

fn run_parity_case(case: ParityCase<'_>) {
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wasm-parity")
        .join(case.plugin);
    assert_fixture_metadata(&fixture);

    let temp = temp_dir("wasm-parity");
    let isolated_home = temp.join("home");
    create_dir_all(&isolated_home).expect("isolated MAW_HOME");
    seed_real_maw_home(&fixture, &isolated_home);
    let old_maw_home = std::env::var_os("MAW_HOME");
    let old_plugins_dir = std::env::var_os("MAW_PLUGINS_DIR");
    std::env::set_var("MAW_HOME", &isolated_home);
    std::env::remove_var("MAW_PLUGINS_DIR");

    let ctx = InvokeContext {
        source: InvokeSource::Cli,
        args: case.args.iter().map(|arg| (*arg).to_owned()).collect(),
        cwd: None,
        home: None,
    };

    let wasm_plugin = load_wasm_fixture(&fixture, case.manifest_name);
    let host = seeded_host(&fixture, &wasm_plugin);
    let host_audit = host.clone();
    let mut wasm_runtime =
        ExtismWasmInvokeRuntime::default().with_host(wasm_plugin.manifest.name.clone(), host);
    let wasm = invoke_plugin(&wasm_plugin, &ctx, &mut wasm_runtime);

    restore_env("MAW_HOME", old_maw_home);
    restore_env("MAW_PLUGINS_DIR", old_plugins_dir);
    let _ = std::fs::remove_dir_all(temp);

    assert_eq!(
        read_golden(&fixture, case.args),
        capture(&wasm),
        "plugin={} args={:?}",
        case.plugin,
        case.args
    );
    let audit = host_audit.audit_json_lines();
    if let Some(expected) = case.expected_host_calls {
        let actual = audit.lines().filter(|line| !line.trim().is_empty()).count();
        assert_eq!(
            actual, expected,
            "host-call audit mismatch for {} {:?}: {audit}",
            case.plugin, case.args
        );
    }
    if let Some(expected) = case.expected_host_transcript {
        assert_host_transcript(case.plugin, case.args, &audit, expected);
    }
}

fn assert_host_transcript(
    plugin: &str,
    args: &[&str],
    audit: &str,
    expected: &[ExpectedHostCall<'_>],
) {
    let actual = audit
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).expect("audit json"))
        .collect::<Vec<_>>();
    assert_eq!(
        actual.len(),
        expected.len(),
        "host transcript length for {plugin} {args:?}: {audit}"
    );
    for (event, expected) in actual.iter().zip(expected.iter()) {
        assert_eq!(
            event["host_fn"], expected.host_fn,
            "host_fn for {plugin} {args:?}"
        );
        assert_eq!(
            event["capability"], expected.capability,
            "capability for {plugin} {args:?}"
        );
        assert_eq!(
            event["resource"], expected.resource,
            "resource for {plugin} {args:?}"
        );
        assert_eq!(event["status"], "ok", "status for {plugin} {args:?}");
    }
}

fn assert_fixture_metadata(fixture: &Path) {
    let metadata: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.join("metadata.json")).expect("metadata"),
    )
    .expect("metadata json");
    assert_eq!(metadata["assemblyscript"], "0.27.31");
    assert_eq!(metadata["extismAsPdk"], "1.0.0");
    assert_eq!(
        hash_file(&fixture.join("plugin.wasm")).expect("wasm hash"),
        metadata["wasmSha256"].as_str().expect("sha")
    );
}

fn seed_real_maw_home(fixture: &Path, isolated_home: &Path) {
    let host_state_path = fixture.join("host-state.json");
    let host_state: Value = serde_json::from_str(
        &std::fs::read_to_string(&host_state_path)
            .unwrap_or_else(|err| panic!("read {}: {err}", host_state_path.display())),
    )
    .unwrap_or_else(|err| panic!("parse {}: {err}", host_state_path.display()));
    if let Some(files) = host_state.get("seedFiles").and_then(Value::as_object) {
        for (relative, content) in files {
            let path = isolated_home.join(relative);
            if let Some(parent) = path.parent() {
                create_dir_all(parent)
                    .unwrap_or_else(|err| panic!("create seed dir {}: {err}", parent.display()));
            }
            std::fs::write(&path, content.as_str().expect("seed file content"))
                .unwrap_or_else(|err| panic!("write seed file {}: {err}", path.display()));
        }
    }
}

fn seeded_host(fixture: &Path, plugin: &LoadedPlugin) -> MawWasmHost {
    let host_state: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.join("host-state.json")).expect("host-state"),
    )
    .expect("host-state json");
    host_state["calls"].as_array().expect("calls").iter().fold(
        MawWasmHost::new(plugin),
        |host, call| {
            host.with_audited_fake_response(
                call["name"].as_str().expect("fake name"),
                call["input"].as_str().expect("fake input"),
                call["output"].as_str().expect("fake output"),
                call.get("capability")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                call.get("resource")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                call.get("status")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            )
        },
    )
}

fn capture(result: &InvokeResult) -> Value {
    serde_json::json!({
        "stdout": result.output.as_deref().unwrap_or(""),
        "stderr": result.error.as_deref().unwrap_or(""),
        "result": { "ok": result.ok, "output": result.output, "error": result.error }
    })
}

fn read_golden(fixture: &Path, args: &[&str]) -> Value {
    let path = golden_path(fixture, args);
    serde_json::from_str(
        &std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read golden {}: {err}", path.display())),
    )
    .unwrap_or_else(|err| panic!("parse golden {}: {err}", path.display()))
}

fn golden_path(fixture: &Path, args: &[&str]) -> PathBuf {
    fixture.join(format!("golden.{}.json", args_slug(args)))
}

fn args_slug(args: &[&str]) -> String {
    if args.is_empty() {
        return "no-args".to_owned();
    }
    args.iter()
        .map(|arg| {
            arg.chars()
                .map(|ch| match ch {
                    'a'..='z' | 'A'..='Z' | '0'..='9' => ch,
                    _ => '-',
                })
                .collect::<String>()
                .trim_matches('-')
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("--")
}

fn load_wasm_fixture(dir: &Path, manifest_name: &str) -> LoadedPlugin {
    let mut plugin = load_manifest_from_dir(dir)
        .unwrap_or_else(|err| panic!("load fixture manifest {}: {err}", dir.display()))
        .unwrap_or_else(|| LoadedPlugin {
            manifest: manifest(manifest_name),
            dir: dir.to_path_buf(),
            wasm_path: dir.join("plugin.wasm"),
            entry_path: None,
            wasm_export: "handle".to_owned(),
            kind: LoadedPluginKind::Wasm,
            disabled: false,
        });
    manifest_name.clone_into(&mut plugin.manifest.name);
    plugin
}

fn manifest(name: &str) -> PluginManifest {
    PluginManifest {
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        weight: None,
        tier: None,
        wasm: None,
        entry: None,
        entry_export: Some("handle".to_owned()),
        sdk: "*".to_owned(),
        cli: None,
        api: None,
        description: None,
        author: None,
        hooks: None,
        cron: None,
        module: None,
        transport: None,
        engine: None,
        target: None,
        capability_namespaces: None,
        capabilities: Some(Vec::new()),
        endpoints: None,
        capability_warnings: Vec::new(),
        dependencies: None,
        artifact: None,
    }
}

fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
    if let Some(value) = value {
        std::env::set_var(key, value);
    } else {
        std::env::remove_var(key);
    }
}

fn temp_dir(prefix: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-{prefix}-{}-{stamp}", std::process::id()));
    create_dir_all(&path).expect("temp dir");
    path
}
