const DISPATCH_106: &[DispatcherEntry] = &[
    DispatcherEntry { command: "token", handler: Handler::Sync(token_run_command) },
    DispatcherEntry { command: "tokens", handler: Handler::Sync(token_tokens_alias_command) },
];

const TOKEN_PASS_PREFIX: &str = "envrc";
const TOKEN_TOKEN_PREFIX: &str = "claude/token-";
const TOKEN_FAKE_ROOT_ENV: &str = "MAW_RS_TOKEN_FAKE_ROOT";
const TOKEN_FAKE_FAIL_ENV: &str = "MAW_RS_TOKEN_FAKE_FAIL";

#[derive(Debug, Clone)]
struct TokenCommandResult {
    ok: bool,
    stdout: String,
}

trait TokenRunner {
    fn token_pass_show(&mut self, name: &str) -> Result<String, ()>;
    fn token_pass_ls(&mut self, prefix: &str) -> Result<String, ()>;
    fn token_pass_insert_multiline(&mut self, name: &str, stdin: &str) -> Result<(), ()>;
    fn token_direnv_allow(&mut self, cwd: &std::path::Path) -> Result<(), ()>;
    fn token_stdin_is_tty(&self) -> bool;
    fn token_confirm(&mut self, prompt: &str) -> bool;
}

struct TokenSystemRunner;

impl TokenRunner for TokenSystemRunner {
    fn token_pass_show(&mut self, name: &str) -> Result<String, ()> {
        token_run_pass_output(&["show", name], None)
    }

    fn token_pass_ls(&mut self, prefix: &str) -> Result<String, ()> {
        token_run_pass_output(&["ls", prefix], None)
    }

    fn token_pass_insert_multiline(&mut self, name: &str, stdin: &str) -> Result<(), ()> {
        token_run_pass_output(&["insert", "--multiline", "--force", name], Some(stdin)).map(|_| ())
    }

    fn token_direnv_allow(&mut self, cwd: &std::path::Path) -> Result<(), ()> {
        token_run_direnv_allow(cwd)
    }

    fn token_stdin_is_tty(&self) -> bool {
        use std::io::IsTerminal as _;
        std::io::stdin().is_terminal()
    }

    fn token_confirm(&mut self, prompt: &str) -> bool {
        token_prompt_confirm(prompt)
    }
}

struct TokenFakeRunner {
    root: std::path::PathBuf,
    fail: Option<String>,
}

impl TokenFakeRunner {
    fn token_new_from_env() -> Option<Self> {
        let root = std::env::var_os(TOKEN_FAKE_ROOT_ENV).map(std::path::PathBuf::from)?;
        let fail = std::env::var(TOKEN_FAKE_FAIL_ENV).ok().filter(|value| !value.is_empty());
        Some(Self { root, fail })
    }

    fn token_entry_path(&self, name: &str) -> std::path::PathBuf {
        self.root.join("pass").join(name)
    }

    fn token_should_fail(&self, op: &str) -> bool {
        self.fail.as_deref().is_some_and(|value| value == op || value == "all")
    }
}

impl TokenRunner for TokenFakeRunner {
    fn token_pass_show(&mut self, name: &str) -> Result<String, ()> {
        if self.token_should_fail("show") { return Err(()); }
        std::fs::read_to_string(self.token_entry_path(name)).map_err(|_| ())
    }

    fn token_pass_ls(&mut self, prefix: &str) -> Result<String, ()> {
        if self.token_should_fail("ls") { return Err(()); }
        let base = self.token_entry_path(prefix);
        let mut out = String::new();
        let entries = std::fs::read_dir(base).map_err(|_| ())?;
        let mut names = entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect::<Vec<_>>();
        names.sort();
        for name in names { let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{name}\n")); }
        Ok(out)
    }

    fn token_pass_insert_multiline(&mut self, name: &str, stdin: &str) -> Result<(), ()> {
        if self.token_should_fail("insert") { return Err(()); }
        let path = self.token_entry_path(name);
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|_| ())?; }
        std::fs::write(path, stdin).map_err(|_| ())
    }

    fn token_direnv_allow(&mut self, cwd: &std::path::Path) -> Result<(), ()> {
        if self.token_should_fail("direnv") { return Err(()); }
        let marker = self.root.join("direnv-allowed.log");
        std::fs::write(marker, format!("{}\n", cwd.display())).map_err(|_| ())
    }

    fn token_stdin_is_tty(&self) -> bool { false }
    fn token_confirm(&mut self, _prompt: &str) -> bool { false }
}

fn token_run_command(argv: &[String]) -> CliOutput {
    let mut runner = token_runner_from_env();
    match token_dispatch(argv, runner.as_mut()) {
        Ok(result) => CliOutput { code: i32::from(!result.ok), stdout: result.stdout, stderr: String::new() },
        Err(error) => token_error(&error),
    }
}

fn token_tokens_alias_command(_argv: &[String]) -> CliOutput {
    let mut runner = token_runner_from_env();
    match token_cmd_list(runner.as_mut()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(error) => token_error(&error),
    }
}

fn token_runner_from_env() -> Box<dyn TokenRunner> {
    if let Some(fake) = TokenFakeRunner::token_new_from_env() { Box::new(fake) } else { Box::new(TokenSystemRunner) }
}

fn token_dispatch(argv: &[String], runner: &mut dyn TokenRunner) -> Result<TokenCommandResult, String> {
    let parsed = token_parse_args(argv)?;
    let Some(sub) = parsed.positionals.first().map(String::as_str) else {
        return Ok(token_ok(format!("{}\n", token_help())));
    };
    match sub {
        "help" | "--help" | "-h" => Ok(token_ok(format!("{}\n", token_help()))),
        "list" | "ls" | "tokens" => token_cmd_list(runner).map(token_ok),
        "current" => Ok(token_ok(token_current().map_or_else(String::new, |name| format!("{name}\n")))),
        "use" => token_cmd_use(&parsed, runner),
        "save" => token_cmd_save(&parsed, runner),
        "load" => token_cmd_load(&parsed, runner),
        "scan" => token_cmd_scan(runner).map(token_ok),
        _ => Ok(TokenCommandResult { ok: false, stdout: format!("{}\n", token_help()) }),
    }
}

#[derive(Debug, Default)]
struct TokenArgs {
    positionals: Vec<String>,
    no_team: bool,
    force: bool,
}

fn token_parse_args(argv: &[String]) -> Result<TokenArgs, String> {
    let mut parsed = TokenArgs::default();
    for value in argv {
        match value.as_str() {
            "--no-team" => parsed.no_team = true,
            "--force" | "-f" => parsed.force = true,
            "--" => return Err("maw token: -- separator is not allowed".to_owned()),
            _ if value.starts_with('-') => return Err(format!("maw token: unknown flag {value}")),
            _ => {
                token_validate_cli_value("argument", value)?;
                parsed.positionals.push(value.clone());
            }
        }
    }
    Ok(parsed)
}

fn token_cmd_list(runner: &mut dyn TokenRunner) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|_| "maw token: current directory unavailable".to_owned())?;
    let dir = cwd.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("/");
    let envrc_path = cwd.join(".envrc");
    let envrc_present = envrc_path.exists();
    let active = token_current();
    let tokens = token_list_token_names(runner);
    let envrcs = token_list_envrc_names(runner);
    let mut out = Vec::new();
    if let Some(active) = active.as_deref() { out.push(format!("Here ({dir}): {active}")); }
    else if envrc_present { out.push(format!("Here ({dir}): .envrc present, no CLAUDE_TOKEN_NAME")); }
    else { out.push(format!("Here ({dir}): no .envrc")); }
    out.push(String::new());
    if !tokens.is_empty() {
        out.push("Tokens (claude/):".to_owned());
        for (idx, name) in tokens.iter().enumerate() {
            let marker = if Some(name.as_str()) == active.as_deref() { " ← active" } else { "" };
            out.push(format!("  {}. {name}{marker}", idx + 1));
        }
        out.push(String::new());
    }
    if !envrcs.is_empty() {
        out.push(format!("Envrcs ({TOKEN_PASS_PREFIX}/):"));
        for (idx, name) in envrcs.iter().enumerate() { out.push(format!("  {}. {name}", idx + 1)); }
        out.push(String::new());
    }
    if tokens.is_empty() && envrcs.is_empty() { out.push("Empty vault. Add tokens: pass insert claude/token-<name>".to_owned()); }
    Ok(format!("{}\n", out.join("\n")))
}

fn token_cmd_use(args: &TokenArgs, runner: &mut dyn TokenRunner) -> Result<TokenCommandResult, String> {
    let Some(name) = args.positionals.get(1).map(String::as_str) else {
        let list = token_cmd_list(runner)?;
        return Ok(token_ok(format!("{list}Usage: maw token use <name> [--no-team]\n")));
    };
    token_validate_name("token name", name)?;
    let pass_path = format!("{TOKEN_TOKEN_PREFIX}{name}");
    if !token_pass_exists(runner, &pass_path) { return Err(format!("token \"{name}\" not found in pass ({pass_path})")); }
    let cwd = std::env::current_dir().map_err(|_| "maw token use: current directory unavailable".to_owned())?;
    let envrc_path = cwd.join(".envrc");
    let existing = std::fs::read_to_string(&envrc_path).unwrap_or_default();
    let content = token_build_envrc_content(&existing, name, args.no_team);
    token_atomic_write(&envrc_path, &content).map_err(|_| "maw token use: failed to write .envrc".to_owned())?;
    let direnv_ok = runner.token_direnv_allow(&cwd).is_ok();
    let mut stdout = format!("Now using: {name}\n");
    if !direnv_ok { stdout.push_str("warning: direnv allow failed — run `direnv allow .` manually\n"); }
    Ok(token_ok(stdout))
}

fn token_cmd_save(args: &TokenArgs, runner: &mut dyn TokenRunner) -> Result<TokenCommandResult, String> {
    let cwd = std::env::current_dir().map_err(|_| "maw token save: current directory unavailable".to_owned())?;
    let name = token_default_name(args.positionals.get(1).map(String::as_str), &cwd)?;
    token_validate_name("envrc name", &name)?;
    let path = format!("{TOKEN_PASS_PREFIX}/{name}");
    let envrc_path = cwd.join(".envrc");
    let content = std::fs::read_to_string(&envrc_path).map_err(|_| "no .envrc in current directory".to_owned())?;
    if token_pass_exists(runner, &path) && !args.force && !token_confirm_overwrite(runner, &format!("Overwrite {path}?")) {
        return Ok(token_ok(format!("Skipped (would overwrite {path})\n")));
    }
    runner.token_pass_insert_multiline(&path, &content).map_err(|()| "pass insert failed".to_owned())?;
    Ok(token_ok(format!("Saved .envrc as {path}\n")))
}

fn token_cmd_load(args: &TokenArgs, runner: &mut dyn TokenRunner) -> Result<TokenCommandResult, String> {
    let cwd = std::env::current_dir().map_err(|_| "maw token load: current directory unavailable".to_owned())?;
    let name = token_default_name(args.positionals.get(1).map(String::as_str), &cwd)?;
    token_validate_name("envrc name", &name)?;
    let path = format!("{TOKEN_PASS_PREFIX}/{name}");
    let envrc_path = cwd.join(".envrc");
    if !token_pass_exists(runner, &path) { return Err(format!("{path} not found in pass")); }
    if envrc_path.exists() && !args.force && !token_confirm_overwrite(runner, "Overwrite .envrc?") {
        return Ok(token_ok(format!("Skipped (would overwrite .envrc; {path})\n")));
    }
    let content = runner.token_pass_show(&path).map_err(|()| "pass show failed".to_owned())?;
    token_reject_literal_oauth_value(&content)?;
    token_atomic_write(&envrc_path, &content).map_err(|_| "maw token load: failed to write .envrc".to_owned())?;
    let direnv_ok = runner.token_direnv_allow(&cwd).is_ok();
    let mut stdout = format!("Loaded {path} into .envrc\n");
    if !direnv_ok { stdout.push_str("warning: direnv allow failed — run `direnv allow .` manually\n"); }
    Ok(token_ok(stdout))
}

fn token_cmd_scan(runner: &mut dyn TokenRunner) -> Result<String, String> {
    let ghq_root = token_resolve_ghq_root()?;
    let fingerprints = token_fingerprint_tokens(runner);
    let files = token_find_envrc_files(&ghq_root);
    let mut rows = Vec::new();
    for (label, path) in files {
        let Ok(content) = std::fs::read_to_string(path) else { continue; };
        if let Some(name) = token_detect_active_token(&content) { rows.push((label, name, "named".to_owned())); continue; }
        if let Some(name) = token_match_fingerprint(&content, &fingerprints) { rows.push((label, name, "matched".to_owned())); }
        else if content.contains("CLAUDE_CODE_OAUTH_TOKEN") { rows.push((label, "unknown".to_owned(), "unmatched".to_owned())); }
    }
    Ok(token_format_scan(&rows))
}

fn token_ok(stdout: String) -> TokenCommandResult { TokenCommandResult { ok: true, stdout } }

fn token_error(message: &str) -> CliOutput {
    CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") }
}

fn token_help() -> &'static str {
    "usage: maw token <list|use|current|save|load|scan> [...]\n  list                                  — list vault tokens + saved .envrcs (active marked)\n  use <name> [--no-team]                — switch active Claude token in local .envrc\n  current                               — print active token name (for statuslines)\n  save [name] [-f|--force]              — save .envrc to pass vault (default name = cwd basename)\n  load [name] [-f|--force]              — restore .envrc from pass vault + direnv allow\n  scan                                  — scan ghq repos, map tokens to oracles\n\naliases:\n  tokens                                — same as `list`\n  ls                                    — same as `list`\n\nsecurity: token values are never printed, logged, or stored outside\n          memory. See README.md for the full threat model."
}

fn token_current() -> Option<String> {
    let path = std::env::current_dir().ok()?.join(".envrc");
    let content = std::fs::read_to_string(path).ok()?;
    token_detect_active_token(&content)
}

fn token_detect_active_token(content: &str) -> Option<String> {
    let active = content.lines().filter(|line| !line.trim_start().starts_with('#')).collect::<Vec<_>>().join("\n");
    if let Some(value) = token_extract_between(&active, "CLAUDE_TOKEN_NAME=\"", "\"") { return Some(value); }
    if let Some(idx) = active.find("pass show claude/token-") {
        let tail = &active[idx + "pass show claude/token-".len()..];
        let name = tail.chars().take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')).collect::<String>();
        if !name.is_empty() { return Some(name); }
    }
    if let Some(var) = token_extract_after(&active, "export CLAUDE_CODE_OAUTH_TOKEN=$") {
        let needle = format!("{var}=\"$(pass show claude/token-");
        if let Some(value) = token_extract_between(&active, &needle, ")\"") { return Some(value); }
    }
    None
}

fn token_build_envrc_content(existing: &str, name: &str, no_team: bool) -> String {
    let mut token_lines = vec![
        format!("export CLAUDE_TOKEN_NAME=\"{name}\""),
        format!("export CLAUDE_CODE_OAUTH_TOKEN=\"$(pass show {TOKEN_TOKEN_PREFIX}{name})\""),
    ];
    if !no_team { token_lines.push("export CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1".to_owned()); }
    if existing.is_empty() { return format!("{}\n", token_lines.join("\n")); }
    let mut kept = Vec::new();
    for line in existing.split('\n') {
        let s = line.trim();
        if s.starts_with("export CLAUDE_TOKEN_NAME=") || s.starts_with("CLAUDE_TOKEN_NAME=") { continue; }
        if s.starts_with("export CLAUDE_CODE_OAUTH_TOKEN=") || s.starts_with("CLAUDE_CODE_OAUTH_TOKEN=") { continue; }
        if s.starts_with("export CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=") { continue; }
        if token_is_legacy_token_line(s) { continue; }
        kept.push(line.to_owned());
    }
    while kept.last().is_some_and(|line| line.trim().is_empty()) { kept.pop(); }
    let mut content = kept.join("\n");
    if !content.is_empty() && !content.ends_with('\n') { content.push('\n'); }
    content.push('\n');
    content.push_str(&token_lines.join("\n"));
    content.push('\n');
    content
}

fn token_is_legacy_token_line(line: &str) -> bool {
    let rest = line.strip_prefix("export ").unwrap_or(line);
    ["TOKEN_PYM=", "TOKEN_DO=", "TOKEN_TING_TING="].iter().any(|prefix| rest.starts_with(prefix))
}

fn token_pass_exists(runner: &mut dyn TokenRunner, name: &str) -> bool {
    runner.token_pass_show(name).is_ok()
}

fn token_list_token_names(runner: &mut dyn TokenRunner) -> Vec<String> {
    let Ok(raw) = runner.token_pass_ls("claude") else { return Vec::new(); };
    let mut out = raw.lines().filter_map(token_parse_token_ls_line).collect::<Vec<_>>();
    out.sort();
    out.dedup();
    out
}

fn token_list_envrc_names(runner: &mut dyn TokenRunner) -> Vec<String> {
    let Ok(raw) = runner.token_pass_ls(TOKEN_PASS_PREFIX) else { return Vec::new(); };
    let mut out = raw.lines().filter_map(token_parse_envrc_ls_line).collect::<Vec<_>>();
    out.sort();
    out.dedup();
    out
}

fn token_parse_token_ls_line(raw: &str) -> Option<String> {
    let clean = token_strip_ansi(raw);
    let idx = clean.find("token-")? + "token-".len();
    let name = clean[idx..].split_whitespace().next()?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn token_parse_envrc_ls_line(raw: &str) -> Option<String> {
    let clean = token_strip_ansi(raw).trim().to_owned();
    if clean.is_empty() || clean.ends_with('/') || clean.contains("Password Store") { return None; }
    let name = clean.split_whitespace().last()?;
    (name != TOKEN_PASS_PREFIX).then(|| name.to_owned())
}

fn token_fingerprint_tokens(runner: &mut dyn TokenRunner) -> Vec<(String, String)> {
    token_list_token_names(runner)
        .into_iter()
        .filter_map(|name| {
            let path = format!("{TOKEN_TOKEN_PREFIX}{name}");
            let token = runner.token_pass_show(&path).ok()?.trim().to_owned();
            (token.len() >= 8).then_some((token, name))
        })
        .collect()
}

fn token_match_fingerprint(content: &str, fingerprints: &[(String, String)]) -> Option<String> {
    fingerprints.iter().find_map(|(value, name)| content.contains(value).then(|| name.clone()))
}

fn token_format_scan(rows: &[(String, String, String)]) -> String {
    if rows.is_empty() { return "No .envrc files with Claude tokens found\n".to_owned(); }
    let mut by_token = std::collections::BTreeMap::<String, Vec<(String, String)>>::new();
    for (label, name, method) in rows { by_token.entry(name.clone()).or_default().push((label.clone(), method.clone())); }
    let mut out = format!("  {} oracles using {} tokens:\n\n", rows.len(), by_token.len());
    for (idx, (name, repos)) in by_token.iter().enumerate() {
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("  {}. {name} ({} repos)\n", idx + 1, repos.len()));
        for (label, method) in repos {
            let flag = if method == "unmatched" { " *" } else { "" };
            let _ = std::fmt::Write::write_fmt(&mut out, format_args!("     {label}{flag}\n"));
        }
        out.push('\n');
    }
    if rows.iter().any(|(_, _, method)| method == "unmatched") { out.push_str("  * = token not in pass vault (unknown)\n"); }
    out
}

fn token_resolve_ghq_root() -> Result<std::path::PathBuf, String> {
    if let Some(root) = std::env::var_os("GHQ_ROOT").map(std::path::PathBuf::from) {
        let github = root.join("github.com");
        if github.is_dir() { return Ok(github); }
    }
    let raw = token_run_output("ghq", &["root"], None, None).map_err(|()| "scan: ghq root unavailable — install ghq or set up ~/ghq/github.com (no hardcoded fallback)".to_owned())?;
    let github = std::path::Path::new(raw.trim()).join("github.com");
    if github.is_dir() { Ok(github) } else { Err("scan: ghq root unavailable — install ghq or set up ~/ghq/github.com (no hardcoded fallback)".to_owned()) }
}

fn token_find_envrc_files(ghq_root: &std::path::Path) -> Vec<(String, std::path::PathBuf)> {
    let mut out = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
        let path = home.join(".envrc");
        if path.is_file() { out.push(("~".to_owned(), path)); }
    }
    let Ok(orgs) = std::fs::read_dir(ghq_root) else { return out; };
    let mut orgs = orgs.filter_map(Result::ok).collect::<Vec<_>>();
    orgs.sort_by_key(std::fs::DirEntry::file_name);
    for org in orgs {
        let org_name = org.file_name().to_string_lossy().into_owned();
        let Ok(repos) = std::fs::read_dir(org.path()) else { continue; };
        let mut repos = repos.filter_map(Result::ok).collect::<Vec<_>>();
        repos.sort_by_key(std::fs::DirEntry::file_name);
        for repo in repos {
            let repo_name = repo.file_name().to_string_lossy().into_owned();
            let envrc = repo.path().join(".envrc");
            if envrc.is_file() { out.push((format!("{org_name}/{repo_name}"), envrc)); }
        }
    }
    out
}

fn token_default_name(name: Option<&str>, cwd: &std::path::Path) -> Result<String, String> {
    let value = name
        .map(str::to_owned)
        .or_else(|| cwd.file_name().and_then(std::ffi::OsStr::to_str).map(str::to_owned))
        .unwrap_or_else(|| "default".to_owned());
    token_validate_name("envrc name", &value)?;
    Ok(value)
}

fn token_confirm_overwrite(runner: &mut dyn TokenRunner, prompt: &str) -> bool {
    runner.token_stdin_is_tty() && runner.token_confirm(prompt)
}

fn token_prompt_confirm(prompt: &str) -> bool {
    use std::io::Write as _;
    let _ = write!(std::io::stdout(), "{prompt} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).is_ok() && line.trim().eq_ignore_ascii_case("y")
}

fn token_validate_cli_value(kind: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value.contains('\0') || value.chars().any(char::is_control) || value == ".." || value.contains("../") || value.contains("..\\") {
        return Err(format!("maw token: invalid {kind} value"));
    }
    Ok(())
}

fn token_validate_name(kind: &str, value: &str) -> Result<(), String> {
    token_validate_cli_value(kind, value)?;
    if value.contains('/') || value.contains('\\') || value.contains(std::path::MAIN_SEPARATOR) {
        return Err(format!("maw token: invalid {kind}"));
    }
    if !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')) {
        return Err(format!("maw token: invalid {kind}"));
    }
    Ok(())
}

fn token_reject_literal_oauth_value(content: &str) -> Result<(), String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.contains("CLAUDE_CODE_OAUTH_TOKEN") { continue; }
        if trimmed.contains("$(pass show claude/token-") || trimmed.starts_with('#') { continue; }
        return Err("saved envrc contains literal token value; refusing to write".to_owned());
    }
    Ok(())
}

fn token_atomic_write(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let tmp = parent.join(format!(".{}.maw-token-{}.tmp", path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("envrc"), std::process::id()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    drop(file);
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(error) => { let _ = std::fs::remove_file(&tmp); Err(error) }
    }
}


fn token_run_pass_output(args: &[&str], stdin: Option<&str>) -> Result<String, ()> {
    token_run_command_output(std::process::Command::new("pass").args(args), stdin)
}

fn token_run_direnv_allow(cwd: &std::path::Path) -> Result<(), ()> {
    token_run_command_output(std::process::Command::new("direnv").args(["allow", "."]).current_dir(cwd), None).map(|_| ())
}

fn token_run_output(program: &str, args: &[&str], stdin: Option<&str>, cwd: Option<&std::path::Path>) -> Result<String, ()> {
    let mut command = std::process::Command::new(program);
    command.args(args);
    if let Some(cwd) = cwd { command.current_dir(cwd); }
    token_run_command_output(&mut command, stdin)
}

fn token_run_command_output(command: &mut std::process::Command, stdin: Option<&str>) -> Result<String, ()> {
    if stdin.is_some() { command.stdin(std::process::Stdio::piped()); }
    command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = command.spawn().map_err(|_| ())?;
    if let Some(input) = stdin {
        use std::io::Write as _;
        let Some(mut child_stdin) = child.stdin.take() else { return Err(()); };
        child_stdin.write_all(input.as_bytes()).map_err(|_| ())?;
    }
    let output = child.wait_with_output().map_err(|_| ())?;
    if !output.status.success() { return Err(()); }
    String::from_utf8(output.stdout).map_err(|_| ())
}

fn token_strip_ansi(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            let _ = chars.next();
            for next in chars.by_ref() { if next == 'm' { break; } }
        } else { out.push(ch); }
    }
    out
}

fn token_extract_between(haystack: &str, start: &str, end: &str) -> Option<String> {
    let start_idx = haystack.find(start)? + start.len();
    let tail = &haystack[start_idx..];
    Some(tail[..tail.find(end)?].to_owned())
}

fn token_extract_after(haystack: &str, start: &str) -> Option<String> {
    let start_idx = haystack.find(start)? + start.len();
    let tail = &haystack[start_idx..];
    let value = tail.chars().take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_').collect::<String>();
    (!value.is_empty()).then_some(value)
}
