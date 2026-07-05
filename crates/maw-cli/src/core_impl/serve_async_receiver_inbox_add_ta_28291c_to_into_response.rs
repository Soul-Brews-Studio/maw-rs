fn receiver_inbox_add_target_lookup_keys(keys: &mut BTreeSet<String>, raw: Option<&str>) {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let raw = receiver_inbox_strip_pane_suffix(raw);
    if let Some(key) = receiver_inbox_lookup_key(raw) {
        keys.insert(key);
    }
    let parts = raw
        .split(':')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    match parts.as_slice() {
        [session, window] => {
            if let Some(key) = receiver_inbox_lookup_key(session) {
                keys.insert(key);
            }
            if !window.bytes().all(|byte| byte.is_ascii_digit()) {
                if let Some(key) = receiver_inbox_lookup_key(window) {
                    keys.insert(key);
                }
            }
        }
        [_, session, window, ..] => {
            if let Some(key) = receiver_inbox_lookup_key(session) {
                keys.insert(key);
            }
            if !window.bytes().all(|byte| byte.is_ascii_digit()) {
                if let Some(key) = receiver_inbox_lookup_key(window) {
                    keys.insert(key);
                }
            }
        }
        _ => {}
    }
}

fn receiver_inbox_target_lookup_keys(input: &ReceiverInboxInput<'_>) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    receiver_inbox_add_target_lookup_keys(&mut keys, input.target);
    receiver_inbox_add_target_lookup_keys(&mut keys, input.to);
    receiver_inbox_add_target_lookup_keys(&mut keys, Some(input.query));
    keys
}

fn receiver_inbox_manifest_entry_matches_target(
    entry: &LocateManifestEntry,
    target_keys: &BTreeSet<String>,
) -> bool {
    entry
        .session
        .as_deref()
        .and_then(receiver_inbox_lookup_key)
        .is_some_and(|key| target_keys.contains(&key))
        || entry
            .window
            .as_deref()
            .and_then(receiver_inbox_lookup_key)
            .is_some_and(|key| target_keys.contains(&key))
}

fn receiver_inbox_push_manifest_entry_candidates(
    candidates: &mut Vec<std::path::PathBuf>,
    entry: &LocateManifestEntry,
) {
    if let Some(local_path) = entry.local_path.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        candidates.push(std::path::PathBuf::from(local_path));
    }
    if let Some(repo) = entry.repo.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        let ghq_root = receiver_inbox_ghq_root();
        candidates.push(ghq_root.join("github.com").join(repo));
        candidates.push(ghq_root.join(repo));
    }
}

fn receiver_inbox_existing_candidates(
    candidates: Vec<std::path::PathBuf>,
) -> Vec<std::path::PathBuf> {
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|candidate| seen.insert(candidate.display().to_string()))
        .filter(|candidate| candidate.exists())
        .collect()
}

fn receiver_inbox_repo_candidates(
    oracle: &str,
    input: &ReceiverInboxInput<'_>,
    psi_root: Option<&std::path::Path>,
) -> Result<Vec<std::path::PathBuf>, String> {
    let mut candidates = Vec::new();
    if let Some(psi_path) = psi_root {
        candidates.push(receiver_inbox_strip_psi_suffix(psi_path));
    } else if let (Some(psi_path), Some(config_oracle)) =
        (receiver_inbox_config_psi_path(), input.config.oracle.as_deref())
    {
        if receiver_inbox_normalize_oracle_name(Some(config_oracle)).as_deref() == Some(oracle) {
            candidates.push(receiver_inbox_strip_psi_suffix(&psi_path));
        }
    }
    if let Some(target) = input.target {
        match receiver_inbox_resolve_target_cwd(target) {
            Ok(Some(path)) => candidates.push(path),
            Ok(None) => {}
            Err(reason) => return Err(reason),
        }
    }
    let manifest = locate_load_manifest();
    if let Some(entry) = manifest.iter().find(|entry| {
        receiver_inbox_normalize_oracle_name(Some(&entry.name)).as_deref() == Some(oracle)
            || entry.window.as_deref().and_then(|window| receiver_inbox_normalize_oracle_name(Some(window))).as_deref()
                == Some(oracle)
    }) {
        receiver_inbox_push_manifest_entry_candidates(&mut candidates, entry);
    }

    let target_keys = receiver_inbox_target_lookup_keys(input);
    if !target_keys.is_empty() {
        let mut phase_b = Vec::new();
        for entry in manifest
            .iter()
            .filter(|entry| receiver_inbox_manifest_entry_matches_target(entry, &target_keys))
        {
            let mut entry_candidates = Vec::new();
            receiver_inbox_push_manifest_entry_candidates(&mut entry_candidates, entry);
            phase_b.extend(receiver_inbox_existing_candidates(entry_candidates));
        }
        let phase_b = receiver_inbox_existing_candidates(phase_b);
        if phase_b.len() > 1 {
            return Err(format!("receiver repo ambiguous for {}", input.query));
        }
        candidates.extend(phase_b);
    }
    Ok(receiver_inbox_existing_candidates(candidates))
}

fn persist_receiver_inbox(
    input: ReceiverInboxInput<'_>,
    now_millis: u128,
    psi_root: Option<&std::path::Path>,
) -> ReceiverInboxResult {
    let Some(oracle) = receiver_inbox_resolve_oracle(&input) else {
        return ReceiverInboxResult::Err { oracle: None, reason: "receiver oracle could not be inferred".to_owned() };
    };
    let repo_candidates = match receiver_inbox_repo_candidates(&oracle, &input, psi_root) {
        Ok(candidates) => candidates,
        Err(reason) => return ReceiverInboxResult::Err { oracle: Some(oracle), reason },
    };
    let Some(repo_path) = repo_candidates.into_iter().next() else {
        return ReceiverInboxResult::Err {
            oracle: Some(oracle.clone()),
            reason: format!("receiver repo not found for {oracle}"),
        };
    };
    let timestamp = receiver_inbox_iso_from_millis(now_millis);
    let date_part = &timestamp[..10];
    let time_part = timestamp[11..16].replace(':', "-");
    let base_filename = format!(
        "{date_part}_{time_part}_{}_{}.md",
        receiver_inbox_safe_segment(input.from),
        receiver_inbox_slugify_body(input.message)
    );
    let inbox_dir = repo_path.join("ψ").join("inbox");
    let body = receiver_inbox_body(input.from, &oracle, &timestamp, input.message);
    if let Err(error) = std::fs::create_dir_all(&inbox_dir) {
        return ReceiverInboxResult::Err { oracle: Some(oracle), reason: error.to_string() };
    }
    for attempt in 1..=1000 {
        let filename = receiver_inbox_filename_with_collision_suffix(&base_filename, attempt);
        let path = inbox_dir.join(&filename);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(error) = std::io::Write::write_all(&mut file, body.as_bytes()) {
                    return ReceiverInboxResult::Err { oracle: Some(oracle), reason: error.to_string() };
                }
                return ReceiverInboxResult::Ok(ReceiverInboxOk { oracle, inbox_dir, path, filename });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return ReceiverInboxResult::Err { oracle: Some(oracle), reason: error.to_string() },
        }
    }
    ReceiverInboxResult::Err {
        oracle: Some(oracle),
        reason: format!("receiver inbox filename collision limit reached for {base_filename}"),
    }
}

async fn api_feed_post(
    State(state): State<Arc<ServeState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Some(response) = verify_protected_request(&state, peer, &method, &uri, &headers, &body) {
        response
    } else {
        Json(json!({"ok": true})).into_response()
    }
}

async fn api_sessions(Query(query): Query<SessionsQuery>) -> impl IntoResponse {
    let _ = query.local.unwrap_or(false);
    Json(Vec::<Value>::new())
}

async fn api_capture(Query(query): Query<CaptureQuery>) -> impl IntoResponse {
    Json(json!({"content": "", "target": query.target}))
}

