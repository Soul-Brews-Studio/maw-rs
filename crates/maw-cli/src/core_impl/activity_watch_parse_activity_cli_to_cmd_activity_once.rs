fn parse_activity_cli(argv: &[String]) -> Result<(Option<String>, ActivityOptions), String> {
    let mut opts = ActivityOptions {
        all: false,
        watch: false,
        json: false,
        stuck_only: false,
        window: None,
        samples: None,
        sampler: None,
        watch_iterations: None,
    };
    let mut target: Option<String> = None;
    let mut index = 0;
    while index < argv.len() {
        let arg = &argv[index];
        match arg.as_str() {
            "--help" | "-h" => return Err(ACTIVITY_USAGE.to_owned()),
            "--all" => opts.all = true,
            "--watch" => opts.watch = true,
            "--json" => opts.json = true,
            "--stuck-only" => opts.stuck_only = true,
            "--window" => {
                index += 1;
                let Some(value) = argv.get(index) else { return Err(ACTIVITY_USAGE.to_owned()); };
                opts.window = Some(value.clone());
            }
            "--samples" => {
                index += 1;
                let Some(value) = argv.get(index) else { return Err(ACTIVITY_USAGE.to_owned()); };
                opts.samples = Some(value.parse::<u32>().map_err(|_| "activity: --samples must be an integer from 2 to 50".to_owned())?);
            }
            "--sampler" => {
                index += 1;
                let Some(value) = argv.get(index) else { return Err(ACTIVITY_USAGE.to_owned()); };
                opts.sampler = Some(value.clone());
            }
            _ if arg.starts_with("--window=") => opts.window = Some(arg[9..].to_owned()),
            _ if arg.starts_with("--samples=") => {
                opts.samples = Some(arg[10..].parse::<u32>().map_err(|_| "activity: --samples must be an integer from 2 to 50".to_owned())?);
            }
            _ if arg.starts_with("--sampler=") => opts.sampler = Some(arg[10..].to_owned()),
            _ if arg.starts_with('-') => return Err(ACTIVITY_USAGE.to_owned()),
            _ => {
                if target.replace(arg.clone()).is_some() {
                    return Err(ACTIVITY_USAGE.to_owned());
                }
            }
        }
        index += 1;
    }
    if opts.all && target.is_some() {
        return Err(ACTIVITY_USAGE.to_owned());
    }
    if !opts.all && target.is_none() {
        return Err(ACTIVITY_USAGE.to_owned());
    }
    if let Some(raw_target) = target.as_deref() {
        validate_activity_tmux_target(raw_target)?;
    }
    Ok((target, opts))
}

fn parse_activity_options(opts: &ActivityOptions) -> Result<ParsedActivityOptions, String> {
    let window_ms = match opts.window.as_deref() {
        None => 30_000,
        Some(value) => parse_activity_duration_ms(value)
            .ok_or_else(|| format!("activity: invalid --window duration: {value}"))?,
    };
    if window_ms == 0 {
        return Err(format!(
            "activity: invalid --window duration: {}",
            opts.window.as_deref().unwrap_or("")
        ));
    }
    let sample_count = opts.samples.unwrap_or(3);
    if !(2..=50).contains(&sample_count) {
        return Err("activity: --samples must be an integer from 2 to 50".to_owned());
    }
    let sampler_kind = match opts.sampler.as_deref().unwrap_or("peek") {
        "peek" => ActivitySampler::Peek,
        "follow" => ActivitySampler::Follow,
        _ => return Err("activity: --sampler must be peek or follow".to_owned()),
    };
    Ok(ParsedActivityOptions {
        window_ms,
        samples: sample_count,
        sampler: sampler_kind,
    })
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)]
fn parse_activity_duration_ms(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed != value {
        return None;
    }
    let split = trimmed
        .find(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .unwrap_or(trimmed.len());
    let (number, unit) = trimmed.split_at(split);
    if number.is_empty() {
        return None;
    }
    let amount = number.parse::<f64>().ok()?;
    if !amount.is_finite() || amount <= 0.0 {
        return None;
    }
    let multiplier = match unit {
        "" | "ms" => 1.0,
        "s" | "sec" | "secs" | "second" | "seconds" => 1_000.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 60_000.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3_600_000.0,
        _ => return None,
    };
    let ms = amount * multiplier;
    if ms > u64::MAX as f64 {
        return None;
    }
    Some(ms.round() as u64)
}

fn validate_activity_tmux_target(target: &str) -> Result<(), String> {
    if target.is_empty() || target.trim() != target || target.starts_with('-') {
        return Err("activity: tmux target must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if target.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("activity: bare numeric tmux targets are refused; use session:window or %pane_id".to_owned());
    }
    if !target
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '%' | '-'))
    {
        return Err("activity: tmux target contains unsupported characters".to_owned());
    }
    Ok(())
}

fn cmd_activity(
    target: Option<&str>,
    opts: &ActivityOptions,
    tmux: &mut dyn ActivityTmux,
    clock: &mut dyn ActivityClock,
) -> Result<ActivityOutput, String> {
    if opts.watch {
        return cmd_activity_watch(target, opts, tmux, clock);
    }
    cmd_activity_once(target, opts, tmux, clock)
}

fn cmd_activity_once(
    target: Option<&str>,
    opts: &ActivityOptions,
    tmux: &mut dyn ActivityTmux,
    clock: &mut dyn ActivityClock,
) -> Result<ActivityOutput, String> {
    let mut stderr = String::new();
    let results = if opts.all {
        if !opts.json {
            let _ = writeln!(stderr, "activity: surveying fleet ({})...", sampling_description(opts)?);
        }
        sample_all_activity(opts, tmux, clock)?
    } else {
        let target = target.ok_or_else(|| ACTIVITY_USAGE.to_owned())?;
        vec![sample_activity(target, opts, tmux, clock)?]
    };
    let visible = filter_activity_results(&results, opts);
    Ok(ActivityOutput {
        stdout: format_activity_output(&visible, opts),
        stderr,
    })
}

