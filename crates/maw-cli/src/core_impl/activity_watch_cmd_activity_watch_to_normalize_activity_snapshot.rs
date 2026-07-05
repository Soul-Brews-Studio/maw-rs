fn cmd_activity_watch(
    target: Option<&str>,
    opts: &ActivityOptions,
    tmux: &mut dyn ActivityTmux,
    clock: &mut dyn ActivityClock,
) -> Result<ActivityOutput, String> {
    if !opts.all && target.is_none() {
        return Err(ACTIVITY_USAGE.to_owned());
    }
    let max = opts.watch_iterations.unwrap_or(u32::MAX);
    let mut stdout = String::new();
    let mut previous = BTreeMap::<String, ActivityState>::new();
    let mut transition_count = 0u32;
    let scope = if opts.all { "fleet" } else { target.unwrap_or("") };
    if !opts.json {
        stdout.push_str(&format_watch_table(scope, &[], opts, Some("sampling"), None)?);
    }
    for iteration in 0..max {
        let results = if opts.all {
            sample_all_activity(opts, tmux, clock)?
        } else {
            vec![sample_activity(target.unwrap_or(""), opts, tmux, clock)?]
        };
        let transitions = record_activity_transitions(&results, &mut previous);
        transition_count = transition_count.saturating_add(u32::try_from(transitions.len()).unwrap_or(u32::MAX));
        if opts.json {
            for result in transitions {
                if opts.stuck_only && result.state != ActivityState::Stuck {
                    continue;
                }
                stdout.push_str(&format_activity_json(&result));
                stdout.push('\n');
            }
            continue;
        }
        let visible = filter_activity_results(&results, opts);
        let footer = format!(
            "watching ({}) · last refresh: {} · transitions={transition_count}",
            sampling_description(opts)?,
            format_activity_time(clock.now_ms())
        );
        stdout.push_str(&format_watch_table(
            scope,
            &visible,
            opts,
            Some(&format!("refresh={}", iteration + 1)),
            Some(&footer),
        )?);
    }
    Ok(ActivityOutput {
        stdout,
        stderr: String::new(),
    })
}

fn sample_activity(
    target: &str,
    opts: &ActivityOptions,
    tmux: &mut dyn ActivityTmux,
    clock: &mut dyn ActivityClock,
) -> Result<ActivityResult, String> {
    validate_activity_tmux_target(target)?;
    let parsed = parse_activity_options(opts)?;
    sample_resolved_activity(target, target, &parsed, tmux, clock)
}

fn sample_all_activity(
    opts: &ActivityOptions,
    tmux: &mut dyn ActivityTmux,
    clock: &mut dyn ActivityClock,
) -> Result<Vec<ActivityResult>, String> {
    let parsed = parse_activity_options(opts)?;
    let sessions = tmux.list_all();
    let targets = all_activity_targets(&load_native_fleet());
    let mut results = Vec::new();
    for target in targets.into_iter().take(ACTIVITY_ALL_CONCURRENCY.max(1) * 1_000) {
        let Some(snapshot_target) = resolve_activity_peek_target(&sessions, &target) else { continue; };
        if validate_activity_tmux_target(&snapshot_target).is_err() {
            continue;
        }
        if let Ok(result) = sample_resolved_activity(&target, &snapshot_target, &parsed, tmux, clock) {
            results.push(result);
        }
    }
    results.sort_by(|a, b| a.pane.cmp(&b.pane));
    Ok(results)
}

fn all_activity_targets(entries: &[NativeFleetSession]) -> Vec<String> {
    let mut targets = BTreeSet::new();
    for entry in entries {
        if entry.windows.is_empty() {
            targets.insert(entry.name.clone());
            continue;
        }
        for window in &entry.windows {
            let name = if window.name.is_empty() {
                entry.name.clone()
            } else {
                window.name.clone()
            };
            targets.insert(if name.contains(':') {
                name
            } else {
                format!("{}:{name}", entry.name)
            });
        }
    }
    targets.into_iter().collect()
}

fn resolve_activity_peek_target(sessions: &[TmuxSession], target: &str) -> Option<String> {
    let (session_name, window_part) = target.split_once(':')?;
    let window_name = window_part
        .rsplit_once('.')
        .and_then(|(window, pane)| pane.parse::<u32>().ok().map(|_| window))
        .unwrap_or(window_part);
    if window_name.parse::<u32>().is_ok() {
        return Some(target.to_owned());
    }
    let session = sessions.iter().find(|session| session.name == session_name)?;
    let window = session.windows.iter().find(|window| window.name == window_name)?;
    let base = format!("{}:{}", session.name, window.index);
    target
        .rsplit_once('.')
        .and_then(|(_, pane)| pane.parse::<u32>().ok().map(|_| pane.to_owned()))
        .map_or(Some(base.clone()), |pane| Some(format!("{base}.{pane}")))
}

fn sample_resolved_activity(
    pane: &str,
    snapshot_target: &str,
    parsed: &ParsedActivityOptions,
    tmux: &mut dyn ActivityTmux,
    clock: &mut dyn ActivityClock,
) -> Result<ActivityResult, String> {
    let interval_ms = if parsed.samples <= 1 {
        0
    } else {
        parsed.window_ms / u64::from(parsed.samples - 1)
    };
    let mut samples = Vec::new();
    for index in 0..parsed.samples {
        if index > 0 {
            clock.sleep_ms(interval_ms);
        }
        let lines = match parsed.sampler {
            ActivitySampler::Peek | ActivitySampler::Follow => ACTIVITY_PEEK_LINES,
        };
        let text = tmux.capture(snapshot_target, lines)?;
        let at_ms = clock.now_ms();
        samples.push(ActivitySample { text, at_ms });
    }
    Ok(classify_activity_snapshots(pane, &samples, parsed.window_ms))
}

#[allow(clippy::cast_precision_loss)]
fn classify_activity_snapshots(pane: &str, raw_samples: &[ActivitySample], window_ms: u64) -> ActivityResult {
    let normalized = raw_samples
        .iter()
        .map(|sample| normalize_activity_snapshot(&sample.text))
        .collect::<Vec<_>>();
    let mut changed_indexes = BTreeSet::new();
    let mut last_change_at = None;
    for index in 1..normalized.len() {
        if normalized[index] != normalized[index - 1] {
            changed_indexes.insert(index - 1);
            changed_indexes.insert(index);
            last_change_at = raw_samples.get(index).map(|sample| sample.at_ms);
        }
    }
    let end = raw_samples.last().map_or(0, |sample| sample.at_ms);
    let state = if changed_indexes.is_empty() {
        if raw_samples.last().is_some_and(|sample| is_stuck_activity_snapshot(&sample.text)) {
            ActivityState::Stuck
        } else {
            ActivityState::Idle
        }
    } else {
        ActivityState::Busy
    };
    let sample_window_seconds = round_seconds(window_ms as f64 / 1000.0);
    let last_change_ago_seconds = last_change_at.map_or(sample_window_seconds, |changed| {
        round_seconds(end.saturating_sub(changed) as f64 / 1000.0)
    });
    ActivityResult {
        pane: pane.to_owned(),
        state,
        confidence: confidence_for_activity(raw_samples.len()),
        samples: u32::try_from(raw_samples.len()).unwrap_or(u32::MAX),
        diff_samples: u32::try_from(changed_indexes.len()).unwrap_or(u32::MAX),
        last_change_ago_seconds,
        sample_window_seconds,
    }
}

fn normalize_activity_snapshot(input: &str) -> String {
    strip_activity_ansi(input)
        .replace('\r', "\n")
        .split('\n')
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

