use maw_cli::{run_cli, CliOutput};

fn run(args: &[&str]) -> CliOutput {
    run_cli(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
}

#[test]
fn top_level_a_alias_matches_attach_plan_json() {
    let alias = run(&["a", "50-mawjs", "--alive", "50-mawjs", "--plan-json"]);
    let attach = run(&["attach", "50-mawjs", "--alive", "50-mawjs", "--plan-json"]);
    assert_eq!(alias.code, 0, "{}", alias.stderr);
    assert_eq!(alias, attach);
    assert!(alias.stdout.contains("\"command\":\"attach\""));
    assert!(alias.stdout.contains("\"alias\":\"a\""));
    assert!(alias
        .stdout
        .contains("\"tmuxArgs\":[\"attach\",\"-t\",\"50-mawjs\"]"));
}

#[test]
fn attach_text_and_readonly_recovery_match_maw_js_surface() {
    let text = run(&["a", "50-mawjs:1.0", "--alive", "50-mawjs", "--print"]);
    assert_eq!(text.code, 0, "{}", text.stderr);
    assert!(text.stdout.contains("Run: tmux attach -t 50-mawjs"));
    assert!(text.stdout.contains("resolved: 50-mawjs:1.0 → 50-mawjs"));

    let readonly = run(&[
        "attach",
        "50-mawjs",
        "--alive",
        "50-mawjs",
        "--readonly",
        "--plan-json",
    ]);
    assert_eq!(readonly.code, 0, "{}", readonly.stderr);
    assert!(readonly
        .stdout
        .contains("\"tmuxArgs\":[\"attach\",\"-r\",\"-t\",\"50-mawjs\"]"));

    let missing = run(&["a", "ghost", "--alive", "50-mawjs", "--plan-json"]);
    assert_eq!(missing.code, 1);
    assert!(missing.stdout.contains("\"action\":\"recover\""));
}

#[test]
fn attach_usage_and_default_menu_show_only_live_ported_commands() {
    let usage = run(&[]);
    assert_eq!(usage.code, 0);
    assert!(usage.stdout.contains("ported commands:"));
    assert!(usage.stdout.contains("a|attach <target>"));
    assert!(usage.stdout.contains("ls [--compact|-c]"));
    assert!(!usage.stdout.contains("pair-api"));
    assert!(!usage.stdout.contains("plugin-manifest"));

    let help = run(&["a", "--help"]);
    assert_eq!(help.code, 0);
    assert!(help.stdout.contains("maw-rs a <target>"));

    let error = run(&["a", "--bad"]);
    assert_eq!(error.code, 2);
    assert!(error.stderr.contains("attach: unknown argument --bad"));
}

#[test]
fn attach_parser_rejects_missing_and_duplicate_targets() {
    let missing_alive = run(&["a", "--alive"]);
    assert_eq!(missing_alive.code, 2);
    assert!(missing_alive
        .stderr
        .contains("attach: missing --alive value"));

    let duplicate = run(&["attach", "50-mawjs", "51-maw-js"]);
    assert_eq!(duplicate.code, 2);
    assert!(duplicate.stderr.contains("attach: target already provided"));
}

#[test]
fn attach_alive_equals_and_text_recover_paths_are_covered() {
    let alive_equals = run(&["a", "50-mawjs", "--alive=50-mawjs", "--plan-json"]);
    assert_eq!(alive_equals.code, 0, "{}", alive_equals.stderr);
    assert!(alive_equals.stdout.contains("\"action\":\"print\""));

    let recover_text = run(&["a", "ghost", "--alive=50-mawjs"]);
    assert_eq!(recover_text.code, 1);
    assert!(recover_text
        .stdout
        .contains("attach: 'ghost' resolved to missing session ghost"));
    assert!(recover_text.stdout.contains("maw wake ghost --attach"));
}

#[test]
fn attach_resolves_numbered_fuzzy_sessions_and_prefers_exact() {
    let fuzzy = run(&["a", "volt", "--alive=05-volt", "--plan-json"]);
    assert_eq!(fuzzy.code, 0, "{}", fuzzy.stderr);
    assert!(fuzzy.stdout.contains("\"target\":\"volt\""));
    assert!(fuzzy.stdout.contains("\"session\":\"05-volt\""));
    assert!(fuzzy
        .stdout
        .contains("\"tmuxArgs\":[\"attach\",\"-t\",\"05-volt\"]"));

    let exact = run(&[
        "a",
        "volt",
        "--alive=05-volt",
        "--alive=volt",
        "--plan-json",
    ]);
    assert_eq!(exact.code, 0, "{}", exact.stderr);
    assert!(exact.stdout.contains("\"session\":\"volt\""));
}

#[test]
fn attach_refuses_ambiguous_fuzzy_session_matches() {
    let ambiguous = run(&[
        "a",
        "call",
        "--alive=05-calliope",
        "--alive=06-caller",
        "--plan-json",
    ]);
    assert_eq!(ambiguous.code, 2);
    assert!(ambiguous
        .stderr
        .contains("attach: 'call' matches multiple sessions"));
    assert!(ambiguous.stderr.contains("05-calliope, 06-caller"));
}

#[test]
fn attach_plan_covers_missing_target_and_non_print_attach_text() {
    let attach = run(&["a", "50-mawjs", "--alive", "50-mawjs"]);
    assert_eq!(attach.code, 0, "{}", attach.stderr);
    assert!(attach.stdout.contains("Run: tmux attach -t 50-mawjs"));

    let missing_target = run(&["attach", "--plan-json"]);
    assert_eq!(missing_target.code, 2);
    assert!(missing_target.stderr.contains("attach: target required"));

    let readonly_print = run(&[
        "a",
        "50-mawjs",
        "--alive",
        "50-mawjs",
        "--readonly",
        "--plan-json",
    ]);
    assert_eq!(readonly_print.code, 0, "{}", readonly_print.stderr);
    assert!(readonly_print
        .stdout
        .contains("\"tmuxArgs\":[\"attach\",\"-r\",\"-t\",\"50-mawjs\"]"));
}

#[test]
fn attach_without_fake_alive_uses_live_probe_recover_path() {
    let missing = run(&[
        "a",
        "unlikely-goal-coverage-session-zz-1850",
        "--print",
        "--plan-json",
    ]);
    assert_eq!(missing.code, 1, "{}{}", missing.stdout, missing.stderr);
    assert!(missing.stdout.contains("\"action\":\"recover\""));
}
