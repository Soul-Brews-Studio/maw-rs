#![allow(clippy::too_many_lines)]

use maw_cli::{run_cli, CliOutput};

fn run(args: &[&str]) -> CliOutput {
    run_cli(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
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

fn assert_ok_exact(args: &[&str], expected: &str) {
    let output = run(args);
    assert_eq!(output.code, 0, "stderr for {args:?}: {}", output.stderr);
    assert_eq!(output.stdout, expected, "stdout for {args:?}");
    assert!(
        output.stderr.is_empty(),
        "stderr for {args:?}: {}",
        output.stderr
    );
}

fn assert_ok_contains(args: &[&str], expected: &str) {
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
fn worktree_calver_normalize_and_resolve_tail_branches_are_covered() {
    assert_usage(
        &["worktree-window", "--main-repo-name", "mawjs-oracle"],
        "worktree-window: expected --wt-name <worktree>",
    );
    assert_usage(
        &[
            "worktree-window",
            "--main-repo-name",
            "mawjs-oracle",
            "--wt-name",
            "1-feature",
            "--session",
            "mawjs",
            "--window",
            "1",
        ],
        "worktree-window: window must use <index:name:active>",
    );
    assert_ok_exact(
        &["worktree-window", "constants"],
        "worktree-window constants results=bound,ambiguous,none window-shape=index:name:active\n",
    );
    assert_ok_exact(
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
        (&["calver", "--now"][..], "calver: missing --now value"),
        (
            &["calver", "--now", "2026-5-21T10:00", "--package-version"][..],
            "calver: missing --package-version value",
        ),
        (
            &["calver", "--now", "2026-5-21T10:00", "--tag"][..],
            "calver: missing --tag value",
        ),
        (
            &["calver", "--now", "2026-5-21T10:00", "--wat"][..],
            "calver: unknown argument --wat",
        ),
        (&["calver"][..], "calver: expected --now <YYYY-M-DTHH:MM>"),
        (
            &["calver", "--now", "2026-5-21T10:00:30"][..],
            "calver: --now time must use HH:MM",
        ),
        (
            &["calver", "--now", "2026-5"][..],
            "calver: --now must use YYYY-M-DTHH:MM",
        ),
        (
            &["calver", "--now", "2026-5T10:00"][..],
            "calver: missing day in --now",
        ),
        (
            &["calver", "--now", "2026-5-21T10"][..],
            "calver: missing minute in --now",
        ),
    ] {
        assert_usage(args, expected);
    }
    assert_ok_exact(
        &["calver", "constants"],
        "calver constants base=YY.M.D channels=alpha,beta stamp=H*100+M max=2359\n",
    );
    assert_ok_exact(&["normalize", "path/.git///"], "path\n");
    assert_ok_exact(
        &["normalize", "constants"],
        "normalize constants steps=trim,strip-trailing-slashes,strip-trailing-dot-git-until-stable\n",
    );

    assert_usage(&["resolve", "--mode"], "resolve: missing --mode value");
    assert_ok_exact(
        &["resolve", "constants"],
        "resolve constants modes=by-name,session,worktree results=exact,fuzzy,ambiguous,none\n",
    );
    assert_ok_exact(
        &["resolve", "--mode", "by-name", "ghost", "alpha", "beta"],
        "resolve by-name ghost: none\n",
    );
}

#[test]
fn ls_parser_text_json_and_duration_branches_are_covered() {
    for (args, expected) in [
        (&["ls", "--active=0s"][..], "ls: invalid --active duration"),
        (&["ls", "--pane"][..], "ls: missing --pane value"),
        (&["ls", "--pane", "bad"][..], "ls: --pane must use"),
        (&["ls", "--now"][..], "ls: missing --now value"),
        (&["ls", "--now", "nan"][..], "ls: --now must be an integer"),
        (
            &["ls", "--session-created"][..],
            "ls: missing --session-created value",
        ),
        (
            &["ls", "--session-created", "bad"][..],
            "ls: --session-created must use <session=epoch_seconds>",
        ),
        (
            &["ls", "--session-created", "s=nan"][..],
            "ls: session-created epoch must be an integer",
        ),
    ] {
        assert_usage(args, expected);
    }

    let filtered = run(&[
        "ls",
        "remote-node",
        "--now",
        "200000",
        "--pane",
        "%1|codex|remote-node:1.0|agent|100|/repo|199990",
        "--pane",
        "%2|zsh|other:1.0|other|101|/repo|199990",
    ]);
    assert_eq!(filtered.code, 0, "{}", filtered.stderr);
    assert!(
        filtered.stdout.contains("remote-node"),
        "{}",
        filtered.stdout
    );
    assert!(!filtered.stdout.contains("other"), "{}", filtered.stdout);
    assert!(!filtered.stdout.contains("no fake sessions"));
    assert_ok_exact(
        &[
            "ls",
            "remote-node",
            "--json",
            "--now",
            "200000",
            "--pane",
            "%1|codex|remote-node:1.0|agent|100|/repo|199990",
            "--pane",
            "%2|zsh|other:1.0|other|101|/repo|199990",
        ],
        "{\"command\":\"ls\",\"mode\":\"compact\",\"scope\":\"local\",\"json\":true,\"sessions\":[{\"session\":\"remote-node\",\"status\":\"active\",\"panes\":1,\"agents\":1}]}\n",
    );
    assert_ok_exact(
        &[
            "ls",
            "--active=2d",
            "--json",
            "--all",
            "--now",
            "200000",
            "--pane",
            "%1|zsh|mawjs-discord:1.0|channel|100|/tmp|199990",
            "--pane",
            "%2|codex|7-mawjs:2.0|agent|101|/repo|199701",
            "--pane",
            "%3|zsh|8-pulse:1.0|stale|102|/repo|100000",
        ],
        "{\"command\":\"ls\",\"mode\":\"compact\",\"scope\":\"local\",\"json\":true,\"activeThresholdSec\":172800,\"sessions\":[{\"session\":\"7-mawjs\",\"status\":\"idle\",\"panes\":1,\"agents\":1},{\"session\":\"8-pulse\",\"status\":\"stale\",\"panes\":1,\"agents\":0}]}\n",
    );
    assert_ok_exact(
        &[
            "ls",
            "--json",
            "--all",
            "--recent",
            "--now",
            "200000",
            "--session-created",
            "old=100",
            "--session-created",
            "new=200",
            "--pane",
            "%1|zsh|old:1.0|old|100|/repo|199990",
            "--pane",
            "%2|node|new:1.0|new|101|/repo|199995",
        ],
        "{\"command\":\"ls\",\"mode\":\"compact\",\"scope\":\"local\",\"json\":true,\"sessions\":[{\"session\":\"new\",\"status\":\"active\",\"panes\":1,\"agents\":1,\"created\":200,\"lastActivityAgeSec\":5},{\"session\":\"old\",\"status\":\"active\",\"panes\":1,\"agents\":0,\"created\":100,\"lastActivityAgeSec\":10}]}\n",
    );

    let no_active = run(&[
        "ls",
        "--active",
        "30s",
        "--all",
        "--now",
        "200000",
        "--pane",
        "%1|zsh|stale:1.0|stale|100|/repo|199000",
    ]);
    assert_eq!(no_active.code, 0, "{}", no_active.stderr);
    assert!(no_active
        .stdout
        .contains("No sessions active in the last 30s."));

    let verbose = run(&[
        "ls",
        "--verbose",
        "--channels",
        "--all",
        "--now",
        "200000",
        "--pane",
        "%1|zsh|1-mawjs:1.0|title|100|/repo|199940",
    ]);
    assert_eq!(verbose.code, 0, "{}", verbose.stderr);
    assert!(verbose.stdout.contains("1-mawjs"), "{}", verbose.stdout);
    assert!(verbose.stdout.contains("\n  "), "{}", verbose.stdout);
    assert!(verbose.stdout.contains("1.0"), "{}", verbose.stdout);
    assert!(verbose.stdout.contains("1m"), "{}", verbose.stdout);

    assert_ok_contains(
        &[
            "ls",
            "--all",
            "--now",
            "200000",
            "--pane",
            "%1|codex|1-mawjs:1.0|agent|100|/repo|199990",
            "--pane",
            "%2|zsh|1-mawjs:2.0|shell|101|/repo|199940",
        ],
        "1-mawjs",
    );
}

#[test]
fn bring_text_and_json_escaping_branches_are_covered() {
    assert_ok_exact(
        &[
            "bring",
            "neo",
            "--engine",
            "codex",
            "--to",
            "50-mawjs:win\\tab\rname",
            "--pick",
        ],
        "wake neo --split\nengine: codex\nsession: 50-mawjs\nsplit-target: 50-mawjs:win\\tab\rname\npick: true\n",
    );
    assert_ok_exact(
        &[
            "bring",
            "neo",
            "--engine",
            "codex",
            "--to",
            "50-mawjs:win\\tab\rname",
            "--pick",
            "--plan-json",
        ],
        "{\"command\":\"bring\",\"opts\":{\"oracle\":\"neo\",\"split\":true,\"engine\":\"codex\",\"pick\":true,\"session\":\"50-mawjs\",\"splitTarget\":\"50-mawjs:win\\\\tab\\rname\"}}\n",
    );
}
