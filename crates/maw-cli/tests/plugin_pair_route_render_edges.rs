use maw_cli::{run_cli, CliOutput};

fn run(args: &[&str]) -> CliOutput {
    run_cli(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
}

fn usage(args: &[&str], expected: &str) {
    let output = run(args);
    assert_eq!(output.code, 2, "stdout for {args:?}: {}", output.stdout);
    assert!(
        output.stderr.contains(expected),
        "stderr for {args:?} did not contain {expected:?}: {}",
        output.stderr
    );
}

fn ok(args: &[&str]) -> CliOutput {
    let output = run(args);
    assert_eq!(output.code, 0, "stderr for {args:?}: {}", output.stderr);
    assert!(
        output.stderr.is_empty(),
        "stderr for {args:?}: {}",
        output.stderr
    );
    output
}

#[test]
fn plugin_manifest_and_bind_host_parser_edges_are_reported() {
    for (args, expected) in [
        (
            &["plugin-manifest"][..],
            "plugin-manifest: expected parse or load",
        ),
        (
            &["plugin-manifest", "bogus"],
            "plugin-manifest: unknown subcommand bogus",
        ),
        (
            &["plugin-manifest", "parse", "--bogus"],
            "plugin-manifest parse: unknown argument --bogus",
        ),
        (
            &["plugin-manifest", "load", "--bogus"],
            "plugin-manifest load: unknown argument --bogus",
        ),
        (
            &["plugin-manifest", "invoke", "--bogus"],
            "plugin-manifest invoke: unknown argument --bogus",
        ),
        (
            &["plugin-manifest", "invoke"],
            "plugin-manifest invoke: --scan-dir is required",
        ),
        (
            &[
                "plugin-manifest",
                "import-symbol",
                "--scan-dir",
                ".",
                "--module-symbol",
                "bad",
            ],
            "--module-symbol must be name=value",
        ),
        (
            &["plugin-manifest", "discover", "--plugin"],
            "plugin-manifest discover: unknown argument --plugin",
        ),
        (
            &["bind-host", "--config-peers-len"],
            "bind-host: missing --config-peers-len value",
        ),
        (
            &["bind-host", "--config-named-peers-len"],
            "bind-host: missing --config-named-peers-len value",
        ),
        (
            &["bind-host", "--maw-host"],
            "bind-host: missing --maw-host value",
        ),
        (
            &["bind-host", "--peers-store-len"],
            "bind-host: missing --peers-store-len value",
        ),
        (
            &["bind-host", "--peers-store-error"],
            "bind-host: missing --peers-store-error value",
        ),
        (
            &["bind-host", "--unknown"],
            "bind-host: unknown argument --unknown",
        ),
    ] {
        usage(args, expected);
    }

    let text = ok(&["bind-host", "--maw-host", "0.0.0.0"]);
    assert_eq!(text.stdout, "0.0.0.0\n");
}

#[test]
fn pair_api_parser_and_text_tails_are_stable() {
    usage(
        &[
            "pair-api", "generate", "--code", "ABCDEF", "--now", "1", "--ttl-ms",
        ],
        "missing --ttl-ms value",
    );
    usage(
        &[
            "pair-api", "generate", "--code", "ABCDEF", "--now", "1", "--ttl-ms", "nope",
        ],
        "--ttl-ms must be a non-negative integer",
    );
    usage(
        &[
            "pair-api",
            "generate",
            "--code",
            "ABCDEF",
            "--now",
            "1",
            "--seed-code",
        ],
        "missing --seed-code value",
    );
    usage(
        &[
            "pair-api",
            "generate",
            "--code",
            "ABCDEF",
            "--now",
            "1",
            "--remote-node",
        ],
        "missing --remote-node value",
    );
    usage(
        &[
            "pair-api",
            "generate",
            "--code",
            "ABCDEF",
            "--now",
            "1",
            "--remote-url",
        ],
        "missing --remote-url value",
    );
    usage(
        &[
            "pair-api",
            "generate",
            "--code",
            "ABCDEF",
            "--now",
            "1",
            "--seed-accepted",
        ],
        "missing --seed-accepted value",
    );
    usage(
        &[
            "pair-api", "generate", "--code", "ABCDEF", "--now", "1", "--bad",
        ],
        "unknown argument --bad",
    );
}

#[test]
fn pair_api_text_tails_are_stable() {
    assert!(ok(&[
        "pair-api",
        "generate",
        "--code",
        "ABCDEF",
        "--now",
        "1",
        "--node",
        "m5",
        "--oracle",
        "mawjs",
        "--port",
        "8787",
        "--base-url",
        "http://127.0.0.1:8787",
        "--federation-token",
        "token",
        "--pubkey",
        "pub"
    ])
    .stdout
    .contains("pair-api generate status="));
    assert!(ok(&[
        "pair-api",
        "probe",
        "--code",
        "ABCDEF",
        "--now",
        "1",
        "--node",
        "m5",
        "--oracle",
        "mawjs",
        "--port",
        "8787",
        "--base-url",
        "http://127.0.0.1:8787",
        "--federation-token",
        "token",
        "--pubkey",
        "pub"
    ])
    .stdout
    .contains("pair-api probe status="));
    assert!(ok(&[
        "pair-api",
        "accept",
        "--code",
        "ABCDEF",
        "--now",
        "1",
        "--node",
        "m5",
        "--oracle",
        "mawjs",
        "--port",
        "8787",
        "--base-url",
        "http://127.0.0.1:8787",
        "--federation-token",
        "token",
        "--pubkey",
        "pub",
        "--remote-node",
        "mba",
        "--remote-url",
        "http://mba"
    ])
    .stdout
    .contains("pair-api accept status="));
}

#[test]
fn route_worktree_calver_and_normalize_tails_are_stable() {
    usage(&["route", "--query"], "route: missing --query value");
    usage(&["route", "--node"], "route: missing --node value");
    usage(
        &["route", "--query", "neo", "--peer"],
        "route: missing --peer value",
    );
    usage(
        &["route", "--query", "neo", "--agent", "bad"],
        "route: --agent must use <agent=node>",
    );
    assert!(ok(&["route", "constants"])
        .stdout
        .contains("route constants"));

    usage(
        &[
            "worktree-window",
            "--main-repo-name",
            "repo",
            "--wt-name",
            "1-x",
            "--session",
        ],
        "missing --session value",
    );
    usage(
        &[
            "worktree-window",
            "--main-repo-name",
            "repo",
            "--wt-name",
            "1-x",
            "--window",
            "1:a:true",
        ],
        "--window must follow a --session",
    );
    assert!(ok(&[
        "worktree-window",
        "--main-repo-name",
        "repo",
        "--wt-name",
        "9-none"
    ])
    .stdout
    .contains("none"));

    usage(
        &["calver", "--now", "2026-5-21T10:00:extra"],
        "--now time must use HH:MM",
    );
    assert!(ok(&["calver", "constants"])
        .stdout
        .contains("calver constants"));
    assert_eq!(ok(&["normalize", "  repo/.git///"]).stdout, "repo\n");
    assert!(ok(&["normalize", "constants", "--plan-json"])
        .stdout
        .contains("strip-trailing-dot-git"));
}

#[test]
fn ls_and_bring_render_remaining_edges_are_stable() {
    let peer_json = ok(&[
        "ls",
        "remote",
        "--json",
        "--now",
        "1000",
        "--pane",
        "%1|codex|remote:1.0|agent|100|/repo|990",
        "--pane",
        "%2|zsh|other:1.0|other|101|/repo|990",
    ]);
    assert_eq!(
        peer_json.stdout,
        "{\"command\":\"ls\",\"mode\":\"compact\",\"scope\":\"local\",\"json\":true,\"sessions\":[{\"session\":\"remote\",\"status\":\"active\",\"panes\":1,\"agents\":1}]}\n"
    );

    assert_eq!(
        ok(&[
            "ls",
            "--active=5s",
            "--pane",
            "%1|zsh|plain:1.0|shell|1|/repo|1",
            "nomatch"
        ])
        .stdout,
        "No sessions active in the last 5s.\n"
    );

    let compact = ok(&[
        "ls",
        "--all",
        "--now",
        "1000",
        "--pane",
        "%1|bash|active:1.0|agent|1|/repo|999",
        "--pane",
        "%2|bash|idle:1.0|shell|2|/repo|880",
        "--pane",
        "%3|bash|stale:1.0|shell|3|/repo|1",
        "--pane",
        "%4|bash|unknown:1.0|shell|4|/repo|-",
    ]);
    assert!(compact.stdout.contains("active"), "{}", compact.stdout);
    assert!(compact.stdout.contains("idle"), "{}", compact.stdout);
    assert!(compact.stdout.contains("stale"), "{}", compact.stdout);
    assert!(compact.stdout.contains("unknown"), "{}", compact.stdout);

    let verbose = ok(&[
        "ls",
        "--verbose",
        "--now",
        "1000",
        "--pane",
        "%1|bash|active:1.0|agent|1|/repo|999",
    ]);
    assert!(verbose.stdout.contains("active"), "{}", verbose.stdout);
    assert!(verbose.stdout.contains("\n  "), "{}", verbose.stdout);
    assert!(verbose.stdout.contains("1.0"), "{}", verbose.stdout);

    usage(&["ls", "--unknown"], "ls: unknown argument --unknown");
    usage(&["bring"], "bring: missing oracle");

    let bring = ok(&[
        "bring",
        "neo\nline",
        "--engine",
        "codex\tengine",
        "--plan-json",
    ]);
    assert!(bring.stdout.contains("neo\\nline"), "{}", bring.stdout);
    assert!(bring.stdout.contains("codex\\tengine"), "{}", bring.stdout);
}
