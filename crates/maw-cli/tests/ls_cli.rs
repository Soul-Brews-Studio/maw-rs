use maw_cli::{run_cli, CliOutput};

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn ls_plan_compact_default_shows_all_sessions_and_fleet_only_filters_shape() {
    let output = run_cli(&args(&[
        "ls",
        "--plan-json",
        "--pane",
        "%1|claude|50-mawjs:1.0|mawjs|100|/repo|1700000000",
        "--pane",
        "%2|codex|maw-rs:1.0|maw-rs|101|/repo|1699999980",
        "--pane",
        "%3|zsh|scratch:1.0|scratch|102|/tmp|1699999700",
    ]));

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        output.stdout,
        concat!(
            "{\"command\":\"ls\",\"mode\":\"compact\",\"scope\":\"local\",\"json\":true,",
            "\"sessions\":[",
            "{\"session\":\"50-mawjs\",\"status\":\"stale\",\"panes\":1,\"agents\":1},",
            "{\"session\":\"maw-rs\",\"status\":\"stale\",\"panes\":1,\"agents\":1},",
            "{\"session\":\"scratch\",\"status\":\"stale\",\"panes\":1,\"agents\":0}]}
"
        )
    );

    let fleet_only = run_cli(&args(&[
        "ls",
        "--plan-json",
        "--fleet-only",
        "--pane",
        "%1|claude|50-mawjs:1.0|mawjs|100|/repo|1700000000",
        "--pane",
        "%2|codex|maw-rs:1.0|maw-rs|101|/repo|1699999980",
    ]));

    assert_eq!(fleet_only.code, 0, "{}", fleet_only.stderr);
    assert_eq!(
        fleet_only.stdout,
        concat!(
            "{\"command\":\"ls\",\"mode\":\"compact\",\"scope\":\"local\",\"json\":true,",
            "\"fleetOnly\":true,",
            "\"sessions\":[{\"session\":\"50-mawjs\",\"status\":\"stale\",\"panes\":1,\"agents\":1}]}
"
        )
    );
}

#[test]
fn ls_plan_positional_filters_local_sessions_without_peer_stub() {
    let output = run_cli(&args(&[
        "ls",
        "maw-rs",
        "--plan-json",
        "--now",
        "1700000100",
        "--pane",
        "%1|codex|maw-rs:1.0|agent|100|/repo|1700000090",
        "--pane",
        "%2|codex|188-maw-rs:1.0|agent|101|/repo|1700000080",
        "--pane",
        "%3|zsh|scratch:1.0|scratch|102|/tmp|1700000070",
    ]));

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        output.stdout,
        concat!(
            "{\"command\":\"ls\",\"mode\":\"compact\",\"scope\":\"local\",\"json\":true,",
            "\"sessions\":[",
            "{\"session\":\"188-maw-rs\",\"status\":\"active\",\"panes\":1,\"agents\":1},",
            "{\"session\":\"maw-rs\",\"status\":\"active\",\"panes\":1,\"agents\":1}]}\n"
        )
    );
    assert!(!output.stdout.contains("scratch"));
    assert!(!output.stdout.contains("no fake sessions"));
}

#[test]
fn ls_plan_verbose_json_keeps_channels_filter_and_statuses() {
    let output = run_cli(&args(&[
        "ls",
        "--json",
        "--verbose",
        "--channels",
        "zzunique",
        "--now",
        "1700000100",
        "--pane",
        "%1|claude|mawjs-oracle-discord:1.0|chan|100|/repo|1700000090",
        "--pane",
        "%2|node|zzunique-scratch:2.0|worker|101|/repo|1700000050",
    ]));

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        output.stdout,
        concat!(
            "{\"command\":\"ls\",\"mode\":\"verbose\",\"scope\":\"local\",\"json\":true,",
            "\"panes\":[{\"id\":\"%2\",\"target\":\"zzunique-scratch:2.0\",\"session\":\"zzunique-scratch\",\"command\":\"node\",\"title\":\"worker\",\"status\":\"idle\",\"ageSec\":50,\"agent\":true,\"annotation\":\"orphan\"}]}
"
        )
    );
}

#[test]
fn ls_plan_active_recent_and_help_match_maw_js_surface() {
    let output = run_cli(&args(&[
        "ls",
        "--active",
        "1h",
        "--recent",
        "1",
        "--plan-json",
        "--now",
        "1700000000",
        "--session-created",
        "old-session=100",
        "--session-created",
        "new-session=300",
        "--pane",
        "%1|claude|old-session:1.0|old|100|/repo|1699999900",
        "--pane",
        "%2|claude|new-session:1.0|new|101|/repo|1699999950",
    ]));

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        output.stdout,
        concat!(
            "{\"command\":\"ls\",\"mode\":\"compact\",\"scope\":\"local\",\"json\":true,",
            "\"activeThresholdSec\":3600,\"recentLimit\":1,",
            "\"sessions\":[{\"session\":\"new-session\",\"status\":\"idle\",\"panes\":1,\"agents\":1,\"created\":300,\"lastActivityAgeSec\":50}]}
"
        )
    );

    let help = run_cli(&args(&["ls", "--help"]));
    assert_eq!(help.code, 0);
    assert!(help.stdout.contains("maw ls --active [30m]"));
    assert!(help.stdout.contains("maw ls <filter>"));
    assert!(help.stdout.contains("maw ls --federation <peer>"));
    assert!(!help.stdout.contains("maw ls <peer>"));
}

#[test]
fn ls_plan_rejects_unknown_flags() {
    let output = run_cli(&args(&["ls", "--bad"]));
    assert_eq!(
        output,
        CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: "ls: unknown argument --bad\nusage: maw-rs ls [<filter>] [--all] [--json|--plan-json] [--compact|-c] [--verbose|-v] [--watch[=secs]] [--recent|-r [N]] [--active [30m|1h]] [--federation] [--fleet-only] [--node <node>] [--verify] [--fix] [--channels] [--pane <id|command|target|title|pid|cwd|last_activity>]...\n".to_owned(),
        }
    );
}
