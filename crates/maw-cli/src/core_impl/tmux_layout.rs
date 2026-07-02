const DISPATCH_283: &[DispatcherEntry] = &[];

const TMUX_SUB_283: &[TmuxSubcommandEntry] = &[TmuxSubcommandEntry {
    names: &["layout"],
    handler: run_tmux_layout_command,
}];

const TMUX_LAYOUT_USAGE: &str = "usage: maw tmux layout <target> <preset>\n  presets: even-horizontal, even-vertical, main-horizontal, main-vertical, tiled";
const TMUX_LAYOUT_PRESETS: &[&str] = &[
    "even-horizontal",
    "even-vertical",
    "main-horizontal",
    "main-vertical",
    "tiled",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct TmuxLayoutOptions {
    target: String,
    preset: String,
}

fn run_tmux_layout_command(argv: &[String]) -> CliOutput {
    match tmux_layout_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err((code, message)) => CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn tmux_layout_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<String, (i32, String)> {
    let opts = tmux_layout_parse(argv)?;
    tmux_layout_validate_target(&opts.target).map_err(|message| (1, message))?;
    tmux_layout_validate_preset(&opts.preset)?;
    let window = tmux_layout_window_target(&opts.target);
    tmux_layout_validate_target(&window).map_err(|message| (1, message))?;
    let select_args = vec!["-t".to_owned(), window.clone(), opts.preset.clone()];
    runner
        .run("select-layout", &select_args)
        .map_err(|error| (1, format!("tmux layout: select-layout failed for '{window}': {}", error.message)))?;
    Ok(format!("✓ layout {} applied to {} → {}\n", opts.preset, opts.target, window))
}

fn tmux_layout_parse(argv: &[String]) -> Result<TmuxLayoutOptions, (i32, String)> {
    let mut positionals = Vec::new();
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" => return Err((0, TMUX_LAYOUT_USAGE.to_owned())),
            value if value.starts_with('-') => {
                return Err((2, format!("tmux layout: unknown argument {value}")));
            }
            value => positionals.push(value.to_owned()),
        }
    }
    match positionals.as_slice() {
        [target, preset] => Ok(TmuxLayoutOptions { target: target.clone(), preset: preset.clone() }),
        [] | [_] => Err((2, "tmux layout: target and preset required".to_owned())),
        _ => Err((2, "tmux layout: expected exactly <target> <preset>".to_owned())),
    }
}

fn tmux_layout_validate_preset(value: &str) -> Result<(), (i32, String)> {
    if TMUX_LAYOUT_PRESETS.contains(&value) {
        Ok(())
    } else {
        Err((2, format!("tmux layout: invalid layout '{value}'. Valid: {}", TMUX_LAYOUT_PRESETS.join(", "))))
    }
}

fn tmux_layout_window_target(target: &str) -> String {
    let Some((head, tail)) = target.rsplit_once('.') else { return target.to_owned(); };
    if !head.is_empty() && !tail.is_empty() && tail.bytes().all(|byte| byte.is_ascii_digit()) {
        head.to_owned()
    } else {
        target.to_owned()
    }
}

fn tmux_layout_validate_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') {
        return Err("tmux layout: target must be non-empty, unpadded, not '--', and not start with '-'".to_owned());
    }
    if value.chars().any(char::is_control) {
        return Err("tmux layout: target must not contain control characters".to_owned());
    }
    if !value.chars().all(tmux_layout_valid_target_char) {
        return Err("tmux layout: target contains unsupported characters".to_owned());
    }
    Ok(())
}

fn tmux_layout_valid_target_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.' | '/' | '@' | '%')
}

#[cfg(test)]
mod tmux_layout_tests {
    use super::*;

    #[derive(Default)]
    struct LayoutFakeRunner {
        calls: Vec<(String, Vec<String>)>,
    }

    impl maw_tmux::TmuxRunner for LayoutFakeRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            Ok(String::new())
        }
    }

    fn strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn tmux_layout_fragment_is_part283_only() {
        assert!(DISPATCH_283.is_empty());
        assert_eq!(TMUX_SUB_283.len(), 1);
        assert_eq!(TMUX_SUB_283[0].names, &["layout"]);
    }

    #[test]
    fn tmux_layout_uses_tmux_runner_arg_vector_and_strips_pane_suffix() {
        let mut runner = LayoutFakeRunner::default();
        let out = tmux_layout_with_runner(&strings(&["session:1.2", "tiled"]), &mut runner).expect("layout");
        assert_eq!(out, "✓ layout tiled applied to session:1.2 → session:1\n");
        assert_eq!(
            runner.calls,
            vec![("select-layout".to_owned(), strings(&["-t", "session:1", "tiled"]))]
        );
    }

    #[test]
    fn tmux_layout_allows_only_known_presets_before_runner() {
        let mut runner = LayoutFakeRunner::default();
        let err = tmux_layout_with_runner(&strings(&["session:1", "bad-layout"]), &mut runner).expect_err("preset guard");
        assert_eq!(err.0, 2);
        assert!(err.1.contains("invalid layout"));
        assert!(runner.calls.is_empty(), "invalid preset reached tmux: {:?}", runner.calls);
    }

    #[test]
    fn tmux_layout_rejects_bad_target_before_runner() {
        let mut runner = LayoutFakeRunner::default();
        let err = tmux_layout_with_runner(&strings(&["bad;target", "tiled"]), &mut runner).expect_err("target guard");
        assert_eq!(err.0, 1);
        assert!(err.1.contains("unsupported"));
        assert!(runner.calls.is_empty(), "bad target reached tmux: {:?}", runner.calls);
    }

    #[test]
    fn tmux_layout_help_reports_presets() {
        let mut runner = LayoutFakeRunner::default();
        let err = tmux_layout_with_runner(&strings(&["--help"]), &mut runner).expect_err("help");
        assert_eq!(err.0, 0);
        assert!(err.1.contains("even-horizontal"));
        assert!(runner.calls.is_empty());
    }
}
