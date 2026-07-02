const DISPATCH_261: &[DispatcherEntry] = &[];

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct TeamResumeManifest261 { members: Vec<String> }

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
        .into_iter()
        .filter(|member| !member.is_empty())
        .collect::<Vec<_>>();

    if members.is_empty() {
        let _ = writeln!(out, "\x1b[90mTeam '{}' has no members to resume.\x1b[0m", opts.name);
        return Ok(out);
    }

    let _ = writeln!(out, "\x1b[36m⏳\x1b[0m resuming team '{}' — {} agent(s)...\n", opts.name, members.len());
    for member in &members {
        team_validate_name(member)?;
        let spawn = TeamT5SpawnOptions127 {
            team: opts.name.clone(),
            role: member.clone(),
            model: opts.model.clone(),
            ..Default::default()
        };
        out.push_str(&team_t5_spawn_one(&spawn)?);
        out.push('\n');
    }
    let _ = writeln!(out, "\x1b[32m✓\x1b[0m team '{}' resumed — {} agent(s) reincarnated", opts.name, members.len());
    Ok(out)
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
}
