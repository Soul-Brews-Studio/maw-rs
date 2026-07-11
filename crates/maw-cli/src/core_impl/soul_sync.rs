const DISPATCH_94: &[DispatcherEntry] = &[];

const SOULSYNC_DIRS: &[&str] = &[
    "memory/learnings",
    "memory/retrospectives",
    "memory/traces",
    "memory/collaborations",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SoulsyncSyncResult {
    from: String,
    to: String,
    synced: Vec<(String, usize)>,
    total: usize,
}

trait SoulsyncHost {
    fn soulsync_now(&mut self) -> String;
}

#[derive(Default)]
struct SoulsyncSystemHost;

impl SoulsyncHost for SoulsyncSystemHost {
    fn soulsync_now(&mut self) -> String {
        now_iso_utc()
    }
}

fn soulsync_resolve_oracle_path(
    name: &str,
    fleet: &[NativeFleetSession],
    repos_root: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let stem = name.trim_end_matches("-oracle");
    if let Some(path) = soulsync_declared_oracle_path(stem, fleet, repos_root) {
        return Some(path);
    }
    if let Some(path) = soulsync_find_oracle_repo(repos_root, stem, fleet) {
        return Some(path);
    }
    fleet
        .iter()
        .find(|session| soulsync_session_name(&session.name) == stem)
        .and_then(|session| {
            session
                .windows
                .iter()
                .find(|window| window.kind != Some(NativeRepoKind::Project))
        })
        .map(|window| {
            repos_root.join(
                window
                    .repo
                    .trim()
                    .strip_prefix("github.com/")
                    .unwrap_or(window.repo.trim()),
            )
        })
        .filter(|path| path.exists())
}

fn soulsync_repo_is_oracle(
    repo: &std::path::Path,
    fallback_name: &str,
    fleet: &[NativeFleetSession],
) -> bool {
    match soulsync_repo_kind_for_path(repo, fleet) {
        Some(NativeRepoKind::Oracle) => true,
        Some(NativeRepoKind::Project) => false,
        None => fallback_name.ends_with("-oracle"),
    }
}

fn soulsync_repo_kind_for_path(
    repo: &std::path::Path,
    fleet: &[NativeFleetSession],
) -> Option<NativeRepoKind> {
    let slugs = native_repo_slugs_for_path(repo);
    for session in fleet {
        for window in &session.windows {
            if window.kind.is_some() && native_fleet_window_matches_slugs(window, &slugs) {
                return window.kind;
            }
        }
    }
    native_repo_marker_kind(repo)
}

fn soulsync_declared_oracle_path(
    name: &str,
    fleet: &[NativeFleetSession],
    repos_root: &std::path::Path,
) -> Option<std::path::PathBuf> {
    for session in fleet {
        let session_name = soulsync_session_name(&session.name);
        for window in &session.windows {
            if window.kind != Some(NativeRepoKind::Oracle) {
                continue;
            }
            let Some(oracle_name) = native_fleet_window_oracle_name(window) else {
                continue;
            };
            if oracle_name == name || session_name == name {
                let path = repos_root.join(
                    window
                        .repo
                        .trim()
                        .strip_prefix("github.com/")
                        .unwrap_or(window.repo.trim()),
                );
                if path.exists() {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn soulsync_find_oracle_repo(
    repos_root: &std::path::Path,
    stem: &str,
    fleet: &[NativeFleetSession],
) -> Option<std::path::PathBuf> {
    let wanted = format!("{stem}-oracle").to_lowercase();
    let Ok(orgs) = std::fs::read_dir(repos_root) else {
        return None;
    };
    for org in orgs.flatten().filter(|entry| entry.path().is_dir()) {
        let Ok(repos) = std::fs::read_dir(org.path()) else {
            continue;
        };
        for repo in repos.flatten().filter(|entry| entry.path().is_dir()) {
            if repo
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(&wanted)
                && soulsync_repo_is_oracle(
                    &repo.path(),
                    &repo.file_name().to_string_lossy(),
                    fleet,
                )
            {
                return Some(repo.path());
            }
        }
    }
    None
}

fn soulsync_sync_oracle_vaults(
    from_path: &std::path::Path,
    to_path: &std::path::Path,
    from_name: &str,
    to_name: &str,
    host: &mut impl SoulsyncHost,
) -> SoulsyncSyncResult {
    let synced = soulsync_sync_dirs(from_path, to_path);
    let total = synced.iter().map(|(_, count)| *count).sum();
    let result = SoulsyncSyncResult {
        from: from_name.to_owned(),
        to: to_name.to_owned(),
        synced,
        total,
    };
    if total > 0 {
        soulsync_append_log(
            to_path,
            &format!("{from_name} → {to_name}"),
            total,
            &result.synced,
            host,
        );
    }
    result
}

fn soulsync_sync_dirs(
    from_path: &std::path::Path,
    to_path: &std::path::Path,
) -> Vec<(String, usize)> {
    let mut synced = Vec::new();
    for dir in SOULSYNC_DIRS {
        let count = soulsync_sync_dir(
            &from_path.join("ψ").join(dir),
            &to_path.join("ψ").join(dir),
        );
        if count > 0 {
            synced.push(((*dir).to_owned(), count));
        }
    }
    synced
}

fn soulsync_sync_dir(src: &std::path::Path, dst: &std::path::Path) -> usize {
    let Ok(entries) = std::fs::read_dir(src) else {
        return 0;
    };
    let mut count = 0;
    for entry in entries.flatten() {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            count += soulsync_sync_dir(&src_path, &dst_path);
        } else if !dst_path.exists() {
            count += soulsync_copy_new_file(&src_path, &dst_path);
        }
    }
    count
}

fn soulsync_copy_new_file(src: &std::path::Path, dst: &std::path::Path) -> usize {
    if let Some(parent) = dst.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::copy(src, dst).map_or(0, |_| 1)
}

fn soulsync_append_log(
    to_path: &std::path::Path,
    label: &str,
    total: usize,
    synced: &[(String, usize)],
    host: &mut impl SoulsyncHost,
) {
    let log_dir = to_path.join("ψ/.soul-sync");
    if std::fs::create_dir_all(&log_dir).is_err() {
        return;
    }
    let summary = soulsync_summary(synced);
    let line = format!(
        "{} | {label} | {total} files | {summary}\n",
        host.soulsync_now()
    );
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("sync.log"))
        .and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
}

fn soulsync_render_oracle_result(output: &mut String, result: &SoulsyncSyncResult) {
    if result.total == 0 {
        let _ = writeln!(
            output,
            "  \x1b[90m○\x1b[0m {} → {}: nothing new",
            result.from, result.to
        );
    } else {
        let _ = writeln!(
            output,
            "  \x1b[32m✓\x1b[0m {} → {}: {}",
            result.from,
            result.to,
            soulsync_summary(&result.synced)
        );
    }
}

fn soulsync_render_total(output: &mut String, total: usize, verb: &str) {
    if total > 0 {
        let _ = writeln!(
            output,
            "\n  \x1b[32m{total} file(s) {verb}.\x1b[0m\n"
        );
    } else {
        output.push('\n');
    }
}

fn soulsync_summary(synced: &[(String, usize)]) -> String {
    synced
        .iter()
        .map(|(dir, count)| format!("{count} {}", dir.rsplit('/').next().unwrap_or(dir)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn soulsync_session_name(name: &str) -> String {
    name.split_once('-')
        .filter(|(prefix, suffix)| {
            prefix.chars().all(|character| character.is_ascii_digit()) && !suffix.is_empty()
        })
        .map_or(name, |(_, suffix)| suffix)
        .trim_end_matches("-oracle")
        .to_owned()
}

fn soulsync_validate_name(value: &str, label: &str) -> Result<String, String> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.contains('/')
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(format!("soul-sync: invalid {label} {value:?}"));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod soulsync_tests {
    use super::*;

    #[test]
    fn soulsync_public_dispatch_is_extracted() {
        assert!(DISPATCH_94.is_empty());
    }

    #[test]
    fn soulsync_archive_helper_rejects_injection_names() {
        assert!(soulsync_validate_name("-bad", "peer").is_err());
        assert!(soulsync_validate_name("bad/name", "peer").is_err());
    }
}
