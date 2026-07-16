// x.rs — mawx WI-8: `maw x <spec>` orchestration + housekeeping subcommands
// (ψ/design/mawx-spec.md §2.1 CLI surface, §2.2 resolution, §2.5 trust, §6
// Nat's locked decisions). The `mawx` argv[0] shim lives in `main.rs`.
//
// Composition, not new policy: this file wires the four finished mawx layers
// — `x_spec` (parse), `x_source` (resolve+fetch), `x_trust` (TOFU /
// default-registry auto-trust), `x_cache` (CAS) — around the SAME extism
// invoke path installed ship-tier plugins use (`ship_tier_wasm_runtime()` +
// `invoke_plugin`), so a no-`wasm-host` build fails loudly with the existing
// rebuild hint (invariant I7) while resolution/trust/cache/`--dry-run` stay
// host-independent.
//
// Flow for `maw x <spec>`:
//   native guard → local shadow (`--remote` bypasses) → parse → resolve+fetch
//   (`--dry-run` prints the plan JSON) → trust decision (default-registry pins
//   auto-trust per Nat's decision 2; TOFU card on stderr otherwise; non-TTY
//   unapproved → exit 3 WITH the observed `--sha256`; `--yes` refused on fully
//   unpinned sources) → CAS verify-on-read → execute wasm → propagate exit.
//
// Exit codes (spec §2.7): 0 ok · 1 plugin/verify refusal · 2 usage/unknown
// verb/native guard · 3 trust declined or non-TTY unapproved · 4 `--offline`
// cache miss · the plugin's own exit code passes through on success.

const DISPATCH_335: &[DispatcherEntry] =
    &[DispatcherEntry { command: "x", handler: Handler::Sync(run_x_command) }];

const X_USAGE: &str = "usage: maw x <spec> [--sha256 <hex>] [-y|--yes] [--offline|--frozen] [--reload]
             [--from <spec>] [--registry <owner/repo>] [--remote]
             [--install|--keep] [--dry-run] [--] [plugin-args...]
       maw x ls
       maw x gc [--max-age <30d|12h|45m|secs>] [--max-size <2g|500m|8k|bytes>] [--dry-run]
       maw x rm <verb|artifact|sha256-prefix>
       maw x trust ls
       maw x trust revoke <source|sha256-prefix>";

/// Trust declined, or a non-interactive run without an approval path.
const X_EXIT_TRUST: i32 = 3;
/// `--offline` and the source is not in the CAS.
const X_EXIT_OFFLINE_MISS: i32 = 4;

/// Parsed `maw x` invocation: the run form or a housekeeping subcommand.
#[derive(Debug, Clone, PartialEq, Eq)]
enum XCliCommand {
    Help,
    Run(Box<XRunArgs>),
    CacheLs,
    CacheGc { max_age_secs: Option<u64>, max_size_bytes: Option<u64>, dry_run: bool },
    CacheRm { needle: String },
    TrustLs,
    TrustRevoke { selector: String },
}

/// Flags for the run form (spec §2.1 flag table).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct XRunArgs {
    spec: String,
    sha256: Option<String>,
    from: Option<String>,
    registry: Option<String>,
    yes: bool,
    offline: bool,
    reload: bool,
    remote: bool,
    install: bool,
    dry_run: bool,
    plugin_args: Vec<String>,
}

/// IO context for one `maw x` run — paths and clock threaded explicitly so
/// tests run against temp roots, plus the injectable TOFU prompt edge.
struct XRunEnv<'a> {
    cache_root: std::path::PathBuf,
    trust_store: std::path::PathBuf,
    plugin_root: std::path::PathBuf,
    interactive: bool,
    now: u64,
    now_ms: i64,
    /// TOFU prompt edge: card text in, answer line out (`None` = EOF/deny).
    prompt: &'a mut dyn FnMut(&str) -> Option<String>,
}

fn run_x_command(argv: &[String]) -> CliOutput {
    let command = match parse_x_cli(argv) {
        Ok(command) => command,
        Err(message) => return x_usage_error(&message),
    };
    let (now, now_ms) = x_now_secs_ms();
    let mut prompt = x_tty_prompt;
    let mut run = XRunEnv {
        cache_root: x_cache_root(&real_xdg_env()),
        trust_store: x_trust_store_path(),
        plugin_root: resolve_default_plugin_root(),
        interactive: std::io::IsTerminal::is_terminal(&std::io::stdin()),
        now,
        now_ms,
        prompt: &mut prompt,
    };
    run_x_parsed(&command, &mut run)
}

fn run_x_parsed(command: &XCliCommand, run: &mut XRunEnv<'_>) -> CliOutput {
    match command {
        XCliCommand::Help => {
            CliOutput { code: 0, stdout: format!("{X_USAGE}\n"), stderr: String::new() }
        }
        XCliCommand::Run(args) => run_x_run(args, run),
        XCliCommand::CacheLs => run_x_cache_ls(&run.cache_root),
        XCliCommand::CacheGc { max_age_secs, max_size_bytes, dry_run } => run_x_cache_gc(
            &run.cache_root,
            run.now,
            &XCacheGcOptions {
                max_age_secs: *max_age_secs,
                max_size_bytes: *max_size_bytes,
                ..XCacheGcOptions::default()
            },
            *dry_run,
        ),
        XCliCommand::CacheRm { needle } => run_x_cache_rm(&run.cache_root, needle),
        XCliCommand::TrustLs => run_x_trust_ls(&run.trust_store),
        XCliCommand::TrustRevoke { selector } => run_x_trust_revoke(&run.trust_store, selector),
    }
}

// ─── argv parsing ────────────────────────────────────────────────────────

fn parse_x_cli(argv: &[String]) -> Result<XCliCommand, String> {
    let Some(first) = argv.first().map(String::as_str) else {
        return Err("x: missing <spec>".to_owned());
    };
    match first {
        "--help" | "-h" | "help" => Ok(XCliCommand::Help),
        "ls" => {
            if argv.len() > 1 {
                return Err(format!("x ls: unexpected argument {}", argv[1]));
            }
            Ok(XCliCommand::CacheLs)
        }
        "gc" => parse_x_gc(&argv[1..]),
        "rm" => match (argv.get(1), argv.len()) {
            (Some(needle), 2) => Ok(XCliCommand::CacheRm { needle: needle.clone() }),
            _ => Err("x rm: needs exactly one <verb|artifact|sha256-prefix>".to_owned()),
        },
        "trust" => parse_x_trust(&argv[1..]),
        _ => parse_x_run(argv),
    }
}

fn parse_x_run(argv: &[String]) -> Result<XCliCommand, String> {
    let mut parsed = XRunArgs::default();
    let mut spec: Option<String> = None;
    let mut index = 0;
    while index < argv.len() {
        let arg = argv[index].as_str();
        match arg {
            "--" => {
                parsed.plugin_args.extend_from_slice(&argv[index + 1..]);
                break;
            }
            "--sha256" => {
                parsed.sha256 = Some(x_take_value(argv, index, "--sha256")?);
                index += 1;
            }
            "--from" => {
                parsed.from = Some(x_take_value(argv, index, "--from")?);
                index += 1;
            }
            "--registry" => {
                parsed.registry = Some(x_take_value(argv, index, "--registry")?);
                index += 1;
            }
            "-y" | "--yes" => parsed.yes = true,
            "--offline" | "--frozen" | "--cached-only" => parsed.offline = true,
            "--reload" => parsed.reload = true,
            "--remote" => parsed.remote = true,
            "--install" | "--keep" => parsed.install = true,
            "--dry-run" => parsed.dry_run = true,
            _ if arg.starts_with("--sha256=") => {
                parsed.sha256 = Some(arg["--sha256=".len()..].to_owned());
            }
            _ if arg.starts_with("--from=") => {
                parsed.from = Some(arg["--from=".len()..].to_owned());
            }
            _ if arg.starts_with("--registry=") => {
                parsed.registry = Some(arg["--registry=".len()..].to_owned());
            }
            _ if spec.is_none() => {
                if arg.starts_with('-') {
                    return Err(format!("x: unknown flag {arg}"));
                }
                spec = Some(arg.to_owned());
            }
            _ => {
                // First token after the spec that is not an x flag: everything
                // from here on belongs to the plugin verbatim (npx-style).
                parsed.plugin_args.extend_from_slice(&argv[index..]);
                break;
            }
        }
        index += 1;
    }
    let Some(spec) = spec else {
        return Err("x: missing <spec>".to_owned());
    };
    parsed.spec = spec;
    Ok(XCliCommand::Run(Box::new(parsed)))
}

fn parse_x_gc(rest: &[String]) -> Result<XCliCommand, String> {
    let mut max_age_secs = None;
    let mut max_size_bytes = None;
    let mut dry_run = false;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--dry-run" => dry_run = true,
            "--max-age" => {
                max_age_secs =
                    Some(parse_x_gc_duration_secs(&x_take_value(rest, index, "--max-age")?)?);
                index += 1;
            }
            "--max-size" => {
                max_size_bytes =
                    Some(parse_x_gc_size_bytes(&x_take_value(rest, index, "--max-size")?)?);
                index += 1;
            }
            other => return Err(format!("x gc: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(XCliCommand::CacheGc { max_age_secs, max_size_bytes, dry_run })
}

fn parse_x_trust(rest: &[String]) -> Result<XCliCommand, String> {
    match (rest.first().map(String::as_str), rest.len()) {
        (Some("ls"), 1) => Ok(XCliCommand::TrustLs),
        (Some("revoke"), 2) => Ok(XCliCommand::TrustRevoke { selector: rest[1].clone() }),
        _ => Err(
            "x trust: usage — maw x trust ls | maw x trust revoke <source|sha256-prefix>"
                .to_owned(),
        ),
    }
}

fn x_take_value(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    argv.get(index + 1)
        .cloned()
        .ok_or_else(|| format!("x: missing value for {flag}"))
}

/// Parse a gc age: `30d`, `12h`, `45m`, `10s`, or plain seconds.
fn parse_x_gc_duration_secs(value: &str) -> Result<u64, String> {
    x_parse_suffixed(value, &[('d', 86_400), ('h', 3_600), ('m', 60), ('s', 1)]).ok_or_else(
        || format!("x gc: --max-age must be like 30d, 12h, 45m, or seconds, got '{value}'"),
    )
}

/// Parse a gc size: `2g`, `500m`, `8k`, or plain bytes.
fn parse_x_gc_size_bytes(value: &str) -> Result<u64, String> {
    x_parse_suffixed(value, &[('g', 1 << 30), ('m', 1 << 20), ('k', 1 << 10)]).ok_or_else(
        || format!("x gc: --max-size must be like 2g, 500m, 8k, or bytes, got '{value}'"),
    )
}

/// Shared suffixed-integer parser; suffixes compare case-insensitively.
fn x_parse_suffixed(value: &str, suffixes: &[(char, u64)]) -> Option<u64> {
    let lower = value.trim().to_ascii_lowercase();
    let (digits, factor) = lower
        .chars()
        .last()
        .and_then(|last| {
            suffixes
                .iter()
                .find(|(suffix, _)| *suffix == last)
                .map(|(_, factor)| (&lower[..lower.len() - 1], *factor))
        })
        .unwrap_or((lower.as_str(), 1));
    digits.parse::<u64>().ok().map(|amount| amount.saturating_mul(factor))
}

// ─── the run form ────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn run_x_run(args: &XRunArgs, run: &mut XRunEnv<'_>) -> CliOutput {
    if args.reload && args.offline {
        return x_usage_error(
            "x: --reload conflicts with --offline (offline never touches the network)",
        );
    }
    let spec = match parse_x_spec(&args.spec) {
        Ok(spec) => spec,
        Err(message) => return x_usage_error(&message),
    };
    let explicit_sha = match x_explicit_sha256(args.sha256.as_deref(), spec.sha256.as_deref()) {
        Ok(value) => value,
        Err(message) => return x_usage_error(&message),
    };

    // Native guard + local shadow apply to bare verbs only (spec §2.2 steps
    // 1-2): scheme'd/direct specs skip both.
    let mut requested_verb: Option<String> = None;
    if let XSource::Verb { verb, .. } = &spec.source {
        requested_verb = Some(verb.clone());
        if dispatcher_status(verb) == DispatchKind::Native {
            return x_usage_error(&format!(
                "x: '{verb}' is a native maw command — refusing to shadow it; maw x runs plugins only"
            ));
        }
        if !args.remote {
            // `--dry-run` short-circuits BEFORE the shadow dispatch (#576):
            // report the installed copy that would run, never execute it.
            if args.dry_run {
                if let Some(plan) = x_installed_shadow_plan(verb, &args.plugin_args) {
                    return x_render_plan_json(&plan);
                }
            } else if let Some(output) = x_dispatch_installed_shadow(verb, &args.plugin_args) {
                return output;
            }
        }
    }

    // `--from` disambiguates verb ≠ package/dir name: the positional stays
    // the verb, the fetch spec comes from `--from` (spec §2.1).
    let fetch_spec = if let Some(from) = &args.from {
        if requested_verb.is_none() {
            return x_usage_error(
                "x: --from requires a bare verb positional (e.g. maw x costs --from gh:owner/repo/packages/20-costs)",
            );
        }
        match parse_x_spec(from) {
            Ok(from_spec) => from_spec,
            Err(message) => return x_usage_error(&message),
        }
    } else {
        spec.clone()
    };

    if args.offline {
        return run_x_offline(
            args,
            &fetch_spec,
            explicit_sha.as_deref(),
            requested_verb.as_deref(),
            run,
        );
    }

    let (registry_owner, registry_repo) = match x_registry_override(args.registry.as_deref()) {
        Ok(pair) => pair,
        Err(message) => return x_usage_error(&message),
    };
    let is_default_registry =
        registry_owner == X_DEFAULT_REGISTRY_OWNER && registry_repo == X_DEFAULT_REGISTRY_REPO;
    let cache_root = run.cache_root.clone();
    let options = XFetchOptions {
        cache_root: &cache_root,
        timeout_secs: X_FETCH_TIMEOUT_SECS,
        now: run.now,
    };
    let resolved = match resolve_x_spec(&fetch_spec, &registry_owner, &registry_repo, &options) {
        Ok(resolved) => resolved,
        Err(message) => return x_resolve_error(&message),
    };
    let registry_pin =
        is_default_registry && matches!(&resolved.plan, XFetchPlan::Raw { registry: Some(_), .. });
    // Raw plans fetch at an immutable commit whose manifest pin is committed
    // — an accountable pin. Clone plans at a named ref/HEAD are unpinned
    // unless the caller passed `--sha256` (I4).
    let pinned = explicit_sha.is_some() || matches!(&resolved.plan, XFetchPlan::Raw { .. });

    if args.dry_run {
        return x_render_plan_json(&x_resolution_plan(&resolved));
    }

    let fetched = match execute_x_fetch(&resolved, explicit_sha.as_deref(), &options) {
        Ok(fetched) => fetched,
        Err(message) => return x_error(1, &message),
    };
    let manifest_info = read_raw_plugin_install_manifest(&fetched.package_dir)
        .ok()
        .flatten()
        .and_then(|raw| x_manifest_fetch_info(&raw).ok());
    if let (Some(requested), Some(info), true) =
        (requested_verb.as_deref(), manifest_info.as_ref(), args.from.is_some())
    {
        if info.verb != requested {
            return x_usage_error(&format!(
                "x: --from package provides verb '{}' but '{requested}' was requested",
                info.verb
            ));
        }
    }

    // Trust gate — skipped for the `file:` local dev route (spec §2.3: the
    // user's own directory, verified in place by the shared fork; same trust
    // posture as `maw plugin install <dir>`).
    let local = matches!(&resolved.plan, XFetchPlan::Local { .. });
    if !local {
        let Some(artifact_sha256) = fetched.resolution.sha256.clone() else {
            return x_error(
                1,
                "x: fetched package carries no verified artifact sha256 — refusing to run",
            );
        };
        let capabilities = fetched.resolution.capabilities.clone().unwrap_or_default();
        let verb_label = manifest_info.as_ref().map_or_else(
            || requested_verb.clone().unwrap_or_else(|| args.spec.clone()),
            |info| info.verb.clone(),
        );
        let query = XTrustQuery {
            source: fetched.resolution.source.clone(),
            artifact_sha256,
            capabilities_hash: x_trust_capabilities_hash(&capabilities),
            is_default_registry_pin: registry_pin,
            pinned,
        };
        if let Some(refusal) = x_run_trust_gate(&query, args.yes, &verb_label, &capabilities, run)
        {
            return refusal;
        }
    }

    // Verify-on-read + last_used stamp for cached fetches (poison-inert CAS).
    let package_dir = if fetched.cached {
        let stamped = fetched
            .resolution
            .sha256
            .as_deref()
            .ok_or_else(|| "x: cached package has no pin".to_owned())
            .and_then(|pin| x_cache_get(&run.cache_root, pin, run.now));
        match stamped {
            Ok(entry) => entry.dir,
            Err(message) => return x_error(1, &message),
        }
    } else {
        fetched.package_dir.clone()
    };

    let mut output = x_execute_package(&package_dir, args.plugin_args.clone());
    if args.install {
        x_apply_install(
            &mut output,
            &package_dir,
            explicit_sha.as_deref(),
            &fetched.resolution.source,
            run,
        );
    }
    output
}

/// The `--offline`/`--frozen` route (spec §2.6): trust store + CAS only,
/// zero network. Miss → exit 4 with the rerun-online hint.
fn run_x_offline(
    args: &XRunArgs,
    fetch_spec: &XSpec,
    explicit_sha: Option<&str>,
    requested_verb: Option<&str>,
    run: &mut XRunEnv<'_>,
) -> CliOutput {
    let entries = match x_cache_ls(&run.cache_root) {
        Ok(entries) => entries,
        Err(message) => return x_error(1, &message),
    };
    let canonical = fetch_spec.source.canonical();
    let chosen = entries
        .into_iter()
        .filter(|entry| x_offline_entry_matches(entry, fetch_spec, explicit_sha, &canonical))
        .max_by_key(|entry| entry.meta.last_used);
    let Some(chosen) = chosen else {
        return x_error(
            X_EXIT_OFFLINE_MISS,
            &format!(
                "x: '{}' is not cached — rerun online, or install it: maw plugin install {canonical}",
                args.spec
            ),
        );
    };
    let entry = match x_cache_get(&run.cache_root, &chosen.sha256, run.now) {
        Ok(entry) => entry,
        Err(message) => return x_error(1, &message),
    };
    let manifest_info = read_raw_plugin_install_manifest(&entry.dir)
        .ok()
        .flatten()
        .and_then(|raw| x_manifest_fetch_info(&raw).ok());
    let capabilities = manifest_info
        .as_ref()
        .and_then(|info| info.capabilities.clone())
        .unwrap_or_default();
    if args.dry_run {
        let plan = XResolutionPlan {
            source: entry.meta.source.clone(),
            commit: None,
            path: None,
            sha256: Some(entry.sha256.clone()),
            capabilities: Some(capabilities),
            sdk: manifest_info.as_ref().and_then(|info| info.sdk.clone()),
            installed: None,
        };
        return x_render_plan_json(&plan);
    }
    let verb_label = manifest_info.as_ref().map_or_else(
        || requested_verb.unwrap_or(&args.spec).to_owned(),
        |info| info.verb.clone(),
    );
    let query = XTrustQuery {
        source: entry.meta.source.clone(),
        artifact_sha256: entry.sha256.clone(),
        capabilities_hash: x_trust_capabilities_hash(&capabilities),
        is_default_registry_pin: false,
        pinned: true,
    };
    if let Some(refusal) = x_run_trust_gate(&query, args.yes, &verb_label, &capabilities, run) {
        return refusal;
    }
    let mut output = x_execute_package(&entry.dir, args.plugin_args.clone());
    if args.install {
        x_apply_install(&mut output, &entry.dir, explicit_sha, &entry.meta.source, run);
    }
    output
}

/// Offline CAS matching: explicit pin prefix, else the verb, else the exact
/// canonical source string recorded at fetch time.
fn x_offline_entry_matches(
    entry: &XCacheEntry,
    fetch_spec: &XSpec,
    explicit_sha: Option<&str>,
    canonical: &str,
) -> bool {
    if let Some(pin) = explicit_sha {
        let hex = pin.strip_prefix("sha256:").unwrap_or(pin);
        return entry
            .sha256
            .strip_prefix("sha256:")
            .unwrap_or(&entry.sha256)
            .starts_with(hex);
    }
    if let XSource::Verb { verb, .. } = &fetch_spec.source {
        return entry.meta.verb == *verb;
    }
    entry.meta.source == canonical
}

// ─── trust gate wiring ───────────────────────────────────────────────────

/// Pure decision over what the trust gate does next.
#[derive(Debug, Clone, PartialEq, Eq)]
enum XTrustGateOutcome {
    Proceed { record_how: Option<&'static str> },
    Prompt { reason: String },
    Deny { code: i32, message: String },
}

/// Map a trust decision + flags + TTY state to the gate outcome (pure; the
/// invariants live here — I4 non-interactive-never-approves-unpinned, `--yes`
/// refused on fully unpinned sources, non-TTY refusals carry the observed
/// `--sha256` rerun line).
fn x_trust_gate_outcome(
    decision: &XTrustDecision,
    yes: bool,
    interactive: bool,
    source: &str,
    observed_sha256: &str,
) -> XTrustGateOutcome {
    match decision {
        XTrustDecision::Trusted { .. } => XTrustGateOutcome::Proceed { record_how: None },
        XTrustDecision::Refused { reason } => XTrustGateOutcome::Deny {
            code: X_EXIT_TRUST,
            message: format!(
                "x: {reason}\n  rerun: maw x {source} --yes --sha256 {observed_sha256}\n"
            ),
        },
        XTrustDecision::NeedsPrompt { reason, pinned } => {
            if yes {
                if *pinned {
                    XTrustGateOutcome::Proceed { record_how: Some(X_TRUST_HOW_YES_FLAG) }
                } else {
                    XTrustGateOutcome::Deny {
                        code: X_EXIT_TRUST,
                        message: format!(
                            "x: --yes is refused for a fully unpinned source ({reason}) — scripts must carry a pin:\n  maw x {source} --yes --sha256 {observed_sha256}\n"
                        ),
                    }
                }
            } else if interactive {
                XTrustGateOutcome::Prompt { reason: reason.clone() }
            } else {
                XTrustGateOutcome::Deny {
                    code: X_EXIT_TRUST,
                    message: format!(
                        "x: {reason}\n  non-interactive run without approval — rerun with the observed pin:\n  maw x {source} --yes --sha256 {observed_sha256}\n"
                    ),
                }
            }
        }
    }
}

/// How a TOFU-card answer maps to an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XPromptAction {
    Once,
    Always,
    Deny,
}

fn x_prompt_answer_action(answer: &str) -> XPromptAction {
    match answer.trim().to_ascii_lowercase().as_str() {
        "o" | "once" | "y" | "yes" => XPromptAction::Once,
        "a" | "always" => XPromptAction::Always,
        _ => XPromptAction::Deny,
    }
}

/// Evaluate + apply the trust gate. Returns `Some(refusal)` when the run must
/// stop, `None` to proceed (recording approvals as decided).
fn x_run_trust_gate(
    query: &XTrustQuery,
    yes: bool,
    verb: &str,
    capabilities: &[String],
    run: &mut XRunEnv<'_>,
) -> Option<CliOutput> {
    let decision = match x_trust_evaluate(&run.trust_store, query, run.now_ms) {
        Ok(decision) => decision,
        Err(message) => return Some(x_error(1, &message)),
    };
    let decision = x_trust_refuse_if_non_interactive(decision, run.interactive);
    let outcome = x_trust_gate_outcome(
        &decision,
        yes,
        run.interactive,
        &query.source,
        &query.artifact_sha256,
    );
    let record_how = match outcome {
        XTrustGateOutcome::Proceed { record_how } => record_how,
        XTrustGateOutcome::Deny { code, message } => {
            return Some(CliOutput { code, stdout: String::new(), stderr: message });
        }
        XTrustGateOutcome::Prompt { reason } => {
            let card = render_x_trust_tofu_card(&XTrustTofuCard {
                verb: verb.to_owned(),
                source: query.source.clone(),
                artifact_sha256: query.artifact_sha256.clone(),
                capabilities: capabilities.to_vec(),
            });
            let text = format!("x: {reason}\n{card}");
            let answer = (run.prompt)(&text);
            match answer.as_deref().map_or(XPromptAction::Deny, x_prompt_answer_action) {
                XPromptAction::Once => None,
                XPromptAction::Always => Some(X_TRUST_HOW_PROMPT),
                XPromptAction::Deny => {
                    return Some(x_error(
                        X_EXIT_TRUST,
                        &format!("x: trust declined for {}", query.source),
                    ));
                }
            }
        }
    };
    if let Some(how) = record_how {
        if let Err(message) = x_trust_record(
            &run.trust_store,
            XTrustEntry {
                source: query.source.clone(),
                artifact_sha256: query.artifact_sha256.clone(),
                capabilities_hash: query.capabilities_hash.clone(),
                approved_at_ms: run.now_ms,
                approved_how: how.to_owned(),
            },
        ) {
            return Some(x_error(1, &message));
        }
    }
    None
}

// ─── execution + promotion ───────────────────────────────────────────────

/// Execute a verified package dir through the SAME extism invoke path
/// installed ship-tier plugins use (`ship_tier_wasm_runtime()`): a
/// no-`wasm-host` build fails loudly here with the existing rebuild hint,
/// never as `UnknownCommand` (invariant I7).
fn x_execute_package(package_dir: &std::path::Path, plugin_args: Vec<String>) -> CliOutput {
    let plugin = match load_manifest_from_dir(package_dir) {
        Ok(Some(plugin)) => plugin,
        Ok(None) => {
            return x_error(1, &format!("x: no plugin.json in {}", package_dir.display()));
        }
        Err(message) => return x_error(1, &message),
    };
    let ctx = InvokeContext::new(InvokeSource::Cli, plugin_args);
    let mut runtime = ship_tier_wasm_runtime();
    render_cli_plugin_result(invoke_plugin(&plugin, &ctx, &mut runtime))
}

/// Local shadow (spec §2.2 step 2): a bare verb already installed dispatches
/// the installed copy — pin-gated at install, strictly safer than fetching —
/// with one dim stderr notice. `--remote` bypasses this in the caller.
fn x_dispatch_installed_shadow(verb: &str, plugin_args: &[String]) -> Option<CliOutput> {
    let report = discover_packages(&DiscoverPackagesOptions::default());
    let mut argv = Vec::with_capacity(plugin_args.len() + 1);
    argv.push(verb.to_owned());
    argv.extend_from_slice(plugin_args);
    let mut output = dispatch_cli_plugin_from_report(&report, &argv)?;
    output.stderr = format!("{}{}", x_local_shadow_notice(verb), output.stderr);
    Some(output)
}

fn x_local_shadow_notice(verb: &str) -> String {
    format!("x: using installed {verb} (--remote to fetch)\n")
}

/// `--dry-run` view of the local shadow (#576): the plan for the installed
/// copy that WOULD dispatch — same match as `x_dispatch_installed_shadow`
/// (`plugin_cli_args` over enabled plugins), zero execution, zero network.
fn x_installed_shadow_plan(verb: &str, plugin_args: &[String]) -> Option<XResolutionPlan> {
    let report = discover_packages(&DiscoverPackagesOptions::default());
    let mut argv = Vec::with_capacity(plugin_args.len() + 1);
    argv.push(verb.to_owned());
    argv.extend_from_slice(plugin_args);
    let plugin = report
        .plugins
        .iter()
        .filter(|plugin| !plugin.disabled)
        .find(|plugin| plugin_cli_args(plugin, &argv).is_some())?;
    Some(XResolutionPlan {
        source: format!("installed:{}@{}", plugin.manifest.name, plugin.manifest.version),
        commit: None,
        path: Some(plugin.dir.display().to_string()),
        sha256: plugin
            .manifest
            .artifact
            .as_ref()
            .and_then(|artifact| artifact.sha256.clone()),
        capabilities: plugin.manifest.capabilities.clone(),
        sdk: Some(plugin.manifest.sdk.clone()),
        installed: Some(true),
    })
}

/// `--install`/`--keep`: promote the exact verified bytes to a permanent
/// install + plugins.lock — reuses the shared WI-1 fork and the normal
/// install/lock writers, no refetch. Runs only after a clean plugin exit.
fn x_apply_install(
    output: &mut CliOutput,
    package_dir: &std::path::Path,
    explicit_sha: Option<&str>,
    lock_source: &str,
    run: &XRunEnv<'_>,
) {
    if output.code != 0 {
        let _ = writeln!(output.stderr, "x: --install skipped (plugin exited {})", output.code);
        return;
    }
    match x_promote_install(package_dir, explicit_sha, &run.plugin_root, lock_source) {
        Ok(note) => output.stdout.push_str(&note),
        Err(message) => {
            let _ = writeln!(output.stderr, "x: --install failed: {message}");
            output.code = 1;
        }
    }
}

fn x_promote_install(
    package_dir: &std::path::Path,
    explicit_sha: Option<&str>,
    plugin_root: &std::path::Path,
    lock_source: &str,
) -> Result<String, String> {
    let verification = match verify_package_dir(package_dir, explicit_sha, false, false)? {
        ResolvedPackage::Wasm(verification) => verification,
        ResolvedPackage::NotWasm => {
            return Err("x: --install supports ship-tier wasm packages only".to_owned());
        }
    };
    let summary = install_plugin_dir(package_dir, plugin_root, false)?;
    record_plugin_install_pin(&summary, verification.resolved_sha256.as_deref(), lock_source)?;
    Ok(format!(
        "x: installed {}@{} {}\n",
        summary.name,
        summary.version,
        summary.install_dir.display()
    ))
}

// ─── housekeeping subcommands ────────────────────────────────────────────

fn run_x_cache_ls(cache_root: &std::path::Path) -> CliOutput {
    match x_cache_ls(cache_root) {
        Err(message) => x_error(1, &message),
        Ok(entries) if entries.is_empty() => CliOutput {
            code: 0,
            stdout: "x: cache is empty\n".to_owned(),
            stderr: String::new(),
        },
        Ok(entries) => {
            let mut stdout = String::new();
            for entry in entries {
                let _ = writeln!(
                    stdout,
                    "{}  {}@{}  {}B  last-used {}  {}",
                    x_trust_sha12(&entry.sha256),
                    entry.meta.verb,
                    entry.meta.version,
                    entry.meta.size,
                    entry.meta.last_used,
                    entry.meta.source
                );
            }
            CliOutput { code: 0, stdout, stderr: String::new() }
        }
    }
}

fn run_x_cache_gc(
    cache_root: &std::path::Path,
    now: u64,
    options: &XCacheGcOptions,
    dry_run: bool,
) -> CliOutput {
    match x_cache_gc(cache_root, now, options, dry_run) {
        Err(message) => x_error(1, &message),
        Ok(plan) => {
            let mut stdout = String::new();
            for entry in &plan.evict {
                let _ =
                    writeln!(stdout, "evict {}  {}", x_trust_sha12(&entry.sha256), entry.meta.verb);
            }
            let suffix = if dry_run { " (dry-run)" } else { "" };
            let _ = writeln!(
                stdout,
                "x gc: evicted {} ({}B reclaimed), kept {} ({}B){suffix}",
                plan.evict.len(),
                plan.reclaimed_bytes,
                plan.keep.len(),
                plan.kept_bytes
            );
            CliOutput { code: 0, stdout, stderr: String::new() }
        }
    }
}

fn run_x_cache_rm(cache_root: &std::path::Path, needle: &str) -> CliOutput {
    match x_cache_rm(cache_root, needle) {
        Err(message) => x_error(1, &message),
        Ok(removed) => {
            let mut stdout = String::new();
            for entry in &removed {
                let _ = writeln!(
                    stdout,
                    "removed {}  {}",
                    x_trust_sha12(&entry.sha256),
                    entry.meta.verb
                );
            }
            CliOutput { code: 0, stdout, stderr: String::new() }
        }
    }
}

fn run_x_trust_ls(store: &std::path::Path) -> CliOutput {
    match x_trust_list(store) {
        Err(message) => x_error(1, &message),
        Ok(entries) if entries.is_empty() => CliOutput {
            code: 0,
            stdout: "x: trust store is empty\n".to_owned(),
            stderr: String::new(),
        },
        Ok(entries) => {
            let mut stdout = String::new();
            for entry in entries {
                let _ = writeln!(
                    stdout,
                    "{}  {}  {}",
                    x_trust_sha12(&entry.artifact_sha256),
                    entry.approved_how,
                    entry.source
                );
            }
            CliOutput { code: 0, stdout, stderr: String::new() }
        }
    }
}

fn run_x_trust_revoke(store: &std::path::Path, selector: &str) -> CliOutput {
    match x_trust_revoke(store, selector) {
        Err(message) => x_error(1, &message),
        Ok(removed) => CliOutput {
            code: 0,
            stdout: format!("x trust: revoked {removed} entries\n"),
            stderr: String::new(),
        },
    }
}

// ─── small helpers ───────────────────────────────────────────────────────

/// Reconcile `--sha256` with an inline `#sha256:` pin; both normalize through
/// the install-route normalizer and must agree.
fn x_explicit_sha256(flag: Option<&str>, inline: Option<&str>) -> Result<Option<String>, String> {
    let normalize = |value: &str| {
        normalize_plugin_install_sha256(value).map_err(|_| {
            "x: --sha256 must be 64 lowercase hex chars (optionally 'sha256:'-prefixed)"
                .to_owned()
        })
    };
    let flag = flag.map(normalize).transpose()?;
    let inline = inline.map(normalize).transpose()?;
    match (flag, inline) {
        (Some(flag), Some(inline)) if flag != inline => Err(format!(
            "x: --sha256 and the inline #sha256: pin disagree\n  --sha256: {flag}\n  inline:   {inline}"
        )),
        (flag, inline) => Ok(flag.or(inline)),
    }
}

/// `--registry owner/repo` override; defaults to the default registry.
fn x_registry_override(value: Option<&str>) -> Result<(String, String), String> {
    let Some(value) = value else {
        return Ok((X_DEFAULT_REGISTRY_OWNER.to_owned(), X_DEFAULT_REGISTRY_REPO.to_owned()));
    };
    match value.split_once('/') {
        Some((owner, repo)) if !owner.is_empty() && !repo.is_empty() && !repo.contains('/') => {
            Ok((owner.to_owned(), repo.to_owned()))
        }
        _ => Err(format!("x: --registry must be owner/repo, got '{value}'")),
    }
}

/// Resolution failures: unknown verbs and unsupported schemes are usage-class
/// (exit 2); everything else (network, malformed registry) is exit 1.
fn x_resolve_error(message: &str) -> CliOutput {
    let code = if message.contains("unknown verb") || message.contains("not yet supported") {
        2
    } else {
        1
    };
    x_error(code, message)
}

fn x_render_plan_json(plan: &XResolutionPlan) -> CliOutput {
    match serde_json::to_string(plan) {
        Ok(mut body) => {
            body.push('\n');
            CliOutput { code: 0, stdout: body, stderr: String::new() }
        }
        Err(error) => x_error(1, &format!("x: failed to encode the resolution plan: {error}")),
    }
}

fn x_error(code: i32, message: &str) -> CliOutput {
    CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") }
}

fn x_usage_error(message: &str) -> CliOutput {
    CliOutput { code: 2, stdout: String::new(), stderr: format!("{message}\n{X_USAGE}\n") }
}

/// Real TOFU prompt edge: the card goes to stderr immediately, the answer is
/// read from stdin (spec §2.1: prompt → stderr, answer from the tty).
fn x_tty_prompt(card: &str) -> Option<String> {
    eprint!("{card}");
    let _ = std::io::Write::flush(&mut std::io::stderr());
    let mut answer = String::new();
    match std::io::stdin().read_line(&mut answer) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(answer),
    }
}

fn x_now_secs_ms() -> (u64, i64) {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    (duration.as_secs(), i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod x_wi8_tests {
    use super::*;

    static TEMP_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn temp_root(label: &str) -> std::path::PathBuf {
        let counter = TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir()
            .join(format!("maw-rs-x-wi8-{label}-{}-{counter}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp root");
        dir
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    struct XTestHarness {
        root: std::path::PathBuf,
        cache_root: std::path::PathBuf,
        trust_store: std::path::PathBuf,
        plugin_root: std::path::PathBuf,
    }

    impl XTestHarness {
        fn new(label: &str) -> Self {
            let root = temp_root(label);
            Self {
                cache_root: root.join("cache"),
                trust_store: root.join("x-trust.json"),
                plugin_root: root.join("plugins"),
                root,
            }
        }

        fn run(
            &self,
            argv: &[&str],
            interactive: bool,
            prompt: &mut dyn FnMut(&str) -> Option<String>,
        ) -> CliOutput {
            let command = match parse_x_cli(&args(argv)) {
                Ok(command) => command,
                Err(message) => return x_usage_error(&message),
            };
            let mut run = XRunEnv {
                cache_root: self.cache_root.clone(),
                trust_store: self.trust_store.clone(),
                plugin_root: self.plugin_root.clone(),
                interactive,
                now: 10_000,
                now_ms: 10_000_000,
                prompt,
            };
            run_x_parsed(&command, &mut run)
        }

        fn run_plain(&self, argv: &[&str]) -> CliOutput {
            let mut prompt =
                |_card: &str| -> Option<String> { panic!("prompt must not fire in this test") };
            self.run(argv, false, &mut prompt)
        }
    }

    impl Drop for XTestHarness {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// Seed a package-shaped CAS entry (pinned fake wasm + manifest) and
    /// return its canonical `sha256:<hex>` pin.
    fn seed_cached_package(harness: &XTestHarness, verb: &str, capabilities: &[&str]) -> String {
        let bytes = format!("\0asm\u{1}\0\0\0x-wi8-{verb}").into_bytes();
        let pin = x_sha256_of_bytes(&bytes).expect("hash bytes");
        let entry = x_cache_put(
            &harness.cache_root,
            &XCachePutRequest {
                artifact_name: "plugin.wasm",
                bytes: &bytes,
                expected_sha256: &pin,
                source: &format!("gh:acme/maw-tools@{}/packages/{verb}", "c".repeat(40)),
                verb,
                version: "1.0.0",
                fetched_at: 1_000,
            },
        )
        .expect("seed cache");
        let caps = capabilities
            .iter()
            .map(|cap| format!("\"{cap}\""))
            .collect::<Vec<_>>()
            .join(",");
        std::fs::write(
            entry.dir.join("plugin.json"),
            format!(
                r#"{{"name":"{verb}","version":"1.0.0","target":"wasm","sdk":"*","capabilities":[{caps}],"entry":{{"kind":"wasm","path":"plugin.wasm","export":"handle"}},"wasm":"./plugin.wasm","artifact":{{"path":"plugin.wasm","sha256":"{pin}"}},"cli":{{"command":"{verb}"}}}}"#
            ),
        )
        .expect("seed manifest");
        pin
    }

    // ── argv parsing ────────────────────────────────────────────────────

    #[test]
    fn x_cli_parse_flags_separator_and_plugin_args() {
        let hex = "a".repeat(64);
        let parsed = parse_x_cli(&args(&[
            "costs", "--dry-run", "--sha256", &hex, "--", "--json", "extra",
        ]))
        .expect("parse");
        let XCliCommand::Run(run) = parsed else { panic!("run form") };
        assert_eq!(run.spec, "costs");
        assert!(run.dry_run);
        assert_eq!(run.sha256.as_deref(), Some(hex.as_str()));
        assert_eq!(run.plugin_args, args(&["--json", "extra"]));

        // npx-style: the first non-x-flag token after the spec starts the
        // plugin args verbatim.
        let parsed = parse_x_cli(&args(&["costs", "list", "--json"])).expect("parse");
        let XCliCommand::Run(run) = parsed else { panic!("run form") };
        assert_eq!(run.plugin_args, args(&["list", "--json"]));
        assert!(!run.dry_run);

        // x flags after the spec still apply; `=` forms parse.
        let parsed = parse_x_cli(&args(&[
            "costs",
            "--remote",
            "--from=gh:acme/tools/packages/costs",
            "--registry=acme/registry",
            "-y",
            "--offline",
            "--install",
        ]))
        .expect("parse");
        let XCliCommand::Run(run) = parsed else { panic!("run form") };
        assert!(run.remote && run.yes && run.offline && run.install);
        assert_eq!(run.from.as_deref(), Some("gh:acme/tools/packages/costs"));
        assert_eq!(run.registry.as_deref(), Some("acme/registry"));

        // Errors: missing spec, unknown flag before the spec, missing value.
        assert!(parse_x_cli(&args(&[])).is_err());
        assert!(parse_x_cli(&args(&["--bogus", "costs"])).is_err());
        assert!(parse_x_cli(&args(&["--sha256"])).is_err());
        assert!(matches!(parse_x_cli(&args(&["--help"])), Ok(XCliCommand::Help)));
    }

    #[test]
    fn x_cli_parse_housekeeping_routing() {
        assert_eq!(parse_x_cli(&args(&["ls"])).expect("ls"), XCliCommand::CacheLs);
        assert_eq!(
            parse_x_cli(&args(&["gc", "--max-age", "30d", "--max-size", "2g", "--dry-run"]))
                .expect("gc"),
            XCliCommand::CacheGc {
                max_age_secs: Some(30 * 86_400),
                max_size_bytes: Some(2 << 30),
                dry_run: true,
            }
        );
        assert_eq!(
            parse_x_cli(&args(&["rm", "abc123"])).expect("rm"),
            XCliCommand::CacheRm { needle: "abc123".to_owned() }
        );
        assert_eq!(parse_x_cli(&args(&["trust", "ls"])).expect("trust ls"), XCliCommand::TrustLs);
        assert_eq!(
            parse_x_cli(&args(&["trust", "revoke", "gh:acme/tools"])).expect("trust revoke"),
            XCliCommand::TrustRevoke { selector: "gh:acme/tools".to_owned() }
        );
        assert!(parse_x_cli(&args(&["ls", "extra"])).is_err());
        assert!(parse_x_cli(&args(&["rm"])).is_err());
        assert!(parse_x_cli(&args(&["trust"])).is_err());
        assert!(parse_x_cli(&args(&["gc", "--bogus"])).is_err());
    }

    #[test]
    fn x_gc_duration_and_size_suffixes() {
        assert_eq!(parse_x_gc_duration_secs("30d"), Ok(30 * 86_400));
        assert_eq!(parse_x_gc_duration_secs("12H"), Ok(12 * 3_600));
        assert_eq!(parse_x_gc_duration_secs("45m"), Ok(45 * 60));
        assert_eq!(parse_x_gc_duration_secs("90"), Ok(90));
        assert!(parse_x_gc_duration_secs("soon").is_err());
        assert_eq!(parse_x_gc_size_bytes("2g"), Ok(2 << 30));
        assert_eq!(parse_x_gc_size_bytes("500M"), Ok(500 << 20));
        assert_eq!(parse_x_gc_size_bytes("8k"), Ok(8 << 10));
        assert_eq!(parse_x_gc_size_bytes("1024"), Ok(1_024));
        assert!(parse_x_gc_size_bytes("big").is_err());
    }

    // ── native guard + local shadow ─────────────────────────────────────

    #[test]
    fn x_native_guard_refuses_native_verbs() {
        let harness = XTestHarness::new("native-guard");
        let output = harness.run_plain(&["hey"]);
        assert_eq!(output.code, 2, "{output:?}");
        assert!(output.stderr.contains("native maw command"), "{}", output.stderr);
        assert!(output.stderr.contains("'hey'"), "{}", output.stderr);
    }

    #[test]
    fn x_local_shadow_dispatches_installed_and_remote_bypasses() {
        let _guard = env_test_lock();
        let harness = XTestHarness::new("local-shadow");
        let verb = "x-wi8-shadow-demo";

        // An installed, pin-verified package under MAW_PLUGINS_DIR.
        let plugins_dir = harness.root.join("installed");
        let package = plugins_dir.join(verb);
        std::fs::create_dir_all(&package).expect("package dir");
        std::fs::write(package.join("plugin.wasm"), b"\0asm\x01\x00\x00\x00x-wi8-shadow")
            .expect("wasm");
        let pin = maw_plugin_manifest::hash_file(&package.join("plugin.wasm")).expect("hash");
        std::fs::write(
            package.join("plugin.json"),
            format!(
                r#"{{"name":"{verb}","version":"1.0.0","target":"wasm","sdk":"*","entry":{{"kind":"wasm","path":"plugin.wasm","export":"handle"}},"wasm":"./plugin.wasm","artifact":{{"path":"plugin.wasm","sha256":"{pin}"}},"cli":{{"command":"{verb}"}}}}"#
            ),
        )
        .expect("manifest");
        let restore = EnvVarRestore::capture("MAW_PLUGINS_DIR");
        std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);

        // Installed copy shadows the fetch, with the dim notice first.
        let output = harness.run_plain(&[verb]);
        assert!(
            output.stderr.starts_with(&x_local_shadow_notice(verb)),
            "shadow notice must lead stderr: {output:?}"
        );
        assert_ne!(output.code, 4, "shadow must not reach the offline path");

        // --remote bypasses the shadow: with --offline and an empty CAS the
        // run falls through to the offline miss (exit 4), proving no local
        // dispatch happened.
        let output = harness.run_plain(&[verb, "--remote", "--offline"]);
        assert_eq!(output.code, X_EXIT_OFFLINE_MISS, "{output:?}");
        assert!(!output.stderr.contains("using installed"), "{}", output.stderr);

        drop(restore);
    }

    /// #576: `--dry-run` must short-circuit BEFORE the local shadow — a
    /// shadowed verb prints the resolution plan (with `installed: true`)
    /// instead of dispatching the installed plugin.
    #[test]
    fn x_dry_run_short_circuits_before_local_shadow() {
        let _guard = env_test_lock();
        let harness = XTestHarness::new("dry-run-shadow");
        let verb = "x-576-dry-shadow-demo";

        // An installed, pin-verified package under MAW_PLUGINS_DIR.
        let plugins_dir = harness.root.join("installed");
        let package = plugins_dir.join(verb);
        std::fs::create_dir_all(&package).expect("package dir");
        std::fs::write(package.join("plugin.wasm"), b"\0asm\x01\x00\x00\x00x-576-dry-shadow")
            .expect("wasm");
        let pin = maw_plugin_manifest::hash_file(&package.join("plugin.wasm")).expect("hash");
        std::fs::write(
            package.join("plugin.json"),
            format!(
                r#"{{"name":"{verb}","version":"1.0.0","target":"wasm","sdk":"*","entry":{{"kind":"wasm","path":"plugin.wasm","export":"handle"}},"wasm":"./plugin.wasm","artifact":{{"path":"plugin.wasm","sha256":"{pin}"}},"cli":{{"command":"{verb}"}}}}"#
            ),
        )
        .expect("manifest");
        let restore = EnvVarRestore::capture("MAW_PLUGINS_DIR");
        std::env::set_var("MAW_PLUGINS_DIR", &plugins_dir);

        let output = harness.run_plain(&[verb, "--dry-run"]);
        drop(restore);

        assert_eq!(output.code, 0, "{output:?}");
        assert!(
            !output.stderr.contains("using installed"),
            "dry-run must not dispatch the shadow: {}",
            output.stderr
        );
        let plan: serde_json::Value =
            serde_json::from_str(output.stdout.trim()).expect("plan json");
        assert_eq!(plan.get("installed").and_then(serde_json::Value::as_bool), Some(true));
        assert_eq!(
            plan.get("source").and_then(serde_json::Value::as_str),
            Some(format!("installed:{verb}@1.0.0").as_str())
        );
        assert_eq!(plan.get("sha256").and_then(serde_json::Value::as_str), Some(pin.as_str()));
        assert!(
            plan.get("path")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|path| path.ends_with(verb)),
            "{plan}"
        );
    }

    // ── offline route ───────────────────────────────────────────────────

    #[test]
    fn x_offline_miss_exits_4() {
        let _guard = env_test_lock();
        let empty = temp_root("offline-miss-plugins");
        let restore = EnvVarRestore::capture("MAW_PLUGINS_DIR");
        std::env::set_var("MAW_PLUGINS_DIR", &empty);
        let harness = XTestHarness::new("offline-miss");
        let output = harness.run_plain(&["x-wi8-not-cached", "--offline"]);
        assert_eq!(output.code, X_EXIT_OFFLINE_MISS, "{output:?}");
        assert!(output.stderr.contains("not cached"), "{}", output.stderr);
        assert!(output.stderr.contains("rerun online"), "{}", output.stderr);
        drop(restore);
        let _ = std::fs::remove_dir_all(empty);
    }

    #[test]
    fn x_offline_dry_run_prints_plan_json() {
        let harness = XTestHarness::new("offline-dry-run");
        let verb = "x-wi8-dry-demo";
        let pin = seed_cached_package(&harness, verb, &["fs:read:cwd"]);
        let output = harness.run_plain(&[verb, "--offline", "--remote", "--dry-run"]);
        assert_eq!(output.code, 0, "{output:?}");
        let plan: serde_json::Value =
            serde_json::from_str(output.stdout.trim()).expect("plan json");
        assert_eq!(plan.get("sha256").and_then(serde_json::Value::as_str), Some(pin.as_str()));
        assert!(
            plan.get("source")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|source| source.starts_with("gh:acme/maw-tools@")),
            "{plan}"
        );
        assert_eq!(
            plan.get("capabilities").and_then(serde_json::Value::as_array).map(Vec::len),
            Some(1)
        );
        assert_eq!(plan.get("sdk").and_then(serde_json::Value::as_str), Some("*"));
        // The plan is a pure resolution artifact: no trust write, no execute.
        assert!(x_trust_list(&harness.trust_store).expect("trust ls").is_empty());
    }

    // ── trust gate ──────────────────────────────────────────────────────

    #[test]
    fn x_non_tty_unapproved_exit_3_names_observed_sha() {
        let harness = XTestHarness::new("non-tty");
        let verb = "x-wi8-nontty-demo";
        let pin = seed_cached_package(&harness, verb, &[]);
        let output = harness.run_plain(&[verb, "--offline", "--remote"]);
        assert_eq!(output.code, X_EXIT_TRUST, "{output:?}");
        assert!(output.stderr.contains(&pin), "must carry the observed pin: {}", output.stderr);
        assert!(output.stderr.contains("--sha256"), "{}", output.stderr);
        assert!(output.stderr.contains("maw x "), "rerun line: {}", output.stderr);
    }

    #[test]
    fn x_yes_on_unpinned_is_refused() {
        // Pure-gate proof of I4: --yes never approves a fully unpinned source.
        let decision = XTrustDecision::NeedsPrompt {
            reason: "first run for gh:acme/tools — not in trust store (TOFU)".to_owned(),
            pinned: false,
        };
        let sha = format!("sha256:{}", "d".repeat(64));
        let outcome = x_trust_gate_outcome(&decision, true, true, "gh:acme/tools", &sha);
        let XTrustGateOutcome::Deny { code, message } = outcome else {
            panic!("--yes on unpinned must deny");
        };
        assert_eq!(code, X_EXIT_TRUST);
        assert!(message.contains("--yes is refused"), "{message}");
        assert!(message.contains(&sha), "carries the observed pin: {message}");

        // The pinned counterpart proceeds and records the yes-flag approval.
        let pinned = XTrustDecision::NeedsPrompt { reason: "first run".to_owned(), pinned: true };
        assert_eq!(
            x_trust_gate_outcome(&pinned, true, false, "gh:acme/tools", &sha),
            XTrustGateOutcome::Proceed { record_how: Some(X_TRUST_HOW_YES_FLAG) }
        );
    }

    #[test]
    fn x_interactive_always_records_trust_and_reaches_execution() {
        let harness = XTestHarness::new("interactive");
        let verb = "x-wi8-tofu-demo";
        let pin = seed_cached_package(&harness, verb, &["fs:read:cwd"]);
        let mut cards = Vec::new();
        let mut prompt = |card: &str| -> Option<String> {
            cards.push(card.to_owned());
            Some("a\n".to_owned())
        };
        let output = harness.run(&[verb, "--offline", "--remote"], true, &mut prompt);
        assert_eq!(cards.len(), 1, "one TOFU card");
        assert!(cards[0].contains("first run"), "{}", cards[0]);
        assert!(cards[0].contains("fs:read:cwd"), "{}", cards[0]);
        // "always" recorded the triple…
        let entries = x_trust_list(&harness.trust_store).expect("trust ls");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].artifact_sha256, pin);
        assert_eq!(entries[0].approved_how, X_TRUST_HOW_PROMPT);
        // …and the run reached the execution seam: the fake wasm cannot run,
        // but the failure is the invoke path's, not a trust/cache refusal.
        assert_eq!(output.code, 1, "{output:?}");
        #[cfg(not(feature = "wasm-host"))]
        assert!(
            output.stderr.contains("wasm-host"),
            "featureless builds must fail loudly with the rebuild hint: {}",
            output.stderr
        );

        // Second run: the triple is trusted, no prompt fires.
        let output = harness.run_plain(&[verb, "--offline", "--remote"]);
        assert_eq!(output.code, 1, "trusted rerun reaches execution: {output:?}");

        // Declining answers deny with exit 3.
        let miss_verb = "x-wi8-tofu-deny";
        seed_cached_package(&harness, miss_verb, &[]);
        let mut deny = |_card: &str| -> Option<String> { Some("n\n".to_owned()) };
        let output = harness.run(&[miss_verb, "--offline", "--remote"], true, &mut deny);
        assert_eq!(output.code, X_EXIT_TRUST, "{output:?}");
        assert!(output.stderr.contains("trust declined"), "{}", output.stderr);
    }

    #[test]
    fn x_yes_flag_approves_pinned_offline_run_end_to_end() {
        let harness = XTestHarness::new("yes-flag");
        let verb = "x-wi8-yes-demo";
        let pin = seed_cached_package(&harness, verb, &[]);
        let output = harness.run_plain(&[verb, "--offline", "--remote", "--yes"]);
        assert_ne!(output.code, X_EXIT_TRUST, "{output:?}");
        let entries = x_trust_list(&harness.trust_store).expect("trust ls");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].approved_how, X_TRUST_HOW_YES_FLAG);
        assert_eq!(entries[0].artifact_sha256, pin);
    }

    // ── housekeeping over real temp stores ──────────────────────────────

    #[test]
    fn x_housekeeping_ls_rm_trust_roundtrip() {
        let harness = XTestHarness::new("housekeeping");
        let verb = "x-wi8-house-demo";
        let pin = seed_cached_package(&harness, verb, &[]);
        let sha12 = x_trust_sha12(&pin);

        let output = harness.run_plain(&["ls"]);
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains(verb), "{}", output.stdout);
        assert!(output.stdout.contains(&sha12), "{}", output.stdout);

        let output = harness.run_plain(&["gc", "--max-age", "1s", "--dry-run"]);
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("(dry-run)"), "{}", output.stdout);

        let output = harness.run_plain(&["rm", &sha12]);
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("removed"), "{}", output.stdout);
        let output = harness.run_plain(&["ls"]);
        assert!(output.stdout.contains("cache is empty"), "{}", output.stdout);

        x_trust_record(
            &harness.trust_store,
            XTrustEntry {
                source: "gh:acme/maw-tools/packages/demo".to_owned(),
                artifact_sha256: pin.clone(),
                capabilities_hash: x_trust_capabilities_hash(&[]),
                approved_at_ms: 1,
                approved_how: X_TRUST_HOW_PROMPT.to_owned(),
            },
        )
        .expect("record");
        let output = harness.run_plain(&["trust", "ls"]);
        assert!(output.stdout.contains("gh:acme/maw-tools/packages/demo"), "{}", output.stdout);
        let output = harness.run_plain(&["trust", "revoke", "gh:acme/maw-tools/packages/demo"]);
        assert!(output.stdout.contains("revoked 1"), "{}", output.stdout);
        let output = harness.run_plain(&["trust", "ls"]);
        assert!(output.stdout.contains("trust store is empty"), "{}", output.stdout);
    }

    #[test]
    fn x_usage_help_and_conflicts() {
        let harness = XTestHarness::new("usage");
        let output = harness.run_plain(&["--help"]);
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("usage: maw x <spec>"), "{}", output.stdout);

        let output = harness.run_plain(&["costs", "--offline", "--reload"]);
        assert_eq!(output.code, 2, "{output:?}");
        assert!(output.stderr.contains("--reload conflicts"), "{}", output.stderr);

        let hex_a = "a".repeat(64);
        let hex_b = "b".repeat(64);
        let spec = format!("costs#sha256:{hex_b}");
        let output = harness.run_plain(&[&spec, "--sha256", &hex_a]);
        assert_eq!(output.code, 2, "{output:?}");
        assert!(output.stderr.contains("disagree"), "{}", output.stderr);

        // --from requires a bare verb positional.
        let output = harness.run_plain(&["gh:acme/tools", "--from", "gh:acme/tools/packages/x"]);
        assert_eq!(output.code, 2, "{output:?}");
        assert!(output.stderr.contains("--from requires a bare verb"), "{}", output.stderr);
    }

    #[test]
    fn x_dispatcher_registers_the_verb() {
        assert_eq!(dispatcher_status("x"), DispatchKind::Native);
        assert_eq!(DISPATCH_335.len(), 1);
    }
}
