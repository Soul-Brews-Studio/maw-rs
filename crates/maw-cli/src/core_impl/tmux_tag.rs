const DISPATCH_82: &[DispatcherEntry] = &[DispatcherEntry {
    command: "tag",
    handler: Handler::Sync(tag_run_command),
}];

const TAG_USAGE: &str = "usage: maw tag <target> [--pane N] [--title <text>] [--meta key=val]";
const TAG_WINDOW_FORMAT: &str = "#{session_name}\t#{window_index}";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TagOptions {
    target: String,
    pane: Option<u32>,
    title: Option<String>,
    meta: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TagSession {
    name: String,
    windows: Vec<u32>,
}

trait TagTmux {
    fn tag_list_sessions(&mut self) -> Result<Vec<TagSession>, String>;
    fn tag_display_title(&mut self, target: &str) -> Result<String, String>;
    fn tag_show_options(&mut self, target: &str) -> Result<String, String>;
    fn tag_set_title(&mut self, target: &str, title: &str) -> Result<(), String>;
    fn tag_set_meta(&mut self, target: &str, key: &str, value: &str) -> Result<(), String>;
}

struct TagSystemTmux {
    runner: maw_tmux::CommandTmuxRunner,
}

impl TagSystemTmux {
    fn tag_new() -> Self {
        Self {
            runner: maw_tmux::CommandTmuxRunner::new(),
        }
    }
}

impl TagTmux for TagSystemTmux {
    fn tag_list_sessions(&mut self) -> Result<Vec<TagSession>, String> {
        tag_tmux_run(
            &mut self.runner,
            "list-windows",
            &["-a", "-F", TAG_WINDOW_FORMAT],
        )
        .map(|raw| tag_parse_sessions(&raw))
    }

    fn tag_display_title(&mut self, target: &str) -> Result<String, String> {
        tag_validate_tmux_target(target)?;
        tag_tmux_run(
            &mut self.runner,
            "display-message",
            &["-p", "-t", target, "#{pane_title}"],
        )
    }

    fn tag_show_options(&mut self, target: &str) -> Result<String, String> {
        tag_validate_tmux_target(target)?;
        tag_tmux_run(&mut self.runner, "show-options", &["-p", "-t", target])
    }

    fn tag_set_title(&mut self, target: &str, title: &str) -> Result<(), String> {
        tag_validate_tmux_target(target)?;
        tag_validate_label(title, "title")?;
        tag_tmux_run(&mut self.runner, "select-pane", &["-t", target, "-T", title]).map(|_| ())
    }

    fn tag_set_meta(&mut self, target: &str, key: &str, value: &str) -> Result<(), String> {
        tag_validate_tmux_target(target)?;
        tag_validate_option_key(key)?;
        tag_validate_label(value, "meta value")?;
        tag_tmux_run(
            &mut self.runner,
            "set-option",
            &["-p", "-t", target, key, value],
        )
        .map(|_| ())
    }
}

fn tag_run_command(argv: &[String]) -> CliOutput {
    tag_run_command_with(argv, &mut TagSystemTmux::tag_new())
}

fn tag_run_command_with(argv: &[String], tmux: &mut impl TagTmux) -> CliOutput {
    match tag_run(argv, tmux) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn tag_run(argv: &[String], tmux: &mut impl TagTmux) -> Result<String, String> {
    let options = tag_parse_args(argv)?;
    tag_validate_user_target(&options.target)?;
    tag_validate_parse_target(&options.target)?;
    tag_validate_options(&options)?;
    let sessions = tmux.tag_list_sessions()?;
    let target = tag_resolve_target(&options, &sessions)?;
    tag_validate_tmux_target(&target)?;
    if tag_is_read(&options) {
        tag_read(tmux, &target)
    } else {
        tag_write(tmux, &target, &options)
    }
}

fn tag_parse_args(argv: &[String]) -> Result<TagOptions, String> {
    let mut options = TagOptions::default();
    let mut index = 0;
    while index < argv.len() {
        index += tag_parse_arg(argv, index, &mut options)?;
    }
    if options.target.is_empty() || options.target == "--help" || options.target == "-h" {
        return Err(TAG_USAGE.to_owned());
    }
    Ok(options)
}

fn tag_parse_arg(
    argv: &[String],
    index: usize,
    options: &mut TagOptions,
) -> Result<usize, String> {
    let arg = argv[index].as_str();
    match arg {
        "--" => Err("tag: -- separator is not allowed for tmux targets or labels".to_owned()),
        "--pane" => tag_parse_value_flag(argv, index, "--pane", |value| {
            options.pane = Some(tag_parse_non_negative(value, "--pane")?);
            Ok(())
        }),
        "--title" => tag_parse_value_flag(argv, index, "--title", |value| {
            options.title = Some(value.to_owned());
            Ok(())
        }),
        "--meta" => tag_parse_value_flag(argv, index, "--meta", |value| {
            options.meta.push(value.to_owned());
            Ok(())
        }),
        value if value.starts_with("--pane=") => {
            options.pane = Some(tag_parse_non_negative(&value[7..], "--pane")?);
            Ok(1)
        }
        value if value.starts_with("--title=") => {
            options.title = Some(value[8..].to_owned());
            Ok(1)
        }
        value if value.starts_with("--meta=") => {
            options.meta.push(value[7..].to_owned());
            Ok(1)
        }
        value if value.starts_with('-') => Err(tag_flag_like_target(value)),
        value => tag_set_target(options, value),
    }
}

fn tag_parse_value_flag<F>(
    argv: &[String],
    index: usize,
    flag: &str,
    mut assign: F,
) -> Result<usize, String>
where
    F: FnMut(&str) -> Result<(), String>,
{
    let value = argv
        .get(index + 1)
        .ok_or_else(|| format!("tag: missing {flag} value"))?;
    if value == "--" || value.starts_with('-') {
        return Err(format!("tag: {flag} value must not start with '-'"));
    }
    assign(value)?;
    Ok(2)
}

fn tag_set_target(options: &mut TagOptions, value: &str) -> Result<usize, String> {
    if !options.target.is_empty() {
        return Err(format!("tag: unexpected argument {value}"));
    }
    value.clone_into(&mut options.target);
    Ok(1)
}

fn tag_parse_non_negative(value: &str, flag: &str) -> Result<u32, String> {
    if value.is_empty() || value == "--" || value.starts_with('-') {
        return Err(format!("tag: {flag} requires a non-negative number"));
    }
    value
        .parse::<u32>()
        .map_err(|_| format!("tag: {flag} requires a non-negative number"))
}

fn tag_flag_like_target(value: &str) -> String {
    format!("\"{value}\" looks like a flag, not a target.\n  usage: maw tag <target> ...")
}


fn tag_validate_parse_target(target: &str) -> Result<(), String> {
    let (raw_session, raw_window) = tag_split_target(target)?;
    tag_validate_user_target(&raw_session)?;
    if let Some(window) = raw_window {
        tag_validate_target_segment(&window, "window")?;
    }
    Ok(())
}

fn tag_validate_options(options: &TagOptions) -> Result<(), String> {
    if let Some(title) = &options.title {
        tag_validate_label(title, "title")?;
    }
    for item in &options.meta {
        let (key, value) = tag_split_meta(item)?;
        tag_validate_option_key(&key)?;
        tag_validate_label(&value, "meta value")?;
    }
    Ok(())
}

fn tag_is_read(options: &TagOptions) -> bool {
    options.title.is_none() && options.meta.is_empty()
}

fn tag_read(tmux: &mut impl TagTmux, target: &str) -> Result<String, String> {
    let title = tmux
        .tag_display_title(target)
        .map_err(|error| format!("read failed: {error}"))?;
    let options = tmux.tag_show_options(target).unwrap_or_default();
    Ok(tag_render_read(target, title.trim(), &options))
}

fn tag_write(
    tmux: &mut impl TagTmux,
    target: &str,
    options: &TagOptions,
) -> Result<String, String> {
    let mut output = String::new();
    if let Some(title) = &options.title {
        tmux.tag_set_title(target, title)
            .map_err(|error| format!("select-pane -T failed: {error}"))?;
        let _ = writeln!(output, "  \x1b[32m✓\x1b[0m title: {target} = '{title}'");
    }
    for item in &options.meta {
        let (key, value) = tag_split_meta(item)?;
        let option_key = tag_option_key(&key);
        tmux.tag_set_meta(target, &option_key, &value)
            .map_err(|error| format!("set-option failed: {error}"))?;
        let _ = writeln!(output, "  \x1b[32m✓\x1b[0m meta: {target} {option_key} = '{value}'");
    }
    Ok(output)
}

fn tag_render_read(target: &str, title: &str, options: &str) -> String {
    let custom = tag_custom_option_lines(options);
    let mut output = String::new();
    let shown_title = if title.is_empty() { "(none)" } else { title };
    let _ = writeln!(output, "  \x1b[36m{target}\x1b[0m");
    let _ = writeln!(output, "  \x1b[90m  title:\x1b[0m {shown_title}");
    if custom.is_empty() {
        let _ = writeln!(output, "  \x1b[90m  meta:  (none)\x1b[0m");
    } else {
        let _ = writeln!(output, "  \x1b[90m  meta:\x1b[0m");
        for line in custom {
            let _ = writeln!(output, "  \x1b[90m    {line}\x1b[0m");
        }
    }
    output
}

fn tag_custom_option_lines(options: &str) -> Vec<String> {
    options
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('@'))
        .map(str::to_owned)
        .collect()
}

fn tag_resolve_target(options: &TagOptions, sessions: &[TagSession]) -> Result<String, String> {
    let (raw_session, raw_window) = tag_split_target(&options.target)?;
    tag_validate_user_target(&raw_session)?;
    let session = tag_resolve_or_error(&raw_session, sessions)?;
    let window = raw_window.unwrap_or_else(|| tag_default_window(session));
    tag_validate_target_segment(&window, "window")?;
    let mut target = format!("{}:{window}", session.name);
    if let Some(pane) = options.pane {
        let _ = write!(target, ".{pane}");
    }
    Ok(target)
}

fn tag_split_target(target: &str) -> Result<(String, Option<String>), String> {
    let Some((left, right)) = target.split_once(':') else {
        return Ok((target.to_owned(), None));
    };
    tag_validate_target_segment(right, "window")?;
    Ok((left.to_owned(), Some(right.to_owned())))
}

fn tag_resolve_or_error<'a>(
    raw: &str,
    sessions: &'a [TagSession],
) -> Result<&'a TagSession, String> {
    let names = sessions.iter().map(|session| session.name.clone()).collect::<Vec<_>>();
    match resolve_session_target(raw, &names) {
        ResolveResult::Exact { matched } | ResolveResult::Fuzzy { matched } => sessions
            .iter()
            .find(|session| session.name == matched)
            .ok_or_else(|| format!("session '{raw}' not found")),
        ResolveResult::Ambiguous { candidates } => Err(tag_ambiguous_error(raw, &candidates)),
        ResolveResult::None { hints } => Err(tag_not_found_error(raw, &hints.unwrap_or_default())),
    }
}

fn tag_default_window(session: &TagSession) -> String {
    session
        .windows
        .first()
        .map_or_else(|| "0".to_owned(), u32::to_string)
}

fn tag_ambiguous_error(target: &str, candidates: &[String]) -> String {
    let mut message = format!(
        "  \x1b[31m✗\x1b[0m '{target}' is ambiguous — matches {} sessions:",
        candidates.len()
    );
    for candidate in candidates {
        let _ = write!(message, "\n  \x1b[90m    • {candidate}\x1b[0m");
    }
    let _ = write!(message, "\n'{target}' is ambiguous");
    message
}

fn tag_not_found_error(target: &str, hints: &[String]) -> String {
    let mut message = format!("  \x1b[31m✗\x1b[0m session '{target}' not found");
    if hints.is_empty() {
        message.push_str("\n  \x1b[90m  try: maw ls\x1b[0m");
    } else {
        message.push_str("\n  \x1b[90m  did you mean:\x1b[0m");
        for hint in hints {
            let _ = write!(message, "\n  \x1b[90m    • {hint}\x1b[0m");
        }
    }
    message
}

fn tag_parse_sessions(raw: &str) -> Vec<TagSession> {
    let mut sessions = Vec::<TagSession>::new();
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.splitn(2, '\t');
        let name = fields.next().unwrap_or_default();
        let index = fields
            .next()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        tag_push_session_window(&mut sessions, name, index);
    }
    sessions
}

fn tag_push_session_window(sessions: &mut Vec<TagSession>, name: &str, index: u32) {
    if let Some(session) = sessions.iter_mut().find(|session| session.name == name) {
        session.windows.push(index);
    } else {
        sessions.push(TagSession {
            name: name.to_owned(),
            windows: vec![index],
        });
    }
}

fn tag_split_meta(item: &str) -> Result<(String, String), String> {
    let Some(index) = item.find('=') else {
        return Err(format!("--meta must be key=val (got: {item})"));
    };
    if index == 0 {
        return Err(format!("--meta must be key=val (got: {item})"));
    }
    let key = item[..index].trim().to_owned();
    let value = item[index + 1..].to_owned();
    if key.is_empty() {
        return Err(format!("--meta must be key=val (got: {item})"));
    }
    Ok((key, value))
}

fn tag_option_key(key: &str) -> String {
    if key.starts_with('@') {
        key.to_owned()
    } else {
        format!("@{key}")
    }
}

fn tag_validate_user_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value == "--" || value.starts_with('-') || value.trim() != value {
        return Err("tag target must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("tag target must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn tag_validate_target_segment(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value == "--" || value.starts_with('-') || value.trim() != value {
        return Err(format!("tag {label} must be non-empty, unpadded, and not start with '-'"));
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err(format!("tag {label} must not contain whitespace or control characters"));
    }
    Ok(())
}

fn tag_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value == "--" || value.starts_with('-') || value.trim() != value {
        return Err("tag tmux target must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("tag tmux target must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn tag_validate_label(value: &str, label: &str) -> Result<(), String> {
    if value == "--" || value.starts_with('-') {
        return Err(format!("tag {label} must not start with '-'"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("tag {label} must not contain control characters"));
    }
    Ok(())
}

fn tag_validate_option_key(key: &str) -> Result<(), String> {
    let trimmed = key.trim_start_matches('@');
    tag_validate_target_segment(trimmed, "meta key")?;
    if trimmed.contains('=') {
        return Err("tag meta key must not contain '='".to_owned());
    }
    Ok(())
}

fn tag_tmux_run<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    subcommand: &str,
    args: &[&str],
) -> Result<String, String> {
    let owned = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    runner
        .run(subcommand, &owned)
        .map_err(|error| error.message)
}

#[cfg(test)]
mod tag_tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum TagCall {
        List,
        Display(String),
        Show(String),
        Title(String, String),
        Meta(String, String, String),
    }

    #[derive(Debug, Default)]
    struct TagFakeTmux {
        sessions: Vec<TagSession>,
        title: String,
        options: String,
        calls: Vec<TagCall>,
        failures: Vec<&'static str>,
    }

    impl TagTmux for TagFakeTmux {
        fn tag_list_sessions(&mut self) -> Result<Vec<TagSession>, String> {
            self.calls.push(TagCall::List);
            Ok(self.sessions.clone())
        }

        fn tag_display_title(&mut self, target: &str) -> Result<String, String> {
            tag_validate_tmux_target(target)?;
            self.calls.push(TagCall::Display(target.to_owned()));
            if tag_has_failure(&self.failures, "display") { Err("no pane".to_owned()) } else { Ok(self.title.clone()) }
        }

        fn tag_show_options(&mut self, target: &str) -> Result<String, String> {
            tag_validate_tmux_target(target)?;
            self.calls.push(TagCall::Show(target.to_owned()));
            if tag_has_failure(&self.failures, "show") { Err("no options".to_owned()) } else { Ok(self.options.clone()) }
        }

        fn tag_set_title(&mut self, target: &str, title: &str) -> Result<(), String> {
            tag_validate_tmux_target(target)?;
            tag_validate_label(title, "title")?;
            self.calls.push(TagCall::Title(target.to_owned(), title.to_owned()));
            if tag_has_failure(&self.failures, "title") { Err("bad title".to_owned()) } else { Ok(()) }
        }

        fn tag_set_meta(&mut self, target: &str, key: &str, value: &str) -> Result<(), String> {
            tag_validate_tmux_target(target)?;
            tag_validate_option_key(key)?;
            tag_validate_label(value, "meta value")?;
            self.calls.push(TagCall::Meta(target.to_owned(), key.to_owned(), value.to_owned()));
            if tag_has_failure(&self.failures, "meta") { Err("bad meta".to_owned()) } else { Ok(()) }
        }
    }

    fn tag_has_failure(failures: &[&str], name: &str) -> bool {
        failures.contains(&name)
    }

    fn tag_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn tag_session(name: &str, windows: &[u32]) -> TagSession {
        TagSession { name: name.to_owned(), windows: windows.to_vec() }
    }

    fn tag_fake() -> TagFakeTmux {
        TagFakeTmux { sessions: vec![tag_session("03-neo", &[2, 4])], ..TagFakeTmux::default() }
    }

    #[test]
    fn tag_dispatch_registers_single_native_command() {
        assert_eq!(DISPATCH_82.len(), 1);
        assert_eq!(DISPATCH_82[0].command, "tag");
    }

    #[test]
    fn tag_read_mode_resolves_default_window_and_prints_meta() {
        let mut tmux = tag_fake();
        tmux.title = "oracle".to_owned();
        tmux.options = "@agent-name neo\nstatus on\n @role oracle\n".to_owned();

        let output = tag_run(&tag_strings(&["neo"]), &mut tmux).expect("tag read");

        assert!(output.contains("\x1b[36m03-neo:2\x1b[0m"));
        assert!(output.contains("title:\x1b[0m oracle"));
        assert!(output.contains("@agent-name neo"));
        assert!(output.contains("@role oracle"));
        assert_eq!(tmux.calls, vec![TagCall::List, TagCall::Display("03-neo:2".to_owned()), TagCall::Show("03-neo:2".to_owned())]);
    }

    #[test]
    fn tag_write_title_and_meta_validates_before_tmux_calls() {
        let mut tmux = tag_fake();
        let args = tag_strings(&["neo:4", "--pane", "1", "--title", "scout", "--meta", "agent-name=scout", "--meta", "@role=teammate"]);

        let output = tag_run(&args, &mut tmux).expect("tag write");

        assert!(output.contains("title: 03-neo:4.1 = 'scout'"));
        assert!(output.contains("meta: 03-neo:4.1 @agent-name = 'scout'"));
        assert!(output.contains("meta: 03-neo:4.1 @role = 'teammate'"));
        assert_eq!(tmux.calls[0], TagCall::List);
        assert_eq!(tmux.calls[1], TagCall::Title("03-neo:4.1".to_owned(), "scout".to_owned()));
        assert_eq!(tmux.calls[2], TagCall::Meta("03-neo:4.1".to_owned(), "@agent-name".to_owned(), "scout".to_owned()));
        assert_eq!(tmux.calls[3], TagCall::Meta("03-neo:4.1".to_owned(), "@role".to_owned(), "teammate".to_owned()));
    }

    #[test]
    fn tag_rejects_leading_dash_target_before_tmux() {
        let mut tmux = tag_fake();
        let error = tag_run(&tag_strings(&["-bad", "--title", "x"]), &mut tmux).expect_err("guard");
        assert!(error.contains("looks like a flag"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn tag_rejects_separator_before_tmux() {
        let mut tmux = tag_fake();
        let error = tag_run(&tag_strings(&["--", "neo"]), &mut tmux).expect_err("guard");
        assert!(error.contains("-- separator"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn tag_rejects_bad_window_and_pane_before_tmux() {
        let mut tmux = tag_fake();
        let window = tag_run(&tag_strings(&["neo:-1"]), &mut tmux).expect_err("window guard");
        assert!(window.contains("window"));
        let pane = tag_run(&tag_strings(&["neo", "--pane", "--"]), &mut tmux).expect_err("pane guard");
        assert!(pane.contains("--pane value"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn tag_rejects_leading_dash_labels_before_tmux() {
        let mut tmux = tag_fake();
        let title = tag_run(&tag_strings(&["neo", "--title", "-bad"]), &mut tmux).expect_err("title guard");
        assert!(title.contains("--title value"));
        let meta = tag_run(&tag_strings(&["neo", "--meta", "role=-bad"]), &mut tmux).expect_err("meta guard");
        assert!(meta.contains("meta value"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn tag_rejects_invalid_meta_before_tmux() {
        let mut tmux = tag_fake();
        let error = tag_run(&tag_strings(&["neo", "--meta", "noval"]), &mut tmux).expect_err("meta guard");
        assert!(error.contains("--meta must be key=val"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn tag_reports_ambiguous_session_without_write() {
        let mut tmux = TagFakeTmux { sessions: vec![tag_session("01-neo", &[0]), tag_session("02-neo", &[0])], ..TagFakeTmux::default() };

        let error = tag_run(&tag_strings(&["neo", "--title", "x"]), &mut tmux).expect_err("ambiguous");

        assert!(error.contains("'neo' is ambiguous"));
        assert_eq!(tmux.calls, vec![TagCall::List]);
    }

    #[test]
    fn tag_missing_session_prints_hints_without_write() {
        let mut tmux = TagFakeTmux { sessions: vec![tag_session("03-neon", &[0])], ..TagFakeTmux::default() };

        let error = tag_run(&tag_strings(&["neo", "--title", "x"]), &mut tmux).expect_err("missing");

        assert!(error.contains("session 'neo' not found"));
        assert!(error.contains("03-neon"));
        assert_eq!(tmux.calls, vec![TagCall::List]);
    }

    #[test]
    fn tag_read_tolerates_show_options_failure_like_js() {
        let mut tmux = tag_fake();
        tmux.failures.push("show");

        let output = tag_run(&tag_strings(&["neo"]), &mut tmux).expect("read");

        assert!(output.contains("meta:  (none)"));
        assert_eq!(tmux.calls.len(), 3);
    }

    #[test]
    fn tag_read_display_failure_is_reported() {
        let mut tmux = tag_fake();
        tmux.failures.push("display");

        let error = tag_run(&tag_strings(&["neo"]), &mut tmux).expect_err("display fail");

        assert_eq!(error, "read failed: no pane");
        assert_eq!(tmux.calls.len(), 2);
    }
}
