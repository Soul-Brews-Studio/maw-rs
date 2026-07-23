// Issue #608: token/kill/bring/feed/calver must answer --help/-h with
// exit 0 + usage on stdout, matching the other curated verbs. Existing
// error paths (unknown flags, missing arguments) stay unchanged.

use maw_cli::run_cli;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn assert_help(argv: &[&str], needle: &str) {
    let output = run_cli(&args(argv));
    assert_eq!(output.code, 0, "{argv:?} stderr: {}", output.stderr);
    assert!(
        output.stdout.contains(needle),
        "{argv:?} stdout: {}",
        output.stdout
    );
    assert!(
        output.stderr.is_empty(),
        "{argv:?} stderr: {}",
        output.stderr
    );
}

#[test]
fn token_help_flag_prints_usage_and_exits_zero() {
    assert_help(
        &["token", "--help"],
        "usage: maw token <list|use|current|resolve|apply|save|load|scan>",
    );
    assert_help(&["token", "-h"], "usage: maw token");
}

#[test]
fn token_unknown_flag_still_errors() {
    let output = run_cli(&args(&["token", "--bogus"]));
    assert_eq!(output.code, 1);
    assert!(
        output.stderr.contains("maw token: unknown flag --bogus"),
        "{}",
        output.stderr
    );
}

#[test]
fn kill_help_flag_prints_usage_and_exits_zero() {
    assert_help(&["kill", "--help"], "usage: maw kill <target>[:window]");
    assert_help(&["kill", "-h"], "usage: maw kill");
}

#[test]
fn kill_flag_like_target_still_errors() {
    let output = run_cli(&args(&["kill", "--bogus"]));
    assert_eq!(output.code, 1);
    assert!(
        output.stderr.contains("looks like a flag, not a target"),
        "{}",
        output.stderr
    );
}

#[test]
fn bring_help_flag_prints_usage_and_exits_zero() {
    assert_help(&["bring", "--help"], "usage: maw bring <oracle>");
    assert_help(&["bring", "-h"], "usage: maw bring <oracle>");
    assert_help(&["b", "--help"], "usage: maw bring <oracle>");
}

#[test]
fn bring_missing_oracle_still_errors() {
    let output = run_cli(&args(&["bring", "--tab"]));
    assert_eq!(output.code, 2);
    assert!(
        output.stderr.contains("bring: missing oracle name"),
        "{}",
        output.stderr
    );
}

#[test]
fn feed_help_flag_prints_usage_and_exits_zero() {
    assert_help(&["feed", "--help"], "usage: maw-rs feed parse-line <line>");
    assert_help(&["feed", "-h"], "usage: maw-rs feed");
}

#[test]
fn feed_parse_line_still_accepts_help_looking_value() {
    // parse-line consumes the next arg as its value, so a literal "--help"
    // line is parsed (and rejected as unparseable, code 1), not intercepted
    // as a help request.
    let output = run_cli(&args(&["feed", "parse-line", "--help"]));
    assert_eq!(output.code, 1, "stderr: {}", output.stderr);
    assert!(!output.stdout.contains("usage:"), "{}", output.stdout);
    assert!(output.stderr.is_empty(), "{}", output.stderr);
}

#[test]
fn feed_unknown_argument_still_errors() {
    let output = run_cli(&args(&["feed", "--bogus"]));
    assert_eq!(output.code, 2);
    assert!(
        output.stderr.contains("feed: unknown argument --bogus"),
        "{}",
        output.stderr
    );
}

#[test]
fn calver_help_flag_prints_usage_and_exits_zero() {
    assert_help(
        &["calver", "--help"],
        "usage: maw-rs calver --now <YYYY-M-DTHH:MM>",
    );
    assert_help(&["calver", "-h"], "usage: maw-rs calver");
}

#[test]
fn calver_unknown_argument_still_errors() {
    let output = run_cli(&args(&["calver", "--bogus"]));
    assert_eq!(output.code, 2);
    assert!(
        output.stderr.contains("calver: unknown argument --bogus"),
        "{}",
        output.stderr
    );
}

#[test]
fn help_all_tiered_output_preserves_registered_count_and_descriptions() {
    let output = run_cli(&args(&["help", "--all"]));
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(output.stderr.is_empty(), "stderr: {}", output.stderr);
    let text = output.stdout;
    assert!(text.starts_with("registered commands (197):"), "{text}");
    assert!(text.contains("\ncore (40):\n"), "{text}");
    // Per-tier counts shift as the fan-out describes more verbs; assert tier
    // presence (count-agnostic) rather than a brittle hardcoded number.
    for tier in ["standard", "extra", "other"] {
        assert!(
            text.contains(&format!("\n{tier} (")),
            "missing {tier} tier in:\n{text}"
        );
    }
    assert!(
        text.contains("  maw wake                      Launch or reuse an oracle engine pane;"),
        "{text}"
    );
    assert!(
        text.contains("  maw send-enter                Press Enter in a target pane;"),
        "{text}"
    );
    assert!(
        text.contains("  maw about                     Show oracle details"),
        "about should render as a described Standard row:\n{text}"
    );
    assert!(
        text.contains("  maw consent-constants\n"),
        "internal consent-constants should remain a name-only Other row:\n{text}"
    );
}
