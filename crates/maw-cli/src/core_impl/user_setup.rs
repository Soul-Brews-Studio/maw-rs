const DISPATCH_68: &[DispatcherEntry] = &[DispatcherEntry { command: "user-setup", handler: Handler::Sync(run_usersetup_command) }];

const USERSETUP_USAGE: &str = "usage: maw user-setup [--dry-run] [--json|--porcelain] or maw user-setup projects audit --json";
const USERSETUP_AUDIT_USAGE: &str = "usage: maw user-setup projects audit --json";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct UsersetupOptions { dry_run: bool, json: bool, porcelain: bool, scope: Option<String>, action: Option<String> }

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct UsersetupPathFinding { encoded: String, #[serde(skip_serializing_if = "Option::is_none")] inferred: Option<String>, confidence: String, evidence: Vec<String> }

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct UsersetupRepoFinding { #[serde(skip_serializing_if = "Option::is_none")] owner: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] name: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] remote: Option<String> }

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct UsersetupWorktreeFinding { detected: bool, #[serde(skip_serializing_if = "Option::is_none")] alive: Option<bool>, evidence: Vec<String> }

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct UsersetupFileFinding { name: String, #[serde(rename = "sizeBytes")] size_bytes: u64, #[serde(rename = "lastModified")] last_modified: Option<String> }

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct UsersetupAuditEntry {
    encoded: String,
    path: UsersetupPathFinding,
    #[serde(rename = "sizeBytes")]
    size_bytes: u64,
    #[serde(rename = "sessionCount")]
    session_count: usize,
    #[serde(rename = "lastModified")]
    last_modified: Option<String>,
    #[serde(rename = "sourceExists", skip_serializing_if = "Option::is_none")]
    source_exists: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<UsersetupRepoFinding>,
    worktree: UsersetupWorktreeFinding,
    #[serde(rename = "largestFiles")]
    largest_files: Vec<UsersetupFileFinding>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct UsersetupAuditResult { root: String, #[serde(rename = "generatedAt")] generated_at: String, #[serde(rename = "projectCount")] project_count: usize, #[serde(rename = "totalSizeBytes")] total_size_bytes: u64, entries: Vec<UsersetupAuditEntry>, warnings: Vec<String> }

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct UsersetupDuplicateFinding { inferred: String, encoded: Vec<String> }

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct UsersetupPrunePlan { root: String, #[serde(rename = "generatedAt")] generated_at: String, #[serde(rename = "dryRun")] dry_run: bool, #[serde(rename = "staleProjectDirs")] stale_project_dirs: Vec<UsersetupAuditEntry>, #[serde(rename = "orphanSessionFiles")] orphan_session_files: Vec<UsersetupFileFinding>, #[serde(rename = "duplicateEncodedPaths")] duplicate_encoded_paths: Vec<UsersetupDuplicateFinding>, warnings: Vec<String> }

#[derive(Debug, Clone, Default)]
struct UsersetupScanSummary { size_bytes: u64, last_modified_ms: Option<u128>, session_count: usize, largest_files: Vec<UsersetupFileFinding>, warnings: Vec<String> }

fn run_usersetup_command(argv: &[String]) -> CliOutput {
    match usersetup_run(argv) {
        Ok((code, stdout)) => CliOutput { code, stdout, stderr: String::new() },
        Err((code, message)) => CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn usersetup_run(argv: &[String]) -> Result<(i32, String), (i32, String)> {
    let opts = usersetup_parse_args(argv)?;
    if opts.scope.as_deref() == Some("projects") || opts.action.as_deref() == Some("audit") { return usersetup_run_audit(&opts); }
    let plan = usersetup_plan_prune(opts.dry_run);
    if opts.json { return usersetup_json(&plan).map(|out| (0, out)).map_err(|e| (1, e)); }
    if opts.porcelain { return Ok((0, usersetup_render_porcelain(&plan))); }
    Ok((0, usersetup_render_plan(&plan)))
}

fn usersetup_run_audit(opts: &UsersetupOptions) -> Result<(i32, String), (i32, String)> {
    if opts.scope.as_deref() != Some("projects") || opts.action.as_deref() != Some("audit") { return Err((2, USERSETUP_USAGE.to_owned())); }
    if !opts.json { return Err((2, format!("{USERSETUP_AUDIT_USAGE}\n\nOnly --json is implemented in the first read-only #1934 slice."))); }
    let audit = usersetup_audit_projects();
    usersetup_json(&audit).map(|out| (0, out)).map_err(|e| (1, e))
}

fn usersetup_parse_args(argv: &[String]) -> Result<UsersetupOptions, (i32, String)> {
    let mut opts = UsersetupOptions { dry_run: true, ..UsersetupOptions::default() };
    let mut positionals = Vec::new();
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" => return Err((2, USERSETUP_USAGE.to_owned())),
            "--dry-run" => opts.dry_run = true,
            "--json" => opts.json = true,
            "--porcelain" => opts.porcelain = true,
            value if value.starts_with('-') => return Err((2, format!("user-setup: unknown argument {value}"))),
            value => { usersetup_validate_positional(value)?; positionals.push(value.to_owned()); },
        }
    }
    match positionals.as_slice() {
        [] => Ok(opts),
        [scope, action] if scope == "projects" && action == "audit" => { opts.scope = Some(scope.clone()); opts.action = Some(action.clone()); Ok(opts) },
        _ => Err((2, USERSETUP_USAGE.to_owned())),
    }
}

fn usersetup_validate_positional(value: &str) -> Result<(), (i32, String)> {
    if value.is_empty() || value.starts_with('-') || value.contains('\0') || value.contains('/') || value.contains('\\') { return Err((2, format!("user-setup: invalid argument {value}"))); }
    Ok(())
}

fn usersetup_audit_projects() -> UsersetupAuditResult {
    let root = usersetup_projects_root();
    let generated_at = usersetup_now_iso();
    let mut warnings = Vec::new();
    let mut entries = Vec::new();
    let dirents = match std::fs::read_dir(&root) {
        Ok(iter) => iter.flatten().collect::<Vec<_>>(),
        Err(error) => return UsersetupAuditResult { root: root.display().to_string(), generated_at, project_count: 0, total_size_bytes: 0, entries, warnings: vec![format!("cannot read Claude projects dir {}: {error}", root.display())] },
    };
    let mut dirs = dirents.into_iter().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir() || kind.is_symlink())).collect::<Vec<_>>();
    dirs.sort_by_key(std::fs::DirEntry::file_name);
    for dir in dirs { entries.push(usersetup_audit_entry(&dir)); }
    let total_size_bytes = entries.iter().map(|entry| entry.size_bytes).sum();
    UsersetupAuditResult { root: root.display().to_string(), generated_at, project_count: entries.len(), total_size_bytes, entries, warnings: std::mem::take(&mut warnings) }
}

fn usersetup_audit_entry(dir: &std::fs::DirEntry) -> UsersetupAuditEntry {
    let encoded = dir.file_name().to_string_lossy().to_string();
    let dir_path = dir.path();
    let path = usersetup_infer_path(&encoded);
    let scan = usersetup_scan_project_dir(&dir_path);
    let mut warnings = scan.warnings.clone();
    if matches!(path.confidence.as_str(), "ambiguous" | "unknown") { warnings.push(format!("path confidence is {}", path.confidence)); }
    if scan.session_count == 0 { warnings.push("no JSONL sessions found".to_owned()); }
    let source_exists = path.inferred.as_ref().map(|inferred| std::path::Path::new(inferred).exists());
    let repo = usersetup_detect_repo(&path);
    let worktree = usersetup_detect_worktree(&encoded, &path);
    UsersetupAuditEntry { encoded, path, size_bytes: scan.size_bytes, session_count: scan.session_count, last_modified: scan.last_modified_ms.map(usersetup_iso_from_ms), source_exists, repo, worktree, largest_files: scan.largest_files, warnings }
}

fn usersetup_projects_root() -> std::path::PathBuf {
    std::env::var_os("MAW_CLAUDE_PROJECTS_DIR").map_or_else(|| usersetup_home_dir().join(".claude/projects"), std::path::PathBuf::from)
}

fn usersetup_home_dir() -> std::path::PathBuf {
    std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from)
}

fn usersetup_encode_project_path(path: &std::path::Path) -> String {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.display().to_string();
    format!("-{}", text.trim_start_matches('/').replace('/', "-"))
}

fn usersetup_infer_path(encoded: &str) -> UsersetupPathFinding {
    let mut evidence = Vec::new();
    if !encoded.starts_with('-') { return UsersetupPathFinding { encoded: encoded.to_owned(), inferred: None, confidence: "unknown".to_owned(), evidence: vec!["encoded-name-does-not-look-like-absolute-claude-project-dir".to_owned()] }; }
    let inferred = format!("/{}", encoded.trim_start_matches('-').replace('-', "/"));
    evidence.push("dash-decoded-from-claude-project-dir".to_owned());
    if std::path::Path::new(&inferred).exists() { evidence.push("inferred-path-exists".to_owned()); }
    if let Ok(cwd) = std::env::current_dir() { if usersetup_encode_project_path(&cwd) == encoded { evidence.push("current-working-directory-reencodes-to-project-dir".to_owned()); return UsersetupPathFinding { encoded: encoded.to_owned(), inferred: Some(cwd.display().to_string()), confidence: "exact".to_owned(), evidence }; } }
    let confidence = if std::path::Path::new(&inferred).exists() { "probable" } else { "ambiguous" };
    UsersetupPathFinding { encoded: encoded.to_owned(), inferred: Some(inferred), confidence: confidence.to_owned(), evidence }
}

fn usersetup_scan_project_dir(dir: &std::path::Path) -> UsersetupScanSummary {
    let mut summary = UsersetupScanSummary::default();
    usersetup_visit_project_dir(dir, dir, &mut summary);
    summary.largest_files.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes).then(a.name.cmp(&b.name)));
    summary.largest_files.truncate(5);
    summary
}

fn usersetup_visit_project_dir(root: &std::path::Path, path: &std::path::Path, summary: &mut UsersetupScanSummary) {
    let meta = match std::fs::metadata(path) { Ok(meta) => meta, Err(error) => { summary.warnings.push(format!("stat failed for {}: {error}", usersetup_rel(root, path))); return; } };
    let mtime = meta.modified().ok().and_then(usersetup_ms_since_epoch);
    if let Some(ms) = mtime { summary.last_modified_ms = Some(summary.last_modified_ms.map_or(ms, |old| old.max(ms))); }
    if meta.is_file() { usersetup_add_file(root, path, &meta, mtime, summary); return; }
    if !meta.is_dir() { return; }
    let entries = match std::fs::read_dir(path) { Ok(entries) => entries.flatten().collect::<Vec<_>>(), Err(error) => { summary.warnings.push(format!("read failed for {}: {error}", usersetup_rel(root, path))); return; } };
    for entry in entries { usersetup_visit_project_dir(root, &entry.path(), summary); }
}

fn usersetup_add_file(root: &std::path::Path, path: &std::path::Path, meta: &std::fs::Metadata, mtime: Option<u128>, summary: &mut UsersetupScanSummary) {
    let name = usersetup_rel(root, path);
    let size_bytes = meta.len();
    summary.size_bytes = summary.size_bytes.saturating_add(size_bytes);
    if std::path::Path::new(&name).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl")) { summary.session_count += 1; }
    summary.largest_files.push(UsersetupFileFinding { name, size_bytes, last_modified: mtime.map(usersetup_iso_from_ms) });
}

fn usersetup_rel(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

fn usersetup_ms_since_epoch(time: std::time::SystemTime) -> Option<u128> {
    time.duration_since(std::time::UNIX_EPOCH).ok().map(|duration| duration.as_millis())
}

fn usersetup_detect_repo(path: &UsersetupPathFinding) -> Option<UsersetupRepoFinding> {
    let inferred = path.inferred.as_ref()?;
    if path.confidence != "exact" && path.confidence != "probable" { return None; }
    let repo_path = std::path::Path::new(inferred);
    if !usersetup_safe_path(repo_path) || !repo_path.exists() { return None; }
    let output = std::process::Command::new("git").arg("-C").arg(repo_path).args(["remote", "get-url", "origin"]).output().ok()?;
    if !output.status.success() { return None; }
    let remote = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!remote.is_empty()).then(|| usersetup_parse_repo(&remote))
}

fn usersetup_parse_repo(remote: &str) -> UsersetupRepoFinding {
    let normalized = remote.trim().trim_start_matches("ssh://").trim_start_matches("git@").trim_start_matches("https://").trim_start_matches("http://").trim_end_matches(".git").replacen(':', "/", 1);
    let parts = normalized.split('/').filter(|part| !part.is_empty()).collect::<Vec<_>>();
    let github_idx = parts.iter().position(|part| *part == "github.com");
    let owner = github_idx.and_then(|idx| parts.get(idx + 1)).or_else(|| parts.get(parts.len().saturating_sub(2))).map(|v| (*v).to_owned());
    let name = github_idx.and_then(|idx| parts.get(idx + 2)).or_else(|| parts.last()).map(|v| (*v).to_owned());
    UsersetupRepoFinding { owner, name, remote: Some(normalized) }
}

fn usersetup_detect_worktree(encoded: &str, path: &UsersetupPathFinding) -> UsersetupWorktreeFinding {
    let mut evidence = Vec::new();
    let inferred = path.inferred.as_deref().unwrap_or_default();
    if encoded.contains("-wt-") || encoded.contains("-agents-") || inferred.contains("/agents/") || inferred.contains("worktree") { evidence.push("path-shape-suggests-worktree".to_owned()); }
    let alive = usersetup_worktree_alive(path, &mut evidence);
    UsersetupWorktreeFinding { detected: !evidence.is_empty(), alive, evidence }
}

fn usersetup_worktree_alive(path: &UsersetupPathFinding, evidence: &mut Vec<String>) -> Option<bool> {
    let inferred = path.inferred.as_ref()?;
    if path.confidence != "exact" && path.confidence != "probable" { return None; }
    let repo_path = std::path::Path::new(inferred);
    if !usersetup_safe_path(repo_path) || !repo_path.exists() { return None; }
    let output = std::process::Command::new("git").arg("-C").arg(repo_path).args(["worktree", "list", "--porcelain"]).output().ok()?;
    if !output.status.success() { return None; }
    let real = repo_path.canonicalize().unwrap_or_else(|_| repo_path.to_path_buf());
    let listed = String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| line.strip_prefix("worktree ")).any(|value| std::path::Path::new(value).canonicalize().unwrap_or_else(|_| std::path::PathBuf::from(value)) == real);
    evidence.push(if listed { "git-worktree-list-confirms-path" } else { "git-worktree-list-did-not-include-path" }.to_owned());
    Some(listed)
}

fn usersetup_safe_path(path: &std::path::Path) -> bool {
    path.is_absolute() && !path.components().any(|component| component.as_os_str().to_string_lossy().starts_with('-'))
}

fn usersetup_plan_prune(dry_run: bool) -> UsersetupPrunePlan {
    let audit = usersetup_audit_projects();
    let stale_project_dirs = audit.entries.iter().filter(|entry| usersetup_is_safe_prune_candidate(entry)).cloned().collect::<Vec<_>>();
    let orphan_session_files = usersetup_root_session_files(std::path::Path::new(&audit.root));
    let duplicate_encoded_paths = usersetup_duplicate_paths(&audit.entries);
    UsersetupPrunePlan { root: audit.root, generated_at: audit.generated_at, dry_run, stale_project_dirs, orphan_session_files, duplicate_encoded_paths, warnings: audit.warnings }
}

fn usersetup_is_safe_prune_candidate(entry: &UsersetupAuditEntry) -> bool {
    usersetup_is_worktree_derived(&entry.encoded) && entry.session_count == 0 && entry.source_exists == Some(false)
}

fn usersetup_is_worktree_derived(encoded: &str) -> bool { encoded.contains("-wt-") || encoded.contains("-agents-") }

fn usersetup_root_session_files(root: &std::path::Path) -> Vec<UsersetupFileFinding> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return files; };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) || !entry.file_name().to_string_lossy().ends_with(".jsonl") { continue; }
        let meta = entry.metadata().ok();
        files.push(UsersetupFileFinding { name: entry.file_name().to_string_lossy().to_string(), size_bytes: meta.as_ref().map_or(0, std::fs::Metadata::len), last_modified: meta.and_then(|m| m.modified().ok()).and_then(usersetup_ms_since_epoch).map(usersetup_iso_from_ms) });
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files
}

fn usersetup_duplicate_paths(entries: &[UsersetupAuditEntry]) -> Vec<UsersetupDuplicateFinding> {
    let mut groups = std::collections::BTreeMap::<String, Vec<String>>::new();
    for entry in entries { if let Some(inferred) = &entry.path.inferred { groups.entry(inferred.clone()).or_default().push(entry.encoded.clone()); } }
    groups.into_iter().filter_map(|(inferred, mut encoded)| { encoded.sort(); (encoded.len() > 1).then_some(UsersetupDuplicateFinding { inferred, encoded }) }).collect()
}

fn usersetup_render_plan(plan: &UsersetupPrunePlan) -> String {
    let mut lines = vec![format!("maw user-setup {}audit", if plan.dry_run { "dry-run " } else { "" }), format!("root: {}", plan.root), format!("safe prune candidates: {}", plan.stale_project_dirs.len()), format!("orphan session files: {}", plan.orphan_session_files.len()), format!("duplicate encoded paths: {}", plan.duplicate_encoded_paths.len())];
    usersetup_push_plan_sections(&mut lines, plan);
    lines.join("\n") + "\n"
}

fn usersetup_push_plan_sections(lines: &mut Vec<String>, plan: &UsersetupPrunePlan) {
    if !plan.stale_project_dirs.is_empty() { lines.push(String::new()); lines.push("safe prune candidates:".to_owned()); for entry in &plan.stale_project_dirs { lines.push(format!("  - {}{}", entry.encoded, entry.path.inferred.as_ref().map_or(String::new(), |p| format!(" -> {p}")))); lines.push("    why: worktree-derived name (-wt-/-agents-), 0 JSONL sessions, decoded path missing".to_owned()); } }
    if !plan.orphan_session_files.is_empty() { lines.push(String::new()); lines.push("orphan session files:".to_owned()); for file in &plan.orphan_session_files { lines.push(format!("  - {} ({} bytes)", file.name, file.size_bytes)); } }
    if !plan.duplicate_encoded_paths.is_empty() { lines.push(String::new()); lines.push("duplicate encoded paths:".to_owned()); for group in &plan.duplicate_encoded_paths { lines.push(format!("  - {}: {}", group.inferred, group.encoded.join(", "))); } }
    if !plan.warnings.is_empty() { lines.push(String::new()); lines.push("warnings:".to_owned()); for warning in &plan.warnings { lines.push(format!("  - {warning}")); } }
    let prune_count = plan.stale_project_dirs.len() + plan.orphan_session_files.len();
    lines.push(String::new());
    lines.push(if prune_count > 0 { "prune offer: archive candidates are listed above; rerun after review when prune execution is enabled." } else { "prune offer: nothing to prune." }.to_owned());
}

fn usersetup_render_porcelain(plan: &UsersetupPrunePlan) -> String {
    let mut lines = vec![format!("root\t{}", plan.root), format!("stale\t{}", plan.stale_project_dirs.len()), format!("orphan\t{}", plan.orphan_session_files.len()), format!("duplicate\t{}", plan.duplicate_encoded_paths.len())];
    for entry in &plan.stale_project_dirs { lines.push(format!("candidate\t{}\t{}", entry.encoded, entry.path.inferred.as_deref().unwrap_or(""))); }
    lines.join("\n") + "\n"
}

fn usersetup_json<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value).map(|mut out| { out.push('\n'); out }).map_err(|error| error.to_string())
}

fn usersetup_now_iso() -> String { std::env::var("MAW_USERSETUP_NOW").unwrap_or_else(|_| "1970-01-01T00:00:00.000Z".to_owned()) }

fn usersetup_iso_from_ms(ms: u128) -> String { format!("epoch-ms:{ms}") }

#[cfg(test)]
mod usersetup_tests {
    use super::*;

    #[test]
    fn usersetup_parser_rejects_leading_dash_positionals() {
        let argv = vec!["projects".to_owned(), "-audit".to_owned()];
        assert_eq!(usersetup_parse_args(&argv).unwrap_err(), (2, "user-setup: unknown argument -audit".to_owned()));
    }

    #[test]
    fn usersetup_infer_path_marks_ambiguous_lossy_decode() {
        let finding = usersetup_infer_path("-tmp-missing-agents-01");
        assert_eq!(finding.confidence, "ambiguous");
        assert_eq!(finding.inferred.as_deref(), Some("/tmp/missing/agents/01"));
    }

    #[test]
    fn usersetup_repo_parser_handles_github_forms() {
        let repo = usersetup_parse_repo("git@github.com:tonkmac/maw-rs.git");
        assert_eq!(repo.owner.as_deref(), Some("tonkmac"));
        assert_eq!(repo.name.as_deref(), Some("maw-rs"));
    }
}
