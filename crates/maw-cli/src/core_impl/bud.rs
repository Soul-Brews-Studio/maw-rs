const DISPATCH_121: &[DispatcherEntry] = &[
    DispatcherEntry { command: "bud", handler: Handler::Sync(bud_run_command) },
    DispatcherEntry { command: "buddy", handler: Handler::Sync(bud_run_command) },
];

const BUD_USAGE: &str = "usage: maw bud <name> [--from <oracle>] [--root] [--seed] [--org <org>] [--repo org/repo] [--issue N] [--issue-repo owner/repo] [--note <text>] [--nickname <pretty>] [--fast] [--split] [--scaffold-only] [--dry-run]\n       Or: maw bud --from-repo <path|url> --stem <stem> [--pr] [--from <parent>] [--seed] [--sync-peers] [--force] [--track-vault] [--dry-run]";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct BudOptions {
    name: Option<String>,
    from: Option<String>,
    from_repo: Option<String>,
    stem: Option<String>,
    org: Option<String>,
    repo: Option<String>,
    issue: Option<u32>,
    issue_repo: Option<String>,
    note: Option<String>,
    nickname: Option<String>,
    fast: bool,
    root: bool,
    dry_run: bool,
    pr: bool,
    split: bool,
    scaffold_only: bool,
    seed: bool,
    blank: bool,
    signal_on_birth: bool,
    force: bool,
    track_vault: bool,
    sync_peers: bool,
    parent_session_id: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BudContext {
    stem: String,
    org: String,
    parent: Option<String>,
    repo_name: String,
    slug: String,
    repo_path: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BudCmdOutput { ok: bool, stdout: String, stderr: String }

trait BudGhGitRunner {
    fn bud_run(&mut self, program: &str, args: &[String]) -> BudCmdOutput;
}

trait BudWakeRunner {
    fn bud_wake(&mut self, args: &[String]) -> Result<String, String>;
    fn bud_split(&mut self, stem: &str) -> Result<String, String>;
}

trait BudHttpClient { fn bud_reload(&mut self, port: u16) -> Result<(), String>; }

trait BudFs {
    fn bud_exists(&self, path: &std::path::Path) -> bool;
    fn bud_create_dir_all(&mut self, path: &std::path::Path) -> Result<(), String>;
    fn bud_read(&self, path: &std::path::Path) -> Result<Vec<u8>, String>;
    fn bud_write_atomic(&mut self, path: &std::path::Path, bytes: &[u8]) -> Result<(), String>;
    fn bud_append_atomic(&mut self, path: &std::path::Path, text: &str) -> Result<(), String>;
    fn bud_copy_atomic(&mut self, from: &std::path::Path, to: &std::path::Path) -> Result<(), String>;
    fn bud_archive_existing(&mut self, path: &std::path::Path) -> Result<Option<std::path::PathBuf>, String>;
}

struct BudSystemGhGit;
struct BudSystemWake;
struct BudSystemHttp;
struct BudSystemFs;

fn bud_self_bin() -> Result<std::path::PathBuf, String> {
    std::env::var_os("MAW_RS_SELF_BIN")
        .map(std::path::PathBuf::from)
        .map_or_else(|| std::env::current_exe().map_err(|error| error.to_string()), Ok)
}

impl BudGhGitRunner for BudSystemGhGit {
    fn bud_run(&mut self, program: &str, args: &[String]) -> BudCmdOutput {
        let output = std::process::Command::new(program).args(args).output();
        match output {
            Ok(out) => BudCmdOutput {
                ok: out.status.success(),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            },
            Err(error) => BudCmdOutput { ok: false, stdout: String::new(), stderr: error.to_string() },
        }
    }
}

impl BudWakeRunner for BudSystemWake {
    fn bud_wake(&mut self, args: &[String]) -> Result<String, String> {
        let exe = bud_self_bin()?;
        let output = std::process::Command::new(exe)
            .args(args)
            .env("MAW_FROM_RS", "1")
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).into_owned()) } else { Err(String::from_utf8_lossy(&output.stderr).into_owned()) }
    }

    fn bud_split(&mut self, stem: &str) -> Result<String, String> {
        let exe = bud_self_bin()?;
        let output = std::process::Command::new(exe)
            .args(["split", stem])
            .env("MAW_FROM_RS", "1")
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).into_owned()) } else { Err(String::from_utf8_lossy(&output.stderr).into_owned()) }
    }
}

impl BudHttpClient for BudSystemHttp {
    fn bud_reload(&mut self, port: u16) -> Result<(), String> {
        use std::io::Write as _;

        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let mut stream = std::net::TcpStream::connect_timeout(
            &addr,
            std::time::Duration::from_millis(400),
        )
        .map_err(|error| error.to_string())?;
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_millis(400)));
        let request = format!(
            "POST /api/config/reload HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        stream.write_all(request.as_bytes()).map_err(|error| error.to_string())
    }
}

impl BudFs for BudSystemFs {
    fn bud_exists(&self, path: &std::path::Path) -> bool { path.exists() }

    fn bud_create_dir_all(&mut self, path: &std::path::Path) -> Result<(), String> {
        std::fs::create_dir_all(path).map_err(|error| error.to_string())
    }

    fn bud_read(&self, path: &std::path::Path) -> Result<Vec<u8>, String> {
        std::fs::read(path).map_err(|error| error.to_string())
    }

    fn bud_write_atomic(&mut self, path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
        bud_write_atomic_path(path, bytes)
    }

    fn bud_append_atomic(&mut self, path: &std::path::Path, text: &str) -> Result<(), String> {
        let mut old = if path.exists() { std::fs::read(path).map_err(|error| error.to_string())? } else { Vec::new() };
        old.extend_from_slice(text.as_bytes());
        bud_write_atomic_path(path, &old)
    }

    fn bud_copy_atomic(&mut self, from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
        let bytes = std::fs::read(from).map_err(|error| error.to_string())?;
        bud_write_atomic_path(to, &bytes)
    }

    fn bud_archive_existing(&mut self, path: &std::path::Path) -> Result<Option<std::path::PathBuf>, String> {
        if !path.exists() { return Ok(None); }
        let archive = path.with_extension(format!("json.archive-{}", bud_epoch_seconds()));
        std::fs::copy(path, &archive).map_err(|error| error.to_string())?;
        Ok(Some(archive))
    }
}

fn bud_run_command(argv: &[String]) -> CliOutput {
    let mut gh = BudSystemGhGit;
    let mut fs = BudSystemFs;
    let mut wake = BudSystemWake;
    let mut http = BudSystemHttp;
    bud_run_with(argv, &mut gh, &mut fs, &mut wake, &mut http)
}

fn bud_run_with(argv: &[String], gh: &mut impl BudGhGitRunner, fs: &mut impl BudFs, wake: &mut impl BudWakeRunner, http: &mut impl BudHttpClient) -> CliOutput {
    match bud_parse(argv).and_then(|options| bud_run_options(&options, gh, fs, wake, http)) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) if message == BUD_USAGE => CliOutput { code: 0, stdout: format!("{BUD_USAGE}\n"), stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn bud_parse(argv: &[String]) -> Result<BudOptions, String> {
    let mut options = BudOptions::default();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" | "help" => return Err(BUD_USAGE.to_owned()),
            "--" => return Err("bud: -- separator is not allowed".to_owned()),
            "--fast" => options.fast = true,
            "--root" => options.root = true,
            "--dry-run" => options.dry_run = true,
            "--pr" => options.pr = true,
            "--split" => options.split = true,
            "--scaffold-only" => options.scaffold_only = true,
            "--seed" => options.seed = true,
            "--blank" => options.blank = true,
            "--signal-on-birth" => options.signal_on_birth = true,
            "--force" => options.force = true,
            "--track-vault" => options.track_vault = true,
            "--sync-peers" => options.sync_peers = true,
            flag @ ("--from" | "--from-repo" | "--stem" | "--org" | "--repo" | "--issue" | "--issue-repo" | "--note" | "--nickname" | "--parent" | "--parent-session-id" | "--session-id") => { bud_assign_value(&mut options, flag, &bud_take_value(argv, &mut index, flag)?)?; }
            value if value.starts_with('-') => return Err(bud_flag_like(value)),
            value => bud_set_name(&mut options, value)?,
        }
        index += 1;
    }
    bud_validate_options(options)
}

fn bud_assign_value(options: &mut BudOptions, flag: &str, value: &str) -> Result<(), String> {
    match flag {
        "--from" => options.from = Some(bud_validate_stem_like(value, "--from")?),
        "--from-repo" => options.from_repo = Some(bud_validate_pathish(value, "--from-repo")?),
        "--stem" => options.stem = Some(bud_validate_oracle_stem(value)?),
        "--org" => options.org = Some(bud_validate_org(value)?),
        "--repo" | "--issue-repo" => bud_assign_slug(options, flag, value)?,
        "--issue" => options.issue = Some(bud_validate_issue(value)?),
        "--note" => options.note = Some(bud_validate_text(value, "--note")?),
        "--nickname" => options.nickname = Some(bud_validate_text(value, "--nickname")?),
        "--parent" | "--parent-session-id" => options.parent_session_id = Some(bud_validate_id(value, flag)?),
        "--session-id" => options.session_id = Some(bud_validate_id(value, flag)?),
        _ => return Err(format!("bud: unknown flag {flag}")),
    }
    Ok(())
}

fn bud_assign_slug(options: &mut BudOptions, flag: &str, value: &str) -> Result<(), String> {
    let slug = bud_validate_slug(value, flag)?;
    if flag == "--repo" { options.repo = Some(slug); } else { options.issue_repo = Some(slug); }
    Ok(())
}

fn bud_take_value(argv: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    let Some(value) = argv.get(*index) else { return Err(format!("bud: {flag} requires a value")); };
    if value.starts_with('-') { return Err(format!("bud: {flag} value must not start with '-'")); }
    Ok(value.to_owned())
}

fn bud_set_name(options: &mut BudOptions, value: &str) -> Result<(), String> {
    if options.name.is_some() { return Err(format!("bud: unexpected argument {value}")); }
    options.name = Some(bud_validate_oracle_stem(value)?);
    Ok(())
}

fn bud_validate_options(options: BudOptions) -> Result<BudOptions, String> {
    if options.from_repo.is_some() {
        if options.stem.is_none() { return Err("--from-repo requires --stem <stem>".to_owned()); }
        return Ok(options);
    }
    if options.name.is_none() { return Err(BUD_USAGE.to_owned()); }
    Ok(options)
}

fn bud_run_options(options: &BudOptions, gh: &mut impl BudGhGitRunner, fs: &mut impl BudFs, wake: &mut impl BudWakeRunner, http: &mut impl BudHttpClient) -> Result<String, String> {
    if options.from_repo.is_some() { return bud_from_repo(options, gh, fs); }
    let ctx = bud_context(options)?;
    if options.dry_run { return Ok(bud_dry_run(&ctx, options)); }
    bud_create_repo(&ctx, gh, fs)?;
    bud_write_skeleton(&ctx, options, fs)?;
    let _ = bud_reload(http);
    if options.scaffold_only { return Ok(bud_scaffold_summary(&ctx)); }
    Ok(bud_finalize(&ctx, options, gh, fs, wake))
}

fn bud_context(options: &BudOptions) -> Result<BudContext, String> {
    let stem = options.name.clone().ok_or_else(|| BUD_USAGE.to_owned())?;
    let org = options.org.clone().unwrap_or_else(|| std::env::var("MAW_BUD_OWNER").unwrap_or_else(|_| "Soul-Brews-Studio".to_owned()));
    let org = bud_validate_org(&org)?;
    let repo_name = format!("{stem}-oracle");
    bud_validate_repo_name(&repo_name)?;
    let slug = format!("{org}/{repo_name}");
    let repo_path = bud_repos_root().join(&org).join(&repo_name);
    Ok(BudContext { stem, org, parent: options.from.clone().filter(|_| !options.root), repo_name, slug, repo_path })
}

fn bud_from_repo(options: &BudOptions, gh: &mut impl BudGhGitRunner, fs: &mut impl BudFs) -> Result<String, String> {
    let stem = options.stem.clone().ok_or_else(|| "--from-repo requires --stem <stem>".to_owned())?;
    let target = options.from_repo.clone().ok_or_else(|| "--from-repo requires a target".to_owned())?;
    if bud_looks_like_url(&target) { return bud_from_repo_url(options, &stem, &target, gh); }
    let path = std::path::PathBuf::from(&target);
    let plan = bud_from_repo_plan(&stem, &path, options);
    if options.dry_run { return Ok(plan); }
    if !options.force && fs.bud_exists(&path.join("ψ")) { return Err(format!("ψ/ already present at {} — pass --force to merge", path.display())); }
    bud_write_from_repo_scaffold(&stem, &path, options, fs)?;
    if options.sync_peers { let _ = bud_sync_peers(&path, fs); }
    Ok(format!("{plan}\n  \x1b[32m✓ done\x1b[0m — run `maw wake {stem}` to start a session\n"))
}

fn bud_from_repo_url(options: &BudOptions, stem: &str, target: &str, gh: &mut impl BudGhGitRunner) -> Result<String, String> {
    let plan = format!("\n  \x1b[36m🧪 Oracle scaffold plan\x1b[0m — {stem} → {target}\n\n  clone url, inject scaffold, optionally push PR\n");
    if options.dry_run { return Ok(plan); }
    let tmp = std::env::temp_dir().join(format!("maw-bud-from-repo-{}", bud_epoch_seconds()));
    let clone_args = vec!["clone".to_owned(), "--depth".to_owned(), "1".to_owned(), target.to_owned(), path_string(&tmp)];
    if !gh.bud_run("git", &clone_args).ok { return Err("bud: git clone failed".to_owned()); }
    Ok(format!("{plan}  \x1b[32m✓\x1b[0m cloned → {}\n", tmp.display()))
}

fn bud_dry_run(ctx: &BudContext, options: &BudOptions) -> String {
    let mut out = String::new();
    let label = ctx.parent.as_deref().map_or_else(|| ctx.stem.clone(), |parent| format!("{parent} → {}", ctx.stem));
    let _ = writeln!(out, "\n  \x1b[36m{}\x1b[0m — {label}\n", if options.root { "🌱 Root Bud" } else { "🧬 Budding" });
    let _ = writeln!(out, "  \x1b[36m⬡\x1b[0m [dry-run] would create repo: {}", ctx.slug);
    let _ = writeln!(out, "  \x1b[36m⬡\x1b[0m [dry-run] would init ψ/ vault at: {}", ctx.repo_path.display());
    let _ = writeln!(out, "  \x1b[36m⬡\x1b[0m [dry-run] would generate CLAUDE.md");
    let _ = writeln!(out, "  \x1b[36m⬡\x1b[0m [dry-run] would create fleet config");
    if options.scaffold_only { let _ = writeln!(out, "  \x1b[36m⬡\x1b[0m [dry-run] scaffold-only: would stop before git commit/push, wake, attach, parent sync_peers, and /awaken"); } else { let _ = writeln!(out, "  \x1b[36m⬡\x1b[0m [dry-run] would wake {}", ctx.stem); }
    out.push('\n');
    out
}

fn bud_create_repo(ctx: &BudContext, gh: &mut impl BudGhGitRunner, fs: &impl BudFs) -> Result<(), String> {
    if fs.bud_exists(&ctx.repo_path) { return Ok(()); }
    let view = gh.bud_run("gh", &["repo".to_owned(), "view".to_owned(), ctx.slug.clone(), "--json".to_owned(), "name".to_owned()]);
    if !view.ok {
        let create = gh.bud_run("gh", &["repo".to_owned(), "create".to_owned(), ctx.slug.clone(), "--private".to_owned(), "--add-readme".to_owned()]);
        if !create.ok { return Err(format!("bud: gh repo create failed: {}", create.stderr.trim())); }
    }
    let get = gh.bud_run("ghq", &["get".to_owned(), format!("github.com/{}", ctx.slug)]);
    if !get.ok { return Err(format!("bud: ghq get failed: {}", get.stderr.trim())); }
    Ok(())
}

fn bud_write_skeleton(ctx: &BudContext, options: &BudOptions, fs: &mut impl BudFs) -> Result<(), String> {
    if fs.bud_exists(&ctx.repo_path.join("CLAUDE.md")) { return Err(format!("bud: refusing to overwrite existing oracle at {}", ctx.repo_path.display())); }
    bud_create_vault(&ctx.repo_path, fs)?;
    fs.bud_write_atomic(&ctx.repo_path.join("CLAUDE.md"), bud_claude_md(ctx).as_bytes())?;
    fs.bud_write_atomic(&ctx.repo_path.join(".claude/settings.json"), bud_settings().as_bytes())?;
    if let Some(nickname) = &options.nickname { fs.bud_write_atomic(&ctx.repo_path.join("ψ/nickname"), format!("{nickname}\n").as_bytes())?; }
    if let Some(note) = &options.note { fs.bud_write_atomic(&ctx.repo_path.join("ψ/memory/birth-note.md"), format!("# Birth note\n\n{note}\n").as_bytes())?; }
    bud_write_fleet(ctx, fs)
}

fn bud_create_vault(root: &std::path::Path, fs: &mut impl BudFs) -> Result<(), String> {
    for dir in ["ψ/memory/learnings", "ψ/memory/retrospectives", "ψ/memory/traces", "ψ/memory/collaborations", ".claude"] { fs.bud_create_dir_all(&root.join(dir))?; }
    Ok(())
}

fn bud_write_fleet(ctx: &BudContext, fs: &mut impl BudFs) -> Result<(), String> {
    let dir = maw_config_path(&current_xdg_env(), &["fleet"]);
    fs.bud_create_dir_all(&dir)?;
    let file = dir.join(format!("99-{}.json", ctx.stem));
    let mut value = if fs.bud_exists(&file) {
        serde_json::from_slice(&fs.bud_read(&file)?).map_err(|error| error.to_string())?
    } else {
        serde_json::json!({ "name": format!("99-{}", ctx.stem), "windows": [], "sync_peers": [] })
    };
    value["name"] = serde_json::json!(format!("99-{}", ctx.stem));
    bud_fleet_ensure_window(&mut value, ctx)?;
    if let Some(parent) = &ctx.parent {
        bud_fleet_ensure_sync_peer(&mut value, parent)?;
        value["budded_from"] = serde_json::json!(parent);
        if value.get("budded_at").is_none() {
            value["budded_at"] = serde_json::json!(bud_now_iso());
        }
    }
    fs.bud_write_atomic(
        &file,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?
        )
        .as_bytes(),
    )
}

fn bud_fleet_ensure_window(value: &mut serde_json::Value, ctx: &BudContext) -> Result<(), String> {
    if !value.get("windows").is_some_and(serde_json::Value::is_array) {
        value["windows"] = serde_json::json!([]);
    }
    let windows = value["windows"]
        .as_array_mut()
        .ok_or_else(|| "bud: invalid windows".to_owned())?;
    if !windows.iter().any(|window| {
        window.get("name").and_then(serde_json::Value::as_str) == Some(ctx.repo_name.as_str())
    }) {
        windows.push(serde_json::json!({ "name": ctx.repo_name, "repo": ctx.slug }));
    }
    Ok(())
}

fn bud_fleet_ensure_sync_peer(value: &mut serde_json::Value, peer: &str) -> Result<(), String> {
    if !value.get("sync_peers").is_some_and(serde_json::Value::is_array) {
        value["sync_peers"] = serde_json::json!([]);
    }
    let peers = value["sync_peers"]
        .as_array_mut()
        .ok_or_else(|| "bud: invalid sync_peers".to_owned())?;
    if !peers.iter().any(|value| value.as_str() == Some(peer)) {
        peers.push(serde_json::json!(peer));
    }
    Ok(())
}

fn bud_scaffold_summary(ctx: &BudContext) -> String {
    format!("\n  \x1b[32m▧ Scaffold complete!\x1b[0m {}\n  \x1b[90m  repo: {}\n  \x1b[90m  path: {}\n  \x1b[90m  skipped: git commit/push, wake, attach, parent sync_peers, /awaken\n\n", ctx.stem, ctx.slug, ctx.repo_path.display())
}

fn bud_finalize(ctx: &BudContext, options: &BudOptions, gh: &mut impl BudGhGitRunner, fs: &mut impl BudFs, wake: &mut impl BudWakeRunner) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "  \x1b[90m○\x1b[0m {}", ctx.parent.as_deref().map_or("root oracle — no parent".to_owned(), |p| format!("born blank — pull memory when ready: maw soul-sync {p} --from")));
    bud_git_commit(ctx, gh, &mut out);
    let _ = bud_update_parent_peers(ctx, fs, &mut out);
    bud_wake(ctx, options, wake, &mut out);
    if options.signal_on_birth { let _ = bud_birth_signal(ctx, fs, &mut out); }
    let _ = writeln!(out, "\n  \x1b[32m{} complete!\x1b[0m {}", if ctx.parent.is_some() { "🧬 Bud" } else { "🌱 Root bud" }, ctx.stem);
    out
}

fn bud_git_commit(ctx: &BudContext, gh: &mut impl BudGhGitRunner, out: &mut String) {
    let repo = path_string(&ctx.repo_path);
    let msg = ctx.parent.as_deref().map_or("feat: birth — root oracle".to_owned(), |parent| format!("feat: birth — budded from {parent}"));
    let ok = gh.bud_run("git", &["-C".to_owned(), repo.clone(), "add".to_owned(), "-A".to_owned()]).ok
        && gh.bud_run("git", &["-C".to_owned(), repo.clone(), "commit".to_owned(), "-m".to_owned(), msg]).ok
        && gh.bud_run("git", &["-C".to_owned(), repo, "push".to_owned(), "-u".to_owned(), "origin".to_owned(), "HEAD".to_owned()]).ok;
    if ok { let _ = writeln!(out, "  \x1b[32m✓\x1b[0m initial commit pushed"); } else { let _ = writeln!(out, "  \x1b[33m⚠\x1b[0m git push failed (may need manual setup)"); }
}

fn bud_update_parent_peers(ctx: &BudContext, fs: &mut impl BudFs, out: &mut String) -> Result<(), String> {
    let Some(parent) = &ctx.parent else { return Ok(()); };
    let parent_file = maw_config_path(&current_xdg_env(), &["fleet", &format!("99-{parent}.json")]);
    if !fs.bud_exists(&parent_file) { return Ok(()); }
    let raw = fs.bud_read(&parent_file)?;
    let mut cfg: serde_json::Value = serde_json::from_slice(&raw).map_err(|error| error.to_string())?;
    let peers = cfg["sync_peers"].as_array_mut().ok_or_else(|| "bud: invalid sync_peers".to_owned())?;
    if !peers.iter().any(|value| value.as_str() == Some(&ctx.stem)) { peers.push(serde_json::json!(ctx.stem)); fs.bud_write_atomic(&parent_file, format!("{}\n", serde_json::to_string_pretty(&cfg).map_err(|error| error.to_string())?).as_bytes())?; let _ = writeln!(out, "  \x1b[32m✓\x1b[0m added {} to {parent}'s sync_peers", ctx.stem); }
    Ok(())
}

fn bud_wake(ctx: &BudContext, options: &BudOptions, wake: &mut impl BudWakeRunner, out: &mut String) {
    let mut args = vec!["wake".to_owned(), ctx.stem.clone(), "--no-attach".to_owned(), "--repo-path".to_owned(), path_string(&ctx.repo_path)];
    if let Some(id) = &options.parent_session_id { args.extend(["--parent-session-id".to_owned(), id.clone()]); }
    if let Some(id) = &options.session_id { args.extend(["--session-id".to_owned(), id.clone()]); }
    match wake.bud_wake(&args) { Ok(_) => { let _ = writeln!(out, "  \x1b[32m✓\x1b[0m {} is alive", ctx.stem); } Err(error) => { let _ = writeln!(out, "  \x1b[33m⚠\x1b[0m wake failed: {}", error.trim()); } }
    if options.split && std::env::var_os("TMUX").is_some() { let _ = wake.bud_split(&ctx.stem); }
}

fn bud_birth_signal(ctx: &BudContext, fs: &mut impl BudFs, out: &mut String) -> Result<(), String> {
    let Some(parent) = &ctx.parent else { return Ok(()); };
    let path = bud_repos_root().join(&ctx.org).join(format!("{parent}-oracle/ψ/memory/signals/{}_{}_birth.json", bud_date(), ctx.stem));
    if let Some(dir) = path.parent() { fs.bud_create_dir_all(dir)?; }
    fs.bud_write_atomic(&path, format!("{}\n", serde_json::json!({"kind":"info","message":format!("bud born: {}", ctx.stem),"context":{"budRepoSlug":ctx.slug}})).as_bytes())?;
    let _ = writeln!(out, "  \x1b[36m⬡\x1b[0m signal dropped → {parent}'s ψ/memory/signals/");
    Ok(())
}

fn bud_from_repo_plan(stem: &str, path: &std::path::Path, options: &BudOptions) -> String {
    let mut out = format!("\n  \x1b[36m🧪 Oracle scaffold plan\x1b[0m — {stem} → {}\n\n", path.display());
    let _ = writeln!(out, "  mkdir ψ/");
    let _ = writeln!(out, "  write/append CLAUDE.md oracle scaffold");
    let _ = writeln!(out, "  write .claude/settings.local.json");
    if !options.track_vault { let _ = writeln!(out, "  append .gitignore ψ/"); }
    if options.sync_peers { let _ = writeln!(out, "  copy peers.json snapshot (redacted output)"); }
    out
}

fn bud_write_from_repo_scaffold(stem: &str, path: &std::path::Path, options: &BudOptions, fs: &mut impl BudFs) -> Result<(), String> {
    bud_create_vault(path, fs)?;
    let claude = path.join("CLAUDE.md");
    if fs.bud_exists(&claude) { fs.bud_append_atomic(&claude, &format!("\n<!-- oracle-scaffold: begin stem={stem} -->\n## Oracle scaffolding\nRun `/awaken` for identity setup.\n<!-- oracle-scaffold: end stem={stem} -->\n"))?; } else { fs.bud_write_atomic(&claude, format!("# {stem}-oracle\n\nRun `/awaken` for identity setup.\n").as_bytes())?; }
    fs.bud_write_atomic(&path.join(".claude/settings.local.json"), b"{}\n")?;
    if !options.track_vault { fs.bud_append_atomic(&path.join(".gitignore"), "ψ/\n")?; }
    Ok(())
}

fn bud_sync_peers(target: &std::path::Path, fs: &mut impl BudFs) -> Result<(), String> {
    let src = maw_state_path(&current_xdg_env(), &["peers.json"]);
    if !fs.bud_exists(&src) { return Ok(()); }
    let dst = target.join("ψ/peers.json");
    if let Some(parent) = dst.parent() { fs.bud_create_dir_all(parent)?; }
    let _archive = fs.bud_archive_existing(&dst)?;
    fs.bud_copy_atomic(&src, &dst)
}

fn bud_reload(http: &mut impl BudHttpClient) -> Result<(), String> {
    let port = std::env::var("MAW_PORT").ok().and_then(|value| value.parse::<u16>().ok()).unwrap_or(3456);
    http.bud_reload(port)
}

fn bud_write_atomic_path(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|error| error.to_string())?; }
    let tmp = path.with_extension(format!("tmp-{}", bud_epoch_seconds()));
    std::fs::write(&tmp, bytes).map_err(|error| error.to_string())?;
    std::fs::rename(tmp, path).map_err(|error| error.to_string())
}

fn bud_validate_oracle_stem(value: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.ends_with("-oracle") || value.ends_with("-view") || value.contains("..") || value.contains('/') || value.contains('\\') || value.contains('\0') || value.chars().any(char::is_control) || !value.bytes().next().is_some_and(|b| b.is_ascii_alphabetic()) || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { return Err(format!("invalid oracle name: {value:?}")); }
    Ok(value.to_owned())
}

fn bud_validate_stem_like(value: &str, label: &str) -> Result<String, String> { bud_validate_oracle_stem(value).map_err(|_| format!("bud: invalid {label} value")) }

fn bud_validate_org(value: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.contains('/') || value.contains("..") || value.contains('\0') || value.chars().any(char::is_control) || !value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')) { return Err(format!("bud: invalid org {value:?}")); }
    Ok(value.to_owned())
}

fn bud_validate_repo_name(value: &str) -> Result<(), String> { let stem = value.strip_suffix("-oracle").ok_or_else(|| "bud: repo must end with -oracle".to_owned())?; bud_validate_oracle_stem(stem).map(|_| ()) }

fn bud_validate_slug(value: &str, label: &str) -> Result<String, String> {
    let Some((org, repo)) = value.split_once('/') else { return Err(format!("bud: {label} must be owner/repo")); };
    if value.matches('/').count() != 1 { return Err(format!("bud: {label} must be owner/repo")); }
    let org = bud_validate_org(org)?;
    bud_validate_repo_name(repo).or_else(|_| bud_validate_oracle_stem(repo).map(|_| ()))?;
    Ok(format!("{org}/{repo}"))
}

fn bud_validate_issue(value: &str) -> Result<u32, String> { value.parse::<u32>().ok().filter(|n| *n > 0).ok_or_else(|| "bud: --issue must be a positive integer".to_owned()) }

fn bud_validate_text(value: &str, label: &str) -> Result<String, String> {
    if value.contains('\0') || value.chars().any(|ch| ch.is_control() && ch != '\n' && ch != '\t') { Err(format!("bud: invalid {label} value")) } else { Ok(value.to_owned()) }
}

fn bud_validate_id(value: &str, label: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.contains('\0') || value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) { Err(format!("bud: invalid {label} value")) } else { Ok(value.to_owned()) }
}

fn bud_validate_pathish(value: &str, label: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.contains('\0') || value.chars().any(char::is_control) { Err(format!("bud: invalid {label} value")) } else { Ok(value.to_owned()) }
}

fn bud_flag_like(value: &str) -> String { format!("\"{value}\" looks like a flag, not an oracle name.\n  {BUD_USAGE}") }
fn bud_repos_root() -> std::path::PathBuf {
    let root = std::env::var_os("GHQ_ROOT").map_or_else(
        || {
            std::env::var_os("HOME").map_or_else(
                || std::path::PathBuf::from(".").join("Code/github.com"),
                |home| std::path::PathBuf::from(home).join("Code/github.com"),
            )
        },
        std::path::PathBuf::from,
    );
    if root.file_name().is_some_and(|name| name == "github.com") { root } else { root.join("github.com") }
}
fn bud_looks_like_url(value: &str) -> bool { value.starts_with("http://") || value.starts_with("https://") || value.starts_with("git@") || (!value.starts_with('/') && value.matches('/').count() == 1) }
fn bud_claude_md(ctx: &BudContext) -> String { format!("# {}-oracle\n\n> Budded from {} via `maw bud`.\n\nRun `/awaken` for the full identity setup ceremony.\n", ctx.stem, ctx.parent.as_deref().unwrap_or("root")) }
fn bud_settings() -> String { "{\n  \"hooks\": {}\n}\n".to_owned() }
fn bud_now_iso() -> String { format!("epoch-seconds:{}", bud_epoch_seconds()) }
fn bud_date() -> String { "1970-01-01".to_owned() }
fn bud_epoch_seconds() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs()) }

#[cfg(test)]
mod bud_tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[derive(Default)]
    struct FakeGh { calls: Vec<(String, Vec<String>)>, fail: BTreeSet<String>, ok_view: bool }
    impl BudGhGitRunner for FakeGh {
        fn bud_run(&mut self, program: &str, args: &[String]) -> BudCmdOutput {
            self.calls.push((program.to_owned(), args.to_vec()));
            let key = format!("{program} {}", args.first().cloned().unwrap_or_default());
            let ok = if program == "gh" && args.get(1).is_some_and(|arg| arg == "view") { self.ok_view } else { !self.fail.contains(&key) };
            BudCmdOutput { ok, stdout: String::new(), stderr: if ok { String::new() } else { "fake failure".to_owned() } }
        }
    }

    #[derive(Default)]
    struct FakeFs { files: BTreeMap<std::path::PathBuf, Vec<u8>>, dirs: BTreeSet<std::path::PathBuf>, archives: Vec<std::path::PathBuf> }
    impl BudFs for FakeFs {
        fn bud_exists(&self, path: &std::path::Path) -> bool { self.files.contains_key(path) || self.dirs.contains(path) }
        fn bud_create_dir_all(&mut self, path: &std::path::Path) -> Result<(), String> { self.dirs.insert(path.to_path_buf()); Ok(()) }
        fn bud_read(&self, path: &std::path::Path) -> Result<Vec<u8>, String> { self.files.get(path).cloned().ok_or_else(|| "missing".to_owned()) }
        fn bud_write_atomic(&mut self, path: &std::path::Path, bytes: &[u8]) -> Result<(), String> { self.files.insert(path.to_path_buf(), bytes.to_vec()); Ok(()) }
        fn bud_append_atomic(&mut self, path: &std::path::Path, text: &str) -> Result<(), String> { self.files.entry(path.to_path_buf()).or_default().extend_from_slice(text.as_bytes()); Ok(()) }
        fn bud_copy_atomic(&mut self, from: &std::path::Path, to: &std::path::Path) -> Result<(), String> { let data = self.bud_read(from)?; self.bud_write_atomic(to, &data) }
        fn bud_archive_existing(&mut self, path: &std::path::Path) -> Result<Option<std::path::PathBuf>, String> { if self.bud_exists(path) { let archive = path.with_extension("json.archive-test"); self.archives.push(archive.clone()); Ok(Some(archive)) } else { Ok(None) } }
    }

    #[derive(Default)]
    struct FakeWake { calls: Vec<Vec<String>>, split: Vec<String> }
    impl BudWakeRunner for FakeWake { fn bud_wake(&mut self, args: &[String]) -> Result<String, String> { self.calls.push(args.to_vec()); Ok(String::new()) } fn bud_split(&mut self, stem: &str) -> Result<String, String> { self.split.push(stem.to_owned()); Ok(String::new()) } }
    #[derive(Default)] struct FakeHttp { ports: Vec<u16> }
    impl BudHttpClient for FakeHttp { fn bud_reload(&mut self, port: u16) -> Result<(), String> { self.ports.push(port); Ok(()) } }

    fn bud_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn bud_dispatch_has_alias() { assert_eq!(DISPATCH_121.iter().map(|entry| entry.command).collect::<Vec<_>>(), ["bud", "buddy"]); }

    #[test]
    fn bud_dry_run_matches_golden_without_ref() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore_ghq = EnvVarRestore::capture("GHQ_ROOT");
        let _restore_home = EnvVarRestore::capture("HOME");
        std::env::set_var("GHQ_ROOT", ".");
        std::env::remove_var("HOME");

        let mut gh = FakeGh::default();
        let mut fs = FakeFs::default();
        let mut wake = FakeWake::default();
        let mut http = FakeHttp::default();
        let out = bud_run_with(&bud_args(&["sprout", "--from", "nova", "--dry-run"]), &mut gh, &mut fs, &mut wake, &mut http);
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout, include_str!("../../tests/fixtures/native-bud/bud-dry-run.stdout"));
        assert!(gh.calls.is_empty());
        assert!(wake.calls.is_empty());
    }

    #[test]
    fn bud_gh_is_idempotent_and_argv_only() {
        let mut gh = FakeGh { ok_view: true, ..Default::default() };
        let fs = FakeFs::default();
        let ctx = bud_context(&bud_parse(&bud_args(&["sprout", "--org", "Soul-Brews-Studio"])).unwrap()).unwrap();
        bud_create_repo(&ctx, &mut gh, &fs).unwrap();
        assert!(gh.calls.iter().any(|(p, a)| p == "gh" && a == &["repo", "view", "Soul-Brews-Studio/sprout-oracle", "--json", "name"].map(str::to_owned)));
        assert!(!gh.calls.iter().any(|(_, a)| a.iter().any(|arg| arg.contains(';'))));
        assert!(!gh.calls.iter().any(|(_, a)| a.get(1).is_some_and(|arg| arg == "create")));
    }

    #[test]
    fn bud_rejects_bad_stem_before_runner() {
        let mut gh = FakeGh::default(); let mut fs = FakeFs::default(); let mut wake = FakeWake::default(); let mut http = FakeHttp::default();
        let out = bud_run_with(&bud_args(&["../bad", "--dry-run"]), &mut gh, &mut fs, &mut wake, &mut http);
        assert_ne!(out.code, 0); assert!(gh.calls.is_empty()); assert!(fs.files.is_empty());
    }

    #[test]
    fn bud_scaffold_only_skips_git_and_wake() {
        let mut gh = FakeGh::default(); let mut fs = FakeFs::default(); let mut wake = FakeWake::default(); let mut http = FakeHttp::default();
        let out = bud_run_with(&bud_args(&["sprout", "--org", "Org", "--scaffold-only"]), &mut gh, &mut fs, &mut wake, &mut http);
        assert_eq!(out.code, 0); assert!(out.stdout.contains("Scaffold complete"));
        assert!(!gh.calls.iter().any(|(p, _)| p == "git")); assert!(wake.calls.is_empty()); assert_eq!(http.ports, [3456]);
    }

    #[test]
    fn bud_sync_peers_archives_before_overwrite_and_redacts() {
        let _guard = env_test_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore_state = EnvVarRestore::capture("MAW_STATE_DIR");
        std::env::set_var("MAW_STATE_DIR", "/tmp/maw-bud-test-state");
        let mut fs = FakeFs::default();
        let src = maw_state_path(&current_xdg_env(), &["peers.json"]);
        let dst = std::path::PathBuf::from("/tmp/target/ψ/peers.json");
        fs.files.insert(src, br#"{"peers":{"a":{"pubkey":"SECRET-PUBKEY"}}}"#.to_vec());
        fs.files.insert(dst.clone(), b"old".to_vec());
        bud_sync_peers(std::path::Path::new("/tmp/target"), &mut fs).unwrap();
        assert_eq!(fs.archives.len(), 1); assert_eq!(fs.files.get(&dst).unwrap(), br#"{"peers":{"a":{"pubkey":"SECRET-PUBKEY"}}}"#);
    }

    #[test]
    fn bud_wake_uses_self_argv_shape() {
        let mut wake = FakeWake::default(); let ctx = BudContext { stem: "sprout".to_owned(), org: "Org".to_owned(), parent: None, repo_name: "sprout-oracle".to_owned(), slug: "Org/sprout-oracle".to_owned(), repo_path: "/tmp/sprout".into() };
        bud_wake(&ctx, &BudOptions::default(), &mut wake, &mut String::new());
        assert_eq!(wake.calls[0][0], "wake"); assert!(wake.calls[0].contains(&"--repo-path".to_owned()));
    }
}
