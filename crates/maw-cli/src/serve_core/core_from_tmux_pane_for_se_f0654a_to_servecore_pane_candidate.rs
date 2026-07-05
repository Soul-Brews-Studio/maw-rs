impl From<TmuxPane> for ServecoreAgentPane {
    fn from(pane: TmuxPane) -> Self {
        Self {
            id: pane.id,
            command: pane.command,
            target: pane.target,
            title: pane.title,
            cwd: pane.cwd,
            pid: pane.pid,
            last_activity: pane.last_activity,
        }
    }
}

#[derive(Clone, Default)]
pub struct ServecoreTriggerBus {
    events: Arc<Mutex<VecDeque<ServecoreTriggerEvent>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServecoreTriggerEvent {
    pub name: String,
    pub payload: String,
}

impl ServecoreTriggerBus {
    pub fn servecore_fire(&self, event: ServecoreTriggerEvent) {
        let mut guard = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.push_back(event);
    }

    pub fn servecore_snapshot(&self) -> Vec<ServecoreTriggerEvent> {
        let guard = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.iter().cloned().collect()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServecoreWorkonRequest {
    pub repo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, rename = "with")]
    pub with_oracles: Vec<String>,
    #[serde(default)]
    pub attach: bool,
    #[serde(default)]
    pub split: bool,
    #[serde(default)]
    pub tiled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServecoreWorkonHandle {
    pub ok: bool,
    pub repo: String,
    pub cwd: String,
    pub engine: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub argv: Vec<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leader_argv: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swarm_argv: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swarm_skipped: Option<String>,
}

pub trait ServecoreOrchestrator: Send + Sync {
    /// Spawn a native workon orchestration using argv vectors only.
    ///
    /// # Errors
    ///
    /// Returns an error when request validation fails, the repo escapes the configured
    /// root, or the child process exits unsuccessfully.
    fn spawn_workon(
        &self,
        request: ServecoreWorkonRequest,
        engine: Arc<dyn ServecoreEngine>,
    ) -> Result<ServecoreWorkonHandle, String>;
}

#[derive(Clone)]
pub struct ServecoreCommandOrchestrator {
    root: Arc<PathBuf>,
    runner: Arc<dyn ServecoreExecRunner>,
    pane_runner: Arc<dyn ServecorePaneRunner>,
}

impl ServecoreCommandOrchestrator {
    #[must_use]
    pub fn servecore_default() -> Self {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::servecore_with_root(root)
    }

    #[must_use]
    pub fn servecore_with_root(root: PathBuf) -> Self {
        Self {
            root: Arc::new(root),
            runner: Arc::new(ServecoreProcessRunner),
            pane_runner: Arc::new(ServecoreTmuxPaneRunner),
        }
    }

    #[cfg(test)]
    pub fn servecore_with_runner(root: PathBuf, runner: Arc<dyn ServecoreExecRunner>) -> Self {
        Self {
            root: Arc::new(root),
            runner,
            pane_runner: Arc::new(TestPaneRunner),
        }
    }

    #[cfg(test)]
    pub fn servecore_with_runners(
        root: PathBuf,
        runner: Arc<dyn ServecoreExecRunner>,
        pane_runner: Arc<dyn ServecorePaneRunner>,
    ) -> Self {
        Self {
            root: Arc::new(root),
            runner,
            pane_runner,
        }
    }
}

impl ServecoreOrchestrator for ServecoreCommandOrchestrator {
    fn spawn_workon(
        &self,
        request: ServecoreWorkonRequest,
        engine: Arc<dyn ServecoreEngine>,
    ) -> Result<ServecoreWorkonHandle, String> {
        let plan = servecore_prepare_workon(&self.root, request, engine.servecore_engine_name())?;
        match plan {
            ServecorePreparedOrchestration::Simple(plan) => {
                self.runner.servecore_run(&plan.argv, &plan.repo_path)?;
                Ok(plan.into_handle("spawned"))
            }
            ServecorePreparedOrchestration::Advanced(plan) => {
                self.runner
                    .servecore_run(&plan.leader_argv, &plan.repo_path)?;
                Ok(self.servecore_finish_advanced(plan))
            }
        }
    }
}

impl ServecoreCommandOrchestrator {
    fn servecore_finish_advanced(&self, plan: ServecoreAdvancedWorkon) -> ServecoreWorkonHandle {
        let Some(swarm_argv) = plan.swarm_argv.clone() else {
            return plan.into_handle("spawned", None, None);
        };
        let Ok(panes) = self.pane_runner.servecore_list_panes() else {
            return plan.into_handle(
                "leader-spawned",
                None,
                Some("pane discovery failed".to_owned()),
            );
        };
        let Ok(pane) = servecore_find_pane_for_task(&panes, &plan.task) else {
            return plan.into_handle(
                "leader-spawned",
                None,
                Some("pane discovery failed".to_owned()),
            );
        };
        let Ok(line) = servecore_shell_line_for_self(&swarm_argv) else {
            return plan.into_handle("leader-spawned", None, Some("pane send failed".to_owned()));
        };
        if self
            .pane_runner
            .servecore_send_literal_enter(&pane, &line)
            .is_err()
        {
            return plan.into_handle("leader-spawned", None, Some("pane send failed".to_owned()));
        }
        plan.into_handle("spawned", Some(pane), None)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServecorePaneCandidate {
    pub id: String,
    pub title: String,
}

