// x_trust.rs — mawx WI-7: trust store + TOFU + default-registry auto-trust.
//
// Core trust logic for `maw x` (spec: ψ/design/mawx-spec.md §2.5, §6).
// Records `(canonical_source, artifact_sha256, capabilities_hash)` triples —
// trust keys on the *triple*, not the artifact alone, so a swapped
// `plugin.json` that keeps the same wasm but widens capabilities re-prompts
// (the cap-swap defense; invariant I3).
//
// Nat's locked decision 2 (2026-07-16): pinned entries from the default
// registry (`Soul-Brews-Studio/maw-plugins` registry.json) are an auto-trust
// root — trusted WITHOUT a first-run prompt and recorded with
// `approved_how = "registry-auto"`. Every other source is prompt-or-pin.
//
// Deterministic core: timestamps are passed in, no TTY, no network. WI-8
// wires the dispatcher/prompt around these functions.

/// Recorded when a pinned default-registry entry is auto-trusted (decision 2).
pub const X_TRUST_HOW_REGISTRY_AUTO: &str = "registry-auto";
/// Recorded when a human answers the TOFU card with "always".
pub const X_TRUST_HOW_PROMPT: &str = "prompt";
/// Recorded when `--yes` approves a pinned first run non-interactively.
pub const X_TRUST_HOW_YES_FLAG: &str = "yes-flag";

/// One approved `(source, artifact, capabilities)` triple in the trust store.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct XTrustEntry {
    pub source: String,
    #[serde(rename = "artifactSha256")]
    pub artifact_sha256: String,
    #[serde(rename = "capabilitiesHash")]
    pub capabilities_hash: String,
    #[serde(rename = "approvedAtMs")]
    pub approved_at_ms: i64,
    #[serde(rename = "approvedHow")]
    pub approved_how: String,
}

/// What the resolver observed for the invocation being decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XTrustQuery {
    /// Canonical source string (e.g. `gh:owner/repo/packages/costs`).
    pub source: String,
    /// Canonical `sha256:<64 lowercase hex>` of the wasm artifact.
    pub artifact_sha256: String,
    /// Canonical capabilities hash (see [`x_trust_capabilities_hash`]).
    pub capabilities_hash: String,
    /// True only when the entry came from the default registry
    /// (`Soul-Brews-Studio/maw-plugins` registry.json) WITH a pin.
    pub is_default_registry_pin: bool,
    /// True when any accountable pin backs this resolution (registry
    /// commit-pin, plugins.lock, or `--sha256`). Carried into the decision so
    /// non-interactive callers can refuse fully unpinned sources (I4).
    pub pinned: bool,
}

/// Outcome of a trust check. `NeedsPrompt` carries the pin state so a
/// non-interactive caller can refuse fully unpinned sources (I4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XTrustDecision {
    Trusted { approved_how: String },
    NeedsPrompt { reason: String, pinned: bool },
    Refused { reason: String },
}

/// Canonical capabilities hash: sha256 over the JSON encoding of the SORTED,
/// deduplicated capabilities list from the manifest. Sorting makes the hash
/// order-independent; any added/removed/edited capability changes it — same
/// wasm + edited manifest caps = different hash = not trusted (I3).
#[must_use]
pub fn x_trust_capabilities_hash(capabilities: &[String]) -> String {
    let mut caps: Vec<&str> = capabilities.iter().map(String::as_str).collect();
    caps.sort_unstable();
    caps.dedup();
    let canonical = serde_json::to_string(&caps).unwrap_or_else(|_| "[]".to_owned());
    format!("sha256:{}", hash_body(Some(canonical.as_bytes())))
}

/// Normalize a sha256 value to the canonical `sha256:<64 lowercase hex>` form
/// used by plugin pins and the trust store.
///
/// # Errors
///
/// Returns an error when the value is not 64 lowercase hex chars (with or
/// without the `sha256:` prefix).
pub fn x_trust_normalize_sha256(value: &str) -> Result<String, String> {
    let hex = value.strip_prefix("sha256:").unwrap_or(value);
    if hex.len() == 64
        && hex
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
    {
        Ok(format!("sha256:{hex}"))
    } else {
        Err("x trust: sha256 must be 64 lowercase hex chars".to_owned())
    }
}

/// First 12 hex chars of a canonical sha256, for card/reason display.
#[must_use]
pub fn x_trust_sha12(sha256: &str) -> String {
    sha256
        .strip_prefix("sha256:")
        .unwrap_or(sha256)
        .chars()
        .take(12)
        .collect()
}

/// Pure trust decision over an in-memory store snapshot.
///
/// Order of checks:
/// 1. Known triple → `Trusted` (with the recorded `approved_how`).
/// 2. Default-registry pin (decision 2) → `Trusted` (`registry-auto`) — the
///    caller records it via [`x_trust_evaluate`].
/// 3. Known source, changed sha256 and/or capabilities → `NeedsPrompt` with a
///    reason naming exactly what changed (cap-widen-same-wasm re-prompts).
/// 4. Unknown source → `NeedsPrompt` (TOFU).
#[must_use]
pub fn x_trust_decide(entries: &[XTrustEntry], query: &XTrustQuery) -> XTrustDecision {
    if let Some(entry) = entries.iter().find(|entry| {
        entry.source == query.source
            && entry.artifact_sha256 == query.artifact_sha256
            && entry.capabilities_hash == query.capabilities_hash
    }) {
        return XTrustDecision::Trusted {
            approved_how: entry.approved_how.clone(),
        };
    }
    if query.is_default_registry_pin {
        return XTrustDecision::Trusted {
            approved_how: X_TRUST_HOW_REGISTRY_AUTO.to_owned(),
        };
    }
    let same_source: Vec<&XTrustEntry> = entries
        .iter()
        .filter(|entry| entry.source == query.source)
        .collect();
    if same_source.is_empty() {
        return XTrustDecision::NeedsPrompt {
            reason: format!("first run for {} — not in trust store (TOFU)", query.source),
            pinned: query.pinned,
        };
    }
    if same_source
        .iter()
        .any(|entry| entry.artifact_sha256 == query.artifact_sha256)
    {
        // Same wasm, different manifest capabilities — the cap-swap defense.
        return XTrustDecision::NeedsPrompt {
            reason: format!(
                "capabilities changed for {}: artifact unchanged (sha256 {}) but capabilities differ from the approved set — re-approval required",
                query.source,
                x_trust_sha12(&query.artifact_sha256)
            ),
            pinned: query.pinned,
        };
    }
    if let Some(entry) = same_source
        .iter()
        .find(|entry| entry.capabilities_hash == query.capabilities_hash)
    {
        return XTrustDecision::NeedsPrompt {
            reason: format!(
                "artifact sha256 changed for {}: trusted {} → observed {}",
                query.source,
                x_trust_sha12(&entry.artifact_sha256),
                x_trust_sha12(&query.artifact_sha256)
            ),
            pinned: query.pinned,
        };
    }
    XTrustDecision::NeedsPrompt {
        reason: format!(
            "artifact sha256 and capabilities both changed for {} (trusted {} → observed {})",
            query.source,
            x_trust_sha12(&same_source[0].artifact_sha256),
            x_trust_sha12(&query.artifact_sha256)
        ),
        pinned: query.pinned,
    }
}

/// Downgrade a `NeedsPrompt` on a fully unpinned source to `Refused` for
/// non-interactive contexts (I4: non-interactive never auto-approves the
/// unpinned). Pinned `NeedsPrompt` passes through so the caller can honor
/// `--yes` (WI-8).
#[must_use]
pub fn x_trust_refuse_if_non_interactive(
    decision: XTrustDecision,
    interactive: bool,
) -> XTrustDecision {
    match decision {
        XTrustDecision::NeedsPrompt {
            reason,
            pinned: false,
        } if !interactive => XTrustDecision::Refused {
            reason: format!(
                "{reason}; non-interactive run with a fully unpinned source is refused — rerun interactively or pass --sha256 <observed>"
            ),
        },
        other => other,
    }
}

/// Read the store, decide, and record default-registry auto-trust approvals
/// (`approved_how = "registry-auto"`, decision 2) when they are not yet in
/// the store. TOFU approvals are recorded separately via
/// [`x_trust_record`] once the caller has an answer.
///
/// # Errors
///
/// Returns an error when the store cannot be read, when the query carries a
/// malformed sha256/capabilities hash, or when recording the registry-auto
/// approval fails.
pub fn x_trust_evaluate(
    path: &Path,
    query: &XTrustQuery,
    now_ms: i64,
) -> Result<XTrustDecision, String> {
    let entries = x_trust_read_store(path)?;
    let decision = x_trust_decide(&entries, query);
    if let XTrustDecision::Trusted { approved_how } = &decision {
        if query.is_default_registry_pin && approved_how == X_TRUST_HOW_REGISTRY_AUTO {
            x_trust_record(
                path,
                XTrustEntry {
                    source: query.source.clone(),
                    artifact_sha256: query.artifact_sha256.clone(),
                    capabilities_hash: query.capabilities_hash.clone(),
                    approved_at_ms: now_ms,
                    approved_how: X_TRUST_HOW_REGISTRY_AUTO.to_owned(),
                },
            )?;
        }
    }
    Ok(decision)
}

/// Record an approved triple. Returns `true` when newly added, `false` when
/// the exact triple was already present (the earlier approval record wins).
///
/// # Errors
///
/// Returns an error when the entry is malformed (empty source/`approved_how`,
/// non-canonical hashes) or when the store cannot be read or written.
pub fn x_trust_record(path: &Path, entry: XTrustEntry) -> Result<bool, String> {
    if entry.source.trim().is_empty() {
        return Err("x trust: source must be non-empty".to_owned());
    }
    if entry.approved_how.trim().is_empty() {
        return Err("x trust: approved_how must be non-empty".to_owned());
    }
    let entry = XTrustEntry {
        artifact_sha256: x_trust_normalize_sha256(&entry.artifact_sha256)?,
        capabilities_hash: x_trust_normalize_sha256(&entry.capabilities_hash)?,
        ..entry
    };
    let mut entries = x_trust_read_store(path)?;
    if entries.iter().any(|existing| {
        existing.source == entry.source
            && existing.artifact_sha256 == entry.artifact_sha256
            && existing.capabilities_hash == entry.capabilities_hash
    }) {
        return Ok(false);
    }
    entries.push(entry);
    x_trust_write_store_atomic(path, &entries)?;
    Ok(true)
}

/// List all trust-store entries (backing for `maw x trust ls`).
///
/// # Errors
///
/// Returns an error when the store exists but cannot be read or parsed.
pub fn x_trust_list(path: &Path) -> Result<Vec<XTrustEntry>, String> {
    x_trust_read_store(path)
}

/// Revoke entries matching a selector (backing for `maw x trust revoke`):
/// an exact canonical source string, or a sha256 prefix (≥4 hex chars, with
/// or without the `sha256:` prefix) matched against the artifact hash.
/// Returns the number of entries removed.
///
/// # Errors
///
/// Returns an error for an empty/too-short selector or when the store cannot
/// be read or written.
pub fn x_trust_revoke(path: &Path, selector: &str) -> Result<usize, String> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Err("x trust revoke: selector must be a source or sha256 prefix".to_owned());
    }
    let mut entries = x_trust_read_store(path)?;
    let before = entries.len();
    if entries.iter().any(|entry| entry.source == selector) {
        entries.retain(|entry| entry.source != selector);
    } else {
        let hex = selector.strip_prefix("sha256:").unwrap_or(selector);
        if hex.len() < 4 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(
                "x trust revoke: no source matched and sha256 prefix needs at least 4 hex chars"
                    .to_owned(),
            );
        }
        let hex = hex.to_ascii_lowercase();
        entries.retain(|entry| {
            !entry
                .artifact_sha256
                .strip_prefix("sha256:")
                .unwrap_or(&entry.artifact_sha256)
                .starts_with(&hex)
        });
    }
    let removed = before - entries.len();
    if removed > 0 {
        x_trust_write_store_atomic(path, &entries)?;
    }
    Ok(removed)
}

/// Default trust-store location: `<maw state dir>/x-trust.json`. Host-side
/// only — outside every plugin fs-scope root, so no plugin can self-approve.
#[must_use]
pub fn x_trust_store_path() -> std::path::PathBuf {
    maw_state_path(&real_xdg_env(), &["x-trust.json"])
}

/// Inputs for the first-run TOFU card (text only; WI-8 owns the TTY).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XTrustTofuCard {
    pub verb: String,
    pub source: String,
    pub artifact_sha256: String,
    pub capabilities: Vec<String>,
}

/// Render the first-run prompt text: source, sha12, capabilities list, and
/// the trust once / always / no question. Pure string building — no TTY.
#[must_use]
pub fn render_x_trust_tofu_card(card: &XTrustTofuCard) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "maw x · {} — first run", card.verb);
    let _ = writeln!(output, "  source   {}", card.source);
    let _ = writeln!(
        output,
        "  artifact sha256:{}…  (verified)",
        x_trust_sha12(&card.artifact_sha256)
    );
    if card.capabilities.is_empty() {
        let _ = writeln!(output, "  caps     (none)");
    } else {
        for (index, cap) in card.capabilities.iter().enumerate() {
            if index == 0 {
                let _ = writeln!(output, "  caps     {cap}");
            } else {
                let _ = writeln!(output, "           {cap}");
            }
        }
    }
    let _ = writeln!(
        output,
        "  Trust this plugin (source + exact hash + caps)? [o]nce / [a]lways / [N]o"
    );
    output
}

fn x_trust_read_store(path: &Path) -> Result<Vec<XTrustEntry>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let body = std::fs::read_to_string(path)
        .map_err(|error| format!("x-trust store: read failed: {error}"))?;
    x_trust_parse_store(&body)
}

fn x_trust_parse_store(body: &str) -> Result<Vec<XTrustEntry>, String> {
    serde_json::from_str::<Vec<XTrustEntry>>(body)
        .map_err(|error| format!("x-trust store: invalid json: {error}"))
}

fn x_trust_write_store_atomic(path: &Path, entries: &[XTrustEntry]) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("x-trust store: create parent failed: {error}"))?;
    let body = serde_json::to_string_pretty(entries)
        .map_err(|error| format!("x-trust store: encode failed: {error}"))?;
    let tmp = x_trust_tmp_path(path);
    {
        let mut file = std::fs::File::create(&tmp)
            .map_err(|error| format!("x-trust store: tmp create failed: {error}"))?;
        std::io::Write::write_all(&mut file, body.as_bytes())
            .map_err(|error| format!("x-trust store: tmp write failed: {error}"))?;
        std::io::Write::write_all(&mut file, b"\n")
            .map_err(|error| format!("x-trust store: tmp newline failed: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("x-trust store: tmp sync failed: {error}"))?;
    }
    let parsed = std::fs::read_to_string(&tmp)
        .map_err(|error| format!("x-trust store: tmp validate read failed: {error}"))?;
    let roundtrip = x_trust_parse_store(&parsed)?;
    if roundtrip.len() != entries.len() {
        let _ = std::fs::remove_file(&tmp);
        return Err("x-trust store: tmp validation mismatch".to_owned());
    }
    std::fs::rename(&tmp, path)
        .map_err(|error| format!("x-trust store: atomic rename failed: {error}"))?;
    Ok(())
}

fn x_trust_tmp_path(path: &Path) -> std::path::PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("x-trust.json");
    parent.join(format!(".{name}.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod x_trust_tests {
    use super::*;

    fn x_trust_temp_store(name: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "maw-rs-x-trust-{name}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        root.join("x-trust.json")
    }

    fn sha_a() -> String {
        format!("sha256:{}", "a".repeat(64))
    }

    fn sha_b() -> String {
        format!("sha256:{}", "b".repeat(64))
    }

    fn caps_narrow() -> String {
        x_trust_capabilities_hash(&["fs:read:claude-projects".to_owned()])
    }

    fn caps_wide() -> String {
        x_trust_capabilities_hash(&[
            "fs:read:claude-projects".to_owned(),
            "tmux:raw:kill-pane".to_owned(),
        ])
    }

    fn query(source: &str, sha: &str, caps: &str) -> XTrustQuery {
        XTrustQuery {
            source: source.to_owned(),
            artifact_sha256: sha.to_owned(),
            capabilities_hash: caps.to_owned(),
            is_default_registry_pin: false,
            pinned: true,
        }
    }

    #[test]
    fn x_trust_capabilities_hash_is_order_independent_and_cap_sensitive() {
        let sorted = x_trust_capabilities_hash(&["a:b".to_owned(), "c:d".to_owned()]);
        let reversed = x_trust_capabilities_hash(&["c:d".to_owned(), "a:b".to_owned()]);
        let widened =
            x_trust_capabilities_hash(&["a:b".to_owned(), "c:d".to_owned(), "e:f".to_owned()]);
        assert_eq!(sorted, reversed);
        assert_ne!(sorted, widened);
        assert!(sorted.starts_with("sha256:"), "{sorted}");
        assert_eq!(sorted.len(), "sha256:".len() + 64, "{sorted}");
    }

    #[test]
    fn x_trust_tofu_unknown_source_prompts_then_record_trusts() {
        let store = x_trust_temp_store("tofu");
        let query = query("gh:acme/maw-tools/packages/costs", &sha_a(), &caps_narrow());
        let decision = x_trust_evaluate(&store, &query, 1_000).expect("evaluate");
        match &decision {
            XTrustDecision::NeedsPrompt { reason, pinned } => {
                assert!(reason.contains("TOFU"), "{reason}");
                assert!(
                    reason.contains("gh:acme/maw-tools/packages/costs"),
                    "{reason}"
                );
                assert!(pinned);
            }
            other => panic!("expected NeedsPrompt, got {other:?}"),
        }
        let added = x_trust_record(
            &store,
            XTrustEntry {
                source: query.source.clone(),
                artifact_sha256: query.artifact_sha256.clone(),
                capabilities_hash: query.capabilities_hash.clone(),
                approved_at_ms: 1_000,
                approved_how: X_TRUST_HOW_PROMPT.to_owned(),
            },
        )
        .expect("record");
        assert!(added);
        let decision = x_trust_evaluate(&store, &query, 2_000).expect("evaluate again");
        assert_eq!(
            decision,
            XTrustDecision::Trusted {
                approved_how: X_TRUST_HOW_PROMPT.to_owned()
            }
        );
    }

    #[test]
    fn x_trust_cap_widen_same_wasm_reprompts() {
        // The spec's key test (I3): same artifact sha256, edited manifest caps
        // → different capabilities_hash → NOT trusted, re-prompt.
        let store = x_trust_temp_store("cap-widen");
        let source = "gh:acme/maw-tools/packages/costs";
        x_trust_record(
            &store,
            XTrustEntry {
                source: source.to_owned(),
                artifact_sha256: sha_a(),
                capabilities_hash: caps_narrow(),
                approved_at_ms: 1_000,
                approved_how: X_TRUST_HOW_PROMPT.to_owned(),
            },
        )
        .expect("record");
        let widened = query(source, &sha_a(), &caps_wide());
        let decision = x_trust_evaluate(&store, &widened, 2_000).expect("evaluate");
        match &decision {
            XTrustDecision::NeedsPrompt { reason, .. } => {
                assert!(reason.contains("capabilities changed"), "{reason}");
                assert!(reason.contains("artifact unchanged"), "{reason}");
                assert!(reason.contains(&x_trust_sha12(&sha_a())), "{reason}");
            }
            other => panic!("cap-widen with same wasm must re-prompt, got {other:?}"),
        }
    }

    #[test]
    fn x_trust_sha_change_reprompts() {
        let store = x_trust_temp_store("sha-change");
        let source = "gh:acme/maw-tools/packages/costs";
        x_trust_record(
            &store,
            XTrustEntry {
                source: source.to_owned(),
                artifact_sha256: sha_a(),
                capabilities_hash: caps_narrow(),
                approved_at_ms: 1_000,
                approved_how: X_TRUST_HOW_PROMPT.to_owned(),
            },
        )
        .expect("record");
        let changed = query(source, &sha_b(), &caps_narrow());
        let decision = x_trust_evaluate(&store, &changed, 2_000).expect("evaluate");
        match &decision {
            XTrustDecision::NeedsPrompt { reason, .. } => {
                assert!(reason.contains("artifact sha256 changed"), "{reason}");
                assert!(reason.contains(&x_trust_sha12(&sha_a())), "{reason}");
                assert!(reason.contains(&x_trust_sha12(&sha_b())), "{reason}");
            }
            other => panic!("sha change must re-prompt, got {other:?}"),
        }
    }

    #[test]
    fn x_trust_registry_auto_trust_records_and_trusts() {
        // Nat's locked decision 2: default-registry pins run without a prompt
        // and the approval is recorded as "registry-auto".
        let store = x_trust_temp_store("registry-auto");
        let mut registry_query = query(
            "gh:Soul-Brews-Studio/maw-plugins/packages/20-costs",
            &sha_a(),
            &caps_narrow(),
        );
        registry_query.is_default_registry_pin = true;
        let decision = x_trust_evaluate(&store, &registry_query, 1_000).expect("evaluate");
        assert_eq!(
            decision,
            XTrustDecision::Trusted {
                approved_how: X_TRUST_HOW_REGISTRY_AUTO.to_owned()
            }
        );
        let entries = x_trust_list(&store).expect("list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].approved_how, X_TRUST_HOW_REGISTRY_AUTO);
        assert_eq!(entries[0].approved_at_ms, 1_000);
        // Later resolution of the same triple without the registry flag (e.g.
        // from the trust store, zero network) is trusted via the record.
        registry_query.is_default_registry_pin = false;
        let decision = x_trust_evaluate(&store, &registry_query, 2_000).expect("re-evaluate");
        assert_eq!(
            decision,
            XTrustDecision::Trusted {
                approved_how: X_TRUST_HOW_REGISTRY_AUTO.to_owned()
            }
        );
        // And the record is not duplicated on repeat runs.
        registry_query.is_default_registry_pin = true;
        let _ = x_trust_evaluate(&store, &registry_query, 3_000).expect("repeat");
        assert_eq!(x_trust_list(&store).expect("list").len(), 1);
    }

    #[test]
    fn x_trust_revoke_by_source_and_sha_prefix() {
        let store = x_trust_temp_store("revoke");
        let source_a = "gh:acme/maw-tools/packages/costs";
        let source_b = "gh:other/repo/packages/deploy";
        for (source, sha) in [(source_a, sha_a()), (source_b, sha_b())] {
            x_trust_record(
                &store,
                XTrustEntry {
                    source: source.to_owned(),
                    artifact_sha256: sha,
                    capabilities_hash: caps_narrow(),
                    approved_at_ms: 1_000,
                    approved_how: X_TRUST_HOW_PROMPT.to_owned(),
                },
            )
            .expect("record");
        }
        assert_eq!(x_trust_revoke(&store, source_a).expect("revoke source"), 1);
        let decision = x_trust_evaluate(&store, &query(source_a, &sha_a(), &caps_narrow()), 2_000)
            .expect("evaluate");
        assert!(
            matches!(&decision, XTrustDecision::NeedsPrompt { reason, .. } if reason.contains("TOFU")),
            "revoked source must be back to TOFU, got {decision:?}"
        );
        assert_eq!(x_trust_revoke(&store, "bbbbbbbb").expect("revoke sha"), 1);
        assert!(x_trust_list(&store).expect("list").is_empty());
        assert!(x_trust_revoke(&store, "zzz").is_err());
    }

    #[test]
    fn x_trust_non_interactive_unpinned_refuses() {
        let mut unpinned = query("gh:acme/maw-tools/packages/costs", &sha_a(), &caps_narrow());
        unpinned.pinned = false;
        let decision = x_trust_decide(&[], &unpinned);
        let refused = x_trust_refuse_if_non_interactive(decision.clone(), false);
        match &refused {
            XTrustDecision::Refused { reason } => {
                assert!(reason.contains("unpinned"), "{reason}");
                assert!(reason.contains("--sha256"), "{reason}");
            }
            other => panic!("expected Refused, got {other:?}"),
        }
        // Interactive keeps the prompt; pinned non-interactive also keeps it
        // (the caller may honor --yes per I4).
        assert_eq!(
            x_trust_refuse_if_non_interactive(decision, true),
            x_trust_decide(&[], &unpinned)
        );
        let pinned = query("gh:acme/maw-tools/packages/costs", &sha_a(), &caps_narrow());
        let pinned_decision = x_trust_decide(&[], &pinned);
        assert_eq!(
            x_trust_refuse_if_non_interactive(pinned_decision.clone(), false),
            pinned_decision
        );
    }

    #[test]
    fn x_trust_tofu_card_renders_source_sha12_and_caps() {
        let card = XTrustTofuCard {
            verb: "costs".to_owned(),
            source: "gh:acme/maw-tools/packages/costs".to_owned(),
            artifact_sha256: sha_a(),
            capabilities: vec![
                "fs:read:claude-projects".to_owned(),
                "tmux:raw:kill-pane".to_owned(),
            ],
        };
        let text = render_x_trust_tofu_card(&card);
        assert!(text.contains("maw x · costs — first run"), "{text}");
        assert!(text.contains("source   gh:acme/maw-tools/packages/costs"), "{text}");
        assert!(text.contains("artifact sha256:aaaaaaaaaaaa…"), "{text}");
        assert!(text.contains("caps     fs:read:claude-projects"), "{text}");
        assert!(text.contains("           tmux:raw:kill-pane"), "{text}");
        assert!(
            text.contains("Trust this plugin (source + exact hash + caps)? [o]nce / [a]lways / [N]o"),
            "{text}"
        );
        let empty = render_x_trust_tofu_card(&XTrustTofuCard {
            capabilities: Vec::new(),
            ..card
        });
        assert!(empty.contains("caps     (none)"), "{empty}");
    }

    #[test]
    fn x_trust_record_normalizes_and_rejects_malformed() {
        let store = x_trust_temp_store("normalize");
        let added = x_trust_record(
            &store,
            XTrustEntry {
                source: "gh:acme/maw-tools/packages/costs".to_owned(),
                artifact_sha256: "a".repeat(64),
                capabilities_hash: caps_narrow(),
                approved_at_ms: 1_000,
                approved_how: X_TRUST_HOW_YES_FLAG.to_owned(),
            },
        )
        .expect("record bare hex");
        assert!(added);
        let entries = x_trust_list(&store).expect("list");
        assert_eq!(entries[0].artifact_sha256, sha_a());
        assert!(x_trust_record(
            &store,
            XTrustEntry {
                source: "gh:acme/maw-tools/packages/costs".to_owned(),
                artifact_sha256: "not-hex".to_owned(),
                capabilities_hash: caps_narrow(),
                approved_at_ms: 1_000,
                approved_how: X_TRUST_HOW_PROMPT.to_owned(),
            },
        )
        .is_err());
        assert!(x_trust_normalize_sha256(&"A".repeat(64)).is_err());
        assert_eq!(
            x_trust_normalize_sha256(&sha_a()).expect("prefixed ok"),
            sha_a()
        );
    }
}
