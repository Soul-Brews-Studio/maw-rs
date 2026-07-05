// Ported from maw-js test/isolated/discover-plugin-peer-sources.test.ts
// into a side-by-side, fake-backed maw-rs discover plan CLI surface.

use maw_cli::run_cli;
use serde_json::json;

fn json_output(output: &maw_cli::CliOutput) -> serde_json::Value {
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
        panic!("invalid json: {error}\n{}", output.stdout);
    })
}

#[test]
fn discover_plan_rejects_invalid_peer_source_before_fetch_or_live_probe() {
    let output = json_output(&run_cli(&[
        "discover".to_owned(),
        "--peers".to_owned(),
        "bogus".to_owned(),
        "--plan-json".to_owned(),
    ]));

    assert_eq!(
        output,
        json!({
            "command": "discover",
            "ok": false,
            "error": "invalid_peer_source",
            "output": "usage: maw discover [--peers config|scout|both] [--json] [--tree] [--awake]",
            "fetchCalls": 0,
            "liveProbeCalls": 0
        })
    );
}

#[test]
fn discover_plan_renders_inline_scout_text_without_live_probe() {
    let output = run_cli(&[
        "discover".to_owned(),
        "--peers=scout".to_owned(),
        "--discovered".to_owned(),
        "scout-node|scout-host|mawjs|http://scout:3456".to_owned(),
    ]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stdout.contains("scout-node"), "{}", output.stdout);
    assert!(
        output.stdout.contains("http://scout:3456"),
        "{}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("tmux"),
        "scout text should not probe/render live state: {}",
        output.stdout
    );
}

#[test]
fn discover_plan_renders_config_json_with_live_peer_metadata() {
    let output = json_output(&run_cli(&[
        "discover".to_owned(),
        "--peers".to_owned(),
        "config".to_owned(),
        "--peer".to_owned(),
        "http://config:3456".to_owned(),
        "--named-peer".to_owned(),
        "named=http://named:3456".to_owned(),
        "--pane".to_owned(),
        "%1|claude|101-mawjs:agent.0|named|-|/repo/mawjs-oracle|-".to_owned(),
        "--json".to_owned(),
        "--plan-json".to_owned(),
    ]));

    assert_eq!(output["command"], "discover");
    assert_eq!(output["ok"], true);
    assert_eq!(output["mode"], "config");
    assert_eq!(output["total"], 2);
    assert_eq!(output["liveTotal"], 1);
    assert_eq!(output["fetchCalls"], 0);
    assert_eq!(output["liveProbeCalls"], 1);
    assert_eq!(output["live"]["panes"][0]["target"], "101-mawjs:agent.0");
    assert_eq!(output["live"]["sessions"][0]["name"], "101-mawjs");
    assert_eq!(
        output["peers"]
            .as_array()
            .expect("peers")
            .iter()
            .map(|peer| peer["url"].as_str().expect("peer url"))
            .collect::<Vec<_>>(),
        ["http://config:3456", "http://named:3456"]
    );
    let named = output["peers"]
        .as_array()
        .expect("peers")
        .iter()
        .find(|peer| peer["name"] == "named")
        .expect("named peer");
    assert_eq!(named["awake"], true);
    assert_eq!(named["liveTargets"], json!(["101-mawjs:agent.0"]));
    assert_eq!(
        output["plugins"],
        json!({"source": "plugin-registry", "total": 0, "records": []})
    );
    assert_eq!(
        output["fleet"],
        json!({"source": "fleet-config", "total": 0, "records": []})
    );
    assert_eq!(
        output["oracles"],
        json!({"source": "oracle-manifest", "total": 0, "records": []})
    );
    assert_eq!(
        output["ghq"],
        json!({"source": "ghq", "total": 0, "repos": []})
    );
}

#[test]
fn discover_plan_awake_json_filters_peers_but_preserves_live_panes() {
    let output = json_output(&run_cli(&[
        "discover".to_owned(),
        "--peer".to_owned(),
        "http://config:3456".to_owned(),
        "--named-peer".to_owned(),
        "named=http://named:3456".to_owned(),
        "--pane".to_owned(),
        "%1|claude|101-mawjs:agent.0|named|-|/repo/mawjs-oracle|-".to_owned(),
        "--awake".to_owned(),
        "--json".to_owned(),
        "--plan-json".to_owned(),
    ]));

    assert_eq!(output["ok"], true);
    assert_eq!(output["awake"], true);
    assert_eq!(output["total"], 1);
    assert_eq!(output["liveTotal"], 1);
    assert_eq!(output["live"]["panes"][0]["target"], "101-mawjs:agent.0");
    assert_eq!(
        output["peers"]
            .as_array()
            .expect("peers")
            .iter()
            .map(|peer| peer["name"].as_str().unwrap_or("-"))
            .collect::<Vec<_>>(),
        ["named"]
    );
    assert_eq!(output["plugins"]["records"], json!([]));
    assert_eq!(output["ghq"]["repos"], json!([]));
}

#[test]
fn discover_plan_includes_registered_plugins_and_deduped_ghq_repos_in_json_tree() {
    let output = json_output(&run_cli(&[
        "discover".to_owned(),
        "--peers".to_owned(),
        "config".to_owned(),
        "--plugin".to_owned(),
        "buddy|1.2.3|ts|standard|12|false|/plugins/buddy|buddy|buddy-alias|sdk:identity|base"
            .to_owned(),
        "--ghq".to_owned(),
        "/opt/Code/github.com/Soul-Brews-Studio/maw-js".to_owned(),
        "--ghq".to_owned(),
        "/opt/Code/github.com/Soul-Brews-Studio/maw-js".to_owned(),
        "--ghq".to_owned(),
        "/opt/Code/github.com/Soul-Brews-Studio/maw-js.wt-features".to_owned(),
        "--ghq".to_owned(),
        "/opt/Code/github.com/Soul-Brews-Studio/maw-js/agents/2-nested".to_owned(),
        "--tree".to_owned(),
        "--json".to_owned(),
        "--plan-json".to_owned(),
    ]));

    assert_eq!(output["ok"], true);
    assert_eq!(output["total"], 4);
    assert_eq!(output["plugins"]["total"], 1);
    assert_eq!(
        output["plugins"]["records"][0],
        json!({
            "source": "plugin-registry",
            "type": "plugin",
            "name": "buddy",
            "version": "1.2.3",
            "kind": "ts",
            "tier": "standard",
            "weight": 12,
            "disabled": false,
            "dir": "/plugins/buddy",
            "command": "buddy",
            "aliases": ["buddy-alias"],
            "capabilities": ["sdk:identity"],
            "dependencies": ["base"]
        })
    );
    assert_eq!(output["ghq"]["total"], 3);
    assert_eq!(
        output["ghq"]["repos"][0],
        json!({
            "source": "ghq",
            "type": "repo",
            "path": "/opt/Code/github.com/Soul-Brews-Studio/maw-js",
            "name": "maw-js",
            "owner": "Soul-Brews-Studio",
            "host": "github.com",
            "oracleLike": false,
            "worktree": false
        })
    );
    assert_eq!(output["ghq"]["repos"][1]["worktree"], true);
    assert_eq!(output["ghq"]["repos"][2]["worktree"], true);
    assert_eq!(output["tree"]["plugins"][0]["name"], "buddy");
    assert_eq!(
        output["tree"]["ghq"][0]["path"],
        "/opt/Code/github.com/Soul-Brews-Studio/maw-js"
    );
    assert_eq!(
        output["tree"]["ghq"][1]["path"],
        "/opt/Code/github.com/Soul-Brews-Studio/maw-js.wt-features"
    );
    assert_eq!(
        output["tree"]["ghq"][2]["path"],
        "/opt/Code/github.com/Soul-Brews-Studio/maw-js/agents/2-nested"
    );
}

#[test]
fn discover_plan_joins_fleet_oracles_ghq_peers_and_live_in_json() {
    let output = json_output(&run_cli(&[
        "discover".to_owned(),
        "--peers".to_owned(),
        "config".to_owned(),
        "--named-peer".to_owned(),
        "mawjs=http://m5:3456".to_owned(),
        "--agent".to_owned(),
        "mawjs-oracle=m5".to_owned(),
        "--fleet".to_owned(),
        "50-mawjs.json|50|mawjs|50-mawjs|mawjs-oracle|Soul-Brews-Studio/maw-js".to_owned(),
        "--ghq".to_owned(),
        "/opt/Code/github.com/Soul-Brews-Studio/maw-js".to_owned(),
        "--oracle".to_owned(),
        "mawjs|fleet+agent+oracles-json|m5|50-mawjs|mawjs-oracle|Soul-Brews-Studio/maw-js|/opt/Code/github.com/Soul-Brews-Studio/maw-js|true|true".to_owned(),
        "--pane".to_owned(),
        "%9|claude|50-mawjs:mawjs-oracle.0|mawjs|-|/opt/Code/github.com/Soul-Brews-Studio/maw-js|-".to_owned(),
        "--json".to_owned(),
        "--tree".to_owned(),
        "--plan-json".to_owned(),
    ]));

    assert_eq!(output["ok"], true);
    assert_eq!(output["fleet"]["records"][0]["endpoint"], "http://m5:3456");
    assert_eq!(output["fleet"]["records"][0]["peerMatched"], true);
    assert_eq!(output["oracles"]["records"][0]["awake"], true);
    assert_eq!(
        output["oracles"]["records"][0]["ghqPath"],
        "/opt/Code/github.com/Soul-Brews-Studio/maw-js"
    );
    assert_eq!(output["oracles"]["records"][0]["worktree"], false);
    assert_eq!(output["oracles"]["records"][0]["fleetMatched"], true);
    assert_eq!(
        output["oracles"]["records"][0]["peerUrls"],
        json!(["http://m5:3456"])
    );
    assert_eq!(output["tree"]["oracles"][0]["awake"], true);
}

#[test]
fn discover_plan_renders_plugin_registry_in_text_output() {
    let output = run_cli(&[
        "discover".to_owned(),
        "--peers=config".to_owned(),
        "--plugin".to_owned(),
        "handover|1.2.3|ts|standard|12|true|/plugins/handover|handover|-|-|-".to_owned(),
    ]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        output.stdout.contains("plugin registry"),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("handover"), "{}", output.stdout);
    assert!(output.stdout.contains("disabled"), "{}", output.stdout);
}
