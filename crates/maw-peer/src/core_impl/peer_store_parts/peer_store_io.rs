fn readable_peer_store_path(env: &PeerStoreEnv) -> PathBuf {
    let primary = peer_store_path(env);
    if primary.exists() {
        return primary;
    }
    legacy_peer_store_path(env)
        .filter(|path| path.exists())
        .unwrap_or(primary)
}

/// Load peers with stale tmp cleanup and corruption quarantine.
#[must_use]
pub fn load_peer_store(env: &PeerStoreEnv) -> PeerStoreFile {
    clear_stale_peer_store_tmp(env);
    let path = readable_peer_store_path(env);
    if !path.exists() {
        return empty_peer_store();
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return empty_peer_store();
    };
    match parse_peer_store(&raw) {
        Ok(store) => store,
        Err(error) => {
            let aside = corrupt_peer_store_path(&path);
            let _ = fs::rename(&path, aside);
            eprintln!(
                "\u{1b}[33m⚠\u{1b}[0m peers store at {} failed to parse ({error}); moved aside",
                path.display()
            );
            empty_peer_store()
        }
    }
}

/// Save peers via temp-file then rename, mirroring maw-js writeAtomic.
///
/// # Errors
///
/// Returns directory creation, JSON serialization, write, or rename failures.
pub fn save_peer_store(env: &PeerStoreEnv, data: &PeerStoreFile) -> io::Result<()> {
    let path = peer_store_path(env);
    create_peer_store_parent_dir(&path)?;
    write_peer_store_atomic(&path, data)
}

/// Read-modify-write peers, re-reading current contents before mutation.
///
/// # Errors
///
/// Returns directory creation, JSON serialization, write, or rename failures.
pub fn mutate_peer_store(
    env: &PeerStoreEnv,
    mutate: impl FnOnce(&mut PeerStoreFile),
) -> io::Result<PeerStoreFile> {
    let path = peer_store_path(env);
    create_peer_store_parent_dir(&path)?;
    let read_path = if path.exists() {
        path.clone()
    } else {
        readable_peer_store_path(env)
    };
    let mut store = read_peer_store_unlocked(&read_path);
    mutate(&mut store);
    write_peer_store_atomic(&path, &store)?;
    Ok(store)
}

fn create_peer_store_parent_dir(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) => fs::create_dir_all(parent),
        None => Ok(()),
    }
}

/// Best-effort stale `.tmp` cleanup for primary and legacy peer stores.
pub fn clear_stale_peer_store_tmp(env: &PeerStoreEnv) {
    for path in [Some(peer_store_path(env)), legacy_peer_store_path(env)]
        .into_iter()
        .flatten()
    {
        let _ = fs::remove_file(tmp_peer_store_path(&path));
    }
}

/// Enumerate stale peers from the peer store in stable alias order.
#[must_use]
pub fn stale_peers(env: &PeerStoreEnv, now_ms: u64) -> Vec<StalePeer> {
    let ttl_ms = parse_stale_ttl_ms(env.var("MAW_PEER_STALE_TTL_MS"));
    load_peer_store(env)
        .peers
        .into_iter()
        .filter(|(_, peer)| is_peer_stale(peer, ttl_ms, now_ms))
        .map(|(alias, peer)| {
            let age_ms = stale_age_ms(&peer, now_ms);
            StalePeer {
                alias,
                url: peer.url,
                age_ms,
            }
        })
        .collect()
}

/// Return the maw-js `peers:stale` doctor check shape.
#[must_use]
pub fn stale_peer_check(env: &PeerStoreEnv, now_ms: u64) -> PeerDoctorCheck {
    let stale = stale_peers(env, now_ms);
    if stale.is_empty() {
        return PeerDoctorCheck {
            name: "peers:stale".to_owned(),
            ok: true,
            message: "no stale peers".to_owned(),
        };
    }
    let days = parse_stale_ttl_ms(env.var("MAW_PEER_STALE_TTL_MS")) / 86_400_000;
    PeerDoctorCheck {
        name: "peers:stale".to_owned(),
        ok: false,
        message: format!(
            "{} stale peer{} (>{days}d) — run 'maw doctor --fix-stale' to remove",
            stale.len(),
            if stale.len() == 1 { "" } else { "s" }
        ),
    }
}

/// Remove stale peers through the peer-store mutation path.
///
/// # Errors
///
/// Returns peer-store mutation write failures.
pub fn remove_stale_peers(env: &PeerStoreEnv, now_ms: u64) -> io::Result<PeerDoctorCheck> {
    let stale = stale_peers(env, now_ms);
    if stale.is_empty() {
        return Ok(PeerDoctorCheck {
            name: "peers:fix-stale".to_owned(),
            ok: true,
            message: "no stale peers".to_owned(),
        });
    }
    let mut removed = 0;
    mutate_peer_store(env, |data| {
        for stale_peer in &stale {
            removed += usize::from(data.peers.remove(&stale_peer.alias).is_some());
        }
    })?;
    Ok(PeerDoctorCheck {
        name: "peers:fix-stale".to_owned(),
        ok: true,
        message: format!(
            "removed {removed} stale peer{}",
            if removed == 1 { "" } else { "s" }
        ),
    })
}

#[must_use]
pub fn evaluate_peer_identity(
    alias: &str,
    peer: Option<&PeerRecord>,
    observed: Option<&str>,
) -> TofuDecision {
    let cached = peer
        .and_then(|peer| peer.pubkey.as_deref())
        .filter(|value| !value.is_empty());
    let observed = observed.filter(|value| !value.is_empty());

    let alias_string = alias.to_owned();
    let cached_string = cached.map(str::to_owned);
    let observed_string = observed.map(str::to_owned);

    match (cached, observed) {
        (None, Some(_)) => TofuDecision {
            kind: TofuDecisionKind::TofuBootstrap,
            alias: alias_string,
            cached: None,
            observed: observed_string,
            message: format!("[tofu] caching pubkey for {alias} (first sight)"),
        },
        (None, None) => TofuDecision {
            kind: TofuDecisionKind::LegacyFirstContact,
            alias: alias_string,
            cached: None,
            observed: None,
            message: format!("[tofu] {alias} did not advertise a pubkey (legacy peer; no pin established)"),
        },
        (Some(cached), None) => TofuDecision {
            kind: TofuDecisionKind::LegacyAfterPinned,
            alias: alias_string,
            cached: cached_string,
            observed: None,
            message: format!(
                "[tofu] {alias} previously advertised pubkey {}… but this response omits it; accepting during alpha migration, will hard-fail at v27",
                prefix16(cached)
            ),
        },
        (Some(cached), Some(observed)) if cached == observed => TofuDecision {
            kind: TofuDecisionKind::Match,
            alias: alias_string,
            cached: cached_string,
            observed: observed_string,
            message: format!("[tofu] {alias} pubkey verified"),
        },
        (Some(cached), Some(observed)) => TofuDecision {
            kind: TofuDecisionKind::Mismatch,
            alias: alias_string,
            cached: cached_string,
            observed: observed_string,
            message: PeerPubkeyMismatchError::new(alias, cached, observed).to_string(),
        },
    }
}
