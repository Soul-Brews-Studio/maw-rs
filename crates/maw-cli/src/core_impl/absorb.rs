const DISPATCH_60: &[DispatcherEntry] = &[ DispatcherEntry { command: "absorb", handler: Handler::Sync(run_absorb_command) } ];

const ABSORB_USAGE: &str = "usage: maw absorb <donor> --into <receiver> [--dry-run]";
const ABSORB_SYNC_DIRS: &[&str] = &["memory/learnings", "memory/retrospectives", "memory/traces", "memory/collaborations"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct AbsorbOptions { donor: String, receiver: String, dry_run: bool }

#[derive(Debug, Clone)]
struct AbsorbFleetEntry { file: String, path: std::path::PathBuf, session: AbsorbFleetSession }

#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AbsorbFleetSession {
    name: String,
    #[serde(default, alias = "group_name")]
    group_name: String,
    #[serde(default)]
    windows: Vec<AbsorbFleetWindow>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
struct AbsorbFleetWindow {
    name: String,
    #[serde(default)]
    repo: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct AbsorbSyncResult { synced: Vec<(String, usize)>, total: usize }

#[derive(Default)]
struct AbsorbLocalTmux { runner: maw_tmux::CommandTmuxRunner }

fn run_absorb_command(argv: &[String]) -> CliOutput {
    match absorb_run(argv, &mut AbsorbLocalTmux::default()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn absorb_run(argv: &[String], tmux: &mut AbsorbLocalTmux) -> Result<String, String> {
    let options = absorb_parse_args(argv)?;
    let entries = absorb_load_fleet_entries()?;
    let donor = absorb_find_fleet_entry(&entries, &options.donor).ok_or_else(|| format!("donor oracle '{}' not found in fleet config", options.donor))?;
    let receiver = absorb_find_fleet_entry(&entries, &options.receiver).ok_or_else(|| format!("receiver oracle '{}' not found in fleet config", options.receiver))?;
    absorb_run_entries(&options, &donor, &receiver, tmux)
}

fn absorb_parse_args(argv: &[String]) -> Result<AbsorbOptions, String> {
    if argv.first().is_some_and(|arg| matches!(arg.as_str(), "--help" | "-h")) { return Err(format!("{ABSORB_USAGE} — absorb donor knowledge, archive donor, and switch to receiver")); }
    let mut donor = None::<String>;
    let mut receiver = None::<String>;
    let mut dry_run = false;
    let mut index = 0_usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--dry-run" => { dry_run = true; index += 1; },
            "--into" => { let Some(value) = argv.get(index + 1) else { return Err(ABSORB_USAGE.to_owned()); }; absorb_validate_name(value, "receiver")?; receiver = Some(value.clone()); index += 2; },
            value if value.starts_with('-') => return Err(format!("absorb: unknown argument {value}")),
            value => { if donor.is_some() { return Err(ABSORB_USAGE.to_owned()); } absorb_validate_name(value, "donor")?; donor = Some(value.to_owned()); index += 1; },
        }
    }
    let (Some(donor), Some(receiver)) = (donor, receiver) else { return Err(ABSORB_USAGE.to_owned()); };
    Ok(AbsorbOptions { donor, receiver, dry_run })
}

fn absorb_run_entries(options: &AbsorbOptions, donor: &AbsorbFleetEntry, receiver: &AbsorbFleetEntry, tmux: &mut AbsorbLocalTmux) -> Result<String, String> {
    let donor_name = absorb_strip_session_prefix(&donor.session.name);
    let receiver_name = absorb_strip_session_prefix(&receiver.session.name);
    if donor.file == receiver.file || absorb_normalize_name(&donor_name) == absorb_normalize_name(&receiver_name) { return Err("donor and receiver must be different oracles".to_owned()); }
    let donor_path = absorb_resolve_entry_path(donor, &donor_name).ok_or_else(|| format!("could not resolve donor oracle path for '{donor_name}'"))?;
    let receiver_path = absorb_resolve_entry_path(receiver, &receiver_name).ok_or_else(|| format!("could not resolve receiver oracle path for '{receiver_name}'"))?;
    let mut out = String::new();
    let _ = writeln!(out, "\n  Absorbing {donor_name} -> {receiver_name}\n");
    if options.dry_run { absorb_render_dry_run(&mut out, &donor_path, &receiver_path, &donor_name); } else { absorb_run_sync_and_archive(&mut out, &donor_path, &receiver_path, &donor_name, &receiver_name)?; }
    absorb_switch_to_receiver(&receiver.session.name, options.dry_run, tmux, &mut out);
    if options.dry_run { out.push_str("\n  [dry-run] absorb preview complete; no files, fleet entries, repos, or tmux clients changed.\n\n"); } else { absorb_render_complete(&mut out, donor, &donor_name, &receiver_name); }
    Ok(out)
}

fn absorb_render_dry_run(out: &mut String, donor_path: &std::path::Path, receiver_path: &std::path::Path, donor_name: &str) {
    let _ = writeln!(out, "  [dry-run] would sync psi memory: {} -> {}", donor_path.display(), receiver_path.display());
    let _ = writeln!(out, "  [dry-run] would archive donor via: maw archive {donor_name}");
}

fn absorb_run_sync_and_archive(out: &mut String, donor_path: &std::path::Path, receiver_path: &std::path::Path, donor_name: &str, receiver_name: &str) -> Result<(), String> {
    let result = absorb_sync_oracle_vaults(donor_path, receiver_path, donor_name, receiver_name);
    if result.total == 0 { out.push_str("  [ok] psi memory sync complete: nothing new\n"); } else { let _ = writeln!(out, "  [ok] psi memory sync complete: {}", absorb_format_sync_summary(&result)); }
    let archive_args = [donor_name.to_owned(), "--yes".to_owned()];
    out.push_str(&archive_run_command_impl(&archive_args)?);
    Ok(())
}

fn absorb_render_complete(out: &mut String, donor: &AbsorbFleetEntry, donor_name: &str, receiver_name: &str) {
    let disabled = donor.path.with_file_name(format!("{}.disabled", donor.file));
    let archived = if disabled.exists() { "archived" } else { "archive attempted" };
    let _ = writeln!(out, "\n  {donor_name} absorbed into {receiver_name}; donor {archived}.\n");
}

fn absorb_load_fleet_entries() -> Result<Vec<AbsorbFleetEntry>, String> {
    let fleet_dir = active_config_dir().join("fleet");
    let Ok(entries) = std::fs::read_dir(&fleet_dir) else { return Ok(Vec::new()); };
    let mut files = entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("json")).collect::<Vec<_>>();
    files.sort();
    files.into_iter().map(absorb_parse_fleet_file).collect()
}

fn absorb_parse_fleet_file(path: std::path::PathBuf) -> Result<AbsorbFleetEntry, String> {
    let text = std::fs::read_to_string(&path).map_err(|error| format!("absorb: read {}: {error}", path.display()))?;
    let session = serde_json::from_str::<AbsorbFleetSession>(&text).map_err(|error| format!("absorb: parse {}: {error}", path.display()))?;
    let file = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or_default().to_owned();
    Ok(AbsorbFleetEntry { file, path, session })
}

fn absorb_find_fleet_entry(entries: &[AbsorbFleetEntry], query: &str) -> Option<AbsorbFleetEntry> {
    let raw = query.to_lowercase();
    let normalized = absorb_normalize_name(query);
    entries.iter().find(|entry| absorb_entry_names(entry).iter().any(|name| { let candidate = name.to_lowercase(); candidate == raw || absorb_normalize_name(name) == normalized })).cloned()
}

fn absorb_entry_names(entry: &AbsorbFleetEntry) -> Vec<String> {
    let session_name = absorb_strip_session_prefix(&entry.session.name);
    let group_name = absorb_strip_session_prefix(&entry.session.group_name);
    let mut names = vec![entry.session.name.clone(), session_name, group_name];
    for window in &entry.session.windows { names.push(window.name.clone()); names.push(absorb_repo_name(&window.repo)); }
    names.into_iter().filter(|name| !name.is_empty()).collect()
}

fn absorb_resolve_entry_path(entry: &AbsorbFleetEntry, display_name: &str) -> Option<std::path::PathBuf> {
    let repos_root = ghq_root().join("github.com");
    let stem = absorb_strip_oracle_suffix(display_name);
    let repo = entry.session.windows.first().map(|window| window.repo.as_str()).unwrap_or_default();
    let fallback = repos_root.join(repo);
    if !repo.is_empty() && fallback.exists() { return Some(fallback); }
    absorb_find_oracle_repo(&repos_root, &stem)
}

fn absorb_find_oracle_repo(repos_root: &std::path::Path, stem: &str) -> Option<std::path::PathBuf> {
    let wanted = format!("{stem}-oracle").to_lowercase();
    let Ok(orgs) = std::fs::read_dir(repos_root) else { return None; };
    for org in orgs.flatten().filter(|entry| entry.path().is_dir()) {
        let Ok(repos) = std::fs::read_dir(org.path()) else { continue; };
        for repo in repos.flatten().filter(|entry| entry.path().is_dir()) { if repo.file_name().to_string_lossy().eq_ignore_ascii_case(&wanted) { return Some(repo.path()); } }
    }
    None
}

fn absorb_switch_to_receiver(session_name: &str, dry_run: bool, tmux: &mut AbsorbLocalTmux, out: &mut String) {
    let command = format!("tmux switch-client -t {}", absorb_shell_quote(session_name));
    if dry_run { let _ = writeln!(out, "  [dry-run] would switch client: {command}"); return; }
    if std::env::var_os("TMUX").is_none() { let _ = writeln!(out, "  [info] not inside tmux; run manually: {command}"); return; }
    match tmux.absorb_switch_client(session_name) { Ok(()) => { let _ = writeln!(out, "  [ok] switched client to {session_name}"); }, Err(error) => { let _ = writeln!(out, "  [warn] could not switch to receiver: {error}"); let _ = writeln!(out, "  [hint] run manually: {command}"); } }
}

impl AbsorbLocalTmux {
    fn absorb_switch_client(&mut self, session_name: &str) -> Result<(), String> {
        absorb_validate_tmux_target(session_name)?;
        maw_tmux::TmuxRunner::run(&mut self.runner, "switch-client", &["-t".to_owned(), session_name.to_owned()]).map(|_| ()).map_err(|error| error.message)
    }
}

fn absorb_sync_oracle_vaults(from_path: &std::path::Path, to_path: &std::path::Path, from_name: &str, to_name: &str) -> AbsorbSyncResult {
    let mut result = AbsorbSyncResult::default();
    for dir in ABSORB_SYNC_DIRS {
        let count = absorb_sync_dir(&from_path.join("ψ").join(dir), &to_path.join("ψ").join(dir));
        if count > 0 { result.synced.push(((*dir).to_owned(), count)); result.total += count; }
    }
    if result.total > 0 { absorb_append_sync_log(to_path, from_name, to_name, &result); }
    result
}

fn absorb_sync_dir(src: &std::path::Path, dst: &std::path::Path) -> usize {
    let Ok(entries) = std::fs::read_dir(src) else { return 0; };
    let mut count = 0_usize;
    for entry in entries.flatten() {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() { count += absorb_sync_dir(&src_path, &dst_path); } else if !dst_path.exists() { count += absorb_copy_new_file(&src_path, &dst_path); }
    }
    count
}

fn absorb_copy_new_file(src: &std::path::Path, dst: &std::path::Path) -> usize {
    if let Some(parent) = dst.parent() { let _ = std::fs::create_dir_all(parent); }
    std::fs::copy(src, dst).map_or(0, |_| 1)
}

fn absorb_append_sync_log(to_path: &std::path::Path, from_name: &str, to_name: &str, result: &AbsorbSyncResult) {
    let log_dir = to_path.join("ψ/.soul-sync");
    if std::fs::create_dir_all(&log_dir).is_err() { return; }
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs());
    let line = format!("{ts} | {from_name} → {to_name} | {} files | {}\n", result.total, absorb_format_sync_summary(result));
    let _ = std::fs::OpenOptions::new().create(true).append(true).open(log_dir.join("sync.log")).and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
}

fn absorb_format_sync_summary(result: &AbsorbSyncResult) -> String {
    result.synced.iter().map(|(dir, count)| format!("{count} {}", dir.rsplit('/').next().unwrap_or(dir))).collect::<Vec<_>>().join(", ")
}

fn absorb_strip_session_prefix(name: &str) -> String { name.split_once('-').filter(|(prefix, suffix)| prefix.chars().all(|ch| ch.is_ascii_digit()) && !suffix.is_empty()).map_or(name, |(_, suffix)| suffix).to_owned() }
fn absorb_strip_oracle_suffix(name: &str) -> String { name.strip_suffix("-oracle").unwrap_or(name).to_owned() }
fn absorb_normalize_name(name: &str) -> String { absorb_strip_oracle_suffix(&absorb_strip_session_prefix(name)).to_lowercase() }
fn absorb_repo_name(repo_slug: &str) -> String { repo_slug.split('/').rfind(|part| !part.is_empty()).unwrap_or_default().to_owned() }
fn absorb_shell_quote(value: &str) -> String { format!("'{}'", value.replace('\'', "'\\''")) }

fn absorb_validate_name(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.contains('/') { Err(format!("absorb: {label} must be non-empty, unpadded, not start with '-', and not contain '/'")) } else { Ok(()) }
}

fn absorb_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') { Err(format!("absorb: invalid tmux target '{value}'")) } else { Ok(()) }
}

#[cfg(test)]
mod absorb_tests {
    use super::*;

    fn absorb_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn absorb_parse_matches_flags_and_blocks_injection() {
        let parsed = absorb_parse_args(&absorb_strings(&["donor", "--into", "receiver", "--dry-run"])).expect("parse");
        assert_eq!(parsed.donor, "donor");
        assert_eq!(parsed.receiver, "receiver");
        assert!(parsed.dry_run);
        assert_eq!(absorb_parse_args(&absorb_strings(&["-Sbad", "--into", "receiver"])).unwrap_err(), "absorb: unknown argument -Sbad");
        assert!(absorb_parse_args(&absorb_strings(&["donor", "--into", "-Rbad"])).unwrap_err().contains("receiver must"));
    }

    #[test]
    fn absorb_entry_lookup_matches_session_group_window_and_repo() {
        let entry = AbsorbFleetEntry { file: "01-neo.json".to_owned(), path: std::path::PathBuf::from("/tmp/01-neo.json"), session: AbsorbFleetSession { name: "01-neo-oracle".to_owned(), group_name: "team-neo".to_owned(), windows: vec![AbsorbFleetWindow { name: "neo-oracle".to_owned(), repo: "org/neo-oracle".to_owned() }] } };
        assert!(absorb_find_fleet_entry(std::slice::from_ref(&entry), "neo").is_some());
        assert!(absorb_find_fleet_entry(std::slice::from_ref(&entry), "team-neo").is_some());
        assert!(absorb_find_fleet_entry(&[entry], "neo-oracle").is_some());
    }

    #[test]
    fn absorb_sync_copies_only_new_memory_files() {
        let root = std::env::temp_dir().join(format!("maw-rs-absorb-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let donor = root.join("donor");
        let receiver = root.join("receiver");
        std::fs::create_dir_all(donor.join("ψ/memory/learnings/nested")).expect("donor dirs");
        std::fs::create_dir_all(receiver.join("ψ/memory/learnings")).expect("receiver dirs");
        std::fs::write(donor.join("ψ/memory/learnings/a.md"), "new").expect("write donor");
        std::fs::write(donor.join("ψ/memory/learnings/nested/b.md"), "new2").expect("write donor nested");
        std::fs::write(receiver.join("ψ/memory/learnings/a.md"), "old").expect("write existing");
        let result = absorb_sync_oracle_vaults(&donor, &receiver, "donor", "receiver");
        assert_eq!(result.total, 1);
        assert_eq!(std::fs::read_to_string(receiver.join("ψ/memory/learnings/a.md")).expect("existing"), "old");
        assert_eq!(std::fs::read_to_string(receiver.join("ψ/memory/learnings/nested/b.md")).expect("copied"), "new2");
        let _ = std::fs::remove_dir_all(root);
    }
}
