use super::*;

const DISCORD_DIR_LINK: &str = ".discord";
const STATE_ENV_KEY: &str = "DISCORD_STATE_DIR";
const CHANNEL_STATE_ENV_KEY: &str = "DISCORD_CHANNEL_STATE_DIR";

pub(super) fn state(env: &DiscordEnv, args: &[String], log: &mut Vec<String>) -> bool {
    let action = args
        .first()
        .map_or("current", String::as_str)
        .to_lowercase();
    match action.as_str() {
        "current" | "path" => state_current(env, args.get(1), log),
        "list" | "ls" => state_list(env, log),
        "use" => state_use(env, &args[1..], log),
        "link" => state_link(env, &args[1..], log),
        "help" | "-h" | "--help" => {
            state_usage(log);
            true
        }
        other => {
            log.push(format!("unknown subcommand: state {other}"));
            state_usage(log);
            false
        }
    }
}

fn state_usage(log: &mut Vec<String>) {
    log.extend([
        "usage: maw discord state <current|list|use|link> [args]".to_owned(),
        String::new(),
        "subcommands:".to_owned(),
        "  current [name]                  show resolved global state path".to_owned(),
        "  list                            list global Discord state slots".to_owned(),
        "  use <name> [--cwd DIR] [--dry-run]".to_owned(),
        "                                  write .envrc override (DISCORD_STATE_DIR)".to_owned(),
        "  link <name> [--cwd DIR] [--force] [--dry-run]".to_owned(),
        "                                  symlink <cwd>/.discord -> global state slot".to_owned(),
        String::new(),
        "state root: ~/.maw/state/discord-channel/<name>".to_owned(),
    ]);
}

fn state_root(env: &DiscordEnv) -> PathBuf {
    env.home.join(".maw/state/discord-channel")
}

fn state_path(env: &DiscordEnv, name: &str) -> PathBuf {
    state_root(env).join(name)
}

fn state_current(env: &DiscordEnv, name: Option<&String>, log: &mut Vec<String>) -> bool {
    if let Some(name) = name {
        if !discord_validate_name(name, "state name", log) {
            return false;
        }
        log.push(state_path(env, name).display().to_string());
        return true;
    }
    if let Ok(dir) = env::var(CHANNEL_STATE_ENV_KEY).or_else(|_| env::var(STATE_ENV_KEY)) {
        log.push(dir);
        return true;
    }
    log.push(state_root(env).display().to_string());
    true
}

fn state_list(env: &DiscordEnv, log: &mut Vec<String>) -> bool {
    let root = state_root(env);
    let Ok(entries) = fs::read_dir(&root) else {
        log.push(format!("no state slots ({})", root.display()));
        return true;
    };
    let mut names = entries
        .flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            meta.is_dir()
                .then(|| entry.file_name().to_string_lossy().to_string())
        })
        .collect::<Vec<_>>();
    names.sort();
    if names.is_empty() {
        log.push(format!("no state slots ({})", root.display()));
    } else {
        for name in names {
            log.push(name);
        }
    }
    true
}

fn state_use(env: &DiscordEnv, args: &[String], log: &mut Vec<String>) -> bool {
    let (pos, flags) = parse_flags(args);
    let Some(name) = pos.first() else {
        log.push("usage: maw discord state use <name> [--cwd DIR] [--dry-run]".to_owned());
        return true;
    };
    if !discord_validate_name(name, "state name", log) {
        return false;
    }
    let cwd = cwd_from_flags(&flags)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| env.home.clone()));
    let dir = state_path(env, name);
    let envrc = cwd.join(".envrc");
    let body = build_state_envrc(&fs::read_to_string(&envrc).unwrap_or_default(), name, &dir);
    if has_flag(&flags, "dry-run") {
        log.push(body.trim_end().to_owned());
        return true;
    }
    if let Err(error) = fs::create_dir_all(&dir) {
        log.push(format!("✗ failed to create state dir: {error}"));
        return false;
    }
    if let Err(error) = fs::write(&envrc, body) {
        log.push(format!("✗ failed to write {}: {error}", envrc.display()));
        return false;
    }
    let _ = Command::new("direnv")
        .args(["allow", "."])
        .current_dir(&cwd)
        .status();
    log.push(format!("✓ wrote .envrc state override: {name}"));
    log.push(format!("  {STATE_ENV_KEY}={}", dir.display()));
    true
}

fn state_link(env: &DiscordEnv, args: &[String], log: &mut Vec<String>) -> bool {
    let (pos, flags) = parse_flags(args);
    let Some(name) = pos.first() else {
        log.push(
            "usage: maw discord state link <name> [--cwd DIR] [--force] [--dry-run]".to_owned(),
        );
        return true;
    };
    if !discord_validate_name(name, "state name", log) {
        return false;
    }
    let cwd = cwd_from_flags(&flags)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| env.home.clone()));
    let target = state_path(env, name);
    let link = cwd.join(DISCORD_DIR_LINK);
    if has_flag(&flags, "dry-run") {
        log.push(format!("{} -> {}", link.display(), target.display()));
        return true;
    }
    if let Err(error) = fs::create_dir_all(&target) {
        log.push(format!("✗ failed to create state dir: {error}"));
        return false;
    }
    if link.exists() || fs::symlink_metadata(&link).is_ok() {
        if !has_flag(&flags, "force") {
            log.push(format!("✗ {} exists (use --force)", link.display()));
            return false;
        }
        if let Err(error) = fs::remove_file(&link).or_else(|_| fs::remove_dir(&link)) {
            log.push(format!("✗ failed to remove existing link/path: {error}"));
            return false;
        }
    }
    #[cfg(unix)]
    {
        if let Err(error) = std::os::unix::fs::symlink(&target, &link) {
            log.push(format!("✗ symlink failed: {error}"));
            return false;
        }
    }
    #[cfg(not(unix))]
    {
        if let Err(error) = std::os::windows::fs::symlink_dir(&target, &link) {
            log.push(format!("✗ symlink failed: {error}"));
            return false;
        }
    }
    log.push(format!(
        "✓ linked {} -> {}",
        link.display(),
        target.display()
    ));
    true
}

fn cwd_from_flags(flags: &HashMap<String, Vec<String>>) -> Option<PathBuf> {
    flags
        .get("cwd")
        .and_then(|values| values.first())
        .map(PathBuf::from)
}

fn build_state_envrc(existing: &str, name: &str, dir: &Path) -> String {
    let mut kept = Vec::new();
    for line in existing.lines() {
        let s = line.trim();
        if s.starts_with("export DISCORD_CHANNEL_STATE_NAME=")
            || s.starts_with("DISCORD_CHANNEL_STATE_NAME=")
            || s.starts_with("export DISCORD_CHANNEL_STATE_DIR=")
            || s.starts_with("DISCORD_CHANNEL_STATE_DIR=")
            || s.starts_with("export DISCORD_STATE_DIR=")
            || s.starts_with("DISCORD_STATE_DIR=")
        {
            continue;
        }
        kept.push(line.to_owned());
    }
    while kept.last().is_some_and(|line| line.trim().is_empty()) {
        kept.pop();
    }
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(&format!(
        "export DISCORD_CHANNEL_STATE_NAME=\"{name}\"\nexport DISCORD_CHANNEL_STATE_DIR=\"{}\"\nexport DISCORD_STATE_DIR=\"{}\"\n",
        dir.display(),
        dir.display()
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_state_envrc_replaces_old_state_lines_and_keeps_other_content() {
        let existing = r#"export FOO=bar
export DISCORD_CHANNEL_STATE_NAME="old"
DISCORD_CHANNEL_STATE_DIR=/old
export DISCORD_STATE_DIR=/old
export KEEP_ME=1
"#;
        let got = build_state_envrc(existing, "atom", Path::new("/state/atom"));
        assert!(got.contains("export FOO=bar"));
        assert!(got.contains("export KEEP_ME=1"));
        assert!(!got.contains("old"));
        assert!(got.contains("export DISCORD_CHANNEL_STATE_NAME=\"atom\""));
        assert!(got.contains("export DISCORD_CHANNEL_STATE_DIR=\"/state/atom\""));
        assert!(got.contains("export DISCORD_STATE_DIR=\"/state/atom\""));
    }

    #[test]
    fn build_state_envrc_empty_file_writes_only_state_block() {
        let got = build_state_envrc("", "school", Path::new("/state/school"));
        assert_eq!(
            got,
            "export DISCORD_CHANNEL_STATE_NAME=\"school\"\nexport DISCORD_CHANNEL_STATE_DIR=\"/state/school\"\nexport DISCORD_STATE_DIR=\"/state/school\"\n"
        );
    }
}
