const DISPATCH_98: &[DispatcherEntry] = &[
    DispatcherEntry {
        command: "trust",
        handler: Handler::Sync(trust_run_command),
    },
    DispatcherEntry {
        command: "trusts",
        handler: Handler::Sync(trust_run_command),
    },
];

const TRUST_USAGE: &str = "usage: maw trust <list|add|pin|trust|remove|revoke> [...]\n  list                                      — list all trust entries (sorted by addedAt)\n  add|pin|trust <sender> <target> --peer-key <key>\n                                            — trust/pin a peer key (explicit human step)\n  remove|revoke <sender> <target> [--yes]  — revoke a trust relationship\n\nstorage: live native trust-store; writes use tmp+rename and never echo peer keys";
const TRUST_FAKE_STORE_ENV: &str = "MAW_RS_TRUST_FAKE_STORE";
const TRUST_AUTO_REFUSE: &str =
    "trust: consent mutation requires explicit human flow; no auto-trust surface is exposed";
const TRUST_AUTO_BLOCKLIST: &[&str] = &[
    "auto-trust",
    "auto-approve",
    "trust-all",
    "trust-everyone",
    "trust-all-peers",
    "approve-all",
    "auto",
    "auto-pair",
    "pair-auto",
    "pair-approve",
    "allow-all",
    "allowlist-all",
];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct TrustEntryPlan {
    sender: String,
    target: String,
    #[serde(rename = "peerKey", skip_serializing_if = "Option::is_none")]
    peer_key: Option<String>,
    #[serde(rename = "addedAt")]
    added_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TrustWriteOutcome {
    Added,
    AlreadyTrusted,
    UpdatedPin,
}

fn trust_run_command(argv: &[String]) -> CliOutput {
    match trust_validate_auto_surface(argv).and_then(|()| trust_parse_command(argv)) {
        Ok(command) => trust_execute_command(&command),
        Err(message) if message.is_empty() => trust_ok(TRUST_USAGE),
        Err(message) => trust_error(&message),
    }
}

fn trust_parse_command(argv: &[String]) -> Result<TrustCommandPlan, String> {
    trust_validate_separator(argv)?;
    let Some(subcommand) = argv.first() else {
        return Err(String::new());
    };
    if subcommand == "help" || subcommand == "--help" || subcommand == "-h" {
        return Err(String::new());
    }
    if subcommand.starts_with('-') {
        return Err("trust subcommand must not start with '-'".to_owned());
    }
    match subcommand.as_str() {
        "list" | "ls" => trust_parse_list(argv),
        "add" | "pin" | "trust" => trust_parse_add(argv),
        "remove" | "rm" | "delete" | "revoke" => trust_parse_remove(argv),
        _ => Err(format!(
            "maw trust: unknown subcommand \"{subcommand}\" (expected list|add|pin|trust|remove|revoke)"
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TrustCommandPlan {
    List,
    Add {
        sender: String,
        target: String,
        peer_key: String,
    },
    Remove {
        sender: String,
        target: String,
        yes: bool,
    },
}

fn trust_parse_list(argv: &[String]) -> Result<TrustCommandPlan, String> {
    if argv.len() == 1 {
        Ok(TrustCommandPlan::List)
    } else {
        Err("trust list: unexpected argument".to_owned())
    }
}

fn trust_parse_add(argv: &[String]) -> Result<TrustCommandPlan, String> {
    if argv.len() < 3 {
        return Err("trust add: expected <sender> <target> --peer-key <key>".to_owned());
    }
    let sender = trust_validate_actor("sender", &argv[1])?;
    let target = trust_validate_actor("target", &argv[2])?;
    trust_validate_not_self(&sender, &target)?;
    if argv.len() < 5 {
        return Err("trust add: expected --peer-key <key>".to_owned());
    }
    let peer_key = trust_parse_peer_key_flag(&argv[3..])?;
    Ok(TrustCommandPlan::Add {
        sender,
        target,
        peer_key,
    })
}

fn trust_parse_peer_key_flag(argv: &[String]) -> Result<String, String> {
    let mut peer_key = None;
    let mut index = 0;
    while index < argv.len() {
        let arg = &argv[index];
        if arg == "--peer-key" || arg == "--pubkey" || arg == "--key" {
            let Some(value) = argv.get(index + 1) else {
                return Err("trust add: missing peer-key value".to_owned());
            };
            peer_key = Some(trust_validate_peer_key(value)?);
            index += 2;
        } else if let Some(value) = arg.strip_prefix("--peer-key=") {
            peer_key = Some(trust_validate_peer_key(value)?);
            index += 1;
        } else if let Some(value) = arg.strip_prefix("--pubkey=") {
            peer_key = Some(trust_validate_peer_key(value)?);
            index += 1;
        } else if let Some(value) = arg.strip_prefix("--key=") {
            peer_key = Some(trust_validate_peer_key(value)?);
            index += 1;
        } else if arg.starts_with('-') {
            return Err("trust add: unknown flag".to_owned());
        } else {
            return Err("trust add: unexpected argument".to_owned());
        }
    }
    peer_key.ok_or_else(|| "trust add: expected --peer-key <key>".to_owned())
}

fn trust_parse_remove(argv: &[String]) -> Result<TrustCommandPlan, String> {
    if argv.len() != 3 && argv.len() != 4 {
        return Err("trust remove: expected <sender> <target> [--yes]".to_owned());
    }
    let sender = trust_validate_actor("sender", &argv[1])?;
    let target = trust_validate_actor("target", &argv[2])?;
    trust_validate_not_self(&sender, &target)?;
    let yes = argv.get(3).is_some_and(|flag| flag == "--yes" || flag == "-y");
    if argv.get(3).is_some_and(|flag| flag != "--yes" && flag != "-y") {
        return Err("trust remove: only --yes is supported as a flag".to_owned());
    }
    Ok(TrustCommandPlan::Remove {
        sender,
        target,
        yes,
    })
}

fn trust_execute_command(command: &TrustCommandPlan) -> CliOutput {
    match command {
        TrustCommandPlan::List => match trust_read_store(&trust_store_path()) {
            Ok(entries) => trust_ok(&trust_format_list(&entries)),
            Err(message) => trust_error(&message),
        },
        TrustCommandPlan::Add {
            sender,
            target,
            peer_key,
        } => trust_execute_add(sender, target, peer_key),
        TrustCommandPlan::Remove {
            sender,
            target,
            yes,
        } => trust_execute_remove(sender, target, *yes),
    }
}

fn trust_execute_add(sender: &str, target: &str, peer_key: &str) -> CliOutput {
    match trust_store_add(&trust_store_path(), sender, target, peer_key, trust_now_ms()) {
        Ok(TrustWriteOutcome::Added | TrustWriteOutcome::UpdatedPin) => trust_ok(&format!(
            "trusted: \"{sender}\" ↔ \"{target}\" (peer key received redacted)"
        )),
        Ok(TrustWriteOutcome::AlreadyTrusted) => trust_ok(&format!(
            "already trusted: \"{sender}\" ↔ \"{target}\" (peer key matched redacted)"
        )),
        Err(message) => trust_error(&message),
    }
}

fn trust_execute_remove(sender: &str, target: &str, yes: bool) -> CliOutput {
    if !yes {
        return CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: format!(
                "refusing to remove trust relationship without --yes\n  to confirm: maw trust remove {sender} {target} --yes\n"
            ),
        };
    }
    match trust_store_remove(&trust_store_path(), sender, target) {
        Ok(true) => trust_ok(&format!("removed trust relationship \"{sender}\" ↔ \"{target}\"")),
        Ok(false) => trust_error("trust remove: no entry found for requested sender/target"),
        Err(message) => trust_error(&message),
    }
}

fn trust_store_add(
    path: &Path,
    sender: &str,
    target: &str,
    peer_key: &str,
    now_ms: i64,
) -> Result<TrustWriteOutcome, String> {
    let sender = trust_validate_actor("sender", sender)?;
    let target = trust_validate_actor("target", target)?;
    trust_validate_not_self(&sender, &target)?;
    let peer_key = trust_validate_peer_key(peer_key)?;
    let mut entries = trust_read_store(path)?;
    let outcome = trust_upsert_entry(&mut entries, &sender, &target, &peer_key, now_ms)?;
    trust_write_store_atomic(path, &entries)?;
    Ok(outcome)
}

fn trust_store_remove(path: &Path, sender: &str, target: &str) -> Result<bool, String> {
    let sender = trust_validate_actor("sender", sender)?;
    let target = trust_validate_actor("target", target)?;
    trust_validate_not_self(&sender, &target)?;
    let mut entries = trust_read_store(path)?;
    let before = entries.len();
    entries.retain(|entry| !trust_same_relationship(&entry.sender, &entry.target, &sender, &target));
    let removed = entries.len() != before;
    if removed {
        trust_write_store_atomic(path, &entries)?;
    }
    Ok(removed)
}

fn trust_upsert_entry(
    entries: &mut Vec<TrustEntryPlan>,
    sender: &str,
    target: &str,
    peer_key: &str,
    now_ms: i64,
) -> Result<TrustWriteOutcome, String> {
    if let Some(entry) = entries
        .iter_mut()
        .find(|entry| trust_same_relationship(&entry.sender, &entry.target, sender, target))
    {
        if let Some(existing) = &entry.peer_key {
            if existing != peer_key {
                return Err("trust add: peer-key mismatch for known peer; refusing to re-pin".to_owned());
            }
            return Ok(TrustWriteOutcome::AlreadyTrusted);
        }
        entry.peer_key = Some(peer_key.to_owned());
        return Ok(TrustWriteOutcome::UpdatedPin);
    }
    entries.push(TrustEntryPlan {
        sender: sender.to_owned(),
        target: target.to_owned(),
        peer_key: Some(peer_key.to_owned()),
        added_at: trust_iso_from_ms(now_ms),
    });
    Ok(TrustWriteOutcome::Added)
}

fn trust_read_store(path: &Path) -> Result<Vec<TrustEntryPlan>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let body = std::fs::read_to_string(path).map_err(|error| format!("trust-store: read failed: {error}"))?;
    trust_parse_store_entries(&body)
}

fn trust_parse_store_entries(body: &str) -> Result<Vec<TrustEntryPlan>, String> {
    let value = serde_json::from_str::<serde_json::Value>(body)
        .map_err(|_| "trust-store: invalid trust-store json".to_owned())?;
    let Some(items) = value.as_array() else {
        return Err("trust-store: expected trust-store array".to_owned());
    };
    let entries = items.iter().filter_map(trust_entry_from_json).collect::<Vec<_>>();
    if entries.len() != items.len() {
        return Err("trust-store: invalid trust-store entry".to_owned());
    }
    Ok(entries)
}

fn trust_write_store_atomic(path: &Path, entries: &[TrustEntryPlan]) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("trust-store: create parent failed: {error}"))?;
    let body = serde_json::to_string_pretty(entries)
        .map_err(|error| format!("trust-store: encode failed: {error}"))?;
    let tmp = trust_tmp_path(path);
    {
        let mut file = std::fs::File::create(&tmp)
            .map_err(|error| format!("trust-store: tmp create failed: {error}"))?;
        std::io::Write::write_all(&mut file, body.as_bytes())
            .map_err(|error| format!("trust-store: tmp write failed: {error}"))?;
        std::io::Write::write_all(&mut file, b"\n")
            .map_err(|error| format!("trust-store: tmp newline failed: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("trust-store: tmp sync failed: {error}"))?;
    }
    let parsed = std::fs::read_to_string(&tmp)
        .map_err(|error| format!("trust-store: tmp validate read failed: {error}"))?;
    let roundtrip = trust_parse_store_entries(&parsed)?;
    if roundtrip.len() != entries.len() {
        let _ = std::fs::remove_file(&tmp);
        return Err("trust-store: tmp validation mismatch".to_owned());
    }
    std::fs::rename(&tmp, path).map_err(|error| format!("trust-store: atomic rename failed: {error}"))?;
    Ok(())
}

fn trust_tmp_path(path: &Path) -> std::path::PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("trust-store.json");
    parent.join(format!(".{name}.{}.tmp", std::process::id()))
}

fn trust_store_path() -> std::path::PathBuf {
    if let Some(path) = std::env::var_os(TRUST_FAKE_STORE_ENV) {
        return std::path::PathBuf::from(path);
    }
    maw_state_path(&real_xdg_env(), &["trust-store.json"])
}

fn trust_validate_auto_surface(argv: &[String]) -> Result<(), String> {
    for arg in argv {
        let token = trust_normalize_token(arg);
        if TRUST_AUTO_BLOCKLIST.iter().any(|blocked| *blocked == token) {
            return Err(TRUST_AUTO_REFUSE.to_owned());
        }
        if token.contains("auto-trust")
            || token.contains("auto-approve")
            || token.contains("trust-all")
            || token.contains("approve-all")
        {
            return Err(TRUST_AUTO_REFUSE.to_owned());
        }
    }
    Ok(())
}

fn trust_validate_separator(argv: &[String]) -> Result<(), String> {
    if argv.iter().any(|arg| arg == "--") {
        return Err("trust: -- separator is not allowed".to_owned());
    }
    Ok(())
}

fn trust_validate_actor(label: &str, value: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value {
        return Err(format!("trust: {label} must be a non-empty exact value"));
    }
    if value.starts_with('-') {
        return Err(format!("trust: {label} must not start with '-'"));
    }
    if value == "--" || value.contains('/') || value.contains('\\') || value.contains("..") {
        return Err(format!("trust: {label} contains a rejected path-like value"));
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err(format!(
            "trust: {label} must not contain whitespace or control characters"
        ));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':' | '.' | '@'))
    {
        return Err(format!("trust: {label} contains unsupported characters"));
    }
    Ok(value.to_owned())
}

fn trust_validate_peer_key(value: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value {
        return Err("trust: peer-key is missing".to_owned());
    }
    if value.starts_with('-') || value == "--" {
        return Err("trust: peer-key must not start with '-'".to_owned());
    }
    if value.len() > 4096 {
        return Err("trust: peer-key is too long".to_owned());
    }
    if value.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) {
        return Err("trust: peer-key must not contain control characters".to_owned());
    }
    if value.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err("trust: peer-key must not contain whitespace".to_owned());
    }
    Ok(value.to_owned())
}

fn trust_validate_not_self(sender: &str, target: &str) -> Result<(), String> {
    if sender == target {
        return Err("trust add: refusing self-trust relationship; self-messages are always allowed".to_owned());
    }
    Ok(())
}

#[cfg(test)]
fn trust_parse_fake_entries(body: &str) -> Vec<TrustEntryPlan> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items.iter().filter_map(trust_entry_from_json).collect()
}

fn trust_entry_from_json(value: &serde_json::Value) -> Option<TrustEntryPlan> {
    let sender = value.get("sender")?.as_str()?.to_owned();
    let target = value.get("target")?.as_str()?.to_owned();
    let added_at = value.get("addedAt")?.as_str()?.to_owned();
    let peer_key = value
        .get("peerKey")
        .and_then(serde_json::Value::as_str)
        .map(trust_validate_peer_key)
        .transpose()
        .ok()?;
    if trust_validate_actor("sender", &sender).is_err()
        || trust_validate_actor("target", &target).is_err()
        || trust_validate_not_self(&sender, &target).is_err()
        || !trust_valid_timestamp(&added_at)
    {
        return None;
    }
    Some(TrustEntryPlan {
        sender,
        target,
        peer_key,
        added_at,
    })
}

fn trust_valid_timestamp(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(char::is_control) && value.len() <= 64
}

fn trust_format_list(entries: &[TrustEntryPlan]) -> String {
    if entries.is_empty() {
        return "no trust entries".to_owned();
    }
    let mut rows = entries.to_vec();
    rows.sort_by(|left, right| left.added_at.cmp(&right.added_at));
    let mut text = String::from("trusted relationships:\n");
    for entry in rows {
        let key_state = if entry.peer_key.is_some() {
            "peer key received (redacted)"
        } else {
            "peer key missing"
        };
        let _ = writeln!(
            text,
            "  {} ↔ {}   added {}   {}",
            entry.sender, entry.target, entry.added_at, key_state
        );
    }
    text.trim_end().to_owned()
}

#[cfg(test)]
fn trust_find_entry<'a>(
    entries: &'a [TrustEntryPlan],
    sender: &str,
    target: &str,
) -> Option<&'a TrustEntryPlan> {
    entries
        .iter()
        .find(|entry| trust_same_relationship(&entry.sender, &entry.target, sender, target))
}

fn trust_same_relationship(
    left_sender: &str,
    left_target: &str,
    right_sender: &str,
    right_target: &str,
) -> bool {
    (left_sender == right_sender && left_target == right_target)
        || (left_sender == right_target && left_target == right_sender)
}

fn trust_now_ms() -> i64 {
    i64::try_from(current_epoch_seconds())
        .unwrap_or(i64::MAX)
        .saturating_mul(1_000)
}

fn trust_iso_from_ms(ms: i64) -> String {
    format!("epoch-ms:{ms}")
}

fn trust_normalize_token(value: &str) -> String {
    value
        .trim_start_matches('-')
        .replace('_', "-")
        .to_ascii_lowercase()
}

fn trust_ok(message: &str) -> CliOutput {
    CliOutput {
        code: 0,
        stdout: format!("{message}\n"),
        stderr: String::new(),
    }
}

fn trust_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n"),
    }
}

#[cfg(test)]
mod trust_native_tests {
    use super::*;

    fn trust_args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn trust_dispatch_registers_native_command_and_alias() {
        let commands = DISPATCH_98
            .iter()
            .map(|entry| entry.command)
            .collect::<Vec<_>>();
        assert_eq!(commands, ["trust", "trusts"]);
        assert_eq!(dispatcher_status("trust"), DispatchKind::Native);
        assert_eq!(dispatcher_status("trusts"), DispatchKind::Native);
    }

    #[test]
    fn trust_blocks_auto_consent_surface_without_secret_echo() {
        let output = trust_run_command(&trust_args(&[
            "add",
            "--auto-trust=fake-secret-token",
            "alpha",
            "beta",
        ]));
        assert_eq!(output.code, 2);
        assert!(output.stderr.contains("no auto-trust"));
        assert!(!output.stderr.contains("fake-secret-token"));

        let blocked = trust_run_command(&trust_args(&["trust-all", "alpha", "beta"]));
        assert_eq!(blocked.code, 2);
        assert!(blocked.stderr.contains("explicit human flow"));
    }

    #[test]
    fn trust_guards_separator_and_leading_dash_without_echo() {
        let sep = trust_run_command(&trust_args(&["add", "alpha", "--", "beta"]));
        assert_eq!(sep.code, 2);
        assert!(sep.stderr.contains("separator"));
        let guarded = trust_run_command(&trust_args(&["add", "-secret-token", "beta"]));
        assert_eq!(guarded.code, 2);
        assert!(guarded.stderr.contains("sender must not start"));
        assert!(!guarded.stderr.contains("secret-token"));
    }

    #[test]
    fn trust_parses_fake_entries_and_lists_sorted() {
        let entries = trust_parse_fake_entries(
            r#"[
              {"sender":"beta","target":"alpha","addedAt":"2026-06-10T00:00:00.000Z"},
              {"sender":"gamma","target":"delta","addedAt":"2026-06-09T00:00:00.000Z"},
              {"sender":"bad/name","target":"delta","addedAt":"2026-06-08T00:00:00.000Z"}
            ]"#,
        );
        assert_eq!(entries.len(), 2);
        assert!(trust_find_entry(&entries, "alpha", "beta").is_some());
        let list = trust_format_list(&entries);
        let gamma = list.find("gamma ↔ delta").expect("gamma row");
        let beta = list.find("beta ↔ alpha").expect("beta row");
        assert!(gamma < beta, "{list}");
    }

    #[test]
    fn trust_add_list_and_remove_mutate_store_without_key_echo() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore = EnvVarRestore::capture(TRUST_FAKE_STORE_ENV);
        let path = std::env::temp_dir().join(format!(
            "maw-rs-trust-cli-{}-{}.json",
            std::process::id(),
            current_epoch_seconds()
        ));
        std::env::set_var(TRUST_FAKE_STORE_ENV, &path);
        let key = "ed25519:secret-cli-peer-key";

        let missing_key = trust_run_command(&trust_args(&["add", "alpha", "beta"]));
        assert_eq!(missing_key.code, 2);
        assert!(missing_key.stderr.contains("peer-key"));

        let add = trust_run_command(&trust_args(&["add", "alpha", "beta", "--peer-key", key]));
        assert_eq!(add.code, 0, "{}", add.stderr);
        assert!(add.stdout.contains("trusted"));
        assert!(!add.stdout.contains(key));
        assert!(std::fs::read_to_string(&path).expect("store").contains(key));

        let list = trust_run_command(&trust_args(&["list"]));
        assert_eq!(list.code, 0);
        assert!(list.stdout.contains("received (redacted)"));
        assert!(!list.stdout.contains(key));

        let mismatch = trust_run_command(&trust_args(&[
            "pin",
            "beta",
            "alpha",
            "--peer-key",
            "ed25519:different-secret-key",
        ]));
        assert_eq!(mismatch.code, 2);
        assert!(mismatch.stderr.contains("peer-key mismatch"));
        assert!(!mismatch.stderr.contains("different-secret-key"));

        let remove = trust_run_command(&trust_args(&["remove", "alpha", "beta"]));
        assert_eq!(remove.code, 2);
        assert!(remove.stderr.contains("without --yes"));
        let removed = trust_run_command(&trust_args(&["revoke", "alpha", "beta", "--yes"]));
        assert_eq!(removed.code, 0, "{}", removed.stderr);
        assert!(trust_read_store(&path).expect("read").is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn trust_rejects_self_and_control_values() {
        let self_trust = trust_run_command(&trust_args(&["add", "alpha", "alpha"]));
        assert_eq!(self_trust.code, 2);
        assert!(self_trust.stderr.contains("self-trust"));
        let control = trust_run_command(&trust_args(&["add", "alpha\nbeta", "gamma"]));
        assert_eq!(control.code, 2);
        assert!(control.stderr.contains("control"));
        assert!(!control.stderr.contains("alpha\nbeta"));
    }
}
