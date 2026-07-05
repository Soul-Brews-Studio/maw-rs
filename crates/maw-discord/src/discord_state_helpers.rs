use super::*;

#[must_use]
pub fn is_numeric_snowflake(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_digit())
}

pub(super) fn channel_type_label(kind: u8) -> String {
    match kind {
        0 => "text".to_owned(),
        2 => "voice".to_owned(),
        4 => "cat".to_owned(),
        5 => "news".to_owned(),
        10..=12 => "thread".to_owned(),
        13 => "stage".to_owned(),
        15 => "forum".to_owned(),
        16 => "media".to_owned(),
        _ => format!("t{kind}"),
    }
}

pub(super) fn reverse_map(map: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    map.iter().map(|(k, v)| (v.clone(), k.clone())).collect()
}

pub(super) fn find_legacy_state_dir(env: &DiscordEnv, bot: &str) -> Option<PathBuf> {
    let path = env.legacy_state_root().join(bot);
    path.exists().then_some(path)
}

pub(super) fn find_hybrid_discord(env: &DiscordEnv, bot: &str) -> Option<PathBuf> {
    let path = find_ghq_path(env, bot)?.join(".discord");
    path.exists().then_some(path)
}

pub(super) fn find_ghq_path(env: &DiscordEnv, name: &str) -> Option<PathBuf> {
    if rejects_option_arg(name) {
        return None;
    }
    let mut found = Vec::new();
    collect_dirs_named(&env.ghq_root, name, 0, &mut found);
    found.sort();
    found
        .iter()
        .find(|p| p.to_string_lossy().contains("/Soul-Brews-Studio/"))
        .cloned()
        .or_else(|| found.into_iter().next())
}

pub(super) fn collect_dirs_named(root: &Path, name: &str, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > 5 || found.len() > 32 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().and_then(|s| s.to_str()) == Some(name) {
            found.push(path.clone());
        }
        collect_dirs_named(&path, name, depth + 1, found);
    }
}

pub(super) fn load_state_dirs_registry(env: &DiscordEnv) -> BTreeSet<String> {
    let Some(repo) = find_ghq_path(env, "discord-oracle") else {
        return BTreeSet::new();
    };
    let Ok(content) = fs::read_to_string(repo.join("src/state-dirs.ts")) else {
        return BTreeSet::new();
    };
    let state_block = content
        .split("export const ANCHORS")
        .next()
        .unwrap_or_default();
    quoted_keys(state_block).into_iter().collect()
}

pub(super) fn load_anchors(env: &DiscordEnv) -> BTreeMap<String, String> {
    let Some(repo) = find_ghq_path(env, "discord-oracle") else {
        return BTreeMap::new();
    };
    let Ok(content) = fs::read_to_string(repo.join("src/state-dirs.ts")) else {
        return BTreeMap::new();
    };
    let Some(block) = content.split("export const ANCHORS").nth(1) else {
        return BTreeMap::new();
    };
    quoted_pairs(block).into_iter().collect()
}

pub(super) fn quoted_keys(input: &str) -> Vec<String> {
    input
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix('"')?;
            let (key, after) = rest.split_once('"')?;
            after.trim_start().starts_with(':').then(|| key.to_owned())
        })
        .collect()
}

pub(super) fn quoted_pairs(input: &str) -> Vec<(String, String)> {
    input
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix('"')?;
            let (key, after_key) = rest.split_once('"')?;
            let value_start = after_key.split_once('"')?.1;
            let (value, _) = value_start.split_once('"')?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

pub(super) fn find_tmux_session(bot: &str) -> Option<String> {
    let out = Command::new("tmux").arg("ls").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .find(|line| line.contains(bot))
        .map(ToOwned::to_owned)
}

pub(super) fn find_online_bun_for_bot(bot: &str) -> Option<(u32, Option<String>)> {
    let out = Command::new("pgrep")
        .args(["-f", "discord/0.0.4"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for pid in text.lines().filter_map(|s| s.trim().parse::<u32>().ok()) {
        let env_out = Command::new("ps")
            .args(["Eww", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let env_text = String::from_utf8_lossy(&env_out.stdout);
        if env_text.contains("DISCORD_STATE_DIR=") && env_text.contains(&format!("/{bot}")) {
            return Some((
                pid,
                find_tmux_session(bot)
                    .map(|line| line.split(':').next().unwrap_or(&line).to_owned()),
            ));
        }
    }
    None
}

pub(super) fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find_map(|w| (w[0] == flag).then(|| w[1].clone()))
}

pub(super) fn fmt_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else {
        format!("{:.1}K", bytes as f64 / 1024.0)
    }
}

pub(super) fn ymd_utc(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

pub(super) fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(m <= 2);
    (
        i32::try_from(year).unwrap_or(1970),
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}
