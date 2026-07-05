fn format_watch_table(
    scope: &str,
    results: &[ActivityResult],
    opts: &ActivityOptions,
    status: Option<&str>,
    footer: Option<&str>,
) -> Result<String, String> {
    let rows = results.iter().map(format_activity_human).collect::<Vec<_>>();
    let empty = if opts.stuck_only { "(no stuck panes)" } else { "(no panes resolved)" };
    let body = if rows.is_empty() {
        if status == Some("sampling") { "(sampling...)".to_owned() } else { empty.to_owned() }
    } else {
        rows.join("\n")
    };
    let description = if let Some(status) = status {
        format!("{}, {status}", sampling_description(opts)?)
    } else {
        sampling_description(opts)?
    };
    let footer_block = footer.map_or(String::new(), |footer| {
        format!("\n───────────────────────────────────────────────────────────────────────────────\n{footer}")
    });
    Ok(format!(
        "activity: watching {scope} ({description}); press Ctrl-C to stop\n{body}{footer_block}\n"
    ))
}

fn json_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn format_activity_time(ms: u64) -> String {
    let seconds = (ms / 1_000) % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3_600,
        (seconds / 60) % 60,
        seconds % 60
    )
}

#[cfg(test)]
#[allow(clippy::redundant_closure_for_method_calls)]
mod activity_tests {
    use super::*;

    #[derive(Debug, Default)]
    struct FakeTmux {
        captures: Vec<String>,
        sessions: Vec<TmuxSession>,
        seen_targets: Vec<String>,
    }

    impl ActivityTmux for FakeTmux {
        fn capture(&mut self, target: &str, _lines: u32) -> Result<String, String> {
            self.seen_targets.push(target.to_owned());
            if self.captures.is_empty() {
                Ok(String::new())
            } else {
                Ok(self.captures.remove(0))
            }
        }

        fn list_all(&mut self) -> Vec<TmuxSession> {
            self.sessions.clone()
        }
    }

    #[derive(Debug)]
    struct FakeClock {
        now: u64,
        sleeps: Vec<u64>,
    }

    impl ActivityClock for FakeClock {
        fn now_ms(&mut self) -> u64 {
            let now = self.now;
            self.now += 1_000;
            now
        }

        fn sleep_ms(&mut self, ms: u64) {
            self.sleeps.push(ms);
            self.now += ms;
        }
    }

    #[test]
    fn activity_classifies_busy_idle_and_stuck_like_maw_js_shape() {
        let busy = classify_activity_snapshots(
            "s:1",
            &[
                ActivitySample { text: "hello".to_owned(), at_ms: 1_000 },
                ActivitySample { text: "hello world".to_owned(), at_ms: 2_000 },
                ActivitySample { text: "hello world".to_owned(), at_ms: 3_000 },
            ],
            30_000,
        );
        assert_eq!(busy.state, ActivityState::Busy);
        assert_eq!(busy.confidence, ActivityConfidence::High);
        assert_eq!(busy.diff_samples, 2);
        assert_eq!(format_activity_human(&busy), "s:1: 🟢 BUSY (last change 1s ago, 2/3 samples diff)");

        let idle = classify_activity_snapshots(
            "s:1",
            &[
                ActivitySample { text: "working".to_owned(), at_ms: 1_000 },
                ActivitySample { text: "working".to_owned(), at_ms: 2_000 },
            ],
            2_000,
        );
        assert_eq!(idle.state, ActivityState::Idle);
        assert_eq!(idle.confidence, ActivityConfidence::Medium);

        let stuck = classify_activity_snapshots(
            "s:1",
            &[
                ActivitySample { text: "> ▌".to_owned(), at_ms: 1_000 },
                ActivitySample { text: "> ▌".to_owned(), at_ms: 2_000 },
            ],
            2_000,
        );
        assert_eq!(stuck.state, ActivityState::Stuck);
    }

    #[test]
    fn activity_json_and_watch_single_shot_are_offline() {
        let opts = ActivityOptions {
            all: false,
            watch: true,
            json: false,
            stuck_only: false,
            window: Some("2s".to_owned()),
            samples: Some(2),
            sampler: Some("peek".to_owned()),
            watch_iterations: Some(1),
        };
        let mut tmux = FakeTmux {
            captures: vec!["old".to_owned(), "new".to_owned()],
            ..FakeTmux::default()
        };
        let mut clock = FakeClock { now: 0, sleeps: Vec::new() };
        let output = cmd_activity(Some("agent:main"), &opts, &mut tmux, &mut clock).expect("activity");
        assert!(output.stdout.contains("activity: watching agent:main"));
        assert!(output.stdout.contains("agent:main: 🟢 BUSY"));
        assert_eq!(clock.sleeps, vec![2_000]);
        assert_eq!(tmux.seen_targets, vec!["agent:main", "agent:main"]);
    }

    #[test]
    fn activity_all_resolves_fleet_window_names_to_numeric_tmux_targets() {
        let fleet = vec![NativeFleetSession {
            name: "s".to_owned(),
            windows: vec![NativeFleetWindow { name: "main".to_owned(), repo: String::new() }],
            ..NativeFleetSession::default()
        }];
        assert_eq!(all_activity_targets(&fleet), vec!["s:main".to_owned()]);
        let sessions = vec![TmuxSession {
            name: "s".to_owned(),
            windows: vec![maw_tmux::TmuxWindow { index: 2, name: "main".to_owned(), active: true, cwd: None }],
        }];
        assert_eq!(resolve_activity_peek_target(&sessions, "s:main"), Some("s:2".to_owned()));
        assert_eq!(resolve_activity_peek_target(&sessions, "s:main.1"), Some("s:2.1".to_owned()));
    }

    #[test]
    fn activity_parser_and_target_guard_match_plugin_contract() {
        assert!(parse_activity_cli(&["--all".to_owned(), "pane".to_owned()]).is_err());
        assert!(parse_activity_cli(&["--samples=1".to_owned(), "pane".to_owned()]).is_ok());
        let (_, opts) = parse_activity_cli(&[
            "pane".to_owned(),
            "--json".to_owned(),
            "--stuck-only".to_owned(),
            "--window=1.5s".to_owned(),
            "--samples=2".to_owned(),
            "--sampler=follow".to_owned(),
        ])
        .expect("parse");
        let parsed = parse_activity_options(&opts).expect("options");
        assert_eq!(parsed.window_ms, 1_500);
        assert_eq!(parsed.sampler, ActivitySampler::Follow);
        assert!(validate_activity_tmux_target("-pane").is_err());
        assert!(validate_activity_tmux_target("123").is_err());
        assert!(validate_activity_tmux_target("s:1.0").is_ok());
        assert!(validate_activity_tmux_target("%42").is_ok());
    }

    #[test]
    fn activity_json_matches_committed_golden_shape_without_ref_checkout() {
        let opts = ActivityOptions {
            all: false,
            watch: false,
            json: true,
            stuck_only: false,
            window: Some("2s".to_owned()),
            samples: Some(2),
            sampler: None,
            watch_iterations: None,
        };
        let mut tmux = FakeTmux {
            captures: vec!["ready".to_owned(), "ready".to_owned()],
            ..FakeTmux::default()
        };
        let mut clock = FakeClock { now: 0, sleeps: Vec::new() };
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let output = cmd_activity(Some("s:main"), &opts, &mut tmux, &mut clock).expect("activity");
        assert_eq!(
            output.stdout,
            "{\"pane\":\"s:main\",\"state\":\"idle\",\"confidence\":\"medium\",\"samples\":2,\"diff_samples\":0,\"last_change_ago_seconds\":2,\"sample_window_seconds\":2}\n"
        );
    }
}
