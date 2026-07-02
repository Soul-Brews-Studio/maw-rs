fn render_route_plan_json(query: &str, result: &RouteResult) -> String {
    let mut fields = vec![
        "\"command\":\"route\"".to_owned(),
        format!("\"query\":{}", json_string(query)),
    ];
    match result {
        RouteResult::Local { target } => {
            fields.push("\"type\":\"local\"".to_owned());
            fields.push(format!("\"target\":{}", json_string(target)));
        }
        RouteResult::Peer {
            peer_url,
            target,
            node,
        } => {
            fields.push("\"type\":\"peer\"".to_owned());
            fields.push(format!("\"peerUrl\":{}", json_string(peer_url)));
            fields.push(format!("\"target\":{}", json_string(target)));
            fields.push(format!("\"node\":{}", json_string(node)));
        }
        RouteResult::SelfNode { target } => {
            fields.push("\"type\":\"self-node\"".to_owned());
            fields.push(format!("\"target\":{}", json_string(target)));
        }
        RouteResult::Error {
            reason,
            detail,
            hint,
        } => {
            fields.push("\"type\":\"error\"".to_owned());
            fields.push(format!("\"reason\":{}", json_string(reason)));
            fields.push(format!("\"detail\":{}", json_string(detail)));
            if let Some(hint) = hint {
                fields.push(format!("\"hint\":{}", json_string(hint)));
            }
        }
    }
    format!("{{{}}}\n", fields.join(","))
}

fn render_route_plan_text(query: &str, result: &RouteResult) -> String {
    match result {
        RouteResult::Local { target } => format!("route {query}: local {target}\n"),
        RouteResult::Peer {
            peer_url,
            target,
            node,
        } => format!("route {query}: peer {node} {target} via {peer_url}\n"),
        RouteResult::SelfNode { target } => format!("route {query}: self-node {target}\n"),
        RouteResult::Error {
            reason,
            detail,
            hint,
        } => hint.as_ref().map_or_else(
            || format!("route {query}: error {reason} {detail}\n"),
            |hint| format!("route {query}: error {reason} {detail} hint={hint}\n"),
        ),
    }
}

fn run_worktree_window_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_worktree_window_constants_plan(&argv[1..]);
    }

    let mut plan_json = false;
    let mut main_repo_name = None;
    let mut wt_name = None;
    let mut sessions: Vec<WorktreeSession> = Vec::new();
    let mut current_session: Option<WorktreeSession> = None;

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--main-repo-name" => {
                let Some(value) = argv.get(index + 1) else {
                    return worktree_window_usage_error(
                        "worktree-window: missing --main-repo-name value",
                    );
                };
                main_repo_name = Some(value.to_owned());
                index += 1;
            }
            "--wt-name" => {
                let Some(value) = argv.get(index + 1) else {
                    return worktree_window_usage_error("worktree-window: missing --wt-name value");
                };
                wt_name = Some(value.to_owned());
                index += 1;
            }
            "--session" => {
                if let Some(session) = current_session.take() {
                    sessions.push(session);
                }
                let Some(value) = argv.get(index + 1) else {
                    return worktree_window_usage_error("worktree-window: missing --session value");
                };
                current_session = Some(WorktreeSession {
                    name: value.to_owned(),
                    windows: Vec::new(),
                });
                index += 1;
            }
            "--window" => {
                let Some(value) = argv.get(index + 1) else {
                    return worktree_window_usage_error("worktree-window: missing --window value");
                };
                let Some(session) = &mut current_session else {
                    return worktree_window_usage_error(
                        "worktree-window: --window must follow a --session",
                    );
                };
                match parse_worktree_window(value) {
                    Ok(window) => session.windows.push(window),
                    Err(message) => return worktree_window_usage_error(&message),
                }
                index += 1;
            }
            arg => {
                return worktree_window_usage_error(&format!(
                    "worktree-window: unknown argument {arg}"
                ));
            }
        }
        index += 1;
    }
    if let Some(session) = current_session.take() {
        sessions.push(session);
    }

    let Some(main_repo_name) = main_repo_name else {
        return worktree_window_usage_error("worktree-window: expected --main-repo-name <repo>");
    };
    let Some(wt_name) = wt_name else {
        return worktree_window_usage_error("worktree-window: expected --wt-name <worktree>");
    };

    let result = resolve_worktree_window(&main_repo_name, &wt_name, &sessions);
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_worktree_window_plan_json(&main_repo_name, &wt_name, &result)
        } else {
            render_worktree_window_plan_text(&main_repo_name, &wt_name, &result)
        },
        stderr: String::new(),
    }
}

fn worktree_window_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs worktree-window --main-repo-name <repo> --wt-name <worktree> [--session <name>] [--window <index:name:active>]... [--plan-json]\nusage: maw-rs worktree-window constants [--plan-json]\n"
        ),
    }
}

fn run_worktree_window_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            _ => {
                return worktree_window_constants_usage_error(&format!(
                    "worktree-window constants: unknown argument {arg}"
                ))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_worktree_window_constants_json()
        } else {
            "worktree-window constants results=bound,ambiguous,none window-shape=index:name:active\n"
                .to_owned()
        },
        stderr: String::new(),
    }
}

fn render_worktree_window_constants_json() -> String {
    concat!(
        "{\"command\":\"worktree-window\",\"kind\":\"constants\",",
        "\"inputs\":[\"main-repo-name\",\"wt-name\",\"session\",\"window\"],",
        "\"windowShape\":\"index:name:active\",",
        "\"resultKinds\":[\"bound\",\"ambiguous\",\"none\"],",
        "\"parentSessionRules\":[\"strip -oracle suffix from main repo\",\"match fleet numeric session suffix\",\"prefer parent-scoped windows before global fallback\"],",
        "\"queryRules\":[\"strip numeric worktree prefix\",\"try repo-qualified worktree name before stripped suffix\",\"dedupe same-named windows across sessions\",\"fallback to global single match\",\"fail loud on ambiguous stripped suffix\"],",
        "\"usageErrors\":[\"missing-main-repo-name\",\"missing-wt-name\",\"window-without-session\",\"bad-window-shape\",\"unknown-argument\"],",
        "\"fixtureCounts\":{\"total\":8,\"bound\":6,\"ambiguous\":1,\"none\":1}}\n"
    )
    .to_owned()
}

fn worktree_window_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\nusage: maw-rs worktree-window constants [--plan-json]\n"),
    }
}

fn parse_worktree_window(value: &str) -> Result<WorktreeWindow, String> {
    let mut parts = value.splitn(3, ':');
    let index = parts
        .next()
        .unwrap_or_default()
        .parse::<u32>()
        .map_err(|_| "worktree-window: invalid window index".to_owned())?;
    let Some(name) = parts.next() else {
        return Err("worktree-window: window must use <index:name:active>".to_owned());
    };
    let active = match parts.next() {
        Some("true") => true,
        Some("false") => false,
        _ => return Err("worktree-window: window active must be true or false".to_owned()),
    };
    Ok(WorktreeWindow {
        index,
        name: name.to_owned(),
        active,
    })
}

fn render_worktree_window_plan_json(
    main_repo_name: &str,
    wt_name: &str,
    result: &WorktreeWindowResolution,
) -> String {
    let mut fields = vec![
        "\"command\":\"worktree-window\"".to_owned(),
        format!("\"mainRepoName\":{}", json_string(main_repo_name)),
        format!("\"wtName\":{}", json_string(wt_name)),
    ];
    match result {
        WorktreeWindowResolution::Bound { window } => {
            fields.push("\"kind\":\"bound\"".to_owned());
            fields.push(format!("\"window\":{}", json_string(window)));
        }
        WorktreeWindowResolution::Ambiguous { query, candidates } => {
            fields.push("\"kind\":\"ambiguous\"".to_owned());
            fields.push(format!("\"query\":{}", json_string(query)));
            fields.push(format!("\"candidates\":{}", json_string_array(candidates)));
        }
        WorktreeWindowResolution::None => fields.push("\"kind\":\"none\"".to_owned()),
    }
    format!("{{{}}}\n", fields.join(","))
}

fn render_worktree_window_plan_text(
    main_repo_name: &str,
    wt_name: &str,
    result: &WorktreeWindowResolution,
) -> String {
    match result {
        WorktreeWindowResolution::Bound { window } => {
            format!("worktree-window {main_repo_name} {wt_name}: bound {window}\n")
        }
        WorktreeWindowResolution::Ambiguous { query, candidates } => format!(
            "worktree-window {main_repo_name} {wt_name}: ambiguous {query} candidates={}\n",
            candidates.join(", ")
        ),
        WorktreeWindowResolution::None => {
            format!("worktree-window {main_repo_name} {wt_name}: none\n")
        }
    }
}

fn run_calver_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_calver_constants_plan(&argv[1..]);
    }

    let mut plan_json = false;
    let mut stable = false;
    let mut channel = None;
    let mut now = None;
    let mut package_version = String::new();
    let mut tags = Vec::new();

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--stable" => stable = true,
            "--alpha" => channel = Some(Channel::Alpha),
            "--beta" => channel = Some(Channel::Beta),
            "--now" => {
                let Some(value) = argv.get(index + 1) else {
                    return calver_usage_error("calver: missing --now value");
                };
                match parse_date_parts(value) {
                    Ok(parsed) => now = Some(parsed),
                    Err(message) => return calver_usage_error(&message),
                }
                index += 1;
            }
            "--package-version" => {
                let Some(value) = argv.get(index + 1) else {
                    return calver_usage_error("calver: missing --package-version value");
                };
                package_version.clone_from(value);
                index += 1;
            }
            "--tag" => {
                let Some(value) = argv.get(index + 1) else {
                    return calver_usage_error("calver: missing --tag value");
                };
                tags.push(value.to_owned());
                index += 1;
            }
            arg => return calver_usage_error(&format!("calver: unknown argument {arg}")),
        }
        index += 1;
    }

    let Some(now) = now else {
        return calver_usage_error("calver: expected --now <YYYY-M-DTHH:MM>");
    };

    let compute_args = ComputeArgs {
        stable,
        channel,
        now,
    };
    match compute_version(compute_args, &tags, &package_version) {
        Ok(version) => CliOutput {
            code: 0,
            stdout: if plan_json {
                render_calver_plan_json(compute_args, &tags, &package_version, &version)
            } else {
                format!("{version}\n")
            },
            stderr: String::new(),
        },
        Err(error) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("calver: {error}\n"),
        },
    }
}

fn calver_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs calver --now <YYYY-M-DTHH:MM> [--stable|--alpha|--beta] [--package-version <version>] [--tag <tag>]... [--plan-json]\nusage: maw-rs calver constants [--plan-json]\n"
        ),
    }
}

fn run_calver_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            _ => {
                return calver_constants_usage_error(&format!(
                    "calver constants: unknown argument {arg}"
                ))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_calver_constants_json()
        } else {
            "calver constants base=YY.M.D channels=alpha,beta stamp=H*100+M max=2359\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn render_calver_constants_json() -> String {
    concat!(
        "{\"command\":\"calver\",\"kind\":\"constants\",",
        "\"baseFormat\":\"YY.M.D\",\"prereleaseFormat\":\"YY.M.D-channel.HHMM\",",
        "\"channels\":[\"alpha\",\"beta\"],\"defaultChannel\":\"alpha\",",
        "\"dateRules\":{\"zeroPadding\":false,\"februaryMaxDay\":29,\"yearModulo\":100},",
        "\"stamp\":{\"shape\":\"H*100+M\",\"leadingZeroes\":false,\"max\":2359},",
        "\"versionInputs\":[\"tags\",\"packageVersion\",\"now\",\"stable\",\"channel\"],",
        "\"monotonicRules\":[\"stable-uses-today-base\",\"prerelease-preserves-future-package-base\",\"roll-base-forward-when-existing-suffix-gte-stamp\",\"reject-ghost-package-date\"],",
        "\"fixtureCounts\":{\"dateBase\":3,\"hhmmStamp\":5,\"extractBaseFromVersion\":7,\"compareBases\":5,\"isValidCalendarDate\":8,\"nextCalendarBase\":3,\"maxNFromTags\":5,\"maxNFromPackageJson\":7,\"effectiveBase\":6,\"computeVersion\":10}}\n"
    )
    .to_owned()
}

fn calver_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\nusage: maw-rs calver constants [--plan-json]\n"),
    }
}

