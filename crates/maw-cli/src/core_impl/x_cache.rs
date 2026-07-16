// x_cache.rs — mawx WI-6: content-addressed store (CAS) for fetched plugin
// artifacts (ψ/design/mawx-spec.md §2.4).
//
// Layout: `<cache-root>/x/<sha256[0..2]>/<sha256>/` holding the artifact +
// `.meta.json` (source canonical string, verb, version, artifact file name,
// fetched_at, last_used, size). The cache root comes from maw-xdg's
// `maw_cache_path(env, ["x"])` — cache is disposable, trust is not.
//
// Discipline:
// - put: the bytes are hashed BEFORE anything lands in the CAS; a pin
//   mismatch refuses the store (the staged temp file is quarantined).
// - get: verify-on-read — the artifact is re-hashed on every read; a
//   mismatch refuses and names the poisoned entry, never returning bad bytes.
// - rm/gc: entries are moved to a `.trash/` subdir, never directly deleted
//   (fleet no-rm discipline).
// - Determinism: no clock reads in this module — callers thread timestamps
//   (seconds) into `put`/`get`/`gc`. Wiring into verbs is WI-8.

/// Meta sidecar file name inside each CAS entry dir.
pub const X_CACHE_META_FILE: &str = ".meta.json";

/// Quarantine subdir under the cache root — `rm`/`gc`/refused `put`s move
/// entries here instead of deleting them.
pub const X_CACHE_TRASH_DIR: &str = ".trash";

/// Staging subdir under the cache root for not-yet-verified `put` bytes.
pub const X_CACHE_TMP_DIR: &str = ".tmp";

/// Default gc guard: entries used within the last 7 days are never evicted
/// (spec §2.4), even under `max_size` pressure.
pub const X_CACHE_GC_PROTECT_RECENT_SECS: u64 = 7 * 24 * 60 * 60;

/// Process-monotonic counter for unique temp/trash names (never pid alone).
static X_CACHE_UNIQUE_COUNTER: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

fn x_cache_unique_suffix() -> String {
    let counter = X_CACHE_UNIQUE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    format!("{}-{counter}", std::process::id())
}

/// `.meta.json` contents for one CAS entry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct XCacheMeta {
    /// Canonical source string (e.g. `gh:owner/repo/packages/costs@<commit>`).
    pub source: String,
    /// CLI verb the artifact serves (manifest `cli.command`).
    pub verb: String,
    /// Package version at fetch time.
    pub version: String,
    /// Artifact file name inside the entry dir.
    pub artifact: String,
    /// Unix seconds when the artifact was fetched (caller-threaded).
    pub fetched_at: u64,
    /// Unix seconds of the last verified read (caller-threaded).
    pub last_used: u64,
    /// Artifact size in bytes.
    pub size: u64,
}

/// One resolved CAS entry: normalized pin, on-disk locations, and meta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XCacheEntry {
    /// Normalized `sha256:<64 lowercase hex>` pin — the entry identity.
    pub sha256: String,
    /// Entry directory: `<root>/<hex[0..2]>/<hex>/`.
    pub dir: std::path::PathBuf,
    /// Artifact file path inside [`Self::dir`].
    pub artifact: std::path::PathBuf,
    /// Parsed `.meta.json`.
    pub meta: XCacheMeta,
}

/// Everything `x_cache_put` needs to stage one artifact.
#[derive(Debug, Clone, Copy)]
pub struct XCachePutRequest<'a> {
    /// Artifact file name to store (bare file name, no separators, must not
    /// start with `.`).
    pub artifact_name: &'a str,
    /// Raw artifact bytes.
    pub bytes: &'a [u8],
    /// Expected pin — 64 lowercase hex chars, optionally `sha256:`-prefixed.
    pub expected_sha256: &'a str,
    /// Canonical source string recorded in the meta.
    pub source: &'a str,
    /// CLI verb recorded in the meta.
    pub verb: &'a str,
    /// Package version recorded in the meta.
    pub version: &'a str,
    /// Unix seconds of the fetch (also the initial `last_used`).
    pub fetched_at: u64,
}

/// Tuning for [`plan_x_cache_gc`] / [`x_cache_gc`].
#[derive(Debug, Clone, Copy)]
pub struct XCacheGcOptions {
    /// Evict entries whose `now - last_used` exceeds this many seconds.
    pub max_age_secs: Option<u64>,
    /// Evict LRU entries until the kept total is at most this many bytes.
    pub max_size_bytes: Option<u64>,
    /// Entries used within this many seconds are never evicted.
    pub protect_recent_secs: u64,
}

impl Default for XCacheGcOptions {
    fn default() -> Self {
        Self {
            max_age_secs: None,
            max_size_bytes: None,
            protect_recent_secs: X_CACHE_GC_PROTECT_RECENT_SECS,
        }
    }
}

/// Deterministic gc plan: what would be (or was) evicted vs kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XCacheGcPlan {
    /// Entries selected for eviction, LRU-first.
    pub evict: Vec<XCacheEntry>,
    /// Entries kept, LRU-first.
    pub keep: Vec<XCacheEntry>,
    /// Total bytes across [`Self::evict`].
    pub reclaimed_bytes: u64,
    /// Total bytes across [`Self::keep`].
    pub kept_bytes: u64,
}

/// CAS root for `maw x`: `maw_cache_path(env, ["x"])` — honors
/// `MAW_CACHE_DIR`/XDG and stays distinct from the data dir.
#[must_use]
pub fn x_cache_root(env: &MawXdgEnv) -> std::path::PathBuf {
    maw_cache_path(env, &["x"])
}

/// Entry dir for a bare (unprefixed) 64-hex digest: `<root>/<hex[0..2]>/<hex>`.
#[must_use]
pub fn x_cache_entry_dir(root: &std::path::Path, hex: &str) -> std::path::PathBuf {
    root.join(&hex[..2]).join(hex)
}

fn x_cache_is_lower_hex(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
}

/// Normalize a pin to `sha256:<64 lowercase hex>` (reuses the install-route
/// normalizer so both gates accept exactly the same spellings).
fn normalize_x_cache_sha256(value: &str) -> Result<String, String> {
    normalize_plugin_install_sha256(value).map_err(|_| {
        "x cache: sha256 must be 64 lowercase hex chars (optionally 'sha256:'-prefixed)".to_owned()
    })
}

fn x_cache_hex(normalized_pin: &str) -> &str {
    normalized_pin.strip_prefix("sha256:").unwrap_or(normalized_pin)
}

fn read_x_cache_meta(dir: &std::path::Path) -> Option<XCacheMeta> {
    let text = std::fs::read_to_string(dir.join(X_CACHE_META_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_x_cache_meta(dir: &std::path::Path, meta: &XCacheMeta) -> Result<(), String> {
    let text = serde_json::to_string_pretty(meta)
        .map_err(|error| format!("x cache: serialize meta: {error}"))?;
    std::fs::write(dir.join(X_CACHE_META_FILE), text)
        .map_err(|error| format!("x cache: write {}: {error}", dir.join(X_CACHE_META_FILE).display()))
}

fn read_x_cache_entry(dir: &std::path::Path, hex: &str) -> Option<XCacheEntry> {
    let meta = read_x_cache_meta(dir)?;
    let artifact = dir.join(&meta.artifact);
    Some(XCacheEntry {
        sha256: format!("sha256:{hex}"),
        dir: dir.to_path_buf(),
        artifact,
        meta,
    })
}

/// Move a path into `<root>/.trash/` under a unique name — the CAS never
/// directly deletes user bytes (fleet no-rm discipline).
///
/// # Errors
///
/// Returns the filesystem error text if the trash dir cannot be created or
/// the rename fails.
fn x_cache_move_to_trash(
    root: &std::path::Path,
    from: &std::path::Path,
    label: &str,
) -> Result<std::path::PathBuf, String> {
    let trash = root.join(X_CACHE_TRASH_DIR);
    std::fs::create_dir_all(&trash)
        .map_err(|error| format!("x cache: create {}: {error}", trash.display()))?;
    let destination = trash.join(format!("{label}-{}", x_cache_unique_suffix()));
    std::fs::rename(from, &destination).map_err(|error| {
        format!(
            "x cache: quarantine {} -> {}: {error}",
            from.display(),
            destination.display()
        )
    })?;
    Ok(destination)
}

/// Store artifact bytes in the CAS after verifying them against the expected
/// pin. The bytes are staged to `<root>/.tmp/`, hashed there, and only
/// renamed into `<root>/<hex[0..2]>/<hex>/<artifact_name>` when the digest
/// matches — a mismatch refuses the store and quarantines the staged bytes.
///
/// Idempotent: a healthy existing entry (artifact re-hashes to the pin) is
/// returned untouched; a poisoned existing artifact is quarantined and
/// replaced by the freshly verified bytes.
///
/// # Errors
///
/// Returns an error when the artifact name is unsafe, the pin is malformed,
/// the observed digest does not match the expected pin, or a filesystem
/// operation fails.
pub fn x_cache_put(
    root: &std::path::Path,
    request: &XCachePutRequest<'_>,
) -> Result<XCacheEntry, String> {
    let name = request.artifact_name;
    if name.is_empty()
        || name.starts_with('.')
        || name.contains('/')
        || name.contains('\\')
    {
        return Err(format!(
            "x cache: artifact name must be a bare file name not starting with '.': {name:?}"
        ));
    }
    let pin = normalize_x_cache_sha256(request.expected_sha256)?;
    let hex = x_cache_hex(&pin).to_owned();

    let tmp_dir = root.join(X_CACHE_TMP_DIR);
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|error| format!("x cache: create {}: {error}", tmp_dir.display()))?;
    let staged = tmp_dir.join(format!("put-{}", x_cache_unique_suffix()));
    std::fs::write(&staged, request.bytes)
        .map_err(|error| format!("x cache: stage {}: {error}", staged.display()))?;
    let observed = hash_file(&staged)
        .map_err(|error| format!("x cache: hash staged artifact: {error}"))?;
    if observed != pin {
        let _ = x_cache_move_to_trash(root, &staged, "refused-put");
        return Err(format!(
            "x cache: refusing to store '{name}' — sha256 mismatch.\n  expected: {pin}\n  observed: {observed}"
        ));
    }

    let dir = x_cache_entry_dir(root, &hex);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("x cache: create {}: {error}", dir.display()))?;
    let destination = dir.join(name);
    let healthy = destination.is_file()
        && hash_file(&destination).is_ok_and(|existing| existing == pin);
    if healthy {
        // Immutable CAS: the verified bytes are already present.
        let _ = x_cache_move_to_trash(root, &staged, "duplicate-put");
    } else {
        if destination.exists() {
            x_cache_move_to_trash(root, &destination, "poisoned-put")?;
        }
        std::fs::rename(&staged, &destination)
            .map_err(|error| format!("x cache: place {}: {error}", destination.display()))?;
    }

    let fresh = XCacheMeta {
        source: request.source.to_owned(),
        verb: request.verb.to_owned(),
        version: request.version.to_owned(),
        artifact: name.to_owned(),
        fetched_at: request.fetched_at,
        last_used: request.fetched_at,
        size: u64::try_from(request.bytes.len()).unwrap_or(u64::MAX),
    };
    let meta = match (healthy, read_x_cache_meta(&dir)) {
        (true, Some(existing)) => existing,
        _ => {
            write_x_cache_meta(&dir, &fresh)?;
            fresh
        }
    };
    Ok(XCacheEntry {
        sha256: pin,
        artifact: dir.join(&meta.artifact),
        dir,
        meta,
    })
}

/// Fetch a CAS entry by pin, verifying on read: the artifact is re-hashed
/// and a mismatch refuses with a poisoned-entry report — bad bytes are never
/// returned. A successful read stamps `last_used = now` back into the meta.
///
/// # Errors
///
/// Returns an error when the pin is malformed, the entry is not cached, the
/// artifact file is missing, the re-hash does not match the pin (poisoned
/// entry), or the meta write-back fails.
pub fn x_cache_get(
    root: &std::path::Path,
    sha256: &str,
    now: u64,
) -> Result<XCacheEntry, String> {
    let pin = normalize_x_cache_sha256(sha256)?;
    let hex = x_cache_hex(&pin).to_owned();
    let dir = x_cache_entry_dir(root, &hex);
    let Some(mut entry) = read_x_cache_entry(&dir, &hex) else {
        return Err(format!("x cache: sha256:{hex} is not cached"));
    };
    if !entry.artifact.is_file() {
        return Err(format!(
            "x cache: poisoned entry '{}' sha256:{hex} — artifact '{}' is missing; refusing. quarantine with: maw x rm {}",
            entry.meta.verb,
            entry.meta.artifact,
            &hex[..12]
        ));
    }
    let observed = hash_file(&entry.artifact)
        .map_err(|error| format!("x cache: hash cached artifact: {error}"))?;
    if observed != pin {
        return Err(format!(
            "x cache: poisoned entry '{}' sha256:{hex} — artifact re-hash mismatch; refusing to return bytes.\n  expected: {pin}\n  observed: {observed}\n  quarantine with: maw x rm {}",
            entry.meta.verb,
            &hex[..12]
        ));
    }
    entry.meta.last_used = now;
    write_x_cache_meta(&dir, &entry.meta)?;
    Ok(entry)
}

/// List all readable CAS entries, sorted by sha256. Entries with a missing
/// or unreadable `.meta.json` are skipped (they are unreachable by `get`);
/// `.tmp/` and `.trash/` are never listed.
///
/// # Errors
///
/// Returns the filesystem error text when a cache directory cannot be read.
pub fn x_cache_ls(root: &std::path::Path) -> Result<Vec<XCacheEntry>, String> {
    let mut entries = Vec::new();
    if !root.is_dir() {
        return Ok(entries);
    }
    let shards = std::fs::read_dir(root)
        .map_err(|error| format!("x cache: read {}: {error}", root.display()))?;
    for shard in shards {
        let shard = shard.map_err(|error| format!("x cache: read {}: {error}", root.display()))?;
        let shard_name = shard.file_name();
        let Some(shard_name) = shard_name.to_str() else {
            continue;
        };
        if shard_name.len() != 2 || !x_cache_is_lower_hex(shard_name) || !shard.path().is_dir() {
            continue;
        }
        let inner = std::fs::read_dir(shard.path())
            .map_err(|error| format!("x cache: read {}: {error}", shard.path().display()))?;
        for candidate in inner {
            let candidate =
                candidate.map_err(|error| format!("x cache: read {}: {error}", root.display()))?;
            let hex = candidate.file_name();
            let Some(hex) = hex.to_str() else {
                continue;
            };
            if hex.len() != 64
                || !x_cache_is_lower_hex(hex)
                || !hex.starts_with(shard_name)
                || !candidate.path().is_dir()
            {
                continue;
            }
            if let Some(entry) = read_x_cache_entry(&candidate.path(), hex) {
                entries.push(entry);
            }
        }
    }
    entries.sort_by(|left, right| left.sha256.cmp(&right.sha256));
    Ok(entries)
}

/// Remove entries matching `needle` — the meta `verb`, the artifact file
/// name, or a sha256 hex prefix (optionally `sha256:`-prefixed). Matched
/// entry dirs are moved to `<root>/.trash/`, never deleted.
///
/// # Errors
///
/// Returns an error when the cache cannot be listed, nothing matches, or a
/// quarantine move fails.
pub fn x_cache_rm(root: &std::path::Path, needle: &str) -> Result<Vec<XCacheEntry>, String> {
    if needle.is_empty() {
        return Err("x cache: rm needs a name or sha256 prefix".to_owned());
    }
    let hex_needle = needle.strip_prefix("sha256:").unwrap_or(needle);
    let by_prefix = x_cache_is_lower_hex(hex_needle);
    let entries = x_cache_ls(root)?;
    let mut removed = Vec::new();
    for entry in entries {
        let hex = x_cache_hex(&entry.sha256);
        let matched = entry.meta.verb == needle
            || entry.meta.artifact == needle
            || (by_prefix && hex.starts_with(hex_needle));
        if matched {
            removed.push(entry);
        }
    }
    if removed.is_empty() {
        return Err(format!("x cache: nothing matches '{needle}'"));
    }
    for entry in &removed {
        x_cache_move_to_trash(root, &entry.dir, x_cache_hex(&entry.sha256))?;
    }
    Ok(removed)
}

/// Pure LRU gc planner (no IO, no clock): order entries by `last_used`
/// (oldest first), evict past `max_age_secs`, then keep evicting LRU-first
/// until the kept total fits `max_size_bytes`. Entries used within
/// `protect_recent_secs` are never evicted.
#[must_use]
pub fn plan_x_cache_gc(
    entries: Vec<XCacheEntry>,
    now: u64,
    options: &XCacheGcOptions,
) -> XCacheGcPlan {
    let mut ordered = entries;
    ordered.sort_by(|left, right| {
        left.meta
            .last_used
            .cmp(&right.meta.last_used)
            .then_with(|| left.sha256.cmp(&right.sha256))
    });
    let protected = |entry: &XCacheEntry| {
        now.saturating_sub(entry.meta.last_used) < options.protect_recent_secs
    };
    let mut evict_flags = vec![false; ordered.len()];
    if let Some(max_age) = options.max_age_secs {
        for (index, entry) in ordered.iter().enumerate() {
            if !protected(entry) && now.saturating_sub(entry.meta.last_used) > max_age {
                evict_flags[index] = true;
            }
        }
    }
    if let Some(max_size) = options.max_size_bytes {
        let mut kept_total: u64 = ordered
            .iter()
            .zip(&evict_flags)
            .filter(|(_, &evicted)| !evicted)
            .map(|(entry, _)| entry.meta.size)
            .sum();
        for (index, entry) in ordered.iter().enumerate() {
            if kept_total <= max_size {
                break;
            }
            if evict_flags[index] || protected(entry) {
                continue;
            }
            evict_flags[index] = true;
            kept_total = kept_total.saturating_sub(entry.meta.size);
        }
    }
    let mut plan = XCacheGcPlan {
        evict: Vec::new(),
        keep: Vec::new(),
        reclaimed_bytes: 0,
        kept_bytes: 0,
    };
    for (entry, evicted) in ordered.into_iter().zip(evict_flags) {
        if evicted {
            plan.reclaimed_bytes = plan.reclaimed_bytes.saturating_add(entry.meta.size);
            plan.evict.push(entry);
        } else {
            plan.kept_bytes = plan.kept_bytes.saturating_add(entry.meta.size);
            plan.keep.push(entry);
        }
    }
    plan
}

/// Run gc over the CAS: list entries, plan via [`plan_x_cache_gc`], and —
/// unless `dry_run` — move every planned eviction to `<root>/.trash/`. The
/// returned plan is identical in dry-run and real mode for the same inputs.
///
/// # Errors
///
/// Returns an error when the cache cannot be listed or a quarantine move
/// fails.
pub fn x_cache_gc(
    root: &std::path::Path,
    now: u64,
    options: &XCacheGcOptions,
    dry_run: bool,
) -> Result<XCacheGcPlan, String> {
    let plan = plan_x_cache_gc(x_cache_ls(root)?, now, options);
    if !dry_run {
        for entry in &plan.evict {
            x_cache_move_to_trash(root, &entry.dir, x_cache_hex(&entry.sha256))?;
        }
    }
    Ok(plan)
}

/// Verify a package-shaped CAS entry dir through the ONE shared install
/// verify fork (WI-1 [`verify_package_dir`]): manifest pin present,
/// traversal-guarded artifact, digest match, `--sha256` mismatch fatal.
/// Returns `true` for a verified wasm package, `false` when the dir is not a
/// wasm package (no plugin.json / non-wasm target) — the caller owns that
/// route.
///
/// # Errors
///
/// Propagates the shared fork's refusal text (missing pin, hash mismatch,
/// traversal, malformed manifest).
pub fn x_cache_verify_package_dir(
    dir: &std::path::Path,
    expected_sha256: Option<&str>,
) -> Result<bool, String> {
    match verify_package_dir(dir, expected_sha256, false, false)? {
        ResolvedPackage::Wasm(_) => Ok(true),
        ResolvedPackage::NotWasm => Ok(false),
    }
}

#[cfg(test)]
mod x_cache_tests {
    use super::{
        plan_x_cache_gc, x_cache_entry_dir, x_cache_gc, x_cache_get, x_cache_ls, x_cache_put,
        x_cache_rm, x_cache_verify_package_dir, XCacheEntry, XCacheGcOptions, XCachePutRequest,
        X_CACHE_TRASH_DIR,
    };

    static TEMP_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// Unique-per-test temp root: temp_dir + pid + a monotonic counter
    /// (never pid alone — parallel tests in one process share the pid).
    fn temp_root(label: &str) -> std::path::PathBuf {
        let counter = TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "maw-rs-x-cache-{label}-{}-{counter}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp root");
        dir
    }

    /// sha256 of raw bytes via the shared file hasher (no extra hash dep).
    fn sha_of(root: &std::path::Path, bytes: &[u8]) -> String {
        let counter = TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let scratch = root.join(format!("scratch-{counter}"));
        std::fs::write(&scratch, bytes).expect("scratch");
        let sha = maw_plugin_manifest::hash_file(&scratch).expect("hash scratch");
        let _ = std::fs::remove_file(&scratch);
        sha
    }

    fn put_fixture(
        root: &std::path::Path,
        verb: &str,
        bytes: &[u8],
        fetched_at: u64,
    ) -> XCacheEntry {
        let pin = sha_of(root, bytes);
        x_cache_put(
            root,
            &XCachePutRequest {
                artifact_name: "plugin.wasm",
                bytes,
                expected_sha256: &pin,
                source: "gh:acme/maw-tools/packages/demo@3f9a2c1",
                verb,
                version: "1.0.0",
                fetched_at,
            },
        )
        .expect("put fixture")
    }

    fn trash_entries(root: &std::path::Path) -> usize {
        std::fs::read_dir(root.join(X_CACHE_TRASH_DIR))
            .map(|dir| dir.count())
            .unwrap_or(0)
    }

    #[test]
    fn put_get_round_trip_updates_last_used() {
        let root = temp_root("round-trip");
        let bytes: &[u8] = b"\0asm\x01\x00\x00\x00x-cache-round-trip";
        let entry = put_fixture(&root, "costs", bytes, 1_000);

        let hex = entry.sha256.strip_prefix("sha256:").expect("prefixed");
        assert_eq!(entry.dir, x_cache_entry_dir(&root, hex));
        assert_eq!(std::fs::read(&entry.artifact).expect("artifact"), bytes);
        assert_eq!(entry.meta.size, bytes.len() as u64);
        assert_eq!(entry.meta.fetched_at, 1_000);
        assert_eq!(entry.meta.last_used, 1_000);
        assert_eq!(entry.meta.verb, "costs");

        let got = x_cache_get(&root, &entry.sha256, 2_000).expect("get");
        assert_eq!(got.sha256, entry.sha256);
        assert_eq!(std::fs::read(&got.artifact).expect("artifact"), bytes);
        assert_eq!(got.meta.last_used, 2_000);

        // last_used persisted to disk, visible through ls.
        let listed = x_cache_ls(&root).expect("ls");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].sha256, entry.sha256);
        assert_eq!(listed[0].meta.last_used, 2_000);

        // Idempotent duplicate put keeps the entry (and its meta) intact.
        let again = put_fixture(&root, "costs", bytes, 5_000);
        assert_eq!(again.sha256, entry.sha256);
        assert_eq!(again.meta.last_used, 2_000, "healthy entry meta untouched");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn put_refuses_wrong_hash_and_stores_nothing() {
        let root = temp_root("refuse-put");
        let bytes: &[u8] = b"\0asm\x01\x00\x00\x00x-cache-wrong-hash";
        let wrong = sha_of(&root, b"different bytes entirely");
        let error = x_cache_put(
            &root,
            &XCachePutRequest {
                artifact_name: "plugin.wasm",
                bytes,
                expected_sha256: &wrong,
                source: "gh:acme/maw-tools/packages/demo@3f9a2c1",
                verb: "costs",
                version: "1.0.0",
                fetched_at: 1_000,
            },
        )
        .expect_err("refused");
        assert!(error.contains("sha256 mismatch"), "{error}");
        assert!(error.contains("refusing to store"), "{error}");
        assert!(x_cache_ls(&root).expect("ls").is_empty(), "nothing stored");
        assert_eq!(trash_entries(&root), 1, "staged bytes quarantined, not deleted");

        // Malformed pin refused before any bytes are staged.
        let error = x_cache_put(
            &root,
            &XCachePutRequest {
                artifact_name: "plugin.wasm",
                bytes,
                expected_sha256: "not-a-sha",
                source: "s",
                verb: "v",
                version: "1",
                fetched_at: 0,
            },
        )
        .expect_err("malformed pin");
        assert!(error.contains("64 lowercase hex"), "{error}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn poisoned_cas_get_refuses_and_names_entry() {
        let root = temp_root("poisoned");
        let bytes: &[u8] = b"\0asm\x01\x00\x00\x00x-cache-poison-me";
        let entry = put_fixture(&root, "costs", bytes, 1_000);

        // Corrupt the stored artifact on disk.
        std::fs::write(&entry.artifact, b"\0asm\x01\x00\x00\x00TAMPERED").expect("tamper");

        let hex = entry.sha256.strip_prefix("sha256:").expect("prefixed");
        let error = x_cache_get(&root, &entry.sha256, 2_000).expect_err("poisoned get refused");
        assert!(error.contains("poisoned entry"), "{error}");
        assert!(error.contains("'costs'"), "names the entry: {error}");
        assert!(error.contains(&hex[..12]), "names the sha: {error}");
        assert!(error.contains("refusing"), "{error}");

        // Still refuses on retry — bad bytes are never returned.
        let error = x_cache_get(&root, &entry.sha256, 3_000).expect_err("still refused");
        assert!(error.contains("poisoned entry"), "{error}");

        // A fresh put with the correct bytes self-heals: the poisoned
        // artifact is quarantined, never deleted.
        let healed = put_fixture(&root, "costs", bytes, 4_000);
        assert_eq!(healed.sha256, entry.sha256);
        assert!(trash_entries(&root) >= 1, "poisoned artifact quarantined");
        let got = x_cache_get(&root, &entry.sha256, 5_000).expect("healed get");
        assert_eq!(std::fs::read(&got.artifact).expect("artifact"), bytes);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rm_by_sha_prefix_and_by_name_moves_to_trash() {
        let root = temp_root("rm");
        let alpha = put_fixture(&root, "alpha-verb", b"x-cache-rm-alpha", 1_000);
        let beta = put_fixture(&root, "beta-verb", b"x-cache-rm-beta", 2_000);
        assert_eq!(x_cache_ls(&root).expect("ls").len(), 2);

        // rm by sha prefix.
        let hex = alpha.sha256.strip_prefix("sha256:").expect("prefixed");
        let removed = x_cache_rm(&root, &hex[..10]).expect("rm by prefix");
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].sha256, alpha.sha256);
        assert!(!alpha.dir.exists(), "entry dir moved away");
        assert_eq!(trash_entries(&root), 1, "moved to trash, not deleted");
        let listed = x_cache_ls(&root).expect("ls");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].sha256, beta.sha256);

        // rm by verb name.
        let removed = x_cache_rm(&root, "beta-verb").expect("rm by name");
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].sha256, beta.sha256);
        assert!(x_cache_ls(&root).expect("ls").is_empty());
        assert_eq!(trash_entries(&root), 2);

        // No match → error.
        let error = x_cache_rm(&root, "no-such-thing").expect_err("no match");
        assert!(error.contains("nothing matches"), "{error}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn gc_lru_respects_max_age_max_size_and_dry_run_plan_matches_actual() {
        let root = temp_root("gc");
        // Sizes 10/20/30 bytes; last_used 100/200/300 (LRU order).
        let oldest = put_fixture(&root, "verb-old", &[b'a'; 10], 100);
        let middle = put_fixture(&root, "verb-mid", &[b'b'; 20], 200);
        let newest = put_fixture(&root, "verb-new", &[b'c'; 30], 300);
        let now = 400;

        // max-age only: age(a)=300 > 250 → evict a; b (200) and c (100) stay.
        let age_options = XCacheGcOptions {
            max_age_secs: Some(250),
            max_size_bytes: None,
            protect_recent_secs: 0,
        };
        let dry = x_cache_gc(&root, now, &age_options, true).expect("dry run");
        assert_eq!(
            dry.evict.iter().map(|entry| entry.sha256.clone()).collect::<Vec<_>>(),
            vec![oldest.sha256.clone()]
        );
        assert_eq!(dry.reclaimed_bytes, 10);
        assert_eq!(dry.kept_bytes, 50);
        assert_eq!(x_cache_ls(&root).expect("ls").len(), 3, "dry run evicts nothing");

        // Real run matches the dry-run plan exactly.
        let real = x_cache_gc(&root, now, &age_options, false).expect("gc");
        assert_eq!(real, dry, "dry-run plan matches actual");
        assert!(!oldest.dir.exists(), "evicted entry moved away");
        assert_eq!(trash_entries(&root), 1, "evicted to trash, not deleted");
        let listed = x_cache_ls(&root).expect("ls");
        assert_eq!(
            listed.iter().map(|entry| entry.sha256.clone()).collect::<Vec<_>>(),
            {
                let mut keep: Vec<_> =
                    real.keep.iter().map(|entry| entry.sha256.clone()).collect();
                keep.sort();
                keep
            }
        );

        // max-size only: kept total 50 > 35 → evict LRU-first: b (kept 30).
        let size_options = XCacheGcOptions {
            max_age_secs: None,
            max_size_bytes: Some(35),
            protect_recent_secs: 0,
        };
        let plan = x_cache_gc(&root, now, &size_options, false).expect("gc size");
        assert_eq!(
            plan.evict.iter().map(|entry| entry.sha256.clone()).collect::<Vec<_>>(),
            vec![middle.sha256.clone()]
        );
        assert_eq!(plan.kept_bytes, 30);
        let listed = x_cache_ls(&root).expect("ls");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].sha256, newest.sha256);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn gc_never_evicts_recently_used_entries() {
        let root = temp_root("gc-protect");
        let entry = put_fixture(&root, "fresh-verb", b"x-cache-gc-protect", 350);
        // Age 50 < protect 100 → immune to both max-age and max-size.
        let options = XCacheGcOptions {
            max_age_secs: Some(10),
            max_size_bytes: Some(1),
            protect_recent_secs: 100,
        };
        let plan = plan_x_cache_gc(x_cache_ls(&root).expect("ls"), 400, &options);
        assert!(plan.evict.is_empty(), "protected entry never evicted");
        assert_eq!(plan.keep.len(), 1);
        assert_eq!(plan.keep[0].sha256, entry.sha256);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verify_package_dir_seam_delegates_to_shared_fork() {
        let _guard = super::env_test_lock();
        let root = temp_root("verify-seam");
        let old = std::env::var_os("MAW_PLUGINS_LOCK");
        std::env::set_var("MAW_PLUGINS_LOCK", root.join("plugins.lock"));

        // Not package-shaped (no plugin.json) → the caller owns the route.
        let bare = root.join("bare");
        std::fs::create_dir_all(&bare).expect("bare dir");
        assert_eq!(x_cache_verify_package_dir(&bare, None), Ok(false));

        // A pinned wasm package verifies through the WI-1 fork.
        let package = root.join("pkg");
        std::fs::create_dir_all(&package).expect("package dir");
        std::fs::write(package.join("plugin.wasm"), b"\0asm\x01\x00\x00\x00x-cache-seam")
            .expect("wasm artifact");
        let sha256 = maw_plugin_manifest::hash_file(&package.join("plugin.wasm")).expect("hash");
        std::fs::write(
            package.join("plugin.json"),
            format!(
                r#"{{"name":"x-cache-seam-demo","version":"1.0.0","target":"wasm","sdk":"*","entry":{{"kind":"wasm","path":"plugin.wasm","export":"handle"}},"wasm":"./plugin.wasm","artifact":{{"path":"plugin.wasm","sha256":"{sha256}"}},"cli":{{"command":"x-cache-seam-demo"}}}}"#
            ),
        )
        .expect("manifest");
        assert_eq!(x_cache_verify_package_dir(&package, Some(&sha256)), Ok(true));

        // Tampered artifact refuses via the same shared fork.
        std::fs::write(package.join("plugin.wasm"), b"\0asm\x01\x00\x00\x00TAMPERED")
            .expect("tamper");
        let error = x_cache_verify_package_dir(&package, Some(&sha256)).expect_err("refused");
        assert!(error.contains("sha256 mismatch"), "{error}");

        match old {
            Some(value) => std::env::set_var("MAW_PLUGINS_LOCK", value),
            None => std::env::remove_var("MAW_PLUGINS_LOCK"),
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
