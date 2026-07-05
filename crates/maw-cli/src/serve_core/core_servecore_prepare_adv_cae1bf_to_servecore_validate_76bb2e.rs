fn servecore_prepare_advanced_workon(
    request: ServecoreWorkonRequest,
    repo_path: PathBuf,
) -> Result<ServecorePreparedOrchestration, String> {
    if request.attach {
        return Err("serve-orchestration attach is not supported for advanced wake".to_owned());
    }
    let task = request
        .task
        .clone()
        .ok_or_else(|| "serve-orchestration advanced wake requires task".to_owned())?;
    let engine = request
        .engine
        .clone()
        .unwrap_or_else(|| "claude47".to_owned());
    servecore_validate_command_token(&engine, "engine")?;
    let oracle = request
        .target
        .clone()
        .map_or_else(|| servecore_oracle_from_repo(&request.repo), Ok)?;
    servecore_validate_command_token(&oracle, "target")?;
    let mut leader_argv = vec![
        "wake".to_owned(),
        oracle,
        "--task".to_owned(),
        task.clone(),
        "--engine".to_owned(),
        engine.clone(),
        "--split".to_owned(),
        "--no-attach".to_owned(),
    ];
    if servecore_repo_arg_is_safe(&request.repo) {
        leader_argv.extend(["--repo".to_owned(), request.repo.clone()]);
    }
    if let Some(prompt) = &request.prompt {
        leader_argv.extend(["--prompt".to_owned(), prompt.clone()]);
    }
    let public_leader_argv = servecore_redact_prompt_argv(&leader_argv);
    let swarm_argv = if request.with_oracles.is_empty() {
        None
    } else {
        let mut argv = vec!["swarm".to_owned()];
        argv.extend(request.with_oracles.iter().cloned());
        if request.tiled {
            argv.push("--tiled".to_owned());
        }
        Some(argv)
    };
    Ok(ServecorePreparedOrchestration::Advanced(
        ServecoreAdvancedWorkon {
            request,
            repo_path,
            task,
            engine,
            leader_argv,
            public_leader_argv,
            swarm_argv,
        },
    ))
}

fn servecore_has_advanced_fields(request: &ServecoreWorkonRequest) -> bool {
    request.engine.is_some()
        || request.prompt.is_some()
        || request.target.is_some()
        || !request.with_oracles.is_empty()
        || request.attach
        || request.split
        || request.tiled
}

fn servecore_redact_prompt_argv(argv: &[String]) -> Vec<String> {
    let mut redacted = Vec::with_capacity(argv.len());
    let mut redact_next = false;
    for arg in argv {
        if redact_next {
            redacted.push("<redacted>".to_owned());
            redact_next = false;
            continue;
        }
        redact_next = arg == "--prompt";
        redacted.push(arg.clone());
    }
    redacted
}

fn servecore_oracle_from_repo(repo: &str) -> Result<String, String> {
    let name = Path::new(repo)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "serve-orchestration target must be safe".to_owned())?;
    let oracle = name.strip_suffix("-oracle").unwrap_or(name).to_owned();
    servecore_validate_command_token(&oracle, "target")?;
    Ok(oracle)
}

fn servecore_repo_arg_is_safe(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let Some(owner) = parts.next() else {
        return false;
    };
    let Some(name) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && servecore_validate_command_token(owner, "repo").is_ok()
        && servecore_validate_command_token(name, "repo").is_ok()
}

fn servecore_shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn servecore_shell_line_for_self(argv: &[String]) -> Result<String, String> {
    let mut parts = vec![servecore_shell_quote(
        &engine::serveengine_self_bin()?.to_string_lossy(),
    )];
    parts.extend(argv.iter().map(|arg| servecore_shell_quote(arg)));
    Ok(parts.join(" "))
}

fn servecore_find_pane_for_task(
    panes: &[ServecorePaneCandidate],
    task: &str,
) -> Result<String, String> {
    let needle = task.to_ascii_lowercase();
    let Some(pane) = panes
        .iter()
        .find(|pane| pane.title.to_ascii_lowercase().contains(&needle))
    else {
        return Err("serve-orchestration: pane discovery failed".to_owned());
    };
    servecore_validate_pane_id(&pane.id)?;
    Ok(pane.id.clone())
}

fn servecore_validate_pane_id(value: &str) -> Result<(), String> {
    if value
        .strip_prefix('%')
        .is_none_or(|rest| rest.is_empty() || !rest.chars().all(|ch| ch.is_ascii_digit()))
    {
        return Err("serve-orchestration pane must be safe".to_owned());
    }
    Ok(())
}

fn servecore_resolve_workon_repo(root: &Path, repo: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("serve-orchestration: root invalid: {error}"))?;
    let direct = PathBuf::from(repo);
    let first = if direct.is_absolute() {
        direct
    } else {
        root.join(repo)
    };
    if first.exists() {
        return servecore_worktree_inside_root(&root, &first);
    }
    let Some(found) = servecore_find_repo_under_root(&root, repo, 5) else {
        return Err("serve-orchestration: repo not found under root".to_owned());
    };
    servecore_worktree_inside_root(&root, &found)
}

fn servecore_find_repo_under_root(root: &Path, repo: &str, max_depth: usize) -> Option<PathBuf> {
    fn walk(root: &Path, repo: &Path, depth: usize, max_depth: usize) -> Option<PathBuf> {
        if depth > max_depth {
            return None;
        }
        let entries = fs::read_dir(root).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.ends_with(repo) {
                return Some(path);
            }
            if path.is_dir() {
                if let Some(found) = walk(&path, repo, depth + 1, max_depth) {
                    return Some(found);
                }
            }
        }
        None
    }
    walk(root, Path::new(repo), 0, max_depth)
}

fn servecore_worktree_inside_root(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("serve-orchestration: repo invalid: {error}"))?;
    if !canonical.starts_with(root) {
        return Err("serve-orchestration: repo escapes root".to_owned());
    }
    Ok(canonical)
}

fn servecore_validate_path_text(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" {
        return Err(format!("serve-orchestration {label} must be safe"));
    }
    if value.chars().any(|ch| ch.is_control() || ch == '\0') {
        return Err(format!("serve-orchestration {label} must be safe"));
    }
    if Path::new(value)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(format!("serve-orchestration {label} must be safe"));
    }
    Ok(())
}

