const DISPATCH_109: &[DispatcherEntry] = &[
    DispatcherEntry { command: "scope", handler: Handler::Sync(scope_run_command) },
];

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct ScopeNativeRecord {
    name: String,
    members: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lead: Option<String>,
    created: String,
    ttl: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum ScopeAclDecision {
    Allow,
    Queue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
struct ScopeAclTrustPair {
    sender: String,
    target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct ScopeTrustEntry {
    sender: String,
    target: String,
    #[serde(rename = "addedAt")]
    added_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum ScopeTrustWriteOutcome {
    Added,
    AlreadyTrusted,
}

#[derive(Debug, Clone, Default)]
struct ScopeArgs {
    subcommand: Option<String>,
    positionals: Vec<String>,
    members: Option<String>,
    lead: Option<String>,
    ttl: Option<String>,
    yes: bool,
    help: bool,
}

fn scope_run_command(cli_args: &[String]) -> CliOutput {
    let parsed = match scope_parse_args(cli_args) {
        Ok(parsed) => parsed,
        Err(error) => return scope_native_error(&error),
    };
    let Some(sub) = parsed.subcommand.as_deref() else {
        return CliOutput { code: 0, stdout: format!("{}\n", scope_native_help()), stderr: String::new() };
    };
    if parsed.help {
        return CliOutput { code: 0, stdout: format!("{}\n", scope_native_help()), stderr: String::new() };
    }
    match sub {
        "list" | "ls" => scope_run_list(),
        "create" | "new" => scope_run_create(&parsed),
        "show" | "info" => scope_run_show(&parsed),
        "delete" | "rm" | "remove" => scope_run_delete(&parsed),
        _ => CliOutput {
            code: 1,
            stdout: format!("{}\n", scope_native_help()),
            stderr: format!(
                "maw scope: unknown subcommand \"{sub}\" (expected list|create|show|delete)\n"
            ),
        },
    }
}

fn scope_parse_args(cli_args: &[String]) -> Result<ScopeArgs, String> {
    let mut parsed = ScopeArgs::default();
    let mut index = 0;
    while index < cli_args.len() {
        let token = &cli_args[index];
        if token == "--" {
            index += 1;
            while index < cli_args.len() {
                scope_push_positional(&mut parsed, &cli_args[index])?;
                index += 1;
            }
            break;
        }
        if token == "--help" || token == "-h" {
            parsed.help = true;
            index += 1;
            continue;
        }
        match token.as_str() {
            "--members" => {
                parsed.members = Some(scope_take_value(cli_args, &mut index, "--members")?);
            }
            "--lead" => {
                parsed.lead = Some(scope_take_value(cli_args, &mut index, "--lead")?);
            }
            "--ttl" => {
                parsed.ttl = Some(scope_take_value(cli_args, &mut index, "--ttl")?);
            }
            "--yes" | "-y" => {
                parsed.yes = true;
                index += 1;
            }
            _ if token.starts_with('-') => return Err(format!("scope: unknown flag {token}")),
            _ => {
                scope_push_positional(&mut parsed, token)?;
                index += 1;
            }
        }
    }
    parsed.subcommand = parsed.positionals.first().cloned();
    Ok(parsed)
}

fn scope_take_value(argv: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    let Some(value) = argv.get(*index) else { return Err(format!("scope: missing {flag} value")); };
    scope_validate_value(flag, value)?;
    *index += 1;
    Ok(value.clone())
}

fn scope_push_positional(args: &mut ScopeArgs, value: &str) -> Result<(), String> {
    scope_validate_value("positional", value)?;
    args.positionals.push(value.to_owned());
    Ok(())
}

fn scope_validate_value(kind: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value.contains('\0') || value.contains('\n') {
        return Err(format!("scope: invalid {kind} value"));
    }
    Ok(())
}

fn scope_run_list() -> CliOutput {
    match scope_list_records() {
        Ok(scopes) => CliOutput {
            code: 0,
            stdout: format!("{}\n", scope_format_list(&scopes)),
            stderr: String::new(),
        },
        Err(error) => scope_native_error(&error),
    }
}

fn scope_run_create(args: &ScopeArgs) -> CliOutput {
    let Some(name) = args.positionals.get(1).map(String::as_str) else {
        return scope_native_error("usage: maw scope create <name> --members <a,b,c> [--lead <m>] [--ttl <iso>]");
    };
    let Some(members_raw) = args.members.as_deref() else {
        return scope_native_error(&format!(
            "usage: maw scope create {name} --members <a,b,c> [--lead <m>] [--ttl <iso>]"
        ));
    };
    let members = scope_parse_members(members_raw);
    match scope_create_record(name, members, args.lead.clone(), args.ttl.clone()) {
        Ok(scope) => scope_created_output(&scope),
        Err(error) => scope_native_error(&error),
    }
}

fn scope_run_show(args: &ScopeArgs) -> CliOutput {
    let Some(name) = args.positionals.get(1).map(String::as_str) else {
        return scope_native_error("usage: maw scope show <name>");
    };
    if let Err(error) = scope_validate_name(name) {
        return scope_native_error(&error);
    }
    match scope_load_record(name) {
        Ok(Some(scope)) => match serde_json::to_string_pretty(&scope) {
            Ok(json) => CliOutput { code: 0, stdout: format!("{json}\n"), stderr: String::new() },
            Err(error) => scope_native_error(&format!("scope: failed to render {name}: {error}")),
        },
        Ok(None) => scope_native_error(&format!("scope \"{name}\" not found")),
        Err(error) => scope_native_error(&error),
    }
}

fn scope_run_delete(args: &ScopeArgs) -> CliOutput {
    let Some(name) = args.positionals.get(1).map(String::as_str) else {
        return scope_native_error("usage: maw scope delete <name> [--yes]");
    };
    if !args.yes {
        return CliOutput {
            code: 1,
            stdout: format!(
                "refusing to delete scope \"{name}\" without --yes\n  to confirm: maw scope delete {name} --yes\n"
            ),
            stderr: "delete requires --yes\n".to_owned(),
        };
    }
    match scope_delete_record(name) {
        Ok(true) => CliOutput { code: 0, stdout: format!("deleted scope \"{name}\"\n"), stderr: String::new() },
        Ok(false) => CliOutput {
            code: 0,
            stdout: format!("no-op: scope \"{name}\" not present\n"),
            stderr: String::new(),
        },
        Err(error) => scope_native_error(&error),
    }
}

fn scope_native_help() -> &'static str {
    "usage: maw scope <list|create|show|delete> [...]
  list                                                    — list all scopes used by the live ACL gate
  create   <name> --members <a,b,c> [--lead <m>] [--ttl <iso>]
                                                          — create new scope (refuses overwrite)
  show     <name>                                         — print one scope's JSON
  delete   <name> [--yes]                                 — remove scope file (confirms unless --yes)

storage: <CONFIG_DIR>/scopes/<name>.json (one file per scope)
trust:   <STATE_DIR>/scope-trust.json (symmetric sender/target approvals)

ACL is live for peer sends: same-scope and trusted sender/target pairs deliver;
cross-scope untrusted peer sends queue for human approval via `maw inbox pending`.
Use `maw hey|send --approve` for one human-approved replay, and
`--approve --trust` to add a symmetric scope trust pair before delivery.
If scope/trust files are corrupt, peer send fails open with a loud stderr warning."
}

fn scope_native_error(message: &str) -> CliOutput {
    CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") }
}

fn scope_parse_members(raw: &str) -> Vec<String> {
    raw.split(',').map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned).collect()
}

fn scope_created_output(scope: &ScopeNativeRecord) -> CliOutput {
    CliOutput {
        code: 0,
        stdout: format!(
            "created scope \"{}\" ({} member{})\n  {}\n",
            scope.name,
            scope.members.len(),
            if scope.members.len() == 1 { "" } else { "s" },
            scope_native_path(&scope.name).display()
        ),
        stderr: String::new(),
    }
}

fn scope_validate_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err("invalid scope name \"\" (must match ^[a-z0-9][a-z0-9_-]{0,63}$)".to_owned());
    };
    if name.len() > 64 || !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(format!("invalid scope name \"{name}\" (must match ^[a-z0-9][a-z0-9_-]{{0,63}}$)"));
    }
    if !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-')) {
        return Err(format!("invalid scope name \"{name}\" (must match ^[a-z0-9][a-z0-9_-]{{0,63}}$)"));
    }
    Ok(())
}

fn scope_create_record(
    name: &str,
    members: Vec<String>,
    lead: Option<String>,
    ttl: Option<String>,
) -> Result<ScopeNativeRecord, String> {
    scope_validate_create(name, &members, lead.as_deref())?;
    std::fs::create_dir_all(scope_native_dir()).map_err(|error| format!("scope: create scopes dir: {error}"))?;
    let path = scope_native_path(name);
    if path.exists() {
        return Err(format!(
            "scope \"{name}\" already exists at {} — delete it first to recreate",
            path.display()
        ));
    }
    let scope = ScopeNativeRecord {
        name: name.to_owned(),
        members,
        lead,
        created: scope_now_iso_utc(),
        ttl: ttl.or(Some(String::new())).filter(|value| !value.is_empty()),
    };
    scope_write_record(&path, &scope)?;
    Ok(scope)
}

fn scope_validate_create(name: &str, members: &[String], lead: Option<&str>) -> Result<(), String> {
    scope_validate_name(name)?;
    if members.is_empty() {
        return Err(format!("scope \"{name}\" must have at least one member"));
    }
    if members.iter().any(|member| member.is_empty() || member.starts_with('-')) {
        return Err(format!("scope \"{name}\" has an empty/invalid member entry"));
    }
    if let Some(lead) = lead {
        if !members.iter().any(|member| member == lead) {
            return Err(format!("scope \"{name}\" lead \"{lead}\" is not in members"));
        }
    }
    Ok(())
}

fn scope_write_record(path: &std::path::Path, scope: &ScopeNativeRecord) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(scope).map_err(|error| format!("scope: render {}: {error}", scope.name))? + "\n";
    std::fs::write(&tmp, json).map_err(|error| format!("scope: write {}: {error}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|error| format!("scope: rename {}: {error}", path.display()))?;
    Ok(())
}

fn scope_delete_record(name: &str) -> Result<bool, String> {
    scope_validate_name(name)?;
    let path = scope_native_path(name);
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(&path).map_err(|error| format!("scope: delete {}: {error}", path.display()))?;
    Ok(true)
}

fn scope_list_records() -> Result<Vec<ScopeNativeRecord>, String> {
    std::fs::create_dir_all(scope_native_dir()).map_err(|error| format!("scope: create scopes dir: {error}"))?;
    Ok(scope_acl_load_scopes())
}

#[allow(dead_code)]
fn scope_acl_evaluate(
    sender: &str,
    target: &str,
    scopes: &[ScopeNativeRecord],
    trust: &[ScopeAclTrustPair],
) -> ScopeAclDecision {
    if sender == target {
        return ScopeAclDecision::Allow;
    }
    if scopes
        .iter()
        .any(|scope| scope_acl_scope_contains_pair(scope, sender, target))
    {
        return ScopeAclDecision::Allow;
    }
    if trust
        .iter()
        .any(|pair| scope_acl_trust_pair_matches(pair, sender, target))
    {
        return ScopeAclDecision::Allow;
    }
    ScopeAclDecision::Queue
}

#[allow(dead_code)]
fn scope_acl_scope_contains_pair(scope: &ScopeNativeRecord, sender: &str, target: &str) -> bool {
    let has_sender = scope.members.iter().any(|member| member == sender);
    let has_target = scope.members.iter().any(|member| member == target);
    has_sender && has_target
}

#[allow(dead_code)]
fn scope_acl_trust_pair_matches(pair: &ScopeAclTrustPair, sender: &str, target: &str) -> bool {
    (pair.sender == sender && pair.target == target)
        || (pair.sender == target && pair.target == sender)
}

#[allow(dead_code)]
fn scope_trust_load() -> Vec<ScopeTrustEntry> {
    scope_trust_load_from_path(&scope_trust_path())
}

#[allow(dead_code)]
fn scope_trust_add(sender: &str, target: &str) -> Result<ScopeTrustWriteOutcome, String> {
    scope_trust_add_to_path(&scope_trust_path(), sender, target, &scope_trust_now_iso())
}

#[allow(dead_code)]
fn scope_trust_path() -> std::path::PathBuf {
    maw_state_path(&current_xdg_env(), &["scope-trust.json"])
}

#[allow(dead_code)]
fn scope_trust_pairs(entries: &[ScopeTrustEntry]) -> Vec<ScopeAclTrustPair> {
    entries
        .iter()
        .map(|entry| ScopeAclTrustPair {
            sender: entry.sender.clone(),
            target: entry.target.clone(),
        })
        .collect()
}

fn scope_trust_add_to_path(
    path: &std::path::Path,
    sender: &str,
    target: &str,
    added_at: &str,
) -> Result<ScopeTrustWriteOutcome, String> {
    let sender = scope_trust_validate_actor("sender", sender)?;
    let target = scope_trust_validate_actor("target", target)?;
    scope_trust_validate_not_self(&sender, &target)?;
    scope_trust_validate_added_at(added_at)?;
    let mut entries = scope_trust_load_from_path(path);
    if entries
        .iter()
        .any(|entry| scope_trust_same_relationship(&entry.sender, &entry.target, &sender, &target))
    {
        return Ok(ScopeTrustWriteOutcome::AlreadyTrusted);
    }
    entries.push(ScopeTrustEntry {
        sender,
        target,
        added_at: added_at.to_owned(),
    });
    scope_trust_write_atomic(path, &entries)?;
    Ok(ScopeTrustWriteOutcome::Added)
}

fn scope_trust_load_from_path(path: &std::path::Path) -> Vec<ScopeTrustEntry> {
    let Ok(body) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    scope_trust_parse_entries(&body)
}

fn scope_trust_parse_entries(body: &str) -> Vec<ScopeTrustEntry> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items.iter().filter_map(scope_trust_entry_from_json).collect()
}

fn scope_trust_entry_from_json(value: &serde_json::Value) -> Option<ScopeTrustEntry> {
    let sender = value.get("sender")?.as_str()?.to_owned();
    let target = value.get("target")?.as_str()?.to_owned();
    let added_at = value.get("addedAt")?.as_str()?.to_owned();
    if scope_trust_validate_actor("sender", &sender).is_err()
        || scope_trust_validate_actor("target", &target).is_err()
        || scope_trust_validate_not_self(&sender, &target).is_err()
        || scope_trust_validate_added_at(&added_at).is_err()
    {
        return None;
    }
    Some(ScopeTrustEntry {
        sender,
        target,
        added_at,
    })
}

fn scope_trust_write_atomic(path: &std::path::Path, entries: &[ScopeTrustEntry]) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| format!("scope-trust: create parent failed: {error}"))?;
    let body = serde_json::to_string_pretty(entries).map_err(|error| format!("scope-trust: encode failed: {error}"))? + "\n";
    let tmp = scope_trust_tmp_path(path);
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        }
        let mut file = options
            .open(&tmp)
            .map_err(|error| format!("scope-trust: tmp create failed: {error}"))?;
        std::io::Write::write_all(&mut file, body.as_bytes())
            .map_err(|error| format!("scope-trust: tmp write failed: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("scope-trust: tmp sync failed: {error}"))?;
    }
    let parsed = std::fs::read_to_string(&tmp)
        .map_err(|error| format!("scope-trust: tmp validate read failed: {error}"))?;
    if scope_trust_parse_entries(&parsed).len() != entries.len() {
        let _ = std::fs::remove_file(&tmp);
        return Err("scope-trust: tmp validation mismatch".to_owned());
    }
    std::fs::rename(&tmp, path).map_err(|error| format!("scope-trust: atomic rename failed: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, permissions)
            .map_err(|error| format!("scope-trust: chmod 0600 failed: {error}"))?;
    }
    Ok(())
}

fn scope_trust_tmp_path(path: &std::path::Path) -> std::path::PathBuf {
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("scope-trust.json");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    parent.join(format!(".{name}.{}-{nanos}.tmp", std::process::id()))
}

fn scope_trust_validate_actor(label: &str, value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("scope-trust: {label} must be non-empty"));
    }
    if trimmed != value || trimmed.starts_with('-') {
        return Err(format!("scope-trust: {label} is rejected"));
    }
    if trimmed.contains('\0') || trimmed.chars().any(char::is_control) || trimmed.chars().any(char::is_whitespace) {
        return Err(format!("scope-trust: {label} contains a rejected character"));
    }
    Ok(trimmed.to_owned())
}

fn scope_trust_validate_not_self(sender: &str, target: &str) -> Result<(), String> {
    if sender == target {
        return Err("scope-trust: refusing self-trust relationship; self-sender is already allowed".to_owned());
    }
    Ok(())
}

fn scope_trust_validate_added_at(value: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().any(char::is_control) || !value.ends_with('Z') || !value.contains('T') {
        return Err("scope-trust: addedAt timestamp is rejected".to_owned());
    }
    Ok(())
}

fn scope_trust_same_relationship(left_sender: &str, left_target: &str, sender: &str, target: &str) -> bool {
    (left_sender == sender && left_target == target) || (left_sender == target && left_target == sender)
}

fn scope_trust_now_iso() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX));
    trust_iso_from_ms(ms)
}

fn scope_acl_load_scopes() -> Vec<ScopeNativeRecord> {
    scope_acl_load_scopes_from_dir(&scope_native_dir())
}

fn scope_acl_load_scopes_from_dir(dir: &std::path::Path) -> Vec<ScopeNativeRecord> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        scope_maybe_push_record(entry.path(), &mut out);
    }
    out.sort_by(|left, right| left.name.cmp(&right.name));
    out
}

fn scope_maybe_push_record(path: std::path::PathBuf, out: &mut Vec<ScopeNativeRecord>) {
    if path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
        return;
    }
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Ok(scope) = serde_json::from_str::<ScopeNativeRecord>(&text) {
            out.push(scope);
        }
    }
}

fn scope_load_record(name: &str) -> Result<Option<ScopeNativeRecord>, String> {
    let path = scope_native_path(name);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|error| format!("scope: read {}: {error}", path.display()))?;
    Ok(serde_json::from_str(&text).ok())
}

fn scope_format_list(rows: &[ScopeNativeRecord]) -> String {
    if rows.is_empty() {
        return "no scopes".to_owned();
    }
    let header = ["name", "members", "lead", "ttl", "created"];
    let data = scope_list_rows(rows);
    let widths = scope_widths(&header, &data);
    let mut lines = Vec::new();
    lines.push(scope_format_row(&header.map(str::to_owned), &widths));
    lines.push(scope_format_row(&widths.iter().map(|width| "-".repeat(*width)).collect::<Vec<_>>(), &widths));
    lines.extend(data.iter().map(|cols| scope_format_row(cols, &widths)));
    lines.join("\n")
}

fn scope_list_rows(rows: &[ScopeNativeRecord]) -> Vec<[String; 5]> {
    rows.iter()
        .map(|row| {
            [
                row.name.clone(),
                row.members.join(","),
                row.lead.clone().unwrap_or_else(|| "-".to_owned()),
                row.ttl.clone().unwrap_or_else(|| "-".to_owned()),
                row.created.clone(),
            ]
        })
        .collect()
}

fn scope_widths(header: &[&str; 5], data: &[[String; 5]]) -> Vec<usize> {
    (0..header.len())
        .map(|idx| data.iter().map(|cols| cols[idx].len()).chain([header[idx].len()]).max().unwrap_or(0))
        .collect()
}

fn scope_format_row(cols: &[String], widths: &[usize]) -> String {
    cols.iter()
        .enumerate()
        .map(|(idx, col)| format!("{col:<width$}", width = widths[idx]))
        .collect::<Vec<_>>()
        .join("  ")
}

fn scope_native_dir() -> std::path::PathBuf { scope_native_config_dir().join("scopes") }

fn scope_native_path(name: &str) -> std::path::PathBuf { scope_native_dir().join(format!("{name}.json")) }

fn scope_native_config_dir() -> std::path::PathBuf {
    let env = scope_native_current_xdg_env();
    maw_config_dir(&env)
}

fn scope_native_current_xdg_env() -> MawXdgEnv {
    let home = std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let vars = [
        "MAW_HOME",
        "MAW_CONFIG_DIR",
        "MAW_XDG",
        "XDG_CONFIG_HOME",
        "XDG_STATE_HOME",
        "MAW_STATE_DIR",
        "XDG_DATA_HOME",
        "MAW_DATA_DIR",
        "XDG_CACHE_HOME",
        "MAW_CACHE_DIR",
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)));
    MawXdgEnv::with_vars(home, vars)
}

fn scope_now_iso_utc() -> String {
    if let Ok(fake) = std::env::var("MAW_RS_SCOPE_FAKE_NOW") {
        return fake;
    }
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs());
    format!("{seconds}")
}

#[cfg(test)]
mod scope_acl_tests {
    use super::*;

    fn scope_acl_record(name: &str, members: &[&str]) -> ScopeNativeRecord {
        ScopeNativeRecord {
            name: name.to_owned(),
            members: members.iter().map(|member| (*member).to_owned()).collect(),
            lead: members.first().map(|member| (*member).to_owned()),
            created: "2026-06-26T00:00:00.000Z".to_owned(),
            ttl: None,
        }
    }

    fn scope_acl_trust(sender: &str, target: &str) -> ScopeAclTrustPair {
        ScopeAclTrustPair {
            sender: sender.to_owned(),
            target: target.to_owned(),
        }
    }

    fn scope_acl_temp_dir(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!(
            "maw-rs-scope-acl-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn scope_acl_write_json(path: &std::path::Path, value: &ScopeNativeRecord) {
        let parent = path.parent().expect("scope path parent");
        std::fs::create_dir_all(parent).expect("create temp scope dir");
        let json = serde_json::to_string_pretty(value).expect("scope json");
        std::fs::write(path, format!("{json}\n")).expect("write scope json");
    }

    fn scope_trust_test_path(name: &str) -> std::path::PathBuf {
        scope_acl_temp_dir(name).join("state").join("scope-trust.json")
    }

    #[test]
    fn scope_help_matches_live_acl_golden_and_has_no_stale_wording() {
        let help = format!("{}\n", scope_native_help());
        assert_eq!(help, include_str!("../../tests/fixtures/native-scope-acl/scope-help.stdout"));
        let lower = help.to_ascii_lowercase();
        let stale_terms = [
            ["de", "ferred"].concat(),
            ["plan", "-only"].concat(),
            ["phase", " 1"].concat(),
            ["phase", "-1"].concat(),
            ["not", " yet"].concat(),
        ];
        for stale in stale_terms {
            assert!(!lower.contains(stale.as_str()), "stale scope help wording: {stale}");
        }
        assert!(help.contains("ACL is live for peer sends"));
        assert!(help.contains("--approve --trust"));
        assert!(help.contains("fails open with a loud stderr warning"));
    }

    #[test]
    fn scope_acl_allows_self_sender() {
        let decision = scope_acl_evaluate("alice", "alice", &[], &[]);
        assert_eq!(decision, ScopeAclDecision::Allow);
    }

    #[test]
    fn scope_acl_allows_same_scope_members() {
        let scopes = vec![scope_acl_record("team", &["alice", "bob"])];
        let decision = scope_acl_evaluate("alice", "bob", &scopes, &[]);
        assert_eq!(decision, ScopeAclDecision::Allow);
    }

    #[test]
    fn scope_acl_allows_symmetric_trust_pair_from_param() {
        let trust = vec![scope_acl_trust("alice", "bob")];
        assert_eq!(
            scope_acl_evaluate("alice", "bob", &[], &trust),
            ScopeAclDecision::Allow
        );
        assert_eq!(
            scope_acl_evaluate("bob", "alice", &[], &trust),
            ScopeAclDecision::Allow
        );
    }

    #[test]
    fn scope_acl_queues_non_shared_without_trust() {
        let scopes = vec![scope_acl_record("team", &["alice", "carol"])];
        let decision = scope_acl_evaluate("alice", "bob", &scopes, &[]);
        assert_eq!(decision, ScopeAclDecision::Queue);
    }

    #[test]
    fn scope_acl_load_scopes_returns_empty_for_missing_dir() {
        let dir = scope_acl_temp_dir("missing").join("scopes");
        let rows = scope_acl_load_scopes_from_dir(&dir);
        assert!(rows.is_empty());
    }

    #[test]
    fn scope_acl_load_scopes_ignores_malformed_and_sorts_by_name() {
        let dir = scope_acl_temp_dir("load").join("scopes");
        scope_acl_write_json(&dir.join("zeta.json"), &scope_acl_record("zeta", &["z"]));
        scope_acl_write_json(&dir.join("alpha.json"), &scope_acl_record("alpha", &["a"]));
        std::fs::write(dir.join("bad.json"), "{not json").expect("write malformed json");
        std::fs::write(dir.join("note.txt"), "not a scope").expect("write non-json file");

        let rows = scope_acl_load_scopes_from_dir(&dir);
        let names = rows.iter().map(|scope| scope.name.as_str()).collect::<Vec<_>>();
        assert_eq!(names, vec!["alpha", "zeta"]);
    }

    #[test]
    fn scope_trust_load_missing_and_malformed_are_empty_or_filtered() {
        let path = scope_trust_test_path("load");
        assert!(scope_trust_load_from_path(&path).is_empty());

        let parent = path.parent().expect("parent");
        std::fs::create_dir_all(parent).expect("state dir");
        std::fs::write(&path, "{not json").expect("malformed");
        assert!(scope_trust_load_from_path(&path).is_empty());

        std::fs::write(
            &path,
            r#"[
              {"sender":"alice","target":"bob","addedAt":"2026-06-26T00:00:00.000Z"},
              {"sender":"missing-target","addedAt":"2026-06-26T00:00:00.000Z"},
              {"sender":"carol","target":"carol","addedAt":"2026-06-26T00:00:00.000Z"},
              {"sender":7,"target":"eve","addedAt":"2026-06-26T00:00:00.000Z"}
            ]"#,
        )
        .expect("mixed trust");

        let entries = scope_trust_load_from_path(&path);
        assert_eq!(
            entries,
            vec![ScopeTrustEntry {
                sender: "alice".to_owned(),
                target: "bob".to_owned(),
                added_at: "2026-06-26T00:00:00.000Z".to_owned(),
            }]
        );
    }

    #[test]
    fn scope_trust_add_preserves_entries_is_symmetric_idempotent_and_feeds_acl() {
        let path = scope_trust_test_path("add");
        let outcome = scope_trust_add_to_path(&path, "alice", "bob", "2026-06-26T00:00:00.000Z")
            .expect("first add");
        assert_eq!(outcome, ScopeTrustWriteOutcome::Added);
        let outcome = scope_trust_add_to_path(&path, "bob", "alice", "2026-06-26T00:01:00.000Z")
            .expect("symmetric no-op");
        assert_eq!(outcome, ScopeTrustWriteOutcome::AlreadyTrusted);
        let outcome = scope_trust_add_to_path(&path, "carol", "dave", "2026-06-26T00:02:00.000Z")
            .expect("second add");
        assert_eq!(outcome, ScopeTrustWriteOutcome::Added);

        let entries = scope_trust_load_from_path(&path);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sender, "alice");
        assert_eq!(entries[0].target, "bob");
        assert_eq!(entries[1].sender, "carol");
        assert_eq!(entries[1].target, "dave");
        let pairs = scope_trust_pairs(&entries);
        assert_eq!(
            scope_acl_evaluate("dave", "carol", &[], &pairs),
            ScopeAclDecision::Allow
        );
        assert_eq!(
            scope_acl_evaluate("alice", "dave", &[], &pairs),
            ScopeAclDecision::Queue
        );
    }

    #[test]
    fn scope_trust_add_rejects_empty_self_and_injection_without_write() {
        let path = scope_trust_test_path("reject");
        let empty = scope_trust_add_to_path(&path, "", "bob", "2026-06-26T00:00:00.000Z").expect_err("empty");
        assert!(empty.contains("non-empty"));
        let self_trust =
            scope_trust_add_to_path(&path, "alice", "alice", "2026-06-26T00:00:00.000Z").expect_err("self");
        assert!(self_trust.contains("self-trust"));
        let injected =
            scope_trust_add_to_path(&path, "-alice", "bob", "2026-06-26T00:00:00.000Z").expect_err("dash");
        assert!(injected.contains("rejected"));
        let bad_time = scope_trust_add_to_path(&path, "alice", "bob", "not-time").expect_err("time");
        assert!(bad_time.contains("addedAt"));
        assert!(!path.exists());
    }

    #[test]
    fn scope_trust_writes_scope_trust_json_only_0600_and_never_consent_trust_json() {
        let root = scope_acl_temp_dir("consent-separate").join("state");
        let scope_path = root.join("scope-trust.json");
        let consent_path = root.join("trust.json");
        std::fs::create_dir_all(&root).expect("root");
        std::fs::write(&consent_path, r#"[{"from":"lead","to":"peer","action":"team-invite"}]"#)
            .expect("consent trust");

        scope_trust_add_to_path(&scope_path, "alice", "bob", "2026-06-26T00:00:00.000Z").expect("add");

        assert!(scope_path.exists());
        assert_eq!(
            std::fs::read_to_string(&consent_path).expect("consent untouched"),
            r#"[{"from":"lead","to":"peer","action":"team-invite"}]"#
        );
        let names = std::fs::read_dir(&root)
            .expect("state listing")
            .map(|entry| entry.expect("entry").file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(names.contains(&"scope-trust.json".to_owned()));
        assert!(names.contains(&"trust.json".to_owned()));
        assert!(!names
            .iter()
            .any(|name| std::path::Path::new(name).extension().is_some_and(|ext| ext == "tmp")));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&scope_path).expect("metadata").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}
