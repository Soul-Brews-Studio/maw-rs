use maw_cli::{run_cli, CliOutput};

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn run(values: &[&str]) -> CliOutput {
    run_cli(&args(values))
}

fn assert_usage_error(values: &[&str], expected: &str) {
    let output = run(values);
    assert_eq!(output.code, 2, "stdout for {values:?}: {}", output.stdout);
    assert!(
        output.stderr.contains(expected),
        "stderr for {values:?} did not contain {expected:?}: {}",
        output.stderr
    );
}

#[test]
fn ls_parser_remaining_error_branches_are_stable() {
    assert_usage_error(&["ls", "--active=0"], "ls: invalid --active duration");
    assert_usage_error(&["ls", "--pane"], "ls: missing --pane value");
    assert_usage_error(
        &["ls", "--pane", "too|few|fields"],
        "ls: --pane must use <id|command|target|title|pid|cwd|last_activity>",
    );
    assert_usage_error(&["ls", "--now"], "ls: missing --now value");
    assert_usage_error(&["ls", "--now", "soon"], "ls: --now must be an integer");
    assert_usage_error(
        &["ls", "--session-created"],
        "ls: missing --session-created value",
    );
    assert_usage_error(
        &["ls", "--session-created", "missing-equals"],
        "ls: --session-created must use <session=epoch_seconds>",
    );
    assert_usage_error(
        &["ls", "--session-created", "alpha=old"],
        "ls: session-created epoch must be an integer",
    );
}

#[test]
fn ls_active_equals_recent_and_peer_text_branches_are_stable() {
    let active = run(&[
        "ls",
        "--active=2h",
        "--recent",
        "1",
        "--plan-json",
        "--now",
        "1700000000",
        "--session-created",
        "50-mawjs=300",
        "--pane",
        "%1|node|50-mawjs:1.0|mawjs|100|/repo|1699999995",
    ]);
    assert_eq!(active.code, 0, "{}", active.stderr);
    assert_eq!(
        active.stdout,
        concat!(
            "{\"command\":\"ls\",\"mode\":\"compact\",\"scope\":\"local\",\"json\":true,",
            "\"activeThresholdSec\":7200,\"recentLimit\":1,",
            "\"sessions\":[{\"session\":\"50-mawjs\",\"status\":\"active\",\"panes\":1,\"agents\":1,\"created\":300,\"lastActivityAgeSec\":5}]}\n"
        )
    );

    let filtered = run(&[
        "ls",
        "remote-oracle",
        "--plan-json",
        "--now",
        "1700000000",
        "--pane",
        "%1|codex|remote-oracle:1.0|agent|100|/repo|1699999995",
        "--pane",
        "%2|zsh|other:1.0|other|101|/repo|1699999995",
    ]);
    assert_eq!(filtered.code, 0, "{}", filtered.stderr);
    assert_eq!(
        filtered.stdout,
        "{\"command\":\"ls\",\"mode\":\"compact\",\"scope\":\"local\",\"json\":true,\"sessions\":[{\"session\":\"remote-oracle\",\"status\":\"active\",\"panes\":1,\"agents\":1}]}\n"
    );
    assert!(filtered.stderr.is_empty());
}

#[test]
fn ls_text_rendering_remaining_status_and_duration_branches_are_stable() {
    let active_empty = run(&[
        "ls",
        "--active=2d",
        "--now",
        "200000",
        "--pane",
        "%1|zsh|50-mawjs:1.0|old|100|/repo|1",
    ]);
    assert_eq!(active_empty.code, 0, "{}", active_empty.stderr);
    assert_eq!(active_empty.stdout, "No sessions active in the last 2d.\n");

    let compact = run(&[
        "ls",
        "--all",
        "--now",
        "1700000000",
        "--pane",
        "%1|node|50-mawjs:1.0|agent|100|/repo|1699999999",
        "--pane",
        "%2|zsh|50-mawjs:1.1|shell|101|/repo|1699999700",
    ]);
    assert_eq!(compact.code, 0, "{}", compact.stderr);
    assert!(compact.stdout.contains("50-mawjs"), "{}", compact.stdout);
    assert!(compact.stdout.contains("2 panes"), "{}", compact.stdout);
    assert!(compact.stdout.contains("1 agent"), "{}", compact.stdout);
    assert!(compact.stdout.contains("maw ls -v"), "{}", compact.stdout);

    let verbose = run(&[
        "ls",
        "--all",
        "--verbose",
        "--now",
        "200000",
        "--pane",
        "%1|bash|50-mawjs:1.0|seconds|100|/repo|199995",
        "--pane",
        "%2|bash|51-mawjs:1.0|minutes|101|/repo|199880",
        "--pane",
        "%3|bash|52-mawjs:1.0|hours|102|/repo|192800",
    ]);
    assert_eq!(verbose.code, 0, "{}", verbose.stderr);
    assert!(verbose.stdout.contains("50-mawjs"), "{}", verbose.stdout);
    assert!(verbose.stdout.contains("51-mawjs"), "{}", verbose.stdout);
    assert!(verbose.stdout.contains("52-mawjs"), "{}", verbose.stdout);
    assert!(verbose.stdout.contains("\n  "), "{}", verbose.stdout);
    assert!(verbose.stdout.contains("5s"), "{}", verbose.stdout);
    assert!(verbose.stdout.contains("2m"), "{}", verbose.stdout);
    assert!(verbose.stdout.contains("2h"), "{}", verbose.stdout);
}

#[test]
fn ls_recent_filter_and_session_status_json_edges_are_stable() {
    let output = run(&[
        "ls",
        "--recent",
        "50-mawjs",
        "--json",
        "--now",
        "1700000000",
        "--session-created",
        "50-mawjs=200",
        "--pane",
        "%1|bash|50-mawjs:1.0|fresh-shell|100|/repo|1699999999",
        "--pane",
        "%2|bash|50-mawjs:1.1|idle-shell|101|/repo|1699999800",
        "--pane",
        "%3|bash|99-other:1.0|other|102|/repo|1699999999",
    ]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        output.stdout,
        concat!(
            "{\"command\":\"ls\",\"mode\":\"compact\",\"scope\":\"local\",\"json\":true,",
            "\"sessions\":[{\"session\":\"50-mawjs\",\"status\":\"active\",\"panes\":2,\"agents\":0,\"created\":200,\"lastActivityAgeSec\":1}]}\n"
        )
    );
}

#[test]
fn bring_text_rendering_and_json_escaping_edges_are_stable() {
    let text = run(&[
        "bring",
        "mawjs-features",
        "--engine",
        "codex",
        "--to",
        "50-mawjs:maw-js-1816",
        "--pick",
    ]);
    assert_eq!(text.code, 0, "{}", text.stderr);
    assert_eq!(
        text.stdout,
        concat!(
            "wake mawjs-features --split\n",
            "engine: codex\n",
            "session: 50-mawjs\n",
            "split-target: 50-mawjs:maw-js-1816\n",
            "pick: true\n"
        )
    );

    let escaped = run(&[
        "bring",
        "neo\"line",
        "--engine",
        "codex\\night",
        "--to",
        "sess\ttab:win\rname",
        "--plan-json",
    ]);
    assert_eq!(escaped.code, 0, "{}", escaped.stderr);
    assert_eq!(
        escaped.stdout,
        "{\"command\":\"bring\",\"opts\":{\"oracle\":\"neo\\\"line\",\"split\":true,\"engine\":\"codex\\\\night\",\"session\":\"sess\\ttab\",\"splitTarget\":\"sess\\ttab:win\\rname\"}}\n"
    );
}
