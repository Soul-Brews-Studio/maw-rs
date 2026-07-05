pub trait ServecorePaneRunner: Send + Sync {
    /// Lists panes that may receive a follow-up swarm command.
    ///
    /// # Errors
    ///
    /// Returns an error when the pane backend cannot enumerate panes.
    fn servecore_list_panes(&self) -> Result<Vec<ServecorePaneCandidate>, String>;

    /// Sends one literal command line to the selected pane and presses Enter.
    ///
    /// # Errors
    ///
    /// Returns an error when the pane id is invalid or the backend rejects the send.
    fn servecore_send_literal_enter(&self, pane: &str, line: &str) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct ServecoreTmuxPaneRunner;

impl ServecorePaneRunner for ServecoreTmuxPaneRunner {
    fn servecore_list_panes(&self) -> Result<Vec<ServecorePaneCandidate>, String> {
        let mut tmux = TmuxClient::local();
        Ok(tmux
            .list_panes()
            .into_iter()
            .map(|pane| ServecorePaneCandidate {
                id: pane.id,
                title: pane.title,
            })
            .collect())
    }

    fn servecore_send_literal_enter(&self, pane: &str, line: &str) -> Result<(), String> {
        servecore_validate_pane_id(pane)?;
        let mut tmux = TmuxClient::local();
        tmux.send_keys(pane, &["C-u".to_owned()])
            .map_err(|_| "serve-orchestration: tmux send failed".to_owned())?;
        tmux.send_keys_literal(pane, line)
            .map_err(|_| "serve-orchestration: tmux send failed".to_owned())?;
        tmux.send_enter(pane)
            .map_err(|_| "serve-orchestration: tmux send failed".to_owned())
    }
}

#[cfg(test)]
#[derive(Default)]
struct TestPaneRunner;

#[cfg(test)]
impl ServecorePaneRunner for TestPaneRunner {
    fn servecore_list_panes(&self) -> Result<Vec<ServecorePaneCandidate>, String> {
        Ok(Vec::new())
    }

    fn servecore_send_literal_enter(&self, _pane: &str, _line: &str) -> Result<(), String> {
        Ok(())
    }
}

enum ServecorePreparedOrchestration {
    Simple(ServecorePreparedWorkon),
    Advanced(ServecoreAdvancedWorkon),
}

struct ServecorePreparedWorkon {
    request: ServecoreWorkonRequest,
    repo_path: PathBuf,
    engine: String,
    argv: Vec<String>,
}

impl ServecorePreparedWorkon {
    fn into_handle(self, status: &str) -> ServecoreWorkonHandle {
        ServecoreWorkonHandle {
            ok: true,
            repo: self.request.repo,
            cwd: self.repo_path.to_string_lossy().into_owned(),
            engine: self.engine,
            target: self.request.target,
            argv: self.argv,
            status: status.to_owned(),
            message: None,
            leader_argv: None,
            swarm_argv: None,
            pane: None,
            swarm_skipped: None,
        }
    }
}

struct ServecoreAdvancedWorkon {
    request: ServecoreWorkonRequest,
    repo_path: PathBuf,
    task: String,
    engine: String,
    leader_argv: Vec<String>,
    public_leader_argv: Vec<String>,
    swarm_argv: Option<Vec<String>>,
}

impl ServecoreAdvancedWorkon {
    fn into_handle(
        self,
        status: &str,
        pane: Option<String>,
        swarm_skipped: Option<String>,
    ) -> ServecoreWorkonHandle {
        ServecoreWorkonHandle {
            ok: true,
            repo: self.request.repo,
            cwd: self.repo_path.to_string_lossy().into_owned(),
            engine: self.engine,
            target: self.request.target,
            argv: self.public_leader_argv.clone(),
            status: status.to_owned(),
            message: None,
            leader_argv: Some(self.public_leader_argv),
            swarm_argv: self.swarm_argv,
            pane,
            swarm_skipped,
        }
    }
}

fn servecore_prepare_workon(
    root: &Path,
    request: ServecoreWorkonRequest,
    default_engine: &str,
) -> Result<ServecorePreparedOrchestration, String> {
    servecore_validate_path_text(&request.repo, "repo")?;
    if let Some(task) = &request.task {
        servecore_validate_command_token(task, "task")?;
    }
    if let Some(target) = &request.target {
        servecore_validate_command_token(target, "target")?;
    }
    if let Some(prompt) = &request.prompt {
        servecore_validate_prompt_text(prompt)?;
    }
    for oracle in &request.with_oracles {
        servecore_validate_command_token(oracle, "with")?;
    }
    let repo_path = servecore_resolve_workon_repo(root, &request.repo)?;
    if servecore_has_advanced_fields(&request) {
        return servecore_prepare_advanced_workon(request, repo_path);
    }
    let engine = request
        .engine
        .clone()
        .unwrap_or_else(|| default_engine.to_owned());
    servecore_validate_engine_token(&engine, "engine")?;
    let mut argv = vec!["workon".to_owned(), request.repo.clone()];
    if let Some(task) = &request.task {
        argv.push(task.clone());
    }
    argv.extend(["--layout".to_owned(), "nested".to_owned()]);
    Ok(ServecorePreparedOrchestration::Simple(
        ServecorePreparedWorkon {
            request,
            repo_path,
            engine,
            argv,
        },
    ))
}

