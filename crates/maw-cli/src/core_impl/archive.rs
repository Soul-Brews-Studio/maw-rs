const DISPATCH_45: &[DispatcherEntry] = &[
    DispatcherEntry { command: "archive", handler: Handler::Sync(run_archive_command) },
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArchiveOptions {
    oracle: String,
    dry_run: bool,
    yes: bool,
}

#[derive(Debug, Clone)]
struct ArchiveFleetEntry {
    file: String,
    path: std::path::PathBuf,
    session: NativeFleetSession,
}

fn run_archive_command(argv: &[String]) -> CliOutput {
    match archive_run_command_impl(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn archive_usage_text() -> &'static str {
    "usage: maw archive <oracle> [--dry-run] [--yes] — archive an oracle's tmux session and data"
}

fn archive_run_command_impl(argv: &[String]) -> Result<String, String> {
    let Some(options) = archive_parse_args(argv)? else {
        return Ok(format!("{}\n", archive_usage_text()));
    };
    let entry = archive_find_entry(&options.oracle)?
        .ok_or_else(|| format!("oracle '{}' not found in fleet config", options.oracle))?;
    archive_render_and_apply(&entry, &options)
}

fn archive_parse_args(argv: &[String]) -> Result<Option<ArchiveOptions>, String> {
    let mut oracle = None::<String>;
    let mut dry_run = false;
    let mut yes = false;

    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--dry-run" => dry_run = true,
            "--yes" | "-y" => yes = true,
            value if value.starts_with('-') => return Err(format!("archive: unknown argument {value}")),
            value => {
                if oracle.is_some() {
                    return Err(archive_usage_text().to_owned());
                }
                archive_validate_oracle(value)?;
                oracle = Some(value.to_owned());
            }
        }
    }

    let Some(oracle) = oracle else {
        return Err("usage: maw archive <oracle> [--dry-run]".to_owned());
    };
    Ok(Some(ArchiveOptions { oracle, dry_run, yes }))
}

fn archive_validate_oracle(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.contains('/') {
        Err("archive: oracle must be non-empty, unpadded, not start with '-', and not contain '/'".to_owned())
    } else {
        Ok(())
    }
}

fn archive_find_entry(oracle: &str) -> Result<Option<ArchiveFleetEntry>, String> {
    let fleet_dir = active_config_dir().join("fleet");
    let Ok(entries) = std::fs::read_dir(&fleet_dir) else {
        return Ok(None);
    };
    let mut files = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("json"))
        .collect::<Vec<_>>();
    files.sort();

    for path in files {
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("archive: read {}: {error}", path.display()))?;
        let session = serde_json::from_str::<NativeFleetSession>(&text)
            .map_err(|error| format!("archive: parse {}: {error}", path.display()))?;
        if archive_session_oracle_name(&session.name) == oracle {
            let file = path
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_owned();
            return Ok(Some(ArchiveFleetEntry { file, path, session }));
        }
    }
    Ok(None)
}

fn archive_session_oracle_name(session_name: &str) -> &str {
    session_name
        .split_once('-')
        .filter(|(prefix, suffix)| prefix.chars().all(|ch| ch.is_ascii_digit()) && !suffix.is_empty())
        .map_or(session_name, |(_, suffix)| suffix)
}

fn archive_render_and_apply(entry: &ArchiveFleetEntry, options: &ArchiveOptions) -> Result<String, String> {
    let repo_slug = entry
        .session
        .windows
        .first()
        .map_or("", |window| window.repo.as_str());
    if !repo_slug.is_empty() {
        archive_validate_repo_slug(repo_slug)?;
    }

    let mut out = String::new();
    let _ = writeln!(out, "\n  \x1b[36m⚰️  Archiving\x1b[0m — {}\n", options.oracle);
    archive_render_soul_sync(&mut out, entry, repo_slug, options)?;
    archive_render_disable(&mut out, entry, options)?;
    archive_render_repo_archive(&mut out, repo_slug, options)?;
    archive_render_death_certificate(&mut out, entry, options);
    Ok(out)
}

fn archive_render_soul_sync(
    out: &mut String,
    entry: &ArchiveFleetEntry,
    repo_slug: &str,
    options: &ArchiveOptions,
) -> Result<(), String> {
    let mut host = SoulsyncSystemHost;
    let fleet = load_native_fleet();
    let github_root = ghq_root().join("github.com");
    soul_sync_archive_render_soul_sync_with(out, entry, repo_slug, options, &mut host, &fleet, &github_root)
}

fn soul_sync_archive_render_soul_sync_with(
    out: &mut String,
    entry: &ArchiveFleetEntry,
    repo_slug: &str,
    options: &ArchiveOptions,
    host: &mut impl SoulsyncHost,
    fleet: &[NativeFleetSession],
    github_root: &std::path::Path,
) -> Result<(), String> {
    if entry.session.sync_peers.is_empty() {
        let _ = writeln!(out, "  \x1b[90m○\x1b[0m no sync_peers configured — knowledge stays local");
        return Ok(());
    }
    if options.dry_run {
        let _ = writeln!(
            out,
            "  \x1b[36m⬡\x1b[0m [dry-run] would soul-sync to {}",
            entry.session.sync_peers.join(", ")
        );
        return Ok(());
    }

    archive_require_yes(options)?;
    let _ = writeln!(out, "  \x1b[36m⏳\x1b[0m final soul-sync to peers...");
    if repo_slug.is_empty() {
        let _ = writeln!(out, "  \x1b[33m⚠\x1b[0m soul-sync failed: oracle repo not configured");
        return Ok(());
    }
    let oracle_path = soul_sync_archive_repo_path(github_root, repo_slug)?;
    if !oracle_path.exists() {
        let _ = writeln!(
            out,
            "  \x1b[33m⚠\x1b[0m soul-sync failed: oracle repo not found at {}",
            oracle_path.display()
        );
        return Ok(());
    }

    let mut total = 0_usize;
    for peer in &entry.session.sync_peers {
        let peer = soulsync_validate_name(peer, "peer")?;
        let Some(peer_path) = soulsync_resolve_oracle_path(&peer, fleet, github_root) else {
            let _ = writeln!(out, "  \x1b[33m⚠\x1b[0m {peer}: repo not found, skipping");
            continue;
        };
        let result = soulsync_sync_oracle_vaults(&oracle_path, &peer_path, &options.oracle, &peer, host);
        total += result.total;
        soulsync_render_oracle_result(out, &result);
    }
    soulsync_render_total(out, total, "synced");
    let _ = writeln!(out, "  \x1b[32m✓\x1b[0m soul-sync complete");
    Ok(())
}

fn soul_sync_archive_repo_path(github_root: &std::path::Path, repo_slug: &str) -> Result<std::path::PathBuf, String> {
    archive_validate_repo_slug(repo_slug)?;
    let mut path = github_root.to_path_buf();
    for part in repo_slug.split('/') {
        path.push(part);
    }
    Ok(path)
}

fn archive_render_disable(out: &mut String, entry: &ArchiveFleetEntry, options: &ArchiveOptions) -> Result<(), String> {
    if options.dry_run {
        let _ = writeln!(
            out,
            "  \x1b[36m⬡\x1b[0m [dry-run] would disable: {} → {}.disabled",
            entry.file, entry.file
        );
        return Ok(());
    }
    archive_require_yes(options)?;
    let disabled = entry.path.with_file_name(format!("{}.disabled", entry.file));
    match std::fs::rename(&entry.path, &disabled) {
        Ok(()) => {
            let _ = writeln!(out, "  \x1b[32m✓\x1b[0m fleet config disabled: {}.disabled", entry.file);
        }
        Err(error) => {
            let _ = writeln!(out, "  \x1b[33m⚠\x1b[0m could not disable fleet config: {error}");
        }
    }
    Ok(())
}

fn archive_render_repo_archive(out: &mut String, repo_slug: &str, options: &ArchiveOptions) -> Result<(), String> {
    if repo_slug.is_empty() {
        return Ok(());
    }
    if options.dry_run {
        let _ = writeln!(out, "  \x1b[36m⬡\x1b[0m [dry-run] would archive: gh repo archive {repo_slug}");
        return Ok(());
    }
    archive_require_yes(options)?;
    match std::process::Command::new("gh")
        .args(["repo", "archive", repo_slug, "--yes"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let _ = writeln!(out, "  \x1b[32m✓\x1b[0m GitHub repo archived: {repo_slug}");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let message = if stderr.is_empty() { format!("gh exited {}", output.status) } else { stderr };
            let _ = writeln!(out, "  \x1b[33m⚠\x1b[0m archive failed: {message}");
        }
        Err(error) => {
            let _ = writeln!(out, "  \x1b[33m⚠\x1b[0m archive failed: {error}");
        }
    }
    Ok(())
}

fn archive_render_death_certificate(out: &mut String, entry: &ArchiveFleetEntry, options: &ArchiveOptions) {
    if options.dry_run {
        let _ = writeln!(out, "  \x1b[36m⬡\x1b[0m [dry-run] would log death to family registry");
        out.push('\n');
    } else {
        let _ = writeln!(
            out,
            "  \x1b[32m✓\x1b[0m {} archived — ψ/ preserved locally, knowledge synced to peers\n",
            options.oracle
        );
        let _ = writeln!(out, "  \x1b[90mNothing is deleted (Principle 1). ψ/ and git history remain.\x1b[0m");
        let _ = writeln!(
            out,
            "  \x1b[90mTo unarchive: rename {}.disabled → {} + gh repo unarchive\x1b[0m",
            entry.file, entry.file
        );
        out.push('\n');
    }
    out.push('\n');
}

fn archive_require_yes(options: &ArchiveOptions) -> Result<(), String> {
    if options.yes {
        Ok(())
    } else {
        Err("archive: refusing real archive without --yes; use --dry-run to preview".to_owned())
    }
}

fn archive_validate_repo_slug(value: &str) -> Result<(), String> {
    let parts = value.split('/').collect::<Vec<_>>();
    if parts.len() != 2 || parts.iter().any(|part| !archive_safe_repo_part(part)) || value.starts_with('-') {
        return Err(format!("archive: invalid repo slug '{value}'"));
    }
    Ok(())
}

fn archive_safe_repo_part(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

#[cfg(test)]
mod archive_tests {
    use super::*;

    fn archive_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[derive(Clone)]
    struct ArchiveSoulsyncFakeHost {
        now: String,
    }

    impl SoulsyncHost for ArchiveSoulsyncFakeHost {
        fn soulsync_current_dir(&mut self) -> std::path::PathBuf { std::path::PathBuf::from(".") }
        fn soulsync_tmux_cwd(&mut self) -> Option<std::path::PathBuf> { None }
        fn soulsync_git_common_dir(&mut self, _: &std::path::Path) -> Option<std::path::PathBuf> { None }
        fn soulsync_git_top_level(&mut self, _: &std::path::Path) -> Option<std::path::PathBuf> { None }
        fn soulsync_now(&mut self) -> String { self.now.clone() }
    }

    struct ArchiveEnvRestore {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl ArchiveEnvRestore {
        fn set(key: &'static str, value: &str) -> Self {
            let old = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, old }
        }
    }

    impl Drop for ArchiveEnvRestore {
        fn drop(&mut self) {
            if let Some(value) = self.old.take() {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn archive_session(name: &str, repo: &str, peers: &[&str]) -> NativeFleetSession {
        NativeFleetSession {
            name: name.to_owned(),
            windows: vec![NativeFleetWindow { name: name.to_owned(), repo: repo.to_owned() }],
            sync_peers: peers.iter().map(|value| (*value).to_owned()).collect(),
            project_repos: Vec::new(),
        }
    }

    fn archive_write(path: &std::path::Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, text).expect("write");
    }

    fn archive_temp_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("maw-rs-archive-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn archive_parse_help_and_flags() {
        assert!(archive_parse_args(&archive_strings(&["--help"])).expect("parse").is_none());
        let parsed = archive_parse_args(&archive_strings(&["neo", "--dry-run", "--yes"]))
            .expect("parse")
            .expect("options");
        assert_eq!(parsed.oracle, "neo");
        assert!(parsed.dry_run);
        assert!(parsed.yes);
    }

    #[test]
    fn archive_oracle_guard_blocks_option_injection() {
        let error = archive_parse_args(&archive_strings(&["-oProxyCommand=touch-pwned", "--dry-run"]))
            .expect_err("guard");
        assert!(error.contains("unknown argument"));
        assert!(archive_validate_repo_slug("-bad/repo").is_err());
        assert!(archive_validate_repo_slug("org/-repo").is_err());
        assert!(archive_validate_repo_slug("org/repo").is_ok());
    }

    #[test]
    fn archive_session_oracle_strips_numeric_prefix() {
        assert_eq!(archive_session_oracle_name("03-neo"), "neo");
        assert_eq!(archive_session_oracle_name("neo"), "neo");
        assert_eq!(archive_session_oracle_name("dev-neo"), "dev-neo");
    }

    #[test]
    fn archive_soul_sync_copies_new_memory_only_matches_golden_without_js_ref() {
        let _env = ArchiveEnvRestore::set("MAW_JS_REF_DIR", "/nonexistent");
        let root = archive_temp_root("soul-sync-golden");
        let github_root = root.join("github.com");
        let neo = github_root.join("org/neo-oracle");
        let trinity = github_root.join("org/trinity-oracle");
        archive_write(&neo.join("ψ/memory/learnings/new.md"), "new learning");
        archive_write(&neo.join("ψ/memory/learnings/existing.md"), "source should not overwrite");
        archive_write(&neo.join("ψ/identity.json"), r#"{"secret":true}"#);
        archive_write(&neo.join("ψ/vault/secret.txt"), "vault secret");
        archive_write(&trinity.join("ψ/memory/learnings/existing.md"), "keep destination");

        let entry = ArchiveFleetEntry {
            file: "01-neo.json".to_owned(),
            path: root.join("fleet/01-neo.json"),
            session: archive_session("01-neo", "org/neo-oracle", &["trinity"]),
        };
        let options = ArchiveOptions { oracle: "neo".to_owned(), dry_run: false, yes: true };
        let fleet = vec![
            archive_session("01-neo", "org/neo-oracle", &["trinity"]),
            archive_session("02-trinity", "org/trinity-oracle", &[]),
        ];
        let mut host = ArchiveSoulsyncFakeHost { now: "2026-06-26T00:00:00.000Z".to_owned() };
        let mut out = String::new();

        soul_sync_archive_render_soul_sync_with(
            &mut out,
            &entry,
            "org/neo-oracle",
            &options,
            &mut host,
            &fleet,
            &github_root,
        )
        .expect("sync");

        assert_eq!(out, include_str!("../../tests/fixtures/native-archive/soul-sync.stdout"));
        assert_eq!(std::fs::read_to_string(trinity.join("ψ/memory/learnings/new.md")).expect("copied"), "new learning");
        assert_eq!(
            std::fs::read_to_string(trinity.join("ψ/memory/learnings/existing.md")).expect("preserved"),
            "keep destination"
        );
        assert!(!trinity.join("ψ/identity.json").exists(), "identity files must not be copied");
        assert!(!trinity.join("ψ/vault/secret.txt").exists(), "vault secrets must not be copied");
        let log = std::fs::read_to_string(trinity.join("ψ/.soul-sync/sync.log")).expect("sync log");
        assert!(log.contains("2026-06-26T00:00:00.000Z | neo → trinity | 1 files | 1 learnings"));
        assert!(!log.contains("secret"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn archive_soul_sync_refuses_real_mutation_without_yes() {
        let root = archive_temp_root("soul-sync-no-yes");
        let github_root = root.join("github.com");
        let neo = github_root.join("org/neo-oracle");
        let trinity = github_root.join("org/trinity-oracle");
        archive_write(&neo.join("ψ/memory/learnings/new.md"), "new learning");
        std::fs::create_dir_all(&trinity).expect("peer repo");
        let entry = ArchiveFleetEntry {
            file: "01-neo.json".to_owned(),
            path: root.join("fleet/01-neo.json"),
            session: archive_session("01-neo", "org/neo-oracle", &["trinity"]),
        };
        let options = ArchiveOptions { oracle: "neo".to_owned(), dry_run: false, yes: false };
        let fleet = vec![archive_session("02-trinity", "org/trinity-oracle", &[])];
        let mut host = ArchiveSoulsyncFakeHost { now: "2026-06-26T00:00:00.000Z".to_owned() };
        let mut out = String::new();

        let error = soul_sync_archive_render_soul_sync_with(
            &mut out,
            &entry,
            "org/neo-oracle",
            &options,
            &mut host,
            &fleet,
            &github_root,
        )
        .expect_err("requires --yes");

        assert!(error.contains("without --yes"));
        assert!(out.is_empty());
        assert!(!trinity.join("ψ/memory/learnings/new.md").exists());
        let _ = std::fs::remove_dir_all(root);
    }

}
