use super::*;

#[test]
fn dotted_window_names_are_not_pane_suffixes() {
    assert_eq!(strip_numeric_pane_suffix("s:oracle.v2"), "s:oracle.v2");
    assert_eq!(strip_numeric_pane_suffix("s:oracle.12"), "s:oracle");
}

#[test]
fn bring_args_parse_to_session_and_window_target() {
    let parsed = parse_bring_args(&[
        "pulse".to_owned(),
        "--to".to_owned(),
        "work:3".to_owned(),
        "--engine".to_owned(),
        "codex".to_owned(),
        "--pick".to_owned(),
    ])
    .expect("bring args parse");

    assert_eq!(parsed.oracle, "pulse");
    assert_eq!(parsed.opts.session.as_deref(), Some("work"));
    assert_eq!(parsed.opts.split_target.as_deref(), Some("work:3"));
    assert_eq!(parsed.opts.engine.as_deref(), Some("codex"));
    assert!(parsed.opts.pick);
}

#[test]
fn bring_args_tolerate_trailing_to_without_value() {
    let parsed = parse_bring_args(&["pulse".to_owned(), "--to".to_owned()])
        .expect("trailing --to is left for downstream parsing");

    assert_eq!(parsed.oracle, "pulse");
    assert_eq!(parsed.opts.session, None);
    assert_eq!(parsed.opts.split_target, None);
}

#[test]
fn bring_args_report_missing_oracle_inside_lib_instantiation() {
    let err = parse_bring_args(&["--pick".to_owned()]).expect_err("oracle is required");

    assert_eq!(err.message, "bring: missing oracle name");
    assert_eq!(err.usage, bring_usage_lines());
    assert_eq!(
        translate_bring_to_flag(&["--to".to_owned(), "work:agent".to_owned()]),
        ["--session", "work", "--split-target", "work:agent"]
    );
}
