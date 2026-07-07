const DISPATCH_329: &[DispatcherEntry] = &[
    DispatcherEntry { command: "gather", handler: Handler::Sync(run_team_gather_command) },
    DispatcherEntry { command: "scatter", handler: Handler::Sync(run_team_scatter_command) },
];

const TEAM_GATHER_USAGE: &str = "usage: maw gather [--team <name>] [<member>...]";
const TEAM_SCATTER_USAGE: &str = "usage: maw scatter [--team <name>] [<member>...]";

#[derive(Debug, Clone, Default)]
struct TeamGatherOptions329 { team: Option<String>, members: Vec<String> }

#[derive(Debug, Clone, PartialEq, Eq)]
struct TeamGatherTarget329 { session: String, window: String, pane_id: String }

fn run_team_gather_command(argv: &[String]) -> CliOutput { team_gather_output(team_gather_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new())) }
fn run_team_scatter_command(argv: &[String]) -> CliOutput { team_gather_output(team_scatter_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new())) }

fn team_gather_output(result: Result<String, String>) -> CliOutput {
    match result {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) if message == TEAM_GATHER_USAGE || message == TEAM_SCATTER_USAGE => CliOutput { code: 2, stdout: String::new(), stderr: format!("{message}\n") },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn team_gather_with_runner<R: maw_tmux::TmuxRunner>(argv: &[String], runner: &mut R) -> Result<String, String> {
    let opts = team_gather_parse(argv, TEAM_GATHER_USAGE)?;
    let (team, charter) = team_gather_load(&opts)?;
    let session = team_t3_session(&charter, &TeamT3Options124 { team: Some(team.clone()), ..Default::default() });
    team_t3_validate_session(&session)?;
    let target = team_gather_current_target(runner)?;
    team_t5b_validate_pane_id(&target.pane_id)?;
    let roster = team_gather_roster(&charter, &opts, &team, &session)?;
    let mut rows = Vec::new();
    let mut joined = 0usize;
    for item in &roster {
        if item.state == "skipped" { continue; }
        match (&item.state[..], &item.pane) {
            ("live", Some(pane)) if pane.session == target.session && pane.window == target.window => rows.push(team_gather_row(item, "skip already gathered")),
            ("live", Some(pane)) => {
                team_t5b_validate_pane_id(&pane.pane_id)?;
                join_with_runner(&[pane.pane_id.clone(), "--to".to_owned(), target.pane_id.clone()], runner).map_err(|(_, message)| format!("maw gather: {message}"))?;
                joined += 1;
                rows.push(team_gather_row(item, &format!("join {}", pane.pane_id)));
            }
            ("dead", _) => rows.push(team_gather_row(item, "warn dead pane (skipped)")),
            ("missing", _) => rows.push(team_gather_row(item, "warn missing pane (skipped)")),
            _ => rows.push(team_gather_row(item, "skip")),
        }
    }
    if joined > 0 { tmux_layout_with_runner(&[target.pane_id.clone(), "main-vertical".to_owned()], runner).map_err(|(_, message)| format!("maw gather: {message}"))?; }
    Ok(team_gather_render("maw gather", &team, &session, &target, &rows))
}

fn team_scatter_with_runner<R: maw_tmux::TmuxRunner>(argv: &[String], runner: &mut R) -> Result<String, String> {
    let opts = team_gather_parse(argv, TEAM_SCATTER_USAGE)?;
    let (team, charter) = team_gather_load(&opts)?;
    let session = team_t3_session(&charter, &TeamT3Options124 { team: Some(team.clone()), ..Default::default() });
    team_t3_validate_session(&session)?;
    let target = team_gather_current_target(runner)?;
    team_t5b_validate_pane_id(&target.pane_id)?;
    let roster = team_gather_roster(&charter, &opts, &team, &session)?;
    let mut rows = Vec::new();
    for item in &roster {
        if item.state == "skipped" { continue; }
        match (&item.state[..], &item.pane) {
            ("live", Some(pane)) if pane.session != target.session || pane.window != target.window => rows.push(team_gather_row(item, "skip already scattered")),
            ("live", Some(pane)) if pane.pane_id == target.pane_id || matches!(item.role.as_str(), "lead" | "bridge") => rows.push(team_gather_row(item, "skip lead")),
            ("live", Some(pane)) => {
                team_t5b_validate_pane_id(&pane.pane_id)?;
                let break_args = tmux_break_args(&pane.pane_id, runner);
                runner.run("break-pane", &break_args).map_err(|error| format!("maw scatter: break-pane {} failed: {}", pane.pane_id, error.message))?;
                rows.push(team_gather_row(item, &format!("break {}", pane.pane_id)));
            }
            ("dead", _) => rows.push(team_gather_row(item, "warn dead pane (skipped)")),
            ("missing", _) => rows.push(team_gather_row(item, "warn missing pane (skipped)")),
            _ => rows.push(team_gather_row(item, "skip")),
        }
    }
    Ok(team_gather_render("maw scatter", &team, &session, &target, &rows))
}

fn team_gather_parse(argv: &[String], usage: &str) -> Result<TeamGatherOptions329, String> {
    let mut opts = TeamGatherOptions329::default();
    let mut index = 0usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" => return Err(usage.to_owned()),
            "--team" => { index += 1; opts.team = Some(team_t3_next(argv, index, "--team")?); },
            value if value.starts_with('-') => return Err(format!("maw gather: unknown argument {value}")),
            value => opts.members.push(value.to_owned()),
        }
        index += 1;
    }
    if let Some(team) = &opts.team { team_validate_name(team)?; }
    for member in &opts.members { team_t3_validate_token(member, "member selector")?; }
    Ok(opts)
}

fn team_gather_load(opts: &TeamGatherOptions329) -> Result<(String, TeamCharter122), String> {
    let team = opts.team.clone().or_else(team_gather_current_team).ok_or_else(|| "maw gather: --team required (no current team)".to_owned())?;
    team_validate_name(&team)?;
    let path = team_t3_resolve_charter_path(&team, None)?;
    Ok((team, team_read_charter_path(&path)?))
}

fn team_gather_current_team() -> Option<String> { std::env::var("MAW_TEAM").ok().filter(|team| !team.is_empty()).or_else(team_gather_single_local_charter) }

fn team_gather_single_local_charter() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let mut names = std::collections::BTreeSet::new();
    for dir in [cwd.join(".maw").join("teams"), cwd.join("ψ").join("teams")] {
        let Ok(entries) = std::fs::read_dir(dir) else { continue; };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(ext) = path.extension().and_then(std::ffi::OsStr::to_str) else { continue; };
            if matches!(ext, "yaml" | "json") { if let Some(stem) = path.file_stem().and_then(std::ffi::OsStr::to_str) { names.insert(stem.to_owned()); } }
        }
    }
    (names.len() == 1).then(|| names.into_iter().next().expect("one name"))
}

fn team_gather_current_target<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<TeamGatherTarget329, String> {
    let args = vec!["-p".to_owned(), "#{session_name}|#{window_name}|#{pane_id}".to_owned()];
    let raw = runner.run("display-message", &args).map_err(|error| format!("maw gather: current tmux pane unavailable: {}", error.message))?;
    let mut parts = raw.trim().split('|');
    let target = TeamGatherTarget329 { session: parts.next().unwrap_or_default().to_owned(), window: parts.next().unwrap_or_default().to_owned(), pane_id: parts.next().unwrap_or_default().to_owned() };
    if target.session.is_empty() || target.window.is_empty() || target.pane_id.is_empty() { return Err("maw gather: current tmux pane unavailable".to_owned()); }
    Ok(target)
}

fn team_gather_roster(charter: &TeamCharter122, opts: &TeamGatherOptions329, team: &str, session: &str) -> Result<Vec<TeamRosterItem124>, String> {
    let panes = team_down_panes()?;
    let t3_opts = TeamT3Options124 { team: Some(team.to_owned()), only: opts.members.clone(), ..Default::default() };
    Ok(charter.members.iter().map(|member| team_gather_classify(member, &t3_opts, session, &panes)).collect())
}

fn team_gather_classify(member: &TeamCharterMember122, opts: &TeamT3Options124, session: &str, panes: &[TeamPane124]) -> TeamRosterItem124 {
    let mut item = team_t3_classify(member, opts, session, panes);
    if item.state != "missing" { return item; }
    let Some(pane) = panes.iter().find(|pane| pane.session == session && team_gather_path_matches(&pane.path, &item)).cloned() else { return item; };
    let state = if team_t3_is_live_command(&pane.command) { "live" } else { "dead" };
    state.clone_into(&mut item.state);
    item.pane = Some(pane);
    item
}

fn team_gather_path_matches(path: &str, item: &TeamRosterItem124) -> bool {
    let candidates = [item.role.as_str(), item.identity.as_str(), item.worktree.as_str(), std::path::Path::new(&item.worktree).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("")];
    std::path::Path::new(path).file_name().and_then(std::ffi::OsStr::to_str).is_some_and(|name| candidates.contains(&name))
}

fn team_gather_row(item: &TeamRosterItem124, action: &str) -> String { format!("{}\t{}\t{}", item.role, item.state, action) }

fn team_gather_render(kind: &str, team: &str, session: &str, target: &TeamGatherTarget329, rows: &[String]) -> String {
    let mut out = format!("{kind}: {team} ({session}) -> {}:{}\nrole\tstate\taction\n", target.session, target.window);
    for row in rows { out.push_str(row); out.push('\n'); }
    out
}

#[cfg(test)]
mod team_gather_scatter_tests {
    use super::*;

    #[derive(Default)]
    struct FakeTmux329 { calls: Vec<(String, Vec<String>)> }

    impl maw_tmux::TmuxRunner for FakeTmux329 {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            Ok(if subcommand == "display-message" && args.iter().any(|arg| arg.contains("session_name")) { "s|lead|%1\n".to_owned() } else { String::new() })
        }
    }

    struct CwdRestore329(std::path::PathBuf);
    impl Drop for CwdRestore329 { fn drop(&mut self) { let _ = std::env::set_current_dir(&self.0); } }

    fn fixture_team() -> CwdRestore329 {
        let old = std::env::current_dir().expect("cwd");
        let root = std::env::temp_dir().join(format!("maw-gather-{}-{}", std::process::id(), team_now_millis()));
        let teams = root.join(".maw").join("teams");
        std::fs::create_dir_all(&teams).expect("teams");
        std::fs::write(teams.join("demo.yaml"), "name: demo\nsession: s\nmembers:\n  - role: lead\n    name: lead\n    cwd: lead\n  - role: codex-1\n    name: codex-1\n    cwd: codex-1\n  - role: codex-2\n    name: codex-2\n    cwd: codex-2\n").expect("charter");
        std::env::set_current_dir(root).expect("chdir");
        CwdRestore329(old)
    }

    #[test]
    fn gather_joins_live_members_and_warns_missing() {
        let _guard = env_test_lock().lock().expect("lock");
        let _cwd = fixture_team();
        let _panes = EnvVarRestore::capture("MAW_RS_TEAM_TMUX_PANES");
        std::env::set_var("MAW_RS_TEAM_TMUX_PANES", "s|lead|codex|/repo/lead|%1\ns|codex-1|codex|/repo/codex-1|%2\n");
        let mut tmux = FakeTmux329::default();
        let out = team_gather_with_runner(&[], &mut tmux).expect("gather");
        assert!(out.contains("codex-1\tlive\tjoin %2"), "{out}");
        assert!(out.contains("codex-2\tmissing\twarn missing pane"), "{out}");
        assert!(tmux.calls.iter().any(|call| call.0 == "join-pane"));
        assert!(tmux.calls.iter().any(|call| call.0 == "select-layout"));
    }

    #[test]
    fn scatter_breaks_non_lead_panes_in_current_window() {
        let _guard = env_test_lock().lock().expect("lock");
        let _cwd = fixture_team();
        let _panes = EnvVarRestore::capture("MAW_RS_TEAM_TMUX_PANES");
        std::env::set_var("MAW_RS_TEAM_TMUX_PANES", "s|lead|codex|/repo/lead|%1\ns|lead|codex|/repo/codex-1|%2\n");
        let mut tmux = FakeTmux329::default();
        let out = team_scatter_with_runner(&["codex-1".to_owned()], &mut tmux).expect("scatter");
        assert!(out.contains("codex-1\tlive\tbreak %2"), "{out}");
        assert!(tmux.calls.iter().any(|call| call.0 == "break-pane"));
    }
}
