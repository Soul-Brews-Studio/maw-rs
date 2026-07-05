//maw:order 174

const MORE_STATUS_WINDOW_FORMAT: &str = "#{session_name}|||#{window_index}|||#{window_name}|||#{pane_current_command}|||#{pane_title}|||#{pane_current_path}";
const MORE_STATUS_PANE_FORMAT: &str = "#{session_name}|||#{window_index}|||#{window_name}|||#{pane_index}|||#{pane_id}|||#{pane_current_command}|||#{pane_title}|||#{pane_current_path}";

#[derive(Debug, Clone, PartialEq, Eq)]
struct MoreStatusWindow {
    session: String,
    index: String,
    name: String,
    active_command: String,
    active_title: String,
    active_cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MoreStatusPane {
    session: String,
    window_index: String,
    pane_id: String,
    command: String,
    title: String,
    cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MoreStatusRow {
    window: String,
    worktree: String,
    branch: String,
    alive: bool,
}

fn more_status_live() -> String {
    more_status_live_with_runner(
        &mut maw_tmux::CommandTmuxRunner::new(),
        more_status_git_branch,
    )
}

fn more_status_live_with_runner<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    branch_for: fn(&str) -> String,
) -> String {
    let Ok(windows) = more_status_list_windows(runner) else {
        return more_status_render(&[]);
    };
    let panes = more_status_list_panes(runner).unwrap_or_default();
    let rows = more_status_rows(&windows, &panes, branch_for);
    more_status_render(&rows)
}

fn more_status_list_windows<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
) -> Result<Vec<MoreStatusWindow>, String> {
    let args = ["-a", "-F", MORE_STATUS_WINDOW_FORMAT]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let raw = runner
        .run("list-windows", &args)
        .map_err(|error| format!("more status: tmux list-windows failed: {}", error.message))?;
    Ok(more_status_parse_windows(&raw))
}

fn more_status_list_panes<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
) -> Result<Vec<MoreStatusPane>, String> {
    let args = ["-a", "-F", MORE_STATUS_PANE_FORMAT]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let raw = runner
        .run("list-panes", &args)
        .map_err(|error| format!("more status: tmux list-panes failed: {}", error.message))?;
    Ok(more_status_parse_panes(&raw))
}

fn more_status_parse_windows(raw: &str) -> Vec<MoreStatusWindow> {
    raw.lines()
        .filter_map(|line| {
            let fields = line.splitn(6, "|||").collect::<Vec<_>>();
            (fields.len() == 6).then(|| MoreStatusWindow {
                session: fields[0].to_owned(),
                index: fields[1].to_owned(),
                name: fields[2].to_owned(),
                active_command: fields[3].to_owned(),
                active_title: fields[4].to_owned(),
                active_cwd: fields[5].to_owned(),
            })
        })
        .collect()
}

fn more_status_parse_panes(raw: &str) -> Vec<MoreStatusPane> {
    raw.lines()
        .filter_map(|line| {
            let fields = line.splitn(8, "|||").collect::<Vec<_>>();
            (fields.len() == 8).then(|| MoreStatusPane {
                session: fields[0].to_owned(),
                window_index: fields[1].to_owned(),
                pane_id: fields[4].to_owned(),
                command: fields[5].to_owned(),
                title: fields[6].to_owned(),
                cwd: fields[7].to_owned(),
            })
        })
        .collect()
}

fn more_status_rows(
    windows: &[MoreStatusWindow],
    panes: &[MoreStatusPane],
    branch_for: fn(&str) -> String,
) -> Vec<MoreStatusRow> {
    let mut rows = windows
        .iter()
        .filter_map(|window| {
            let window_panes = panes
                .iter()
                .filter(|pane| pane.session == window.session && pane.window_index == window.index)
                .collect::<Vec<_>>();
            if !more_status_is_coder_window(window, &window_panes) {
                return None;
            }
            let worktree = more_status_worktree_path(window, &window_panes);
            let branch = if worktree == "-" {
                "-".to_owned()
            } else {
                branch_for(&worktree)
            };
            Some(MoreStatusRow {
                window: window.name.clone(),
                worktree,
                branch,
                alive: window_panes.iter().any(|pane| !pane.pane_id.is_empty()),
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.window.cmp(&right.window));
    rows
}

fn more_status_is_coder_window(window: &MoreStatusWindow, panes: &[&MoreStatusPane]) -> bool {
    more_status_is_coder_text(&window.name)
        || more_status_is_coder_text(&window.active_command)
        || more_status_is_coder_text(&window.active_title)
        || more_status_is_agent_worktree(&window.active_cwd)
        || panes.iter().any(|pane| {
            more_status_is_coder_text(&pane.command)
                || more_status_is_coder_text(&pane.title)
                || more_status_is_agent_worktree(&pane.cwd)
        })
}

fn more_status_is_coder_text(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("codex") || lower.contains("coder")
}

fn more_status_is_agent_worktree(value: &str) -> bool {
    value.contains("/agents/") || value.ends_with("/agents")
}

fn more_status_worktree_path(window: &MoreStatusWindow, panes: &[&MoreStatusPane]) -> String {
    panes
        .iter()
        .find_map(|pane| more_status_nonempty_path(&pane.cwd))
        .or_else(|| more_status_nonempty_path(&window.active_cwd))
        .unwrap_or_else(|| "-".to_owned())
}

fn more_status_nonempty_path(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn more_status_git_branch(worktree: &str) -> String {
    more_status_git_output(worktree, &["branch", "--show-current"])
        .filter(|branch| !branch.is_empty())
        .or_else(|| more_status_git_output(worktree, &["rev-parse", "--abbrev-ref", "HEAD"]))
        .filter(|branch| !branch.is_empty())
        .unwrap_or_else(|| "-".to_owned())
}

fn more_status_git_output(worktree: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn more_status_render(rows: &[MoreStatusRow]) -> String {
    let mut out = format!(
        "more status\nlive coders: {}\nwindow | worktree | branch | alive/dead\n",
        rows.len()
    );
    if rows.is_empty() {
        out.push_str("(no codex coder windows)\n");
        return out;
    }
    for row in rows {
        let state = if row.alive { "alive" } else { "dead" };
        let _ = writeln!(
            out,
            "{} | {} | {} | {state}",
            row.window, row.worktree, row.branch
        );
    }
    out
}

#[cfg(test)]
mod more_status_tests {
    use super::*;

    #[derive(Debug, Default)]
    struct MoreStatusFakeTmux {
        windows: String,
        panes: String,
        fail_windows: bool,
        calls: Vec<(String, Vec<String>)>,
    }

    impl maw_tmux::TmuxRunner for MoreStatusFakeTmux {
        fn run(
            &mut self,
            subcommand: &str,
            args: &[String],
        ) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "list-windows" if self.fail_windows => Err(maw_tmux::TmuxError::new("no tmux")),
                "list-windows" => Ok(self.windows.clone()),
                "list-panes" => Ok(self.panes.clone()),
                other => Err(maw_tmux::TmuxError::new(format!("unexpected {other}"))),
            }
        }
    }

    fn fake_branch(path: &str) -> String {
        match path {
            "/repo/agents/r2-auth" => "agents/r2-auth".to_owned(),
            "/repo/app" => "main".to_owned(),
            _ => "-".to_owned(),
        }
    }

    #[test]
    fn more_status_renders_coder_windows_from_tmux() {
        let mut runner = MoreStatusFakeTmux {
            windows: "team|||1|||r2-auth|||zsh|||worker|||/repo/agents/r2-auth\nteam|||2|||notes|||zsh|||notes|||/tmp\nteam|||3|||codex-2|||codex|||busy|||/repo/app\n".to_owned(),
            panes: "team|||1|||r2-auth|||0|||%1|||codex|||busy|||/repo/agents/r2-auth\nteam|||3|||codex-2|||0|||%2|||zsh|||idle|||/repo/app\n".to_owned(),
            ..MoreStatusFakeTmux::default()
        };

        let output = more_status_live_with_runner(&mut runner, fake_branch);

        assert_eq!(
            output,
            "more status\nlive coders: 2\nwindow | worktree | branch | alive/dead\ncodex-2 | /repo/app | main | alive\nr2-auth | /repo/agents/r2-auth | agents/r2-auth | alive\n"
        );
        assert_eq!(runner.calls[0].0, "list-windows");
        assert_eq!(runner.calls[1].0, "list-panes");
    }

    #[test]
    fn more_status_marks_matching_window_without_pane_dead() {
        let mut runner = MoreStatusFakeTmux {
            windows: "team|||4|||codex-dead|||zsh|||dead|||/repo/app\n".to_owned(),
            panes: String::new(),
            ..MoreStatusFakeTmux::default()
        };

        let output = more_status_live_with_runner(&mut runner, fake_branch);

        assert_eq!(
            output,
            "more status\nlive coders: 1\nwindow | worktree | branch | alive/dead\ncodex-dead | /repo/app | main | dead\n"
        );
    }

    #[test]
    fn more_status_empty_when_tmux_is_unavailable() {
        let mut runner = MoreStatusFakeTmux {
            fail_windows: true,
            ..MoreStatusFakeTmux::default()
        };

        let output = more_status_live_with_runner(&mut runner, fake_branch);

        assert_eq!(
            output,
            "more status\nlive coders: 0\nwindow | worktree | branch | alive/dead\n(no codex coder windows)\n"
        );
    }
}
