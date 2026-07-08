// maw oracle recruit + maw fleet join — pair-code recruit flow (#294).
// Recruit mints a pair-code + local consent request (action fleet-recruit) and plans delivery
// over the existing signed send path; join verifies code + approved consent, then appends to
// the #291 roster. Plan-json/dry-run are first-class so the flow tests without live peers.

const ORACLE_RECRUIT_USAGE: &str =
    "usage: maw oracle recruit <fleet> <oracle> [--pin <pin>] [--from <handle>] [--now <ms>] [--dry-run] [--plan-json]";
const FLEET_JOIN_USAGE: &str = "usage: maw fleet join <fleet> --code <code>";

const DISPATCH_332: &[DispatcherEntry] =
    &[DispatcherEntry { command: "oracle-recruit", handler: Handler::Sync(run_oracle_recruit_command) }];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecruitInvite {
    fleet: String,
    oracle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    org_repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    node: Option<String>,
    code: String,
    request_id: String,
    created_at: String,
    expires_at: String,
    status: String,
}

fn run_oracle_recruit_command(argv: &[String]) -> CliOutput {
    match oracle_recruit_run(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

#[derive(Default)]
struct RecruitOptions {
    pin: Option<String>,
    from: Option<String>,
    now_ms: Option<i64>,
    dry_run: bool,
    plan_json: bool,
}

fn oracle_recruit_parse(argv: &[String]) -> Result<(String, String, RecruitOptions), String> {
    let mut options = RecruitOptions::default();
    let mut positional = Vec::new();
    let mut index = 0;
    while index < argv.len() {
        let take_value = |index: usize| {
            argv.get(index + 1).cloned().ok_or_else(|| format!("oracle recruit: missing {} value\n{ORACLE_RECRUIT_USAGE}", argv[index]))
        };
        match argv[index].as_str() {
            "--dry-run" => options.dry_run = true,
            "--plan-json" => options.plan_json = true,
            "--pin" => { options.pin = Some(take_value(index)?); index += 1; }
            "--from" => { options.from = Some(take_value(index)?); index += 1; }
            "--now" => {
                options.now_ms = Some(take_value(index)?.parse().map_err(|_| "oracle recruit: --now must be unix millis".to_owned())?);
                index += 1;
            }
            value if value.starts_with('-') => return Err(format!("oracle recruit: unknown argument {value}\n{ORACLE_RECRUIT_USAGE}")),
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }
    let (Some(fleet), Some(oracle), None) = (positional.first(), positional.get(1), positional.get(2)) else {
        return Err(ORACLE_RECRUIT_USAGE.to_owned());
    };
    Ok((fleet.clone(), oracle.clone(), options))
}

#[allow(clippy::too_many_lines)]
fn oracle_recruit_run(argv: &[String]) -> Result<String, String> {
    let (fleet, oracle, options) = oracle_recruit_parse(argv)?;
    fleet_validate_session_name(&fleet)?;
    let env = current_xdg_env();
    let entries = fleet_load_entries_result_for_env(&env, "oracle recruit")?;
    if !entries.iter().any(|entry| fleet_roster_entry_matches(entry, &fleet)) {
        return Err(format!("oracle recruit: no fleet named {fleet} — try: maw fleet create {fleet}"));
    }
    let cached = locate_load_registry_cache()
        .and_then(|cache| cache.oracles.into_iter().find(|entry| entry.name == oracle))
        .ok_or_else(|| format!("oracle recruit: {oracle} not found in oracles.json cache — run maw oracle scan"))?;
    let org_repo = Some(format!("{}/{}", cached.org, cached.repo));
    let node = cached.federation_node.clone();
    let now_ms = match options.now_ms { Some(value) => value, None => consent_now_ms_135()? };
    let code = maw_auth::normalize_pair_code(&team_invite_generate_pin());
    let pin = options.pin.map_or_else(team_invite_generate_pin, |pin| maw_auth::normalize_pair_code(&pin));
    let request_id = team_invite_generate_request_id();
    let from = options.from.unwrap_or_else(|| "lead".to_owned());

    let mut store = maw_auth::ConsentStore::default();
    let result = maw_auth::request_consent_plan(&mut store, maw_auth::ConsentRequestArgs {
        from: from.clone(),
        to: oracle.clone(),
        action: maw_auth::ConsentAction::FleetRecruit,
        summary: format!("recruit {oracle} into fleet {fleet}"),
        peer_url: None,
        request_id: request_id.clone(),
        pin: pin.clone(),
        now_ms,
        peer_post: maw_auth::PeerPostResult::Skipped,
    });
    if !result.ok {
        return Err(format!("oracle recruit: {}", result.error.unwrap_or_else(|| "consent request failed".to_owned())));
    }
    let pending = store.read_pending(&request_id).ok_or_else(|| "oracle recruit: missing pending request".to_owned())?;
    let invite = RecruitInvite {
        fleet: fleet.clone(),
        oracle: oracle.clone(),
        org_repo,
        node: node.clone(),
        code: code.clone(),
        request_id: request_id.clone(),
        created_at: pending.created_at.clone(),
        expires_at: pending.expires_at.clone(),
        status: "open".to_owned(),
    };
    if !options.dry_run {
        recruit_write_invite(&env, &invite)?;
        recruit_write_pending(&pending)?;
    }
    let deliver_target = format!("{}:{oracle}", node.as_deref().unwrap_or("local"));
    let deliver_message = format!("maw fleet join {fleet} --code {code}");
    if options.plan_json {
        let value = serde_json::json!({
            "command": "oracle-recruit",
            "fleet": fleet,
            "oracle": oracle,
            "orgRepo": invite.org_repo,
            "node": invite.node,
            "code": code,
            "pinRedacted": maw_auth::redact_pair_code(&pin),
            "requestId": request_id,
            "action": maw_auth::ConsentAction::FleetRecruit.as_str(),
            "expiresAt": invite.expires_at,
            "dryRun": options.dry_run,
            "deliver": { "via": "maw hey", "target": deliver_target, "message": deliver_message },
        });
        return serde_json::to_string_pretty(&value).map(|text| format!("{text}\n")).map_err(|error| error.to_string());
    }
    let mut out = format!("oracle recruit {oracle} → fleet {fleet}\n");
    let _ = writeln!(out, "  code:    {code} (expires {})", invite.expires_at);
    let _ = writeln!(out, "  consent: requestId={request_id} action=fleet-recruit pin={pin}");
    let _ = writeln!(out, "  deliver: maw hey {deliver_target} {deliver_message:?}");
    if options.dry_run { out.push_str("  dry-run: nothing written\n"); }
    Ok(out)
}

fn recruit_invites_dir(env: &MawXdgEnv) -> std::path::PathBuf { maw_state_path(env, &["fleet-invites"]) }

fn recruit_write_invite(env: &MawXdgEnv, invite: &RecruitInvite) -> Result<(), String> {
    let dir = recruit_invites_dir(env);
    std::fs::create_dir_all(&dir).map_err(|error| format!("oracle recruit: create {}: {error}", dir.display()))?;
    let path = dir.join(format!("{}.json", invite.code));
    let body = serde_json::to_string_pretty(invite).map_err(|error| error.to_string())?;
    std::fs::write(&path, format!("{body}\n")).map_err(|error| format!("oracle recruit: write {}: {error}", path.display()))
}

fn recruit_write_pending(pending: &maw_auth::PendingRequest) -> Result<(), String> {
    let dir = consent_pending_dirs_135().into_iter().next().ok_or_else(|| "oracle recruit: no consent-pending dir".to_owned())?;
    std::fs::create_dir_all(&dir).map_err(|error| format!("oracle recruit: create {}: {error}", dir.display()))?;
    let value = serde_json::json!({
        "id": pending.id, "from": pending.from, "to": pending.to,
        "action": pending.action.as_str(), "summary": pending.summary, "pinHash": pending.pin_hash,
        "createdAt": pending.created_at, "expiresAt": pending.expires_at, "status": "pending",
    });
    let path = dir.join(format!("{}.json", pending.id));
    let body = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    std::fs::write(&path, format!("{body}\n")).map_err(|error| format!("oracle recruit: write {}: {error}", path.display()))
}

fn fleet_join_run(argv: &[String]) -> Result<(i32, String), String> {
    let mut code = None::<String>;
    let mut positional = Vec::new();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--code" => {
                code = Some(argv.get(index + 1).ok_or_else(|| format!("fleet join: missing --code value\n{FLEET_JOIN_USAGE}"))?.clone());
                index += 1;
            }
            value if value.starts_with('-') => return Err(format!("fleet join: unknown argument {value}\n{FLEET_JOIN_USAGE}")),
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }
    let (Some(sub), Some(fleet), None) = (positional.first(), positional.get(1), positional.get(2)) else {
        return Err(FLEET_JOIN_USAGE.to_owned());
    };
    debug_assert_eq!(sub.as_str(), "join");
    let code = maw_auth::normalize_pair_code(&code.ok_or_else(|| FLEET_JOIN_USAGE.to_owned())?);
    let env = current_xdg_env();
    let path = recruit_invites_dir(&env).join(format!("{code}.json"));
    let text = std::fs::read_to_string(&path).map_err(|_| format!("fleet join: unknown code {}", maw_auth::redact_pair_code(&code)))?;
    let mut invite: RecruitInvite = serde_json::from_str(&text).map_err(|error| format!("fleet join: parse {}: {error}", path.display()))?;
    if invite.fleet != *fleet {
        return Err(format!("fleet join: code is for fleet {}, not {fleet}", invite.fleet));
    }
    if invite.status != "open" {
        return Err(format!("fleet join: code already {}", invite.status));
    }
    let now_ms = u64::try_from(consent_now_ms_135()?).map_err(|_| "fleet join: clock before epoch".to_owned())?;
    let expires_ms = consent_parse_iso_millis_135(&invite.expires_at).ok_or_else(|| "fleet join: invite has invalid expiresAt".to_owned())?;
    if now_ms > expires_ms {
        return Err(format!("fleet join: code expired at {}", invite.expires_at));
    }
    let consent = consent_read_pending_135().into_iter().find(|row| row.id == invite.request_id);
    match consent.as_ref().map(|row| row.status.as_str()) {
        Some("approved") => {}
        Some(status) => return Err(format!("fleet join: consent {} is {status} — run maw consent approve {} <pin>", invite.request_id, invite.request_id)),
        None => return Err(format!("fleet join: consent request {} not found", invite.request_id)),
    }
    let member_count = fleet_join_append_member(&env, fleet, &invite)?;
    "consumed".clone_into(&mut invite.status);
    let body = serde_json::to_string_pretty(&invite).map_err(|error| error.to_string())?;
    std::fs::write(&path, format!("{body}\n")).map_err(|error| format!("fleet join: write {}: {error}", path.display()))?;
    Ok((0, format!("fleet join {fleet}: {} joined ({member_count} members)\n", invite.oracle)))
}

fn fleet_join_append_member(env: &MawXdgEnv, fleet: &str, invite: &RecruitInvite) -> Result<usize, String> {
    let entries = fleet_load_entries_result_for_env(env, "fleet join")?;
    let entry = entries.into_iter().find(|entry| fleet_roster_entry_matches(entry, fleet))
        .ok_or_else(|| format!("fleet join: no fleet named {fleet}"))?;
    if entry.session.members.as_ref().is_some_and(|members| members.iter().any(|member| member.handle == invite.oracle)) {
        return Err(format!("fleet join: {} is already a member of {fleet}", invite.oracle));
    }
    let text = std::fs::read_to_string(&entry.path).map_err(|error| format!("fleet join: read {}: {error}", entry.path.display()))?;
    let mut value: serde_json::Value = serde_json::from_str(&text).map_err(|error| format!("fleet join: parse {}: {error}", entry.path.display()))?;
    let member = NativeFleetMember {
        handle: invite.oracle.clone(),
        org_repo: invite.org_repo.clone(),
        node: invite.node.clone(),
        role: None,
        joined_at: Some(fleet_registry_now_iso()),
    };
    let member = serde_json::to_value(&member).map_err(|error| error.to_string())?;
    let object = value.as_object_mut().ok_or_else(|| "fleet join: fleet file is not a JSON object".to_owned())?;
    let members = object.entry("members".to_owned()).or_insert_with(|| serde_json::json!([]));
    let members = members.as_array_mut().ok_or_else(|| "fleet join: members is not a JSON array".to_owned())?;
    members.push(member);
    let count = members.len();
    let body = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    std::fs::write(&entry.path, format!("{body}\n")).map_err(|error| format!("fleet join: write {}: {error}", entry.path.display()))?;
    Ok(count)
}

#[cfg(test)]
mod oracle_recruit_tests {
    use super::*;

    fn recruit_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn recruit_env(name: &str) -> (std::path::PathBuf, Vec<EnvVarRestore>) {
        let root = std::env::temp_dir().join(format!("maw-rs-oracle-recruit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        let guards: Vec<EnvVarRestore> = ["HOME", "MAW_HOME", "MAW_CONFIG_DIR", "MAW_STATE_DIR", "MAW_CACHE_DIR", "GHQ_ROOT", "CONSENT_PENDING_DIR", "CONSENT_TRUST_FILE"]
            .map(EnvVarRestore::capture).into_iter().collect();
        for key in ["MAW_HOME", "CONSENT_PENDING_DIR", "CONSENT_TRUST_FILE"] { std::env::remove_var(key); }
        for (key, dir) in [("HOME", "home"), ("MAW_CONFIG_DIR", "config"), ("MAW_STATE_DIR", "state"), ("MAW_CACHE_DIR", "cache"), ("GHQ_ROOT", "ghq")] {
            std::env::set_var(key, root.join(dir));
        }
        std::fs::create_dir_all(root.join("cache")).expect("cache dir");
        let cache_json = r#"{"schema":1,"oracles":[{"org":"acme","repo":"fireman-oracle","name":"fireman","local_path":"/tmp/fireman","has_psi":true,"has_fleet_config":true,"federation_node":"white"}]}"#;
        std::fs::write(root.join("cache/oracles.json"), cache_json).expect("cache file");
        assert_eq!(run_fleet_command(&recruit_args(&["create", "3e"])).code, 0);
        (root, guards)
    }

    fn recruit_plan(extra: &[&str]) -> serde_json::Value {
        let mut args = recruit_args(&["3e", "fireman", "--pin", "ABC234", "--from", "neo", "--plan-json"]);
        args.extend(extra.iter().map(|value| (*value).to_owned()));
        let output = run_oracle_recruit_command(&args);
        assert_eq!(output.code, 0, "{}", output.stderr);
        serde_json::from_str(&output.stdout).expect("plan json")
    }

    #[test]
    fn oracle_recruit_dry_run_plan_shape_and_no_writes() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (root, _env) = recruit_env("dry-run");
        let plan = recruit_plan(&["--dry-run", "--now", "1751900000000"]);
        assert_eq!(plan["command"], "oracle-recruit");
        assert_eq!(plan["fleet"], "3e");
        assert_eq!(plan["oracle"], "fireman");
        assert_eq!(plan["orgRepo"], "acme/fireman-oracle");
        assert_eq!(plan["node"], "white", "resolved via oracles.json cache");
        assert_eq!(plan["action"], "fleet-recruit");
        assert_eq!(plan["pinRedacted"], "ABC-***");
        assert_eq!(plan["dryRun"], true);
        let code = plan["code"].as_str().expect("code");
        assert_eq!(code.len(), 6);
        assert_eq!(plan["deliver"]["via"], "maw hey");
        assert_eq!(plan["deliver"]["target"], "white:fireman");
        assert_eq!(plan["deliver"]["message"], format!("maw fleet join 3e --code {code}"));
        assert!(!root.join("state/fleet-invites").exists(), "dry-run writes nothing");
        let missing = run_fleet_command(&recruit_args(&["join", "3e", "--code", code]));
        assert_eq!(missing.code, 1);
        assert!(missing.stderr.contains("unknown code"), "{}", missing.stderr);
    }

    #[test]
    fn fleet_join_round_trip_appends_member_and_consumes_code() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (root, _env) = recruit_env("round-trip");
        let plan = recruit_plan(&[]);
        let code = plan["code"].as_str().expect("code").to_owned();
        let request_id = plan["requestId"].as_str().expect("request id").to_owned();
        assert!(root.join(format!("state/fleet-invites/{code}.json")).exists());
        let unapproved = run_fleet_command(&recruit_args(&["join", "3e", "--code", &code]));
        assert_eq!(unapproved.code, 1);
        assert!(unapproved.stderr.contains("is pending"), "{}", unapproved.stderr);
        let approved = run_consent_command_135(&recruit_args(&["approve", &request_id, "ABC234"]));
        assert_eq!(approved.code, 0, "{}", approved.stderr);
        let joined = run_fleet_command(&recruit_args(&["join", "3e", "--code", &code]));
        assert_eq!(joined.code, 0, "{}", joined.stderr);
        assert!(joined.stdout.contains("fireman joined (1 members)"), "{}", joined.stdout);
        let shown: serde_json::Value =
            serde_json::from_str(&run_fleet_command(&recruit_args(&["show", "3e", "--json"])).stdout).expect("roster json");
        assert_eq!(shown["memberCount"], 1);
        assert_eq!(shown["members"][0]["handle"], "fireman");
        assert_eq!(shown["members"][0]["org_repo"], "acme/fireman-oracle");
        assert_eq!(shown["members"][0]["node"], "white");
        let replay = run_fleet_command(&recruit_args(&["join", "3e", "--code", &code]));
        assert_eq!(replay.code, 1);
        assert!(replay.stderr.contains("already consumed"), "{}", replay.stderr);
    }

    #[test]
    fn fleet_join_rejects_expired_code() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (_root, _env) = recruit_env("expired");
        let plan = recruit_plan(&["--now", "1000"]);
        let code = plan["code"].as_str().expect("code");
        let expired = run_fleet_command(&recruit_args(&["join", "3e", "--code", code]));
        assert_eq!(expired.code, 1);
        assert!(expired.stderr.contains("code expired"), "{}", expired.stderr);
    }
}
