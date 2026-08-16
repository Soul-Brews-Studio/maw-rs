const DISPATCH_261: &[DispatcherEntry] = &[];

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TeamResumeManifest261 {
    members: Vec<String>,
    #[serde(default)]
    member_engines: std::collections::BTreeMap<String, String>,
    /// Launch lines for members whose engine name came from a charter
    /// `engines:` alias. Resume replays the command that launched rather than
    /// re-resolving the key, because the charter it came from is often gone by
    /// the time a team is resumed — that population is exactly what resume is
    /// for. Editing `engines:` therefore does not reach an already-spawned
    /// member; re-spawn to pick it up.
    #[serde(default)]
    member_engine_commands: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TeamLeadClaim261 {
    found: bool,
    claimed: bool,
    old_lead_session_id: Option<String>,
    new_lead_session_id: Option<String>,
    teammates: Vec<String>,
}

fn team_resume(argv: &[String]) -> Result<String, String> {
    use std::fmt::Write as _;
    let opts = team_resume_parse(argv)?;
    let paths = team_paths(&opts.name);
    let manifest_path = paths.vault_manifest.clone();
    let claim = team_claim_orphaned_lead(&opts.name)?;
    let mut out = String::new();

    if claim.claimed {
        team_push_claimed(&mut out, &opts.name, &claim);
        if !manifest_path.exists() { return Ok(out); }
        out.push('\n');
    } else if claim.found
        && !manifest_path.exists()
        && claim.old_lead_session_id.is_some()
        && claim.old_lead_session_id == claim.new_lead_session_id
    {
        team_push_already_claimed(&mut out, &opts.name, &claim);
        return Ok(out);
    }

    if !manifest_path.exists() {
        return Err(format!("no archived team '{}' found — looked in: {}", opts.name, manifest_path.display()));
    }

    let manifest: TeamResumeManifest261 = team_read_json(&manifest_path)
        .ok_or_else(|| format!("team resume: invalid manifest {}", manifest_path.display()))?;
    let members = manifest
        .members
        .iter()
        .filter(|&member| !member.is_empty())
        .cloned()
        .collect::<Vec<_>>();

    if members.is_empty() {
        let _ = writeln!(out, "\x1b[90mTeam '{}' has no members to resume.\x1b[0m", opts.name);
        return Ok(out);
    }

    // Validate every member and every recorded engine BEFORE spawning any of
    // them. `team_t5_spawn_one` writes a spawn prompt and updates the manifest
    // and tool config, so validating inside the spawn loop lets a hostile
    // value on member N leave N-1 members' worth of state on disk before it is
    // rejected — the manifest is untrusted input like argv, and gets the same
    // check-everything-first treatment `team_resume_parse` already gives argv.
    for member in &members {
        team_validate_name(member)?;
        if let Some(engine) = team_resume_member_engine(&manifest, member) {
            team_t5_safe_token(&engine, "engine")?;
        }
        if let Some(command) = team_resume_member_engine_command(&manifest, member) {
            wake_validate_command(&command, "--engine-cmd")?;
        }
    }

    let _ = writeln!(out, "\x1b[36m⏳\x1b[0m resuming team '{}' — {} agent(s)...\n", opts.name, members.len());
    for member in &members {
        let spawn = TeamT5SpawnOptions127 {
            team: opts.name.clone(),
            role: member.clone(),
            engine: team_resume_member_engine(&manifest, member),
            engine_command: team_resume_member_engine_command(&manifest, member),
            model: opts.model.clone(),
            ..Default::default()
        };
        out.push_str(&team_t5_spawn_one(&spawn)?);
        out.push('\n');
    }
    let _ = writeln!(out, "\x1b[32m✓\x1b[0m team '{}' resumed — {} agent(s) reincarnated", opts.name, members.len());
    Ok(out)
}

fn team_resume_member_engine(manifest: &TeamResumeManifest261, member: &str) -> Option<String> {
    manifest
        .member_engines
        .get(member)
        .map(String::as_str)
        .map(str::trim)
        .filter(|engine| !engine.is_empty())
        .map(str::to_owned)
}

/// The launch line recorded for a charter-alias engine, if any.
fn team_resume_member_engine_command(manifest: &TeamResumeManifest261, member: &str) -> Option<String> {
    manifest
        .member_engine_commands
        .get(member)
        .map(String::as_str)
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .map(str::to_owned)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TeamResumeOptions261 { name: String, model: Option<String> }

fn team_resume_parse(argv: &[String]) -> Result<TeamResumeOptions261, String> {
    let name = argv.get(1).ok_or_else(|| "usage: maw team resume <name> [--model <model>]".to_owned())?.clone();
    team_validate_name(&name)?;
    let mut opts = TeamResumeOptions261 { name, model: None };
    let mut index = 2;
    while index < argv.len() {
        match argv[index].as_str() {
            "--model" => {
                index += 1;
                opts.model = Some(team_resume_safe_token(team_resume_next(argv, index, "--model")?, "model")?);
            }
            value if value.starts_with('-') => return Err(format!("team resume: unknown argument {value}")),
            value => return Err(format!("team resume: unexpected argument {value}")),
        }
        index += 1;
    }
    Ok(opts)
}

fn team_resume_next(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    argv.get(index).cloned().ok_or_else(|| format!("team resume: {flag} requires a value"))
}

fn team_resume_safe_token(value: impl AsRef<str>, label: &str) -> Result<String, String> {
    let value = value.as_ref();
    if value.is_empty() { return Err(format!("team resume {label} is empty")); }
    if value.starts_with('-') { return Err(format!("invalid team resume {label} '{value}': leading dash rejected")); }
    if value.contains("..") || value.contains('/') || value.contains('\\') { return Err(format!("invalid team resume {label} '{value}': path traversal rejected")); }
    if value.chars().any(|ch| ch.is_control() || ch == '\0') { return Err(format!("invalid team resume {label}: control character rejected")); }
    Ok(value.to_owned())
}

fn team_claim_orphaned_lead(name: &str) -> Result<TeamLeadClaim261, String> {
    team_validate_name(name)?;
    let path = team_paths(name).tool_config;
    if !path.exists() { return Ok(TeamLeadClaim261::default()); }
    let Some(mut config) = team_read_json::<TeamConfig122>(&path) else { return Ok(TeamLeadClaim261::default()); };
    let old = config.lead_session_id.clone();
    let new = team_current_session_id();
    let teammates = team_teammate_names(&config);
    if old.is_none() || new.is_none() || old == new {
        return Ok(TeamLeadClaim261 { found: true, claimed: false, old_lead_session_id: old, new_lead_session_id: new, teammates });
    }
    config.lead_session_id.clone_from(&new);
    let mut value = serde_json::to_value(&config).map_err(|error| format!("team resume: encode config failed: {error}"))?;
    if let Some(object) = value.as_object_mut() {
        object.insert("leadClaimedAt".to_owned(), serde_json::json!(team_now_millis()));
    }
    team_write_json_atomic_0600(&path, &value)?;
    Ok(TeamLeadClaim261 { found: true, claimed: true, old_lead_session_id: old, new_lead_session_id: new, teammates })
}

fn team_teammate_names(config: &TeamConfig122) -> Vec<String> {
    config
        .members
        .iter()
        .filter(|member| member.agent_type.as_deref() != Some("team-lead") && member.role.as_deref() != Some("lead") && member.name != "team-lead")
        .map(|member| member.name.clone())
        .filter(|name| !name.is_empty())
        .collect()
}

fn team_short_session(id: Option<&str>) -> &str {
    id.filter(|value| !value.is_empty()).map_or("(none)", |value| value.get(..8).unwrap_or(value))
}

fn team_push_claimed(out: &mut String, name: &str, claim: &TeamLeadClaim261) {
    use std::fmt::Write as _;
    let _ = writeln!(out, "\x1b[32m✓\x1b[0m claimed orphaned team '{name}'");
    let _ = writeln!(out, "  old lead: {} (dead)", team_short_session(claim.old_lead_session_id.as_deref()));
    let _ = writeln!(out, "  new lead: {} (this session)", team_short_session(claim.new_lead_session_id.as_deref()));
    team_push_teammates(out, &claim.teammates);
}

fn team_push_already_claimed(out: &mut String, name: &str, claim: &TeamLeadClaim261) {
    use std::fmt::Write as _;
    let _ = writeln!(out, "\x1b[32m✓\x1b[0m team '{name}' already claimed by this lead session");
    team_push_teammates(out, &claim.teammates);
}

fn team_push_teammates(out: &mut String, teammates: &[String]) {
    use std::fmt::Write as _;
    if teammates.is_empty() {
        let _ = writeln!(out, "  teammates: 0");
    } else {
        let _ = writeln!(out, "  teammates: {} ({})", teammates.len(), teammates.join(", "));
    }
}

#[cfg(test)]
mod team_resume_tests261 {
    use super::*;

    fn team_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn team_resume_temp_root(name: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("maw-rs-team-resume-unit-{name}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).expect("git marker");
        root
    }

    fn with_resume_fixture<F>(name: &str, test: F)
    where
        F: FnOnce(&std::path::Path),
    {
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _maw_home = EnvVarRestore::capture("MAW_HOME");
        let _psi = EnvVarRestore::capture("MAW_RS_TEAM_PSI");
        let root = team_resume_temp_root(name);
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_HOME", root.join("maw-home"));
        std::env::set_var("MAW_RS_TEAM_PSI", root.join("psi"));
        let paths = team_paths("phoenix");
        std::fs::create_dir_all(&paths.vault_dir).expect("vault team");
        team_write_json_atomic_0600(
            &paths.vault_manifest,
            &serde_json::json!({
                "name":"phoenix",
                "createdAt":1,
                "members":["builder","reviewer"],
                "memberEngines":{"builder":"codex","reviewer":"thclaws"}
            }),
        )
        .expect("manifest");
        team_atomic_write_0600(&paths.vault_dir.join("builder-spawn-prompt.md"), "builder\nprompt\n").expect("builder prompt");
        team_atomic_write_0600(&paths.vault_dir.join("reviewer-spawn-prompt.md"), "reviewer\nprompt\n").expect("reviewer prompt");
        test(&root);
    }

    #[test]
    fn team_resume_dispatch_part_is_empty_and_parser_guards_inputs() {
        assert!(DISPATCH_261.is_empty());
        assert!(team_resume_parse(&team_strings(&["resume", "alpha", "--model", "gpt-5.5"])).is_ok());
        assert!(team_resume_parse(&team_strings(&["resume", "-bad"])).is_err());
        assert!(team_resume_parse(&team_strings(&["resume", "alpha", "--model", "--bad"])).is_err());
        assert!(team_resume_parse(&team_strings(&["resume", "alpha", "--model", "bad/model"])).is_err());
    }

    #[test]
    fn team_resume_teammates_skip_lead_members() {
        let config = TeamConfig122 { members: vec![
            TeamMember122 { name: "lead".to_owned(), role: Some("lead".to_owned()), ..Default::default() },
            TeamMember122 { name: "team-lead".to_owned(), ..Default::default() },
            TeamMember122 { name: "builder".to_owned(), ..Default::default() },
        ], ..Default::default() };
        assert_eq!(team_teammate_names(&config), vec!["builder".to_owned()]);
    }

    #[test]
    fn team_resume_uses_manifest_engines_and_preserves_existing_prompts() {
        with_resume_fixture("engines", |_| {
            let paths = team_paths("phoenix");
            let builder_prompt = paths.vault_dir.join("builder-spawn-prompt.md");
            let reviewer_prompt = paths.vault_dir.join("reviewer-spawn-prompt.md");
            let builder_before = std::fs::read_to_string(&builder_prompt).expect("builder before");
            let reviewer_before = std::fs::read_to_string(&reviewer_prompt).expect("reviewer before");

            let out = team_resume(&team_strings(&["resume", "phoenix"])).expect("resume");

            assert!(out.contains("engine: codex"), "{out}");
            assert!(out.contains("wake builder --no-attach --session phoenix -e codex"), "{out}");
            assert!(out.contains("engine: thclaws"), "{out}");
            assert!(out.contains("wake reviewer --no-attach --session phoenix -e thclaws"), "{out}");
            assert_eq!(std::fs::read_to_string(&builder_prompt).expect("builder after"), builder_before);
            assert_eq!(std::fs::read_to_string(&reviewer_prompt).expect("reviewer after"), reviewer_before);
        });
    }

    /// The manifest is untrusted input, so every member's engine is validated
    /// before ANY member is spawned.
    ///
    /// The hostile value sits on the SECOND member deliberately. On the first
    /// member the guard's position is unobservable — the run aborts before
    /// anything is written either way — so a test that puts it there passes
    /// against both the broken and the fixed code. With it second, validating
    /// inside the spawn loop leaves builder's spawn prompt rewritten on disk
    /// before reviewer is rejected.
    #[test]
    fn team_resume_validates_every_member_engine_before_spawning_any() {
        with_resume_fixture("hostile-second", |_| {
            let paths = team_paths("phoenix");
            // `newcomer` has NO spawn prompt on disk, so spawning it creates
            // one — an observable side effect. Asserting on a member whose
            // prompt already exists proves nothing: `team_t5_spawn_one` skips
            // rewriting an existing prompt, so that file is byte-identical
            // whether or not the member was processed.
            team_write_json_atomic_0600(
                &paths.vault_manifest,
                &serde_json::json!({
                    "name":"phoenix",
                    "createdAt":1,
                    "members":["newcomer","reviewer"],
                    "memberEngines":{"newcomer":"codex","reviewer":"../../evil"}
                }),
            )
            .expect("hostile manifest");
            let newcomer_prompt = paths.vault_dir.join("newcomer-spawn-prompt.md");
            assert!(!newcomer_prompt.exists(), "fixture must start without newcomer's prompt");

            let error = team_resume(&team_strings(&["resume", "phoenix"])).expect_err("must reject");

            assert!(error.contains("path traversal"), "{error}");
            assert!(
                !newcomer_prompt.exists(),
                "an earlier member was spawned before a later member's engine was rejected"
            );
        });
    }

    /// A charter `engines:` alias records a launch line, and resume replays it
    /// via `--engine-cmd` rather than passing the bare alias key to `-e`,
    /// which the resolver ladder cannot resolve (#738/#758).
    #[test]
    fn team_resume_replays_a_charter_alias_as_engine_cmd() {
        with_resume_fixture("alias", |_| {
            let paths = team_paths("phoenix");
            team_write_json_atomic_0600(
                &paths.vault_manifest,
                &serde_json::json!({
                    "name":"phoenix",
                    "createdAt":1,
                    "members":["builder","reviewer"],
                    "memberEngines":{"builder":"omx-1","reviewer":"codex"},
                    "memberEngineCommands":{"builder":"CODEX_HOME=/tmp/.codex omx --direct"}
                }),
            )
            .expect("alias manifest");

            let out = team_resume(&team_strings(&["resume", "phoenix"])).expect("resume");

            assert!(
                out.contains("-e omx-1 --engine-cmd 'CODEX_HOME=/tmp/.codex omx --direct'"),
                "the alias must carry its resolved command: {out}"
            );
            // A plain engine name resolves on its own and must not gain a flag.
            assert!(out.contains("wake reviewer --no-attach --session phoenix -e codex\n"), "{out}");
        });
    }
}
