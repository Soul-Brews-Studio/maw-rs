// Giving sessions their slot numbers back.
//
// Sessions carry an `NN-` prefix that is meant to be stable and unique; drift
// and the 99- holding pen break that. Renumber plans the whole remapping first,
// so a collision is caught before any file is written or any tmux session is
// renamed.

#[derive(Debug, Clone, PartialEq, Eq)]
struct FleetRenumberItem {
    old_name: String,
    new_name: String,
    old_file: String,
    new_file: String,
    path: std::path::PathBuf,
    changed: bool,
    tmux: Option<String>,
    tmux_error: Option<String>,
}

fn fleet_run_renumber(state: &FleetState, options: &FleetOptions, runtime: &mut impl FleetRuntime) -> Result<(i32, String), String> {
    let mut items = fleet_renumber_plan(&state.fleet_entries, options.include_99, options.only_99);
    let live = runtime.fleet_list_all().into_iter().map(|session| session.name).collect::<Vec<_>>();
    if !options.dry_run {
        fleet_apply_renumber(&mut items, &live, runtime)?;
    }
    if options.json {
        return Ok((0, fleet_json_renumber(state, options, &items)?));
    }
    Ok((0, fleet_render_renumber(state, options, &items)))
}

fn fleet_renumber_plan(entries: &[NativeFleetEntry], include_99: bool, only_99: bool) -> Vec<FleetRenumberItem> {
    if only_99 {
        return fleet_renumber_only_99_plan(entries);
    }
    let mut candidates = entries
        .iter()
        .filter_map(|entry| fleet_renumber_candidate(entry, include_99))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    candidates
        .into_iter()
        .enumerate()
        .map(|(index, (_, stem, entry))| fleet_renumber_item(entry, &format!("{:02}-{stem}", index + 1)))
        .collect()
}

fn fleet_renumber_only_99_plan(entries: &[NativeFleetEntry]) -> Vec<FleetRenumberItem> {
    let mut used = entries
        .iter()
        .filter_map(|entry| fleet_renumber_candidate(entry, true))
        .filter_map(|(number, _, entry)| (number != 99).then_some(entry.session.name.clone()))
        .filter_map(|name| name.split_once('-').and_then(|(prefix, _)| prefix.parse::<u32>().ok()))
        .collect::<BTreeSet<_>>();
    let mut candidates = entries
        .iter()
        .filter_map(|entry| fleet_renumber_candidate(entry, true))
        .filter(|(number, stem, _)| *number == 99 && stem != "overview")
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.1.cmp(&right.1));
    candidates
        .into_iter()
        .filter_map(|(_, stem, entry)| {
            let next = (1..=99).find(|number| !used.contains(number))?;
            used.insert(next);
            Some(fleet_renumber_item(entry, &format!("{next:02}-{stem}")))
        })
        .collect()
}

fn fleet_renumber_item(entry: &NativeFleetEntry, new_name: &str) -> FleetRenumberItem {
    let new_file = format!("{new_name}.json");
    FleetRenumberItem {
        old_name: entry.session.name.clone(),
        new_name: new_name.to_owned(),
        old_file: entry.file.clone(),
        new_file,
        path: entry.path.clone(),
        changed: entry.session.name != new_name,
        tmux: None,
        tmux_error: None,
    }
}

fn fleet_renumber_candidate(entry: &NativeFleetEntry, include_99: bool) -> Option<(u32, String, &NativeFleetEntry)> {
    if !fleet_entry_is_session(entry) {
        return None;
    }
    let (prefix, stem) = entry.session.name.split_once('-')?;
    if prefix.is_empty() || !prefix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = prefix.parse::<u32>().ok()?;
    if number == 99 && !include_99 {
        return None;
    }
    Some((number, stem.to_owned(), entry))
}

fn fleet_apply_renumber(items: &mut [FleetRenumberItem], live: &[String], runtime: &mut impl FleetRuntime) -> Result<(), String> {
    for item in items.iter_mut().filter(|item| item.changed) {
        fleet_write_renumbered_config(item)?;
        let stem = fleet_session_stem(&item.old_name);
        let running = live
            .iter()
            .find(|name| name.as_str() == item.old_name)
            .or_else(|| live.iter().find(|name| fleet_session_stem(name) == stem));
        if let Some(running) = running.filter(|running| running.as_str() != item.new_name) {
            match runtime.fleet_run_command("tmux", &["rename-session".to_owned(), "-t".to_owned(), running.clone(), item.new_name.clone()]) {
                Ok(_) => item.tmux = Some(running.clone()),
                Err(error) => item.tmux_error = Some(format!("{running}: {}", error.trim())),
            }
        }
    }
    Ok(())
}

fn fleet_write_renumbered_config(item: &FleetRenumberItem) -> Result<(), String> {
    let text = std::fs::read_to_string(&item.path).map_err(|error| format!("fleet renumber: read {}: {error}", item.path.display()))?;
    let mut value: serde_json::Value = serde_json::from_str(&text).map_err(|error| format!("fleet renumber: parse {}: {error}", item.path.display()))?;
    value["name"] = serde_json::json!(item.new_name);
    let body = serde_json::to_string_pretty(&value).map_err(|error| format!("fleet renumber: render {}: {error}", item.new_name))? + "\n";
    let dir = item.path.parent().ok_or_else(|| format!("fleet renumber: no parent for {}", item.path.display()))?;
    let target = dir.join(&item.new_file);
    let tmp = dir.join(format!(".tmp-{}", item.new_file));
    std::fs::write(&tmp, body).map_err(|error| format!("fleet renumber: write {}: {error}", tmp.display()))?;
    std::fs::rename(&tmp, &target).map_err(|error| format!("fleet renumber: rename {} -> {}: {error}", tmp.display(), target.display()))?;
    if target != item.path && item.path.exists() {
        std::fs::remove_file(&item.path).map_err(|error| format!("fleet renumber: remove {}: {error}", item.path.display()))?;
    }
    Ok(())
}

fn fleet_render_renumber(state: &FleetState, options: &FleetOptions, items: &[FleetRenumberItem]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "fleet renumber plan node: {}", state.config.node);
    let _ = writeln!(out, "  dry-run: {} · include-99: {} · only-99: {} · configs: {}", options.dry_run, options.include_99, options.only_99, items.len());
    if items.is_empty() {
        out.push_str("  ok: no numbered fleet configs\n");
        return out;
    }
    for item in items {
        if item.changed {
            let verb = if options.dry_run { "would rename" } else { "renamed" };
            let _ = write!(out, "  - {verb} {} -> {}", item.old_file, item.new_file);
            if let Some(tmux) = &item.tmux {
                let _ = write!(out, " (tmux: {tmux} -> {})", item.new_name);
            }
            if let Some(error) = &item.tmux_error {
                let _ = write!(out, " (tmux rename failed: {error})");
            }
            out.push('\n');
        } else {
            let _ = writeln!(out, "  - {} (unchanged)", item.old_file);
        }
    }
    out
}

fn fleet_json_renumber(state: &FleetState, options: &FleetOptions, items: &[FleetRenumberItem]) -> Result<String, String> {
    let value = serde_json::json!({
        "node": state.config.node,
        "action": "renumber",
        "dryRun": options.dry_run,
        "include99": options.include_99,
        "only99": options.only_99,
        "configCount": items.len(),
        "configs": items.iter().map(fleet_json_renumber_item).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&value).map(|text| format!("{text}\n")).map_err(|error| error.to_string())
}

fn fleet_json_renumber_item(item: &FleetRenumberItem) -> serde_json::Value {
    serde_json::json!({
        "oldName": item.old_name,
        "newName": item.new_name,
        "oldFile": item.old_file,
        "newFile": item.new_file,
        "changed": item.changed,
        "tmux": item.tmux,
        "tmuxError": item.tmux_error,
    })
}
