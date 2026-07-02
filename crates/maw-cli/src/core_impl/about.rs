const DISPATCH_39: &[DispatcherEntry] = &[
    #[cfg(not(test))]
    DispatcherEntry {
        command: "about",
        handler: Handler::Sync(run_about_command),
    },
];

#[cfg(not(test))]
#[derive(Debug, Clone)]
struct AboutRepo {
    repo_path: String,
    repo_name: String,
    parent_dir: String,
}

#[cfg(not(test))]
#[derive(Debug, Clone, serde::Deserialize, Default)]
struct AboutFleetEntry {
    file: String,
    session: NativeFleetSession,
}

#[cfg(not(test))]
fn run_about_command(argv: &[String]) -> CliOutput {
    let Some(oracle) = argv.first().filter(|arg| !arg.starts_with('-')) else {
        return CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: "usage: maw about <oracle>\n".to_owned(),
        };
    };
    if argv.len() > 1 || argv.iter().any(|arg| arg.starts_with('-')) {
        return CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: "usage: maw about <oracle>\n".to_owned(),
        };
    }

    match render_about(oracle) {
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

#[cfg(not(test))]
fn render_about(oracle: &str) -> Result<String, String> {
    let name = oracle.to_lowercase();
    let mut tmux = TmuxClient::local();
    let sessions = tmux.list_all();
    let repo = resolve_about_oracle_safe(&name)?;
    let session_name = detect_about_session(&name, &sessions);
    let fleet = about_fleet_entry(&name);

    if repo.is_none() && session_name.is_none() && fleet.is_none() {
        return Err(format!("no oracle named '{oracle}' — try: maw oracle ls"));
    }

    let mut out = format!("\n  \x1b[36mOracle — {oracle}\x1b[0m\n\n");
    let _ = writeln!(
        out,
        "  Repo:      {}",
        repo.as_ref()
            .map_or("(not found)", |repo| repo.repo_path.as_str())
    );

    if let Some(session_name) = &session_name {
        let windows = sessions
            .iter()
            .find(|session| session.name == *session_name)
            .map_or(&[][..], |session| session.windows.as_slice());
        let _ = writeln!(out, "  Session:   {session_name} ({} windows)", windows.len());
        for window in windows {
            let status = match tmux.capture(&format!("{session_name}:{}", window.index), Some(3)) {
                Ok(content) if content.trim().is_empty() => "\x1b[33m●\x1b[0m",
                Ok(_) => "\x1b[32m●\x1b[0m",
                Err(_) => "\x1b[90m○\x1b[0m",
            };
            let _ = writeln!(out, "    {status} {}", window.name);
        }
    } else {
        out.push_str("  Session:   (none)\n");
    }

    if let Some(repo) = &repo {
        let worktrees = find_about_worktrees(&repo.parent_dir, &repo.repo_name);
        let _ = writeln!(out, "  Worktrees: {}", worktrees.len());
        for worktree in worktrees {
            let _ = writeln!(out, "    {} → {}", worktree.0, worktree.1);
        }
    }

    if let Some(fleet) = &fleet {
        let actual_windows = session_name
            .as_ref()
            .and_then(|session_name| sessions.iter().find(|session| session.name == *session_name))
            .map_or(0, |session| session.windows.len());
        let registered_windows = fleet.session.windows.len();
        let _ = writeln!(
            out,
            "  Fleet:     {} ({} registered, {} running)",
            fleet.file, registered_windows, actual_windows
        );
        if actual_windows > registered_windows {
            let registered = fleet
                .session
                .windows
                .iter()
                .map(|window| window.name.as_str())
                .collect::<BTreeSet<_>>();
            let running = session_name
                .as_ref()
                .and_then(|session_name| sessions.iter().find(|session| session.name == *session_name))
                .map_or(&[][..], |session| session.windows.as_slice());
            let unregistered = running
                .iter()
                .filter(|window| !registered.contains(window.name.as_str()))
                .collect::<Vec<_>>();
            let _ = writeln!(
                out,
                "  \x1b[33m⚠\x1b[0m  {} window(s) not in fleet config — won't survive reboot",
                unregistered.len()
            );
            for window in unregistered {
                let _ = writeln!(out, "    \x1b[33m→\x1b[0m {}", window.name);
            }
            out.push_str("\n  \x1b[90mFix: add to fleet/");
            out.push_str(&fleet.file);
            out.push_str("\x1b[0m\n");
            out.push_str("  \x1b[90m  maw fleet init          # regenerate all configs\x1b[0m\n");
            out.push_str("  \x1b[90m  maw fleet validate      # check for problems\x1b[0m\n");
        }
    } else {
        out.push_str("  Fleet:     (no config)\n");
    }

    out.push('\n');
    Ok(out)
}

#[cfg(not(test))]
fn resolve_about_oracle_safe(oracle: &str) -> Result<Option<AboutRepo>, String> {
    let repos = about_ghq_list();
    let candidates = if oracle.contains('/') {
        repos.into_iter()
            .filter(|repo| repo.ends_with(format!("/{oracle}")))
            .collect::<Vec<_>>()
    } else {
        let wanted_oracle = format!("{oracle}-oracle");
        let oracle_candidates = repos
            .iter()
            .filter(|repo| repo.file_name().and_then(std::ffi::OsStr::to_str).is_some_and(|name| name.eq_ignore_ascii_case(&wanted_oracle)))
            .cloned()
            .collect::<Vec<_>>();
        if oracle_candidates.len() > 1 {
            return Err(format!(
                "ambiguous oracle short-name '{oracle}' ({} matches): {}",
                oracle_candidates.len(),
                about_repo_names(&oracle_candidates).join(", ")
            ));
        }
        if oracle_candidates.is_empty() {
            let direct_candidates = repos
                .into_iter()
                .filter(|repo| repo.file_name().and_then(std::ffi::OsStr::to_str).is_some_and(|name| name.eq_ignore_ascii_case(oracle)))
                .collect::<Vec<_>>();
            if direct_candidates.len() > 1 {
                return Err(format!(
                    "ambiguous oracle short-name '{oracle}' ({} matches): {}",
                    direct_candidates.len(),
                    about_repo_names(&direct_candidates).join(", ")
                ));
            }
            direct_candidates
        } else {
            oracle_candidates
        }
    };

    Ok(candidates.first().map(|path| {
        let repo_name = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default()
            .to_owned();
        let parent_dir = path
            .parent()
            .map_or_else(String::new, path_string);
        AboutRepo {
            repo_path: path_string(path),
            repo_name,
            parent_dir,
        }
    }))
}

#[cfg(not(test))]
fn about_ghq_list() -> Vec<std::path::PathBuf> {
    let root = ghq_root().join("github.com");
    let Ok(orgs) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut repos = Vec::new();
    for org in orgs.flatten().filter(|entry| entry.path().is_dir()) {
        let Ok(entries) = std::fs::read_dir(org.path()) else {
            continue;
        };
        repos.extend(entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()));
    }
    repos.sort();
    repos
}

#[cfg(not(test))]
fn about_repo_names(paths: &[std::path::PathBuf]) -> Vec<String> {
    paths
        .iter()
        .filter_map(|path| path.file_name().and_then(std::ffi::OsStr::to_str))
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(not(test))]
fn detect_about_session(oracle: &str, sessions: &[TmuxSession]) -> Option<String> {
    if let Some(mapped) = about_config_session(oracle) {
        if sessions.iter().any(|session| session.name == mapped) {
            return Some(mapped);
        }
    }
    if let Some(fleet) = about_fleet_entry(oracle) {
        if sessions.iter().any(|session| session.name == fleet.session.name) {
            return Some(fleet.session.name);
        }
    }
    let numeric_oracle = sessions
        .iter()
        .filter(|session| {
            session.name.starts_with(|ch: char| ch.is_ascii_digit())
                && session.name.ends_with(&format!("-{oracle}-oracle"))
        })
        .collect::<Vec<_>>();
    if numeric_oracle.len() == 1 {
        return Some(numeric_oracle[0].name.clone());
    }
    let numeric = sessions
        .iter()
        .filter(|session| {
            session.name.starts_with(|ch: char| ch.is_ascii_digit())
                && session.name.ends_with(&format!("-{oracle}"))
        })
        .collect::<Vec<_>>();
    if numeric.len() == 1 {
        return Some(numeric[0].name.clone());
    }
    sessions
        .iter()
        .find(|session| session.name == oracle || session.name == format!("{oracle}-oracle"))
        .map(|session| session.name.clone())
}

#[cfg(not(test))]
fn about_config_session(oracle: &str) -> Option<String> {
    let path = active_config_dir().join("maw.config.json");
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("sessions")
        .and_then(|sessions| sessions.get(oracle))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

#[cfg(not(test))]
fn about_fleet_entry(oracle: &str) -> Option<AboutFleetEntry> {
    let fleet_dir = active_config_dir().join("fleet");
    let entries = std::fs::read_dir(fleet_dir).ok()?;
    let mut files = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("json"))
        .collect::<Vec<_>>();
    files.sort();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(session) = serde_json::from_str::<NativeFleetSession>(&text) else {
            continue;
        };
        let has_oracle = session.windows.iter().any(|window| {
            let window_name = window.name.to_lowercase();
            window_name == format!("{oracle}-oracle") || window_name == oracle
        });
        if has_oracle {
            return Some(AboutFleetEntry {
                file: path
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or_default()
                    .to_owned(),
                session,
            });
        }
    }
    None
}

#[cfg(not(test))]
fn find_about_worktrees(parent_dir: &str, repo_name: &str) -> Vec<(String, String)> {
    let mut paths = Vec::<std::path::PathBuf>::new();
    let parent = std::path::Path::new(parent_dir);
    if let Ok(entries) = std::fs::read_dir(parent) {
        let prefix = format!("{repo_name}.wt-");
        paths.extend(
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .filter(|path| path.file_name().and_then(std::ffi::OsStr::to_str).is_some_and(|name| name.starts_with(&prefix)))
                .filter(|path| path.join(".git").exists()),
        );
    }
    let agents = parent.join(repo_name).join("agents");
    if let Ok(entries) = std::fs::read_dir(agents) {
        paths.extend(
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .filter(|path| path.join(".git").exists()),
        );
    }
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_owned();
            (name, path_string(path))
        })
        .collect()
}
