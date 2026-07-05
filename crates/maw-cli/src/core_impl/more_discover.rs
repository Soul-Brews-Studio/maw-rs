//maw:order 174

#[allow(dead_code)]
pub(crate) mod more_discover {
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct LiveTeamState {
        pub(crate) session: String,
        pub(crate) prefix: String,
        pub(crate) next_index: u32,
        pub(crate) base_branch: Option<String>,
        pub(crate) existing_coders: Vec<LiveCodexCoder>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct LiveCodexCoder {
        pub(crate) session: String,
        pub(crate) window_index: u32,
        pub(crate) window_name: String,
        pub(crate) coder_index: u32,
        pub(crate) worktree: Option<std::path::PathBuf>,
        pub(crate) branch: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct MoreWindowRow174 {
        session: String,
        window_index: u32,
        window_name: String,
        pane_current_path: Option<std::path::PathBuf>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct MoreCoderName174 {
        prefix: Option<String>,
        index: u32,
    }

    pub(crate) fn more_discover_live_team_state<R: maw_tmux::TmuxRunner>(
        runner: &mut R,
        session: &str,
    ) -> Result<LiveTeamState, String> {
        more_validate_session_name(session)?;
        let raw = runner
        .run(
            "list-windows",
            &[
                "-a".to_owned(),
                "-F".to_owned(),
                "#{session_name}|||#{window_index}|||#{window_name}|||#{window_active}|||#{pane_current_path}"
                    .to_owned(),
            ],
        )
        .map_err(|error| format!("more discover: list-windows failed: {}", error.message))?;
        more_discover_live_team_state_from_list_windows(session, &raw)
    }

    pub(crate) fn more_discover_live_team_state_from_list_windows(
        session: &str,
        raw: &str,
    ) -> Result<LiveTeamState, String> {
        more_validate_session_name(session)?;
        let fallback_prefix = more_prefix_from_session(session);
        let rows = more_parse_list_windows(raw)?;
        if !rows.iter().any(|row| row.session == session) {
            return Err(format!("more discover: live session '{session}' not found"));
        }
        let mut coders = rows
            .iter()
            .filter(|row| row.session == session)
            .filter_map(|row| more_coder_from_row(row, &fallback_prefix))
            .collect::<Vec<_>>();
        let prefix = more_choose_prefix(&coders).unwrap_or(fallback_prefix);
        coders.retain(|coder| {
            more_coder_prefix(&coder.window_name, session).as_deref() == Some(prefix.as_str())
        });
        coders.sort_by(|left, right| {
            left.coder_index
                .cmp(&right.coder_index)
                .then_with(|| left.window_index.cmp(&right.window_index))
                .then_with(|| left.window_name.cmp(&right.window_name))
        });
        let next_index = coders
            .iter()
            .map(|coder| coder.coder_index)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let base_branch = more_base_branch_from_existing(&coders);
        Ok(LiveTeamState {
            session: session.to_owned(),
            prefix,
            next_index,
            base_branch,
            existing_coders: coders,
        })
    }

    fn more_parse_list_windows(raw: &str) -> Result<Vec<MoreWindowRow174>, String> {
        raw.lines()
            .filter(|line| !line.trim().is_empty())
            .map(more_parse_window_row)
            .collect()
    }

    fn more_parse_window_row(line: &str) -> Result<MoreWindowRow174, String> {
        let parts = line.split("|||").collect::<Vec<_>>();
        if parts.len() != 5 {
            return Err(format!(
                "more discover: malformed list-windows row: {line:?}"
            ));
        }
        let window_index = parts[1]
            .parse::<u32>()
            .map_err(|_| format!("more discover: bad window index in row: {line:?}"))?;
        let pane_current_path =
            (!parts[4].trim().is_empty()).then(|| std::path::PathBuf::from(parts[4]));
        Ok(MoreWindowRow174 {
            session: parts[0].to_owned(),
            window_index,
            window_name: parts[2].to_owned(),
            pane_current_path,
        })
    }

    fn more_coder_from_row(
        row: &MoreWindowRow174,
        fallback_prefix: &str,
    ) -> Option<LiveCodexCoder> {
        let parsed = more_parse_coder_window_name(&row.window_name)?;
        let prefix = parsed.prefix.unwrap_or_else(|| fallback_prefix.to_owned());
        let branch = row
            .pane_current_path
            .as_deref()
            .and_then(more_agents_branch_from_path);
        Some(LiveCodexCoder {
            session: row.session.clone(),
            window_index: row.window_index,
            window_name: more_prefixed_window_name(&prefix, &row.window_name),
            coder_index: parsed.index,
            worktree: row.pane_current_path.clone(),
            branch,
        })
    }

    fn more_prefixed_window_name(prefix: &str, window_name: &str) -> String {
        if more_parse_coder_window_name(window_name)
            .and_then(|name| name.prefix)
            .is_some()
        {
            window_name.to_owned()
        } else {
            format!("{prefix}-{window_name}")
        }
    }

    fn more_parse_coder_window_name(window_name: &str) -> Option<MoreCoderName174> {
        if let Some((prefix, suffix)) = window_name.rsplit_once("-codex-") {
            if prefix.is_empty() {
                return None;
            }
            let index = more_parse_positive_index(suffix)?;
            return Some(MoreCoderName174 {
                prefix: Some(prefix.to_owned()),
                index,
            });
        }
        let suffix = window_name.strip_prefix("codex-")?;
        let index = more_parse_positive_index(suffix)?;
        Some(MoreCoderName174 {
            prefix: None,
            index,
        })
    }

    fn more_parse_positive_index(raw: &str) -> Option<u32> {
        if raw.is_empty() || !raw.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        let value = raw.parse::<u32>().ok()?;
        (value > 0).then_some(value)
    }

    fn more_choose_prefix(coders: &[LiveCodexCoder]) -> Option<String> {
        let mut counts = std::collections::BTreeMap::<String, (usize, u32)>::new();
        for coder in coders {
            let Some(prefix) = more_coder_prefix(&coder.window_name, &coder.session) else {
                continue;
            };
            let entry = counts.entry(prefix).or_default();
            entry.0 += 1;
            entry.1 = entry.1.max(coder.coder_index);
        }
        counts
            .into_iter()
            .max_by(|left, right| {
                left.1
                     .0
                    .cmp(&right.1 .0)
                    .then_with(|| left.1 .1.cmp(&right.1 .1))
                    .then_with(|| right.0.cmp(&left.0))
            })
            .map(|(prefix, _)| prefix)
    }

    fn more_coder_prefix(window_name: &str, session: &str) -> Option<String> {
        more_parse_coder_window_name(window_name).map(|name| {
            name.prefix
                .unwrap_or_else(|| more_prefix_from_session(session))
        })
    }

    fn more_prefix_from_session(session: &str) -> String {
        let without_numeric = session
            .split_once('-')
            .filter(|(prefix, _)| {
                !prefix.is_empty() && prefix.chars().all(|ch| ch.is_ascii_digit())
            })
            .map_or(session, |(_, rest)| rest);
        without_numeric.trim_matches('-').to_owned()
    }

    fn more_base_branch_from_existing(coders: &[LiveCodexCoder]) -> Option<String> {
        coders.iter().find_map(|coder| coder.branch.clone())
    }

    fn more_agents_branch_from_path(path: &std::path::Path) -> Option<String> {
        let mut components = path.components().peekable();
        while let Some(component) = components.next() {
            if component.as_os_str() == "agents" {
                let name = components.next()?.as_os_str().to_str()?;
                if name.is_empty() || name.starts_with('.') {
                    return None;
                }
                return Some(format!("agents/{name}"));
            }
        }
        None
    }

    fn more_validate_session_name(session: &str) -> Result<(), String> {
        if session.is_empty() {
            return Err("more discover: session is required".to_owned());
        }
        if session.starts_with('-') || session.chars().any(|ch| ch.is_control() || ch == '\0') {
            return Err(format!("more discover: unsafe session {session:?}"));
        }
        Ok(())
    }

    #[cfg(test)]
    mod more_discover_tests {
        use super::*;

        #[derive(Default)]
        struct MoreFakeTmux174 {
            raw: String,
            calls: Vec<(String, Vec<String>)>,
        }

        impl maw_tmux::TmuxRunner for MoreFakeTmux174 {
            fn run(
                &mut self,
                command: &str,
                args: &[String],
            ) -> Result<String, maw_tmux::TmuxError> {
                self.calls.push((command.to_owned(), args.to_vec()));
                Ok(self.raw.clone())
            }
        }

        #[test]
        fn more_discover_derives_prefix_next_index_and_agents_branch() {
            let raw = concat!(
            "188-maw-rs|||0|||maw-rs-oracle|||1|||/repo\n",
            "188-maw-rs|||1|||maw-rs-codex-1|||0|||/repo/agents/maw-rs-codex-1\n",
            "188-maw-rs|||2|||maw-rs-codex-3|||0|||/repo/agents/maw-rs-codex-3/crates/maw-cli\n",
            "other|||1|||other-codex-9|||0|||/repo/agents/other-codex-9\n",
        );
            let state =
                more_discover_live_team_state_from_list_windows("188-maw-rs", raw).expect("state");
            assert_eq!(state.session, "188-maw-rs");
            assert_eq!(state.prefix, "maw-rs");
            assert_eq!(state.next_index, 4);
            assert_eq!(state.base_branch.as_deref(), Some("agents/maw-rs-codex-1"));
            assert_eq!(
                state
                    .existing_coders
                    .iter()
                    .map(|coder| coder.coder_index)
                    .collect::<Vec<_>>(),
                vec![1, 3]
            );
        }

        #[test]
        fn more_discover_supports_bare_legacy_codex_windows_with_session_prefix() {
            let raw = concat!(
                "webhook-relay-v3|||4|||codex-1|||1|||/repo/agents/codex-1\n",
                "webhook-relay-v3|||5|||codex-2|||0|||/repo/agents/codex-2\n",
            );
            let state = more_discover_live_team_state_from_list_windows("webhook-relay-v3", raw)
                .expect("state");
            assert_eq!(state.prefix, "webhook-relay-v3");
            assert_eq!(state.next_index, 3);
            assert_eq!(
                state.existing_coders[0].window_name,
                "webhook-relay-v3-codex-1"
            );
        }

        #[test]
        fn more_discover_runner_uses_issue_174_list_windows_format() {
            let mut tmux = MoreFakeTmux174 {
                raw: "140-arra-oracle-v3|||2|||arra-codex-7|||0|||/repo/agents/arra-codex-7\n"
                    .to_owned(),
                calls: Vec::new(),
            };
            let state =
                more_discover_live_team_state(&mut tmux, "140-arra-oracle-v3").expect("state");
            assert_eq!(state.prefix, "arra");
            assert_eq!(state.next_index, 8);
            assert_eq!(tmux.calls.len(), 1);
            assert_eq!(tmux.calls[0].0, "list-windows");
            assert_eq!(tmux.calls[0].1, vec!["-a", "-F", "#{session_name}|||#{window_index}|||#{window_name}|||#{window_active}|||#{pane_current_path}"]);
        }

        #[test]
        fn more_discover_empty_session_starts_after_one_with_trimmed_session_prefix() {
            let state = more_discover_live_team_state_from_list_windows(
                "188-maw-rs",
                "188-maw-rs|||0|||maw-rs-oracle|||1|||/repo\n",
            )
            .expect("state");
            assert_eq!(state.prefix, "maw-rs");
            assert_eq!(state.next_index, 1);
            assert!(state.base_branch.is_none());
            assert!(state.existing_coders.is_empty());
        }
    }
}
