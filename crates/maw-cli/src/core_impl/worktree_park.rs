// park command implementation — called from DISPATCH_291 in part291.rs.
// No DISPATCH_292 const: this file provides helpers only.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ParkedState {
    window: String,
    session: String,
    branch: String,
    cwd: String,
    #[serde(rename = "lastCommit")]
    last_commit: String,
    #[serde(rename = "dirtyFiles")]
    dirty_files: Vec<String>,
    note: String,
    #[serde(rename = "parkedAt")]
    parked_at: String,
}

fn run_park_command(argv: &[String]) -> CliOutput {
    let sub = argv.first().map_or("", String::as_str);
    let result = if sub == "ls" || sub == "list" {
        cmd_park_ls()
    } else {
        cmd_park(argv)
    };
    match result {
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

/// Resolve (`target_window`, note) from raw args, current window, and known window names.
/// Exported for unit testing.
#[must_use]
pub fn resolve_park(
    raw: &[String],
    current: &str,
    known: &[String],
) -> (String, Option<String>) {
    if raw.is_empty() {
        return (current.to_owned(), None);
    }
    let first = raw[0].as_str();
    let is_other_known = known.iter().any(|w| w == first) && first != current;
    if is_other_known {
        let note_parts: Vec<&str> = raw[1..].iter().map(String::as_str).collect();
        let note = if note_parts.is_empty() {
            None
        } else {
            Some(note_parts.join(" "))
        };
        (first.to_owned(), note)
    } else {
        let note_parts: Vec<&str> = raw.iter().map(String::as_str).collect();
        let note = if note_parts.is_empty() {
            None
        } else {
            Some(note_parts.join(" "))
        };
        (current.to_owned(), note)
    }
}

/// Format a duration (ms) as human-readable ago string.
#[must_use]
pub fn time_ago_ms(elapsed_ms: i64) -> String {
    if elapsed_ms < 0 {
        return "0m ago".to_owned();
    }
    let minutes = elapsed_ms / 60_000;
    if minutes < 60 {
        format!("{minutes}m ago")
    } else {
        let hours = elapsed_ms / 3_600_000;
        if hours < 24 {
            format!("{hours}h ago")
        } else {
            let days = elapsed_ms / 86_400_000;
            format!("{days}d ago")
        }
    }
}

fn park_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn park_iso_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (year, month, day, hour, minute, sec) = epoch_secs_to_ymd_hms(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{sec:02}.000Z")
}

fn epoch_secs_to_ymd_hms(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let sec = secs % 60;
    let total_min = secs / 60;
    let minute = total_min % 60;
    let total_hours = total_min / 60;
    let hour = total_hours % 24;
    let total_days = total_hours / 24;
    let zday = total_days + 719_468;
    let era = zday / 146_097;
    let doe = zday - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day, hour, minute, sec)
}

fn park_parse_iso_ms(iso: &str) -> Option<i64> {
    let trimmed = iso.trim_end_matches('Z');
    let (date_part, time_part) = trimmed.split_once('T')?;
    let date_parts: Vec<&str> = date_part.split('-').collect();
    if date_parts.len() != 3 {
        return None;
    }
    let year: i64 = date_parts[0].parse().ok()?;
    let month: i64 = date_parts[1].parse().ok()?;
    let day: i64 = date_parts[2].parse().ok()?;
    let time_parts: Vec<&str> = time_part.splitn(2, '.').collect();
    let hms: Vec<&str> = time_parts[0].split(':').collect();
    if hms.len() != 3 {
        return None;
    }
    let hour: i64 = hms[0].parse().ok()?;
    let minute: i64 = hms[1].parse().ok()?;
    let sec: i64 = hms[2].parse().ok()?;
    let ms_frac: i64 = if time_parts.len() > 1 {
        let frac = time_parts[1];
        let padded = format!("{frac:0<3}");
        padded[..3].parse().unwrap_or(0)
    } else {
        0
    };
    let adj_year = if month <= 2 { year - 1 } else { year };
    let adj_month = if month <= 2 { month + 9 } else { month - 3 };
    let era = if adj_year >= 0 {
        adj_year / 400
    } else {
        (adj_year - 399) / 400
    };
    let yoe = adj_year - era * 400;
    let doy = (153 * adj_month + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let total_secs = days * 86_400 + hour * 3_600 + minute * 60 + sec;
    Some(total_secs * 1_000 + ms_frac)
}

fn park_tmux(args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("tmux")
        .args(args)
        .output()
        .map_err(|e| format!("park: tmux failed: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(if msg.is_empty() {
            format!("park: tmux exited {}", out.status)
        } else {
            msg
        })
    }
}

fn git_in_dir(cwd: &str, args: &[&str]) -> String {
    let mut cmd_args = vec!["-C", cwd];
    cmd_args.extend_from_slice(args);
    std::process::Command::new("git")
        .args(&cmd_args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

fn parked_dir() -> std::path::PathBuf {
    let env = current_xdg_env();
    maw_state_path(&env, &["parked"])
}

fn legacy_parked_dir() -> std::path::PathBuf {
    let env = current_xdg_env();
    maw_config_path(&env, &["parked"])
}

fn cmd_park(raw: &[String]) -> Result<String, String> {
    let session = park_tmux(&["display-message", "-p", "#S"])?;
    let current_window = park_tmux(&["display-message", "-p", "#W"])?;
    let list_raw =
        park_tmux(&["list-windows", "-t", &session, "-F", "#I:#W"])?;
    let known_names: Vec<String> = list_raw
        .lines()
        .filter_map(|line| line.split_once(':').map(|(_, name)| name.to_owned()))
        .collect();
    let (target, note) = resolve_park(raw, &current_window, &known_names);
    let cwd = park_tmux(&[
        "display-message",
        "-t",
        &format!("{session}:{target}"),
        "-p",
        "#{pane_current_path}",
    ])?;
    let branch = git_in_dir(&cwd, &["branch", "--show-current"]);
    let last_commit = git_in_dir(&cwd, &["log", "-1", "--oneline"]);
    let dirty_raw = git_in_dir(&cwd, &["status", "--short"]);
    let dirty_files: Vec<String> = dirty_raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    let state = ParkedState {
        window: target.clone(),
        session,
        branch,
        cwd,
        last_commit,
        dirty_files,
        note: note.clone().unwrap_or_default(),
        parked_at: park_iso_now(),
    };
    let dir = parked_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("park: cannot create parked dir: {e}"))?;
    let json =
        serde_json::to_string_pretty(&state).map_err(|e| format!("park: serialize: {e}"))?;
    let file_path = dir.join(format!("{target}.json"));
    std::fs::write(&file_path, format!("{json}\n"))
        .map_err(|e| format!("park: write {}: {e}", file_path.display()))?;
    let note_suffix = note
        .as_deref()
        .map_or_else(String::new, |n| format!(" \u{2014} \"{n}\""));
    Ok(format!(
        "\x1b[32m\u{2713}\x1b[0m parked \x1b[33m{target}\x1b[0m{note_suffix}"
    ))
}

fn cmd_park_ls() -> Result<String, String> {
    use std::fmt::Write as _;
    let primary = parked_dir();
    std::fs::create_dir_all(&primary)
        .map_err(|e| format!("park: cannot create parked dir: {e}"))?;
    let legacy = legacy_parked_dir();
    let mut dirs = vec![primary.clone()];
    if legacy != primary {
        dirs.push(legacy);
    }
    let mut seen_names = std::collections::BTreeSet::new();
    let mut entries: Vec<ParkedState> = Vec::new();
    for dir in &dirs {
        let Ok(read) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut files: Vec<std::path::PathBuf> = read
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(std::ffi::OsStr::to_str) == Some("json"))
            .collect();
        files.sort();
        for path in files {
            let Some(fname) = path.file_name().and_then(std::ffi::OsStr::to_str) else {
                continue;
            };
            if seen_names.contains(fname) {
                continue;
            }
            seen_names.insert(fname.to_owned());
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(state) = serde_json::from_str::<ParkedState>(&text) else {
                continue;
            };
            entries.push(state);
        }
    }
    if entries.is_empty() {
        return Ok("\x1b[90mno parked tabs\x1b[0m".to_string());
    }
    let now_ms = park_now_ms();
    let count = entries.len();
    let mut out = format!("\n\x1b[36mPARKED\x1b[0m ({count}):\n\n");
    for state in &entries {
        let elapsed = park_parse_iso_ms(&state.parked_at).map_or(0, |ts| now_ms - ts);
        let ago = time_ago_ms(elapsed);
        let note_display = if state.note.is_empty() {
            "\x1b[90m(no note)\x1b[0m".to_string()
        } else {
            format!("\"{}\"", state.note)
        };
        let branch_display = if state.branch.is_empty() {
            "no branch".to_owned()
        } else {
            state.branch.clone()
        };
        let dirty_display = if state.dirty_files.is_empty() {
            "\x1b[32mclean\x1b[0m".to_string()
        } else {
            let count_dirty = state.dirty_files.len();
            format!("\x1b[33m{count_dirty} dirty\x1b[0m")
        };
        let _ = writeln!(
            out,
            "  \x1b[33m{}\x1b[0m  {note_display}  {ago}  {branch_display}  {dirty_display}",
            state.window
        );
    }
    Ok(out)
}
