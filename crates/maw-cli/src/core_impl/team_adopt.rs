const DISPATCH_381: &[DispatcherEntry] = &[];

const TEAM_ADOPT_USAGE: &str = "usage: maw team adopt <team> <target> [--role <role>] [--engine <engine>] [--worktree <path|false>] [--charter <path>] [--dry-run]";
const TEAM_RELEASE_USAGE: &str = "usage: maw team release <team> <member> [--charter <path>] [--dry-run]";

#[derive(Debug, Clone, Default)]
struct TeamAdoptOptions381 {
    team: String,
    target: String,
    role: Option<String>,
    engine: Option<String>,
    worktree: Option<String>,
    charter_path: Option<String>,
    dry_run: bool,
}

/// Register an already-running oracle into a team roster. Never spawns, never kills.
fn team_adopt(argv: &[String]) -> Result<String, String> {
    let opts = team_adopt_parse(argv)?;
    let charter_path = team_t3_resolve_charter_path(&opts.team, opts.charter_path.as_deref())?;
    let text = std::fs::read_to_string(&charter_path).map_err(|error| format!("team adopt: read charter failed: {error}"))?;
    let charter = team_parse_charter(&text)?;
    let member = team_adopt_resolve(&opts, &charter)?;
    team_adopt_reject_duplicate(&charter, &member)?;
    if opts.dry_run {
        return Ok(team_adopt_render(&opts.team, &member, &charter_path, true));
    }
    let updated = team_adopt_charter_text(&text, &member)?;
    team_atomic_write_0600(std::path::Path::new(&charter_path), &updated)?;
    let paths = team_paths(&opts.team);
    team_t5_upsert_tool_member(&paths.tool_config, &member.role, member.engine.as_deref().unwrap_or("claude"), member.model.as_deref())?;
    Ok(team_adopt_render(&opts.team, &member, &charter_path, false))
}

/// Drop a member from the roster without tearing its pane down (inverse of adopt).
fn team_release(argv: &[String]) -> Result<String, String> {
    let (team, selector, charter_opt, dry_run) = team_release_parse(argv)?;
    let charter_path = team_t3_resolve_charter_path(&team, charter_opt.as_deref())?;
    let text = std::fs::read_to_string(&charter_path).map_err(|error| format!("team release: read charter failed: {error}"))?;
    let updated = team_remove_charter_text(&text, &selector)?;
    if dry_run {
        return Ok(format!("team release: {team}\nmember\t{selector}\naction\tdry-run\n\n  charter: would drop '{selector}' from {charter_path}\n  teardown: none (pane left running)\n\nNo changes made\n"));
    }
    team_atomic_write_0600(std::path::Path::new(&charter_path), &updated)?;
    Ok(format!("team release: {team}\nmember\t{selector}\naction\tcharter-edit\n\n  charter: dropped '{selector}' from {charter_path}\n  teardown: none (pane left running)\n"))
}

fn team_adopt_parse(argv: &[String]) -> Result<TeamAdoptOptions381, String> {
    let mut opts = TeamAdoptOptions381::default();
    let mut positional = Vec::new();
    let mut index = 1;
    while index < argv.len() {
        match argv[index].as_str() {
            "--dry-run" => opts.dry_run = true,
            "--role" => { index += 1; opts.role = Some(team_adopt_next(argv, index, "--role")?); },
            "--engine" | "-e" => { index += 1; opts.engine = Some(team_adopt_next(argv, index, "--engine")?); },
            "--worktree" | "--cwd" => { index += 1; opts.worktree = Some(team_adopt_next(argv, index, "--worktree")?); },
            "--charter" => { index += 1; opts.charter_path = Some(team_adopt_next(argv, index, "--charter")?); },
            value if value.starts_with('-') => return Err(format!("team adopt: unknown argument {value}")),
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }
    if positional.len() != 2 { return Err(TEAM_ADOPT_USAGE.to_owned()); }
    positional[0].clone_into(&mut opts.team);
    positional[1].clone_into(&mut opts.target);
    team_validate_name(&opts.team)?;
    if let Some(path) = &opts.charter_path { team_validate_path_arg(path)?; }
    if let Some(role) = &opts.role { team_t5b_validate_member(role)?; }
    if let Some(engine) = &opts.engine { team_t5b_validate_member(engine)?; }
    Ok(opts)
}

fn team_release_parse(argv: &[String]) -> Result<(String, String, Option<String>, bool), String> {
    let mut positional = Vec::new();
    let mut charter = None;
    let mut dry_run = false;
    let mut index = 1;
    while index < argv.len() {
        match argv[index].as_str() {
            "--dry-run" => dry_run = true,
            "--charter" => { index += 1; charter = Some(team_adopt_next(argv, index, "--charter")?); },
            value if value.starts_with('-') => return Err(format!("team release: unknown argument {value}")),
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }
    if positional.len() != 2 { return Err(TEAM_RELEASE_USAGE.to_owned()); }
    team_validate_name(&positional[0])?;
    team_t3_validate_token(&positional[1], "member selector")?;
    if let Some(path) = &charter { team_validate_path_arg(path)?; }
    Ok((positional[0].clone(), positional[1].clone(), charter, dry_run))
}

fn team_adopt_next(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let value = argv.get(index).ok_or_else(|| format!("team adopt: {flag} requires a value"))?;
    team_t3_validate_token(value, flag)?;
    Ok(value.clone())
}

/// Federation targets (`<node>:<session>`) are roster-only: no tmux, trust required.
fn team_adopt_resolve(opts: &TeamAdoptOptions381, charter: &TeamCharter122) -> Result<TeamCharterMember122, String> {
    let (head, tail) = team_adopt_split_target(&opts.target, charter, opts)?;
    if team_adopt_is_named_peer(&head) {
        team_adopt_require_trust(&head)?;
        return Ok(TeamCharterMember122 {
            role: opts.role.clone().unwrap_or_else(|| tail.clone()),
            name: Some(tail.clone()),
            engine: opts.engine.clone(),
            target: Some(format!("{head}:{tail}")),
            worktree_opt_out: true,
            adopted: true,
            ..Default::default()
        });
    }
    let pane = team_adopt_live_pane(&head, &tail)?;
    let worktree = team_adopt_worktree(opts, &pane);
    Ok(TeamCharterMember122 {
        role: opts.role.clone().unwrap_or_else(|| tail.clone()),
        name: Some(tail.clone()),
        engine: Some(opts.engine.clone().unwrap_or_else(|| team_adopt_engine_of(&pane.command))),
        target: Some(format!("{head}:{tail}")),
        worktree_opt_out: worktree.is_none(),
        worktree,
        adopted: true,
        ..Default::default()
    })
}

fn team_adopt_split_target(target: &str, charter: &TeamCharter122, opts: &TeamAdoptOptions381) -> Result<(String, String), String> {
    let (head, tail) = match target.split_once(':') {
        Some((head, tail)) => (head.to_owned(), tail.to_owned()),
        None => (team_adopt_current_session(charter)?, target.to_owned()),
    };
    team_t3_validate_session(&head)?;
    team_t5b_validate_window(&tail)?;
    let _ = opts;
    Ok((head, tail))
}

fn team_adopt_current_session(charter: &TeamCharter122) -> Result<String, String> {
    if let Ok(session) = std::env::var("MAW_RS_TEAM_SESSION") {
        if !session.trim().is_empty() { return Ok(session.trim().to_owned()); }
    }
    charter.session.clone().filter(|value| !value.trim().is_empty()).ok_or_else(|| "team adopt: bare window needs a session — pass <session>:<window> or set charter session".to_owned())
}

fn team_adopt_is_named_peer(node: &str) -> bool {
    team_invite_load_config().named_peers.iter().any(|peer| peer.name == node || peer.node.as_deref() == Some(node))
}

fn team_adopt_require_trust(node: &str) -> Result<(), String> {
    let config = team_invite_load_config();
    let from = config.node.clone().unwrap_or_default();
    if TeamInviteSystemTrust125.team_invite_is_trusted(&from, node, "team-invite") { return Ok(()); }
    Err(format!("team adopt refuse untrusted federation peer '{node}' — run: maw team invite <team> {node}"))
}

fn team_adopt_live_pane(session: &str, window: &str) -> Result<TeamPane124, String> {
    let panes = team_t3_panes();
    let mut matches = panes.into_iter().filter(|pane| pane.session == session && pane.window == window).collect::<Vec<_>>();
    if matches.is_empty() { return Err(format!("team adopt refuse missing live target: {session}:{window}")); }
    if matches.len() > 1 { return Err(format!("team adopt refuse ambiguous live target: {session}:{window}")); }
    let pane = matches.remove(0);
    if !team_t3_is_live_command(&pane.command) {
        return Err(format!("team adopt refuse target that is not a running oracle: {session}:{window} (command: {})", pane.command));
    }
    Ok(pane)
}

fn team_adopt_worktree(opts: &TeamAdoptOptions381, pane: &TeamPane124) -> Option<String> {
    if let Some(value) = &opts.worktree {
        if team_worktree_literal_is_opt_out(value) { return None; }
        return Some(value.clone());
    }
    team_t5_canonical_work_path(&pane.path).ok().map(|path| path.display().to_string())
}

fn team_adopt_engine_of(command: &str) -> String {
    let lower = command.to_ascii_lowercase();
    for engine in ["claude", "codex", "omx"] {
        if lower.contains(engine) { return engine.to_owned(); }
    }
    "claude".to_owned()
}

fn team_adopt_reject_duplicate(charter: &TeamCharter122, member: &TeamCharterMember122) -> Result<(), String> {
    let name = member.name.as_deref().unwrap_or_default();
    for existing in &charter.members {
        if existing.role == member.role || existing.name.as_deref() == Some(name) {
            return Err(format!("team adopt refuse duplicate member '{}': already in charter as role '{}'", member.role, existing.role));
        }
    }
    Ok(())
}

/// Append the member to the charter, preserving the existing YAML/JSON shape.
fn team_adopt_charter_text(text: &str, member: &TeamCharterMember122) -> Result<String, String> {
    if text.trim_start().starts_with('{') { return team_adopt_json_charter_text(text, member); }
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let trailing = text.ends_with('\n');
    let mut lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
    let blocks = team_remove_member_blocks(&lines);
    let last = blocks.last().ok_or_else(|| "team adopt: charter has no members block".to_owned())?;
    let dash_indent = team_remove_indent(&lines[last.start]);
    let insert_at = last.end;
    let block = team_adopt_yaml_block(member, dash_indent);
    lines.splice(insert_at..insert_at, block);
    let mut out = lines.join(eol);
    if trailing { out.push_str(eol); }
    Ok(out)
}

fn team_adopt_yaml_block(member: &TeamCharterMember122, dash_indent: usize) -> Vec<String> {
    let dash = " ".repeat(dash_indent);
    let field = " ".repeat(dash_indent + 2);
    let mut out = vec![format!("{dash}- role: {}", member.role)];
    if let Some(name) = &member.name { out.push(format!("{field}name: {name}")); }
    if let Some(engine) = &member.engine { out.push(format!("{field}engine: {engine}")); }
    match &member.worktree {
        Some(worktree) => out.push(format!("{field}worktree: {worktree}")),
        None => out.push(format!("{field}worktree: false")),
    }
    if let Some(target) = &member.target { out.push(format!("{field}target: {target}")); }
    out.push(format!("{field}adopted: true"));
    out
}

fn team_adopt_json_charter_text(text: &str, member: &TeamCharterMember122) -> Result<String, String> {
    let mut value: serde_json::Value = serde_json::from_str(text).map_err(|error| format!("team adopt json charter parse failed: {error}"))?;
    let members = value["members"].as_array_mut().ok_or_else(|| "team adopt json charter missing members".to_owned())?;
    let mut entry = serde_json::json!({"role": member.role, "adopted": true});
    if let Some(name) = &member.name { entry["name"] = serde_json::json!(name); }
    if let Some(engine) = &member.engine { entry["engine"] = serde_json::json!(engine); }
    if let Some(target) = &member.target { entry["target"] = serde_json::json!(target); }
    entry["worktree"] = member.worktree.as_ref().map_or_else(|| serde_json::json!(false), |value| serde_json::json!(value));
    members.push(entry);
    serde_json::to_string_pretty(&value).map(|body| body + "\n").map_err(|error| format!("team adopt json charter encode failed: {error}"))
}

#[cfg(test)]
mod team_adopt_tests381 {
    use super::*;

    const YAML_CHARTER: &str = "name: alpha\nsession: alpha\nmembers:\n  - role: builder\n    name: builder\n    worktree: false\n";

    fn adopt_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("maw-rs-team-adopt-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        root
    }

    fn write_charter(root: &std::path::Path, body: &str, name: &str) -> String {
        let path = root.join(name);
        std::fs::write(&path, body).expect("charter");
        path.display().to_string()
    }

    fn argv(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn team_adopt_appends_live_pane_without_spawning_or_killing() {
        let _guard = env_test_lock();
        let root = adopt_root("append");
        let charter = write_charter(&root, YAML_CHARTER, "alpha.yaml");
        let spawn_log = root.join("spawn.log");
        let tmux_log = root.join("tmux.log");
        let _panes = EnvVarRestore::capture("MAW_RS_TEAM_TMUX_PANES");
        let _spawn = EnvVarRestore::capture("MAW_RS_TEAM_FAKE_SPAWN_LOG");
        let _tmux = EnvVarRestore::capture("MAW_RS_TEAM_FAKE_TMUX_LOG");
        let _home = EnvVarRestore::capture("HOME");
        std::env::set_var("MAW_RS_TEAM_TMUX_PANES", "alpha|chorus|claude|/nowhere|%7\n");
        std::env::set_var("MAW_RS_TEAM_FAKE_SPAWN_LOG", &spawn_log);
        std::env::set_var("MAW_RS_TEAM_FAKE_TMUX_LOG", &tmux_log);
        std::env::set_var("HOME", &root);

        let out = team_adopt(&argv(&["adopt", "alpha", "alpha:chorus", "--charter", &charter])).expect("adopt");

        let body = std::fs::read_to_string(&charter).expect("read back");
        assert!(body.contains("- role: chorus"), "{body}");
        assert!(body.contains("target: alpha:chorus"), "{body}");
        assert!(body.contains("adopted: true"), "{body}");
        assert!(body.contains("- role: builder"), "existing member must survive: {body}");
        assert!(out.contains("no spawn, no kill"), "{out}");
        // The whole point of adopt: nothing was spawned and no tmux command ran.
        assert!(!spawn_log.exists(), "adopt must not spawn");
        assert!(!tmux_log.exists(), "adopt must not drive tmux");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn team_adopt_refuses_missing_dead_and_ambiguous_targets() {
        let _guard = env_test_lock();
        let root = adopt_root("refuse");
        let charter = write_charter(&root, YAML_CHARTER, "alpha.yaml");
        let _panes = EnvVarRestore::capture("MAW_RS_TEAM_TMUX_PANES");
        let _home = EnvVarRestore::capture("HOME");
        std::env::set_var("HOME", &root);

        std::env::set_var("MAW_RS_TEAM_TMUX_PANES", "alpha|other|claude|/nowhere|%1\n");
        let missing = team_adopt(&argv(&["adopt", "alpha", "alpha:chorus", "--charter", &charter])).expect_err("missing");
        assert!(missing.contains("refuse missing live target"), "{missing}");

        std::env::set_var("MAW_RS_TEAM_TMUX_PANES", "alpha|chorus|bash|/nowhere|%1\n");
        let dead = team_adopt(&argv(&["adopt", "alpha", "alpha:chorus", "--charter", &charter])).expect_err("dead");
        assert!(dead.contains("not a running oracle"), "{dead}");

        std::env::set_var("MAW_RS_TEAM_TMUX_PANES", "alpha|chorus|claude|/nowhere|%1\nalpha|chorus|codex|/nowhere|%2\n");
        let ambiguous = team_adopt(&argv(&["adopt", "alpha", "alpha:chorus", "--charter", &charter])).expect_err("ambiguous");
        assert!(ambiguous.contains("ambiguous live target"), "{ambiguous}");

        std::env::set_var("MAW_RS_TEAM_TMUX_PANES", "alpha|builder|claude|/nowhere|%1\n");
        let duplicate = team_adopt(&argv(&["adopt", "alpha", "alpha:builder", "--charter", &charter])).expect_err("duplicate");
        assert!(duplicate.contains("refuse duplicate member"), "{duplicate}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The pane an adopted member points at is not ours: it can die at any time after
    /// the charter was written. A dead adopted member must still be releasable, must
    /// still refuse `team remove`, and must never trigger a teardown of a pane that is
    /// already gone.
    #[test]
    fn team_adopt_member_that_dies_later_still_releases_and_never_tears_down() {
        let _guard = env_test_lock();
        let root = adopt_root("died");
        let charter = write_charter(&root, YAML_CHARTER, "alpha.yaml");
        let remove_log = root.join("remove.log");
        let spawn_log = root.join("spawn.log");
        let _panes = EnvVarRestore::capture("MAW_RS_TEAM_TMUX_PANES");
        let _remove = EnvVarRestore::capture("MAW_RS_TEAM_REMOVE_FAKE_LOG");
        let _spawn = EnvVarRestore::capture("MAW_RS_TEAM_FAKE_SPAWN_LOG");
        let _home = EnvVarRestore::capture("HOME");
        std::env::set_var("MAW_RS_TEAM_REMOVE_FAKE_LOG", &remove_log);
        std::env::set_var("MAW_RS_TEAM_FAKE_SPAWN_LOG", &spawn_log);
        std::env::set_var("HOME", &root);

        // Alive at adopt time.
        std::env::set_var("MAW_RS_TEAM_TMUX_PANES", "alpha|chorus|claude|/nowhere|%9\n");
        team_adopt(&argv(&["adopt", "alpha", "alpha:chorus", "--charter", &charter])).expect("adopt while live");
        let body = std::fs::read_to_string(&charter).expect("read back");
        assert!(body.contains("adopted: true"), "{body}");

        // The pane dies afterwards: no panes left at all.
        std::env::set_var("MAW_RS_TEAM_TMUX_PANES", "");
        let parsed = team_parse_charter(&std::fs::read_to_string(&charter).expect("read")).expect("charter parses");
        let refused = team_remove_reject_adopted(&parsed, "chorus").expect_err("remove still refuses a dead adopted member");
        assert!(refused.contains("team release"), "{refused}");

        // Release is charter-only, so a dead pane must not make it fail.
        let out = team_release(&argv(&["release", "alpha", "chorus", "--charter", &charter])).expect("release after death");
        assert!(out.contains("teardown: none"), "{out}");
        let after = std::fs::read_to_string(&charter).expect("read back");
        assert!(!after.contains("chorus"), "dead adopted member must be dropped: {after}");
        assert!(after.contains("- role: builder"), "spawned member must survive: {after}");
        assert!(!remove_log.exists(), "must not tear down a pane that is already gone");
        assert!(!spawn_log.exists(), "must not respawn a dead adopted member");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The bug this locks: `team_t3_classify` matched panes by window name only, so an adopted
    /// member whose pane died read back as `missing` and `team up` spawned a brand new oracle
    /// over it — and `team down` ran `maw done` on a live adopted pane we never started.
    #[test]
    fn team_up_and_down_never_touch_an_adopted_pane() {
        let _guard = env_test_lock();
        let root = adopt_root("upafterdeath");
        let charter = write_charter(&root, YAML_CHARTER, "alpha.yaml");
        let _panes = EnvVarRestore::capture("MAW_RS_TEAM_TMUX_PANES");
        let _home = EnvVarRestore::capture("HOME");
        std::env::set_var("HOME", &root);

        std::env::set_var("MAW_RS_TEAM_TMUX_PANES", "alpha|chorus|claude|/nowhere|%11\n");
        team_adopt(&argv(&["adopt", "alpha", "alpha:chorus", "--charter", &charter])).expect("adopt while live");
        let parsed = team_parse_charter(&std::fs::read_to_string(&charter).expect("read")).expect("charter parses");
        let opts = TeamT3Options124::default();

        // Still live: team down must keep it instead of running `maw done` on someone else's pane.
        let live = team_t3_classify(parsed.members.iter().find(|m| m.role == "chorus").expect("member"), &opts, "alpha", &team_t3_panes());
        assert_eq!(live.state, "live");
        let member = parsed.members.iter().find(|m| m.role == "chorus").expect("member");
        assert_eq!(team_down_keep_reason(member, &live, &[], true).as_deref(), Some("adopted"), "team down --all must still keep an adopted pane");
        let builder = parsed.members.iter().find(|m| m.role == "builder").expect("builder");
        let builder_item = team_t3_classify(builder, &opts, "alpha", &team_t3_panes());
        assert_eq!(team_down_keep_reason(builder, &builder_item, &[], true), None, "a member we spawned is still torn down normally");

        // Pane dies afterwards: must not classify as missing/dead, must not wake or resume.
        std::env::set_var("MAW_RS_TEAM_TMUX_PANES", "");
        let gone = team_t3_classify(member, &opts, "alpha", &team_t3_panes());
        assert_eq!(gone.state, TEAM_ADOPTED_GONE, "a dead adopted pane must not read back as missing");
        assert_eq!(team_t3_up_action(&gone, &opts), TEAM_ADOPTED_GONE_ACTION, "team up must not fresh wake");
        let forced = TeamT3Options124 { flags: TEAM_T3_FORCE, ..TeamT3Options124::default() };
        assert_eq!(team_t3_up_action(&gone, &forced), TEAM_ADOPTED_GONE_ACTION, "--force must not fresh wake either");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn team_adopt_json_charter_keeps_shape_and_release_drops_without_teardown() {
        let _guard = env_test_lock();
        let root = adopt_root("json");
        let json = "{\n  \"name\": \"alpha\",\n  \"session\": \"alpha\",\n  \"members\": [{\"role\": \"builder\", \"worktree\": false}]\n}\n";
        let charter = write_charter(&root, json, "alpha.json");
        let remove_log = root.join("remove.log");
        let _panes = EnvVarRestore::capture("MAW_RS_TEAM_TMUX_PANES");
        let _remove = EnvVarRestore::capture("MAW_RS_TEAM_REMOVE_FAKE_LOG");
        let _home = EnvVarRestore::capture("HOME");
        std::env::set_var("MAW_RS_TEAM_TMUX_PANES", "alpha|chorus|codex|/nowhere|%3\n");
        std::env::set_var("MAW_RS_TEAM_REMOVE_FAKE_LOG", &remove_log);
        std::env::set_var("HOME", &root);

        team_adopt(&argv(&["adopt", "alpha", "alpha:chorus", "--charter", &charter])).expect("adopt json");
        let body = std::fs::read_to_string(&charter).expect("read back");
        let parsed = team_parse_charter(&body).expect("charter still parses");
        let adopted = parsed.members.iter().find(|member| member.role == "chorus").expect("adopted member");
        assert!(adopted.adopted, "adopted flag must round-trip");
        assert_eq!(adopted.target.as_deref(), Some("alpha:chorus"));
        assert_eq!(adopted.engine.as_deref(), Some("codex"), "engine derived from pane command");

        // remove must refuse it, release must drop it and never call maw done.
        let refused = team_remove_reject_adopted(&parsed, "chorus").expect_err("remove refuses adopted");
        assert!(refused.contains("team release"), "{refused}");
        team_release(&argv(&["release", "alpha", "chorus", "--charter", &charter])).expect("release");
        let after = std::fs::read_to_string(&charter).expect("read back");
        assert!(!after.contains("chorus"), "{after}");
        assert!(!remove_log.exists(), "release must not tear anything down");
        let _ = std::fs::remove_dir_all(&root);
    }
}

fn team_adopt_render(team: &str, member: &TeamCharterMember122, charter_path: &str, dry_run: bool) -> String {
    use std::fmt::Write as _;
    let target = member.target.as_deref().unwrap_or("-");
    let worktree = member.worktree.as_deref().unwrap_or("false");
    let mut out = format!("team adopt: {team}\nrole\tstate\taction\n{}\tlive\t{}\n\n  target: {target}\n  worktree: {worktree}\n", member.role, if dry_run { "would adopt (no spawn, no kill)" } else { "adopted (no spawn, no kill)" });
    if dry_run { let _ = write!(out, "  charter: would append to {charter_path}\n\nNo changes made\n"); }
    else { let _ = writeln!(out, "  charter: appended to {charter_path}"); }
    out
}
