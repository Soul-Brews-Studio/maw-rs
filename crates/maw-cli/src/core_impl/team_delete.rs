const DISPATCH_131: &[DispatcherEntry] = &[];

fn team_delete(argv: &[String]) -> Result<String, String> {
    let team = argv.get(1).ok_or_else(|| "usage: maw team delete <team-name>".to_owned())?;
    if argv.len() > 2 { return Err(format!("team delete: unexpected argument {}", argv[2])); }
    team_validate_name(team)?;
    let mut removed = Vec::new();
    team_delete_archive_team(team)?;
    for path in team_delete_paths(team) {
        if team_bounded_remove_dir_all(&path.root, &path.target, &format!("team delete {team}"))? { removed.push(path.label); }
    }
    Ok(team_delete_render(team, &removed))
}

fn team_prune(argv: &[String]) -> Result<String, String> {
    if argv.len() > 1 { return Err(format!("team prune: unexpected argument {}", argv[1])); }
    let active = team_prune_active_sessions();
    let root = team_home_dir().join(".claude").join("teams");
    let mut pruned = Vec::new();
    let Ok(entries) = std::fs::read_dir(&root) else { return Ok("team prune: no teams pruned\n".to_owned()); };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if team_validate_name(&name).is_err() || team_prune_is_active(&name, &active) || !team_prune_is_empty_team(&name) { continue; }
        team_delete_archive_team(&name)?;
        if team_bounded_remove_dir_all(&root, &entry.path(), &format!("team prune {name}"))? { pruned.push(name); }
    }
    pruned.sort();
    Ok(team_prune_render(&pruned))
}

#[derive(Debug, Clone)]
struct TeamDeletePath131 { label: String, root: std::path::PathBuf, target: std::path::PathBuf }

fn team_delete_paths(team: &str) -> Vec<TeamDeletePath131> {
    let paths = team_paths(team);
    let tasks_root = team_state_dir().join("teams");
    vec![
        TeamDeletePath131 { label: "team dir".to_owned(), root: team_home_dir().join(".claude").join("teams"), target: paths.tool_dir },
        TeamDeletePath131 { label: "tasks".to_owned(), root: tasks_root.clone(), target: tasks_root.join(team).join("tasks") },
    ]
}

fn team_delete_archive_team(team: &str) -> Result<(), String> {
    let stamp = team_delete_stamp();
    let archive = team_psi_dir().join("memory").join("mailbox").join("teams").join(team).join(format!("delete-archive-{stamp}"));
    std::fs::create_dir_all(&archive).map_err(|error| format!("team delete archive mkdir failed: {error}"))?;
    let paths = team_paths(team);
    team_copy_tree_if_exists(&paths.tool_dir, &archive.join("tool-team"))?;
    let tasks = team_state_dir().join("teams").join(team).join("tasks");
    team_copy_tree_if_exists(&tasks, &archive.join("tasks"))?;
    if paths.vault_manifest.exists() { std::fs::copy(&paths.vault_manifest, archive.join("manifest.json")).map_err(|error| format!("team delete archive manifest failed: {error}"))?; }
    Ok(())
}

fn team_delete_stamp() -> String {
    std::env::var("MAW_RS_TEAM_DELETE_STAMP").unwrap_or_else(|_| team_now_millis().to_string())
}

fn team_copy_tree_if_exists(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    if !src.exists() { return Ok(()); }
    let meta = std::fs::symlink_metadata(src).map_err(|error| format!("team delete archive stat failed: {error}"))?;
    if meta.is_file() { if let Some(parent) = dst.parent() { std::fs::create_dir_all(parent).map_err(|error| format!("team delete archive parent failed: {error}"))?; } std::fs::copy(src, dst).map_err(|error| format!("team delete archive copy failed: {error}"))?; return Ok(()); }
    if !meta.is_dir() { return Ok(()); }
    std::fs::create_dir_all(dst).map_err(|error| format!("team delete archive dir failed: {error}"))?;
    for entry in std::fs::read_dir(src).map_err(|error| format!("team delete archive read failed: {error}"))?.flatten() {
        let name = entry.file_name();
        team_copy_tree_if_exists(&entry.path(), &dst.join(name))?;
    }
    Ok(())
}

fn team_bounded_remove_dir_all(root: &std::path::Path, target: &std::path::Path, label: &str) -> Result<bool, String> {
    if !target.exists() { return Ok(false); }
    let root_canon = root.canonicalize().map_err(|error| format!("{label}: canonicalize root {} failed: {error}", root.display()))?;
    let target_canon = target.canonicalize().map_err(|error| format!("{label}: canonicalize target {} failed: {error}", target.display()))?;
    if target_canon == root_canon || !target_canon.starts_with(&root_canon) { return Err(format!("{label}: refuse unbounded remove_dir_all {} outside {}", target_canon.display(), root_canon.display())); }
    std::fs::remove_dir_all(&target_canon).map_err(|error| format!("{label}: remove_dir_all {} failed: {error}", target_canon.display()))?;
    Ok(true)
}

fn team_prune_active_sessions() -> Vec<String> {
    if let Ok(raw) = std::env::var("MAW_RS_TEAM_TMUX_SESSIONS") { return raw.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect(); }
    Vec::new()
}

fn team_prune_is_active(team: &str, sessions: &[String]) -> bool {
    sessions.iter().any(|session| session == team || session.split_once('-').is_some_and(|(_, tail)| tail == team))
}

fn team_prune_is_empty_team(team: &str) -> bool {
    let Some(config) = team_read_json::<TeamConfig122>(&team_paths(team).tool_config) else { return false; };
    config.members.is_empty()
}

fn team_delete_render(team: &str, removed: &[String]) -> String {
    use std::fmt::Write as _;
    let mut out = format!("team delete: {team}\n");
    if removed.is_empty() {
        out.push_str("  no team directories found\n");
    } else {
        for item in removed {
            writeln!(out, "  removed {item}").expect("write string");
        }
    }
    out
}

fn team_prune_render(pruned: &[String]) -> String {
    use std::fmt::Write as _;
    if pruned.is_empty() {
        return "team prune: no teams pruned\n".to_owned();
    }
    let mut out = format!("team prune: {} pruned\n", pruned.len());
    for team in pruned {
        writeln!(out, "  pruned {team}").expect("write string");
    }
    out
}


#[cfg(test)]
mod team_delete_prune_tests {
    use super::*;

    #[test]
    fn team_delete_bounded_remove_refuses_outside_root() {
        let stamp = team_now_millis();
        let base = std::env::temp_dir().join(format!("maw-rs-bounded-remove-{stamp}"));
        let root = base.join("root");
        let outside = base.join("outside");
        std::fs::create_dir_all(&root).expect("root");
        std::fs::create_dir_all(&outside).expect("outside");
        let err = team_bounded_remove_dir_all(&root, &outside, "unit").expect_err("outside rejected");
        assert!(err.contains("refuse unbounded remove_dir_all"));
        assert!(outside.exists(), "outside dir must not be removed");
        let _ = std::fs::remove_dir_all(base);
    }
}
