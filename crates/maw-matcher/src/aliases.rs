use super::fleet::FleetWindowSessionLike;

pub(super) fn strip_oracle_suffix_lower(name: &str) -> String {
    name.strip_suffix("-oracle").unwrap_or(name).to_owned()
}

pub(super) fn repo_basename_lower(repo: &str) -> String {
    repo.rsplit('/').next().unwrap_or(repo).to_lowercase()
}

pub(super) fn aliases_for<T>(item: &T) -> Vec<String>
where
    T: FleetWindowSessionLike,
{
    let mut aliases = Vec::new();
    for window in item.windows() {
        if let Some(win) = window
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let win = win.to_lowercase();
            aliases.push(win.clone());
            aliases.push(strip_oracle_suffix_lower(&win));
        }
        if let Some(repo) = window
            .repo
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let base = repo_basename_lower(repo);
            aliases.push(base.clone());
            aliases.push(strip_oracle_suffix_lower(&base));
        }
    }
    aliases.sort();
    aliases.dedup();
    aliases
}
