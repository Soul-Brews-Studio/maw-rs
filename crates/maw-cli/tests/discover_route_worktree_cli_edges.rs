#![allow(clippy::too_many_lines)]

use maw_cli::{run_cli, CliOutput};
use serde_json::Value;

fn run(args: &[&str]) -> CliOutput {
    run_cli(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
}

fn json(args: &[&str]) -> Value {
    let output = run(args);
    assert_eq!(output.code, 0, "stderr for {args:?}: {}", output.stderr);
    assert!(
        output.stderr.is_empty(),
        "stderr for {args:?}: {}",
        output.stderr
    );
    serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid json for {args:?}: {error}\n{}", output.stdout))
}

fn assert_usage(args: &[&str], expected: &str) {
    let output = run(args);
    assert_eq!(output.code, 2, "stdout for {args:?}: {}", output.stdout);
    assert!(
        output.stderr.contains(expected),
        "stderr for {args:?} did not contain {expected:?}: {}",
        output.stderr
    );
    assert!(
        output.stdout.is_empty(),
        "stdout for {args:?}: {}",
        output.stdout
    );
}

fn assert_text(args: &[&str], expected: &str) {
    let output = run(args);
    assert_eq!(output.code, 0, "stderr for {args:?}: {}", output.stderr);
    assert!(
        output.stdout.contains(expected),
        "stdout for {args:?} did not contain {expected:?}: {}",
        output.stdout
    );
    assert!(
        output.stderr.is_empty(),
        "stderr for {args:?}: {}",
        output.stderr
    );
}

#[test]
fn discover_inventory_matches_ghq_peers_and_live_panes_in_plan_json() {
    let value = json(&[
        "discover",
        "--plan-json",
        "--tree",
        "--named-peer",
        "morpheus=wss://morpheus.example",
        "--named-peer",
        "node-77=wss://node.example",
        "--pane",
        "%1|zsh|oracle-session:morpheus-oracle.1|morpheus|123|/tmp/morpheus-oracle|42",
        "--ghq",
        "/opt/ghq/github.com/Soul-Brews-Studio/smith-oracle",
        "--oracle",
        "smith|manifest|node-77|offline-session|offline-window|github.com/Soul-Brews-Studio/smith-oracle|/elsewhere|true|false",
        "--oracle",
        "morpheus|manifest|node-x|offline-session|not-the-pane-window|github.com/Soul-Brews-Studio/morpheus-oracle|/elsewhere|false|false",
    ]);

    assert_eq!(value["command"], "discover");
    assert_eq!(value["liveTotal"], 1);
    assert_eq!(value["ghq"]["repos"][0]["host"], "github.com");
    assert_eq!(value["ghq"]["repos"][0]["owner"], "Soul-Brews-Studio");
    assert_eq!(value["ghq"]["repos"][0]["oracleLike"], true);

    let oracles = value["oracles"]["records"].as_array().expect("oracles");
    let smith = oracles
        .iter()
        .find(|oracle| oracle["name"] == "smith")
        .expect("smith oracle");
    assert_eq!(
        smith["ghqPath"],
        "/opt/ghq/github.com/Soul-Brews-Studio/smith-oracle"
    );
    assert_eq!(smith["peerUrls"], serde_json::json!(["wss://node.example"]));

    let morpheus = oracles
        .iter()
        .find(|oracle| oracle["name"] == "morpheus")
        .expect("morpheus oracle");
    assert_eq!(morpheus["awake"], true);
    assert_eq!(
        morpheus["peerUrls"],
        serde_json::json!(["wss://morpheus.example"])
    );

    let live_pane = &value["live"]["panes"][0];
    assert_eq!(live_pane["pid"], 123);
    assert_eq!(live_pane["lastActivity"], 42);
    assert_eq!(live_pane["matches"], serde_json::json!(["morpheus"]));
}

#[test]
fn discover_text_modes_cover_empty_live_and_inventory_status_lines() {
    assert_text(
        &["discover", "--awake"],
        "no live tmux sessions/windows found\n",
    );
    assert_text(
        &[
            "discover",
            "--plugin",
            "alpha|1.0.0|ts|core|5|false|/plugins/alpha|run-alpha|a,b|chat,fs|dep-one",
            "--ghq",
            "/opt/ghq/github.com/Soul-Brews-Studio/plain-repo",
        ],
        "alpha 1.0.0 enabled",
    );
    assert_text(
        &[
            "discover",
            "--plugin",
            "alpha|1.0.0|ts|core|5|false|/plugins/alpha|run-alpha|a,b|chat,fs|dep-one",
            "--ghq",
            "/opt/ghq/github.com/Soul-Brews-Studio/plain-repo",
        ],
        "plain-repo /opt/ghq/github.com/Soul-Brews-Studio/plain-repo",
    );
}

#[test]
fn route_parser_and_text_edges_cover_remaining_argument_branches() {
    assert_text(
        &[
            "route",
            "--query",
            "local:alpha",
            "--node",
            "local",
            "--session",
            "alpha",
            "--source",
            "local",
            "--window",
            "1:main:true",
        ],
        "route local:alpha: self-node alpha:1\n",
    );
    assert_text(
        &["route", "constants"],
        "route constants result-types=local,peer,self-node,error window-shape=index:name:active\n",
    );

    for (args, expected) in [
        (&["route", "--query"][..], "route: missing --query value"),
        (&["route", "--node"][..], "route: missing --node value"),
        (
            &["route", "--named-peer"][..],
            "route: missing --named-peer value",
        ),
        (&["route", "--peer"][..], "route: missing --peer value"),
        (&["route", "--agent"][..], "route: missing --agent value"),
        (
            &["route", "--session"][..],
            "route: missing --session value",
        ),
        (&["route", "--source"][..], "route: missing --source value"),
        (
            &["route", "--source", "fixture"][..],
            "route: --source must follow a --session",
        ),
        (&["route", "--window"][..], "route: missing --window value"),
        (
            &["route", "--query", "alpha", "--unknown"][..],
            "route: unknown argument --unknown",
        ),
        (&["route"][..], "route: expected --query <target>"),
        (
            &["route", "--query", "alpha", "--agent", "=node"][..],
            "route: --agent must use <agent=node>",
        ),
        (
            &["route", "--query", "alpha", "--window", "1"][..],
            "route: --window must follow a --session",
        ),
        (
            &[
                "route",
                "--query",
                "alpha",
                "--session",
                "local",
                "--window",
                "1",
            ][..],
            "route: window must use <index:name:active>",
        ),
        (
            &[
                "route",
                "--query",
                "alpha",
                "--session",
                "local",
                "--window",
                "1:alpha:maybe",
            ][..],
            "route: window active must be true or false",
        ),
    ] {
        assert_usage(args, expected);
    }
}

#[test]
fn worktree_window_parser_and_text_edges_cover_remaining_argument_branches() {
    assert_text(
        &["worktree-window", "constants"],
        "worktree-window constants results=bound,ambiguous,none window-shape=index:name:active\n",
    );
    assert_text(
        &[
            "worktree-window",
            "--main-repo-name",
            "mawjs-oracle",
            "--wt-name",
            "1-tile-1",
            "--session",
            "other",
            "--window",
            "1:mawjs-tile-1:false",
            "--window",
            "2:mawjs-6-tile-1:false",
        ],
        "worktree-window mawjs-oracle 1-tile-1: ambiguous tile-1 candidates=mawjs-tile-1, mawjs-6-tile-1\n",
    );

    for (args, expected) in [
        (
            &["worktree-window", "--main-repo-name"][..],
            "worktree-window: missing --main-repo-name value",
        ),
        (
            &["worktree-window", "--wt-name"][..],
            "worktree-window: missing --wt-name value",
        ),
        (
            &["worktree-window", "--session"][..],
            "worktree-window: missing --session value",
        ),
        (
            &["worktree-window", "--window"][..],
            "worktree-window: missing --window value",
        ),
        (
            &["worktree-window", "--unknown"][..],
            "worktree-window: unknown argument --unknown",
        ),
        (
            &["worktree-window"][..],
            "worktree-window: expected --main-repo-name <repo>",
        ),
        (
            &["worktree-window", "--main-repo-name", "repo"][..],
            "worktree-window: expected --wt-name <worktree>",
        ),
        (
            &[
                "worktree-window",
                "--main-repo-name",
                "repo",
                "--wt-name",
                "feature",
                "--session",
                "repo",
                "--window",
                "1",
            ][..],
            "worktree-window: window must use <index:name:active>",
        ),
    ] {
        assert_usage(args, expected);
    }
}
