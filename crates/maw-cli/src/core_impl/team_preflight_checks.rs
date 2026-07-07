type TeamPreflightCheck43 = (bool, String, String);

#[derive(Debug, Clone)]
struct TeamPreflightCodexHome43 {
    role: String,
    worktree: std::path::PathBuf,
    codex_home: std::path::PathBuf,
}

fn team_preflight_checks(charter: &TeamCharter122) -> Vec<TeamPreflightCheck43> {
    vec![
        team_preflight_team_name_check(charter),
        team_preflight_unique_roles_check(charter),
        team_preflight_existing_artifacts_check(charter),
        team_preflight_charter_schema_check(charter),
        team_preflight_ordering_check(charter),
        team_preflight_maw_engine_check(charter),
        team_preflight_pool_auth_health_check(charter),
        team_preflight_trust_check(charter),
        team_preflight_codex_home_guard_check(charter),
        team_preflight_worktree_overlap_check(charter),
        team_preflight_boot_verification_check(charter),
    ]
}

fn team_preflight_team_name_check(charter: &TeamCharter122) -> TeamPreflightCheck43 {
    (team_validate_name(&charter.name).is_ok(), "team name".to_owned(), format!("'{}' is accepted", charter.name))
}

fn team_preflight_unique_roles_check(charter: &TeamCharter122) -> TeamPreflightCheck43 {
    (team_unique_roles(&charter.members), "member roles".to_owned(), format!("{} unique role(s)", charter.members.len()))
}

fn team_preflight_existing_artifacts_check(charter: &TeamCharter122) -> TeamPreflightCheck43 {
    let collisions = team_plan_artifacts(charter).into_iter().filter(|path| path.exists()).map(|p| p.display().to_string()).collect::<Vec<_>>();
    (collisions.is_empty(), "existing artifacts".to_owned(), if collisions.is_empty() { "no config/inbox/manifest collisions found".to_owned() } else { format!("would refuse to overwrite: {}", collisions.join(", ")) })
}

fn team_preflight_charter_schema_check(charter: &TeamCharter122) -> TeamPreflightCheck43 {
    let mut problems = Vec::new();
    if charter.defaults_worktree {
        problems.push("defaults.worktree is unsupported by the v2 parser contract".to_owned());
    }
    match charter.project.as_deref().map(str::trim) {
        Some(project) if team_preflight_project_is_org_qualified(project) => {}
        Some(project) if !project.is_empty() => problems.push(format!("project must be owner/repo, got {project:?}")),
        _ => problems.push("project must be owner/repo".to_owned()),
    }
    let missing_worktree = charter.members.iter().filter(|member| !member.worktree_opt_out && team_preflight_member_worktree_raw(member).is_none()).map(team_preflight_member_label).collect::<Vec<_>>();
    if !missing_worktree.is_empty() {
        problems.push(format!("members missing worktree: {}", missing_worktree.join(", ")));
    }
    let missing_branch = charter.members.iter().filter(|member| member.branch.as_deref().is_none_or(|branch| branch.trim().is_empty())).map(team_preflight_member_label).collect::<Vec<_>>();
    if !missing_branch.is_empty() {
        problems.push(format!("members missing branch: {}", missing_branch.join(", ")));
    }
    (problems.is_empty(), "charter schema".to_owned(), if problems.is_empty() { "project is owner/repo; each member declares worktree and branch; defaults.worktree absent".to_owned() } else { problems.join("; ") })
}

fn team_preflight_project_is_org_qualified(project: &str) -> bool {
    let mut parts = project.split('/');
    let Some(owner) = parts.next() else { return false; };
    let Some(repo) = parts.next() else { return false; };
    parts.next().is_none()
        && !owner.is_empty()
        && !repo.is_empty()
        && [owner, repo].iter().all(|part| part.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')))
}

fn team_preflight_ordering_check(charter: &TeamCharter122) -> TeamPreflightCheck43 {
    let mut problems = Vec::new();
    match charter.session.as_deref().filter(|session| !session.trim().is_empty()) {
        Some(session) if team_preflight_session_exists(session) => {}
        Some(session) => problems.push(format!("session '{session}' does not exist before spawn")),
        None => problems.push("charter session missing; create/reuse a tmux session before window create".to_owned()),
    }
    let missing = charter.members.iter().filter_map(|member| {
        let path = team_preflight_member_worktree_path(member)?;
        (!path.is_dir()).then(|| format!("{}={}", team_preflight_member_label(member), path.display()))
    }).collect::<Vec<_>>();
    if !missing.is_empty() {
        problems.push(format!("worktree dirs missing before window create: {}", missing.join(", ")));
    }
    (problems.is_empty(), "spawn ordering".to_owned(), if problems.is_empty() { format!("session '{}' exists; {} worktree dir(s) exist before window create", charter.session.as_deref().unwrap_or("<missing>"), charter.members.len()) } else { problems.join("; ") })
}

fn team_preflight_session_exists(session: &str) -> bool {
    if let Some(raw) = std::env::var_os("MAW_RS_TEAM_TMUX_PANES") {
        return raw.to_string_lossy().lines().filter_map(team_t3_parse_pane).any(|pane| pane.session == session);
    }
    TmuxClient::local().has_session(session)
}

fn team_preflight_maw_engine_check(charter: &TeamCharter122) -> TeamPreflightCheck43 {
    let mut problems = Vec::new();
    let mut found = Vec::new();
    for member in &charter.members {
        let Some(worktree) = team_preflight_member_worktree_path(member) else {
            problems.push(format!("{} has no worktree to inspect for .maw-engine", team_preflight_member_label(member)));
            continue;
        };
        if !worktree.is_dir() {
            problems.push(format!("{} worktree missing for .maw-engine: {}", team_preflight_member_label(member), worktree.display()));
            continue;
        }
        let path = worktree.join(".maw-engine");
        if !path.exists() {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            problems.push(format!("{} cannot read {}", team_preflight_member_label(member), path.display()));
            continue;
        };
        let engine = raw.trim();
        if engine.is_empty() {
            problems.push(format!("{} has empty .maw-engine", team_preflight_member_label(member)));
            continue;
        }
        found.push(format!("{}={engine}", team_preflight_member_label(member)));
        if team_preflight_config_command(engine, &worktree).is_none() {
            problems.push(format!("{} .maw-engine '{engine}' is not defined in merged commands config for {}", team_preflight_member_label(member), worktree.display()));
        }
    }
    (problems.is_empty(), ".maw-engine respected".to_owned(), if problems.is_empty() { if found.is_empty() { "no .maw-engine files present".to_owned() } else { format!("resolved .maw-engine command(s): {}", found.join(", ")) } } else { problems.join("; ") })
}

fn team_preflight_pool_auth_health_check(charter: &TeamCharter122) -> TeamPreflightCheck43 {
    let mut problems = Vec::new();
    let mut checked = Vec::new();
    let now_secs = team_preflight_now_secs();
    for item in team_preflight_codex_homes(charter) {
        if !team_preflight_is_codex_team_home(&item.codex_home) {
            continue;
        }
        let auth_path = item.codex_home.join("auth.json");
        checked.push(format!("{}={}", item.role, item.codex_home.display()));
        match team_preflight_access_token_expiry(&auth_path) {
            Ok(exp) if exp > now_secs => {}
            Ok(exp) => problems.push(format!("{} access_token expired at unix {exp}: {}", item.role, auth_path.display())),
            Err(error) => problems.push(format!("{} {error}: {}", item.role, auth_path.display())),
        }
    }
    (problems.is_empty(), "pool auth health".to_owned(), if problems.is_empty() { if checked.is_empty() { "no CODEX_HOME=~/.codex-team/N engines declared".to_owned() } else { format!("access_token is live for {}", checked.join(", ")) } } else { problems.join("; ") })
}

fn team_preflight_trust_check(charter: &TeamCharter122) -> TeamPreflightCheck43 {
    let mut problems = Vec::new();
    let mut checked = Vec::new();
    for item in team_preflight_codex_homes(charter) {
        let config_path = item.codex_home.join("config.toml");
        let worktree_candidates = team_preflight_trust_path_candidates(&item.worktree);
        checked.push(format!("{} in {}", item.role, item.codex_home.display()));
        match std::fs::read_to_string(&config_path) {
            Ok(config) if team_preflight_config_has_trust(&config, &worktree_candidates) => {}
            Ok(_) => problems.push(format!("{} missing trusted project entry for {} in {}", item.role, item.worktree.display(), config_path.display())),
            Err(error) => problems.push(format!("{} cannot read trust config {}: {error}", item.role, config_path.display())),
        }
    }
    (problems.is_empty(), "codex trust".to_owned(), if problems.is_empty() { if checked.is_empty() { "no Codex CODEX_HOME trust checks needed".to_owned() } else { format!("trust present for {}", checked.join(", ")) } } else { problems.join("; ") })
}

fn team_preflight_codex_home_guard_check(charter: &TeamCharter122) -> TeamPreflightCheck43 {
    let mut by_home = std::collections::BTreeMap::<String, Vec<String>>::new();
    for item in team_preflight_codex_homes(charter) {
        by_home.entry(item.codex_home.display().to_string()).or_default().push(item.role);
    }
    let shared = by_home.into_iter().filter_map(|(home, roles)| (roles.len() > 1).then(|| format!("{} share {}", roles.join("+"), home))).collect::<Vec<_>>();
    (shared.is_empty(), "CODEX_HOME isolation".to_owned(), if shared.is_empty() { "each Codex member uses a distinct CODEX_HOME".to_owned() } else { format!("shared CODEX_HOME risks SQLite locks: {}", shared.join(", ")) })
}

fn team_preflight_worktree_overlap_check(charter: &TeamCharter122) -> TeamPreflightCheck43 {
    let mut worktrees = charter.members.iter().filter_map(|member| team_preflight_member_worktree_path(member).map(|path| (team_preflight_member_label(member), team_preflight_normalize_path(&path)))).collect::<Vec<_>>();
    worktrees.sort_by(|a, b| a.1.cmp(&b.1));
    let mut overlaps = Vec::new();
    for left_index in 0..worktrees.len() {
        for right_index in (left_index + 1)..worktrees.len() {
            let (left_role, left_path) = &worktrees[left_index];
            let (right_role, right_path) = &worktrees[right_index];
            if left_path == right_path || left_path.starts_with(right_path) || right_path.starts_with(left_path) {
                overlaps.push(format!("{left_role}={} overlaps {right_role}={}", left_path.display(), right_path.display()));
            }
        }
    }
    (overlaps.is_empty(), "dispatch worktree collision".to_owned(), if overlaps.is_empty() { "member worktree paths are distinct and non-nesting".to_owned() } else { overlaps.join("; ") })
}

fn team_preflight_boot_verification_check(charter: &TeamCharter122) -> TeamPreflightCheck43 {
    let session = charter.session.as_deref().filter(|value| !value.trim().is_empty()).unwrap_or("<session>");
    let windows = charter.members.iter().map(|member| member.name.as_deref().unwrap_or(&member.role)).collect::<Vec<_>>();
    let target = windows.first().copied().unwrap_or("<member-window>");
    (true, "post-spawn boot verification".to_owned(), format!("skipped offline; after spawn run 'maw peek {session}:{target}' for each member and expect an engine idle prompt, not shell/trust/update prompt"))
}

fn team_preflight_codex_homes(charter: &TeamCharter122) -> Vec<TeamPreflightCodexHome43> {
    let mut out = Vec::new();
    for member in &charter.members {
        let Some(worktree) = team_preflight_member_worktree_path(member) else { continue; };
        let engine = team_preflight_effective_engine(member, &worktree);
        let command = team_preflight_config_command(&engine, &worktree).unwrap_or_else(|| engine.clone());
        let Some(codex_home) = team_preflight_codex_home_from_command(&command, &worktree) else { continue; };
        out.push(TeamPreflightCodexHome43 { role: team_preflight_member_label(member), worktree, codex_home });
    }
    out
}

fn team_preflight_effective_engine(member: &TeamCharterMember122, worktree: &std::path::Path) -> String {
    let from_file = std::fs::read_to_string(worktree.join(".maw-engine")).ok().map(|raw| raw.trim().to_owned()).filter(|raw| !raw.is_empty());
    from_file.or_else(|| member.engine.clone()).unwrap_or_else(|| "claude".to_owned())
}

fn team_preflight_config_command(engine: &str, worktree: &std::path::Path) -> Option<String> {
    merged_config_value_in_dir(worktree)
        .get("commands")
        .and_then(serde_json::Value::as_object)
        .and_then(|commands| commands.get(engine))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

fn team_preflight_codex_home_from_command(command: &str, cwd: &std::path::Path) -> Option<std::path::PathBuf> {
    if !team_preflight_command_uses_codex(command) {
        return None;
    }
    let raw = team_preflight_shell_words(command)
        .into_iter()
        .find_map(|word| word.strip_prefix("CODEX_HOME=").map(str::to_owned))
        .unwrap_or_else(|| "~/.codex".to_owned());
    Some(team_preflight_expand_code_home(&raw, cwd))
}

fn team_preflight_command_uses_codex(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    lower.split(|ch: char| ch.is_whitespace() || matches!(ch, ';' | '&' | '|')).any(|word| word.contains("codex") || word.contains("omx"))
}

fn team_preflight_shell_words(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .map(|word| word.trim_matches('"').trim_matches('\'').to_owned())
        .collect()
}

fn team_preflight_expand_code_home(raw: &str, cwd: &std::path::Path) -> std::path::PathBuf {
    let value = raw.trim_matches('"').trim_matches('\'');
    if let Some(rest) = value.strip_prefix("~/") {
        return team_home_dir().join(rest);
    }
    if let Some(rest) = value.strip_prefix("$HOME/") {
        return team_home_dir().join(rest);
    }
    if let Some(rest) = value.strip_prefix("${HOME}/") {
        return team_home_dir().join(rest);
    }
    if value == "$PWD" || value == "${PWD}" {
        return cwd.to_path_buf();
    }
    if let Some(rest) = value.strip_prefix("$PWD/") {
        return cwd.join(rest);
    }
    if let Some(rest) = value.strip_prefix("${PWD}/") {
        return cwd.join(rest);
    }
    let path = std::path::PathBuf::from(value);
    if path.is_absolute() { path } else { cwd.join(path) }
}

fn team_preflight_is_codex_team_home(path: &std::path::Path) -> bool {
    let home = team_home_dir().join(".codex-team");
    path.starts_with(&home)
}

fn team_preflight_access_token_expiry(path: &std::path::Path) -> Result<u64, String> {
    let raw = std::fs::read_to_string(path).map_err(|error| format!("cannot read auth file: {error}"))?;
    let json = serde_json::from_str::<serde_json::Value>(&raw).map_err(|error| format!("cannot parse auth json: {error}"))?;
    let token = json["tokens"]["access_token"].as_str().filter(|value| !value.trim().is_empty()).ok_or_else(|| "auth json missing tokens.access_token".to_owned())?;
    team_preflight_jwt_exp(token).ok_or_else(|| "access_token missing numeric exp claim".to_owned())
}

fn team_preflight_jwt_exp(token: &str) -> Option<u64> {
    let payload = token.split('.').nth(1)?;
    let bytes = team_preflight_base64url_decode(payload).ok()?;
    let json = serde_json::from_slice::<serde_json::Value>(&bytes).ok()?;
    json["exp"].as_u64()
}

fn team_preflight_base64url_decode(raw: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut buffer = 0_u32;
    let mut bits = 0_u8;
    for byte in raw.bytes() {
        if byte == b'=' {
            break;
        }
        let value = match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a' + 26),
            b'0'..=b'9' => u32::from(byte - b'0' + 52),
            b'-' => 62,
            b'_' => 63,
            _ => return Err("invalid base64url byte".to_owned()),
        };
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

fn team_preflight_now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |duration| duration.as_secs())
}

fn team_preflight_config_has_trust(config: &str, candidates: &[String]) -> bool {
    let mut current_project = None::<String>;
    for raw in config.lines() {
        let line = raw.trim();
        if let Some(project) = line.strip_prefix("[projects.\"").and_then(|rest| rest.strip_suffix("\"]")) {
            current_project = Some(project.to_owned());
            continue;
        }
        if line.starts_with('[') {
            current_project = None;
            continue;
        }
        if line.starts_with("trust_level") && line.contains("\"trusted\"") && current_project.as_ref().is_some_and(|project| candidates.iter().any(|candidate| candidate == project)) {
            return true;
        }
    }
    false
}

fn team_preflight_trust_path_candidates(path: &std::path::Path) -> Vec<String> {
    let mut out = vec![path.display().to_string()];
    if let Ok(canonical) = path.canonicalize() {
        let canonical = canonical.display().to_string();
        if !out.iter().any(|item| item == &canonical) {
            out.push(canonical);
        }
    }
    out
}

fn team_preflight_member_worktree_raw(member: &TeamCharterMember122) -> Option<&str> {
    if member.worktree_opt_out { return None; }
    member.worktree.as_deref().map(str::trim).filter(|value| !value.is_empty())
}

fn team_preflight_member_worktree_path(member: &TeamCharterMember122) -> Option<std::path::PathBuf> {
    team_preflight_member_worktree_raw(member).map(team_preflight_abs_path)
}

fn team_preflight_abs_path(raw: &str) -> std::path::PathBuf {
    let path = std::path::PathBuf::from(raw);
    if path.is_absolute() { path } else { std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")).join(path) }
}

fn team_preflight_normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn team_preflight_member_label(member: &TeamCharterMember122) -> String {
    if member.role.trim().is_empty() { "<missing-role>".to_owned() } else { member.role.clone() }
}

#[cfg(test)]
mod team_preflight_tests {
    use super::*;

    fn temp_root(name: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("maw-rs-team-preflight-{name}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp root");
        path
    }

    fn with_env<F>(root: &std::path::Path, test: F)
    where
        F: FnOnce(),
    {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = EnvVarRestore::capture("HOME");
        let _maw_home = EnvVarRestore::capture("MAW_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _xdg = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _panes = EnvVarRestore::capture("MAW_RS_TEAM_TMUX_PANES");
        std::env::set_var("HOME", root.join("home"));
        std::env::remove_var("MAW_HOME");
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("xdg"));
        std::env::set_var("MAW_RS_TEAM_TMUX_PANES", "team-sess|lead|codex|/tmp|%1\n");
        std::fs::create_dir_all(root.join("home")).expect("home");
        std::fs::create_dir_all(root.join("config")).expect("config");
        test();
        let _ = std::fs::remove_dir_all(root);
    }

    fn charter(root: &std::path::Path) -> TeamCharter122 {
        TeamCharter122 {
            name: "team".to_owned(),
            project: Some("acme/repo".to_owned()),
            description: String::new(),
            goal: String::new(),
            session: Some("team-sess".to_owned()),
            members: vec![
                member("one", &root.join("wt-one"), "agents/one", "local-one"),
                member("two", &root.join("wt-two"), "agents/two", "local-two"),
            ],
            defaults_worktree: false,
            governance_requires_human_approval: false,
            ..Default::default()
        }
    }

    fn member(role: &str, worktree: &std::path::Path, branch: &str, engine: &str) -> TeamCharterMember122 {
        TeamCharterMember122 {
            role: role.to_owned(),
            name: Some(role.to_owned()),
            engine: Some(engine.to_owned()),
            worktree: Some(worktree.display().to_string()),
            branch: Some(branch.to_owned()),
            ..Default::default()
        }
    }

    fn write_config(root: &std::path::Path) {
        std::fs::write(
            root.join("config/maw.config.50.json"),
            r#"{"commands":{"local-one":"CODEX_HOME=$PWD/.codex codex --model gpt-5.5","local-two":"CODEX_HOME=$PWD/.codex codex --model gpt-5.5","pool-one":"CODEX_HOME=~/.codex-team/1 codex --model gpt-5.5","pool-two":"CODEX_HOME=~/.codex-team/2 codex --model gpt-5.5"}}"#,
        )
        .expect("config");
    }

    fn create_worktrees(charter: &TeamCharter122) {
        for member in &charter.members {
            let path = team_preflight_member_worktree_path(member).expect("worktree");
            std::fs::create_dir_all(path).expect("worktree dir");
        }
    }

    fn write_local_trust(charter: &TeamCharter122) {
        for member in &charter.members {
            let path = team_preflight_member_worktree_path(member).expect("worktree");
            std::fs::create_dir_all(path.join(".codex")).expect("codex dir");
            std::fs::write(path.join(".codex/config.toml"), format!("[projects.\"{}\"]\ntrust_level = \"trusted\"\n", path.display())).expect("trust");
        }
    }

    fn write_pool_auth(root: &std::path::Path, index: u8, access_exp: u64, id_exp: u64) {
        let dir = root.join(format!("home/.codex-team/{index}"));
        std::fs::create_dir_all(&dir).expect("pool dir");
        let body = serde_json::json!({
            "tokens": {
                "access_token": jwt(access_exp),
                "id_token": jwt(id_exp)
            }
        });
        std::fs::write(dir.join("auth.json"), body.to_string()).expect("auth");
    }

    fn write_pool_trust(root: &std::path::Path, index: u8, worktree: &std::path::Path) {
        let dir = root.join(format!("home/.codex-team/{index}"));
        std::fs::create_dir_all(&dir).expect("pool dir");
        std::fs::write(dir.join("config.toml"), format!("[projects.\"{}\"]\ntrust_level = \"trusted\"\n", worktree.display())).expect("trust");
    }

    fn jwt(exp: u64) -> String {
        format!("e30.{}.sig", base64url(&serde_json::json!({"exp": exp}).to_string()))
    }

    fn base64url(raw: &str) -> String {
        const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let bytes = raw.as_bytes();
        let mut out = String::new();
        let mut index = 0;
        while index < bytes.len() {
            let b0 = bytes[index];
            let b1 = bytes.get(index + 1).copied().unwrap_or(0);
            let b2 = bytes.get(index + 2).copied().unwrap_or(0);
            out.push(TABLE[(b0 >> 2) as usize] as char);
            out.push(TABLE[(((b0 & 0b11) << 4) | (b1 >> 4)) as usize] as char);
            if index + 1 < bytes.len() {
                out.push(TABLE[(((b1 & 0b1111) << 2) | (b2 >> 6)) as usize] as char);
            }
            if index + 2 < bytes.len() {
                out.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
            }
            index += 3;
        }
        out
    }

    #[test]
    fn preflight_charter_schema_pass_and_fail() {
        let root = temp_root("schema");
        with_env(&root, || {
            let ok = charter(&root);
            assert!(team_preflight_charter_schema_check(&ok).0);

            let bad = team_parse_charter(
                r"
name: bad
project: repo-only
session: team-sess
defaults:
  worktree: true
members:
  - role: one
    worktree: agents/one
",
            )
            .expect("parse");
            let check = team_preflight_charter_schema_check(&bad);
            assert!(!check.0);
            assert!(check.2.contains("defaults.worktree"));
            assert!(check.2.contains("project must be owner/repo"));
            assert!(check.2.contains("members missing branch"));
        });
    }

    #[test]
    fn preflight_ordering_pass_and_fail() {
        let root = temp_root("ordering");
        with_env(&root, || {
            let mut ok = charter(&root);
            create_worktrees(&ok);
            assert!(team_preflight_ordering_check(&ok).0);

            ok.session = Some("missing-session".to_owned());
            let check = team_preflight_ordering_check(&ok);
            assert!(!check.0);
            assert!(check.2.contains("does not exist"));

            let mut missing = charter(&root);
            missing.members[0].worktree = Some(root.join("missing-wt").display().to_string());
            let check = team_preflight_ordering_check(&missing);
            assert!(!check.0);
            assert!(check.2.contains("worktree dirs missing"));
        });
    }

    #[test]
    fn preflight_maw_engine_pass_and_fail() {
        let root = temp_root("maw-engine");
        with_env(&root, || {
            write_config(&root);
            let ok = charter(&root);
            create_worktrees(&ok);
            std::fs::write(root.join("wt-one/.maw-engine"), "local-one\n").expect("engine");
            assert!(team_preflight_maw_engine_check(&ok).0);

            std::fs::write(root.join("wt-one/.maw-engine"), "missing-engine\n").expect("engine");
            let check = team_preflight_maw_engine_check(&ok);
            assert!(!check.0);
            assert!(check.2.contains("not defined in merged commands config"));
        });
    }

    #[test]
    fn preflight_pool_auth_health_uses_access_token_not_id_token() {
        let root = temp_root("auth");
        with_env(&root, || {
            write_config(&root);
            let mut ok = charter(&root);
            ok.members[0].engine = Some("pool-one".to_owned());
            ok.members.truncate(1);
            create_worktrees(&ok);
            write_pool_auth(&root, 1, 4_102_444_800, 1);
            assert!(team_preflight_pool_auth_health_check(&ok).0);

            write_pool_auth(&root, 1, 1, 4_102_444_800);
            let check = team_preflight_pool_auth_health_check(&ok);
            assert!(!check.0);
            assert!(check.2.contains("access_token expired"));
        });
    }

    #[test]
    fn preflight_trust_pass_and_fail_for_actual_codex_home() {
        let root = temp_root("trust");
        with_env(&root, || {
            write_config(&root);
            let ok = charter(&root);
            create_worktrees(&ok);
            write_local_trust(&ok);
            assert!(team_preflight_trust_check(&ok).0);

            std::fs::remove_file(root.join("wt-one/.codex/config.toml")).expect("remove trust");
            let check = team_preflight_trust_check(&ok);
            assert!(!check.0);
            assert!(check.2.contains("cannot read trust config"));
        });
    }

    #[test]
    fn preflight_codex_home_guard_pass_and_fail() {
        let root = temp_root("home-guard");
        with_env(&root, || {
            write_config(&root);
            let ok = charter(&root);
            create_worktrees(&ok);
            assert!(team_preflight_codex_home_guard_check(&ok).0);

            let mut bad = ok.clone();
            bad.members[0].engine = Some("pool-one".to_owned());
            bad.members[1].engine = Some("pool-one".to_owned());
            let check = team_preflight_codex_home_guard_check(&bad);
            assert!(!check.0);
            assert!(check.2.contains("shared CODEX_HOME"));
        });
    }

    #[test]
    fn preflight_worktree_overlap_pass_and_fail() {
        let root = temp_root("overlap");
        with_env(&root, || {
            let ok = charter(&root);
            assert!(team_preflight_worktree_overlap_check(&ok).0);

            let mut bad = ok.clone();
            bad.members[1].worktree = Some(root.join("wt-one/nested").display().to_string());
            let check = team_preflight_worktree_overlap_check(&bad);
            assert!(!check.0);
            assert!(check.2.contains("overlaps"));
        });
    }

    #[test]
    fn preflight_boot_verification_is_manual_helper() {
        let root = temp_root("boot");
        with_env(&root, || {
            let ok = charter(&root);
            let check = team_preflight_boot_verification_check(&ok);
            assert!(check.0);
            assert!(check.2.contains("maw peek team-sess:one"));
            assert!(check.2.contains("not shell/trust/update prompt"));
        });
    }

    #[test]
    fn preflight_full_check_passes_for_complete_fixture() {
        let root = temp_root("full");
        with_env(&root, || {
            write_config(&root);
            let ok = charter(&root);
            create_worktrees(&ok);
            write_local_trust(&ok);
            let checks = team_preflight_checks(&ok);
            assert!(checks.iter().all(|(ok, _, _)| *ok), "{checks:?}");
        });
    }

    #[test]
    fn preflight_pool_trust_uses_pool_config_not_worktree_local() {
        let root = temp_root("pool-trust");
        with_env(&root, || {
            write_config(&root);
            let mut ok = charter(&root);
            ok.members[0].engine = Some("pool-one".to_owned());
            ok.members.truncate(1);
            create_worktrees(&ok);
            let worktree = team_preflight_member_worktree_path(&ok.members[0]).expect("worktree");
            write_pool_trust(&root, 1, &worktree);
            assert!(team_preflight_trust_check(&ok).0);

            std::fs::remove_file(root.join("home/.codex-team/1/config.toml")).expect("remove pool trust");
            write_local_trust(&ok);
            let check = team_preflight_trust_check(&ok);
            assert!(!check.0);
            assert!(check.2.contains("home/.codex-team/1/config.toml"));
        });
    }
}
