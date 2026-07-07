const DISPATCH_263: &[DispatcherEntry] = &[DispatcherEntry { command: "footer", handler: Handler::Sync(footer_run_command263) }];
const FOOTER_USAGE_263: &str = "usage: maw footer [--via <skill>] [--emoji]";

fn footer_run_command263(argv: &[String]) -> CliOutput {
    if wants_help(argv, &["--via"]) { return help_output(FOOTER_USAGE_263); }
    match footer_parse263(argv).and_then(|(via, emoji)| footer_output263(&via, emoji)) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err((code, message)) => CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn footer_parse263(argv: &[String]) -> Result<(String, bool), (i32, String)> {
    let (mut via, mut emoji, mut index) = ("chat".to_owned(), false, 0);
    while index < argv.len() {
        match argv[index].as_str() {
            "--emoji" => emoji = true,
            "--via" => { index += 1; via = footer_token263(argv.get(index).map(String::as_str).unwrap_or_default(), "via")?; }
            value if value.starts_with("--via=") => via = footer_token263(value.strip_prefix("--via=").unwrap_or_default(), "via")?,
            "--" => return Err((2, "footer: -- separator is not supported".to_owned())),
            value if value.starts_with('-') => return Err((2, format!("footer: unknown flag '{value}'\n  {FOOTER_USAGE_263}"))),
            value => return Err((2, format!("footer: unexpected argument '{value}'\n  {FOOTER_USAGE_263}"))),
        }
        index += 1;
    }
    Ok((via, emoji))
}

fn footer_token263(value: &str, label: &str) -> Result<String, (i32, String)> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('-') || value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
        return Err((2, format!("footer: {label} must be a non-empty token")));
    }
    Ok(value.to_owned())
}

fn footer_output263(via: &str, emoji: bool) -> Result<String, (i32, String)> {
    let cwd = std::env::current_dir().map_err(|error| (1, format!("footer: current dir: {error}")))?;
    let short_time = footer_cmd263("date", &["+%H:%M"])?;
    let long_time = if emoji { footer_cmd263("date", &["+%Y-%m-%d %H:%M"])? } else { String::new() };
    let machine = footer_cmd263("hostname", &["-s"])?;
    let commit = footer_commit263()?;
    let (org, repo) = footer_repo263().unwrap_or_else(|| ("unknown".to_owned(), "unknown".to_owned()));
    let (oracle, session) = (footer_oracle263(&cwd), footer_session263(&cwd));
    Ok(if emoji {
        format!("———\n📍 {oracle} @ {machine}\n🕐 {long_time}\n🔗 session: {session}\n🛤️ via: {via}\n📦 {org}/{repo} @ {commit}\n")
    } else {
        format!("[{short_time}] [{oracle}@{machine}] [{session}] [{via}] [{org}] [{repo}] [{commit}]\n")
    })
}

fn footer_cmd263(program: &str, args: &[&str]) -> Result<String, (i32, String)> {
    let output = std::process::Command::new(program).args(args).output().map_err(|error| (1, format!("footer: failed to run {program}: {error}")))?;
    if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned()) } else { Err((1, format!("footer: {program} failed: {}", String::from_utf8_lossy(&output.stderr).trim()))) }
}

fn footer_status263(args: &[&str]) -> bool { std::process::Command::new("git").args(args).status().is_ok_and(|status| status.success()) }

fn footer_commit263() -> Result<String, (i32, String)> {
    let mut commit = footer_cmd263("git", &["rev-parse", "--short", "HEAD"])?;
    if !footer_status263(&["diff", "--quiet"]) || !footer_status263(&["diff", "--cached", "--quiet"]) { commit.push_str("+dirty"); }
    Ok(commit)
}

fn footer_repo263() -> Option<(String, String)> {
    footer_cmd263("git", &["remote", "get-url", "origin"]).ok().and_then(|remote| footer_parse_remote263(&remote)).or_else(|| {
        let root = footer_cmd263("git", &["rev-parse", "--show-toplevel"]).ok()?;
        let repo = std::path::Path::new(&root).file_name()?.to_string_lossy().into_owned();
        Some(("unknown".to_owned(), repo))
    })
}

fn footer_parse_remote263(remote: &str) -> Option<(String, String)> {
    let mut raw = remote.lines().next()?.trim();
    if raw.contains('\t') || raw.contains(" (fetch)") { raw = raw.split_whitespace().nth(1).unwrap_or(raw); }
    let path = raw.strip_prefix("git@github.com:").or_else(|| raw.find("github.com").map(|i| raw[i + "github.com".len()..].trim_start_matches([':', '/']))).unwrap_or(raw).trim_end_matches(".git");
    let parts = path.split('/').filter(|part| !part.is_empty()).collect::<Vec<_>>();
    Some((parts.get(parts.len().checked_sub(2)?)?.to_string(), parts.last()?.to_string()))
}

fn footer_oracle263(cwd: &std::path::Path) -> String {
    let config = merged_config_value_in_dir(cwd);
    ["MAW_ORACLE", "CLAUDE_AGENT_NAME", "CLAUDE_TOKEN_NAME"].into_iter().filter_map(|key| std::env::var(key).ok()).find_map(|value| footer_clean_oracle263(&value))
        .or_else(|| footer_config_oracle263(&config))
        .or_else(|| footer_claude_oracle263(cwd))
        .or_else(|| current_tmux_window_name().and_then(|window| footer_clean_oracle263(&resolve_sender_oracle(None, Some(&window), None))))
        .or_else(|| footer_repo263().and_then(|(_, repo)| footer_clean_oracle263(&repo)))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn footer_config_oracle263(config: &serde_json::Value) -> Option<String> {
    ["/oracle", "/identity/oracle", "/identity/name", "/agent/name", "/agentName", "/agent_name"].into_iter().find_map(|path| config.pointer(path).and_then(serde_json::Value::as_str).and_then(footer_clean_oracle263))
}

fn footer_claude_oracle263(cwd: &std::path::Path) -> Option<String> {
    let mut dir = cwd.to_path_buf();
    loop {
        if let Ok(text) = std::fs::read_to_string(dir.join("CLAUDE.md")) {
            for line in text.lines().take(120) {
                let trimmed = line.trim().trim_start_matches(['#', '-', '*']).trim();
                let lower = trimmed.to_ascii_lowercase();
                for prefix in ["oracle:", "oracle =", "identity:", "name:"] {
                    if let Some(rest) = lower.starts_with(prefix).then(|| &trimmed[prefix.len()..]).and_then(footer_clean_oracle263) { return Some(rest); }
                }
                if trimmed.ends_with("-oracle") { if let Some(value) = footer_clean_oracle263(trimmed) { return Some(value); } }
            }
        }
        if !dir.pop() { return None; }
    }
}

fn footer_clean_oracle263(value: &str) -> Option<String> {
    let first = value.trim().trim_matches(['"', '\'', '`']).split([' ', '\t', '@', '(', '[']).next()?;
    let stripped = first.trim_end_matches(".git").trim_end_matches("-oracle");
    (!stripped.is_empty() && stripped.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))).then(|| stripped.to_owned())
}

fn footer_session263(cwd: &std::path::Path) -> String {
    let root = std::env::var_os("MAW_CLAUDE_PROJECTS_DIR").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".claude").join("projects")));
    root.and_then(|root| footer_newest_jsonl263(cwd, &root)).or_else(|| {
        ["MAW_SESSION_ID", "MAW_PARENT_SESSION_ID", "CLAUDE_SESSION_ID", "CODEX_THREAD_ID", "OMX_SESSION_ID"].into_iter().filter_map(|key| std::env::var(key).ok()).find_map(|value| footer_session8_263(&value))
    }).unwrap_or_else(|| "unknown".to_owned())
}

fn footer_newest_jsonl263(cwd: &std::path::Path, projects_root: &std::path::Path) -> Option<String> {
    let mut encoded = cwd.display().to_string();
    if encoded.starts_with('/') { encoded.replace_range(0..1, "-"); }
    let mut newest = None::<(String, std::time::SystemTime)>;
    for entry in std::fs::read_dir(projects_root.join(encoded.replace('/', "-"))).ok()?.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl")) || name.contains("subagents") { continue; }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else { continue };
        let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified()) else { continue };
        if newest.as_ref().is_none_or(|(_, time)| modified > *time) { newest = Some((stem.to_owned(), modified)); }
    }
    newest.and_then(|(id, _)| footer_session8_263(&id))
}

fn footer_session8_263(value: &str) -> Option<String> {
    let stem = value.trim().rsplit('/').next()?.trim_end_matches(".jsonl");
    let out = stem.chars().filter(char::is_ascii_hexdigit).take(8).collect::<String>();
    (out.len() == 8).then_some(out)
}

#[cfg(test)]
mod footer_tests263 {
    use super::*;
    #[test]
    fn footer_parse_and_helpers_cover_contract_shapes() {
        assert_eq!(footer_parse263(&[]).unwrap(), ("chat".to_owned(), false));
        assert_eq!(footer_parse263(&["--via=recap".to_owned(), "--emoji".to_owned()]).unwrap(), ("recap".to_owned(), true));
        assert!(footer_parse263(&["--via".to_owned(), "bad value".to_owned()]).is_err());
        assert_eq!(footer_parse_remote263("git@github.com:laris-co/nexus-oracle.git"), Some(("laris-co".to_owned(), "nexus-oracle".to_owned())));
        assert_eq!(footer_parse_remote263("https://github.com/Soul-Brews-Studio/maw-rs.git"), Some(("Soul-Brews-Studio".to_owned(), "maw-rs".to_owned())));
        assert_eq!(footer_clean_oracle263("athena-oracle"), Some("athena".to_owned()));
        assert_eq!(footer_session8_263("019f324c-33e6-7622-9338-5ad573581e8d"), Some("019f324c".to_owned()));
    }
}
