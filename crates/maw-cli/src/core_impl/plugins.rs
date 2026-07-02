const DISPATCH_101: &[DispatcherEntry] = &[DispatcherEntry {
    command: "plugins",
    handler: Handler::Sync(plugins_run_command),
}];

const PLUGINS_USAGE: &str = "usage: maw plugins <ls|info|remove|lean|standard|full|nuke|enable|disable> [name] [--json] [--all] [-v|--verbose] [--scan-dir <dir>] [--yes|--confirm <name>]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginsAction {
    Ls,
    Info,
    Remove,
    Nuke,
    Enable,
    Disable,
    Lean,
    Standard,
    Full,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct PluginsOptions {
    action: Option<PluginsAction>,
    target: Option<String>,
    json: bool,
    all: bool,
    verbose: bool,
    yes: bool,
    confirm: Option<String>,
    scan_dirs: Vec<std::path::PathBuf>,
}

fn plugins_run_command(argv: &[String]) -> CliOutput {
    match plugins_run(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 2, stdout: String::new(), stderr: format!("{message}\n{PLUGINS_USAGE}\n") },
    }
}

fn plugins_run(argv: &[String]) -> Result<String, String> {
    let options = plugins_parse_args(argv)?;
    let roots = plugins_scan_dirs(&options);
    let disabled = plugins_read_disabled(&roots[0]);
    let profile = plugins_read_profile(&roots[0]);
    let report = plugins_discover(&roots, &disabled, profile.as_deref());
    match options.action.unwrap_or(PluginsAction::Ls) {
        PluginsAction::Ls => Ok(plugins_render_ls(&report.plugins, &report.warnings, &options)),
        PluginsAction::Info => plugins_info(&report.plugins, &options),
        PluginsAction::Enable => plugins_enable(&roots[0], &report.plugins, &options),
        PluginsAction::Disable => plugins_disable(&roots[0], &report.plugins, &options),
        PluginsAction::Lean => plugins_set_profile(&roots[0], &report.plugins, "lean", &options),
        PluginsAction::Standard => plugins_set_profile(&roots[0], &report.plugins, "standard", &options),
        PluginsAction::Full => plugins_set_profile(&roots[0], &report.plugins, "full", &options),
        PluginsAction::Remove => plugins_remove(&roots[0], &report.plugins, &options, false),
        PluginsAction::Nuke => plugins_remove(&roots[0], &report.plugins, &options, true),
    }
}

fn plugins_parse_args(argv: &[String]) -> Result<PluginsOptions, String> {
    let mut options = PluginsOptions::default();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" | "help" => return Err(PLUGINS_USAGE.to_owned()),
            "--" => return Err("plugins: -- separator is not allowed".to_owned()),
            "--json" => options.json = true,
            "--all" => options.all = true,
            "-v" | "--verbose" => options.verbose = true,
            "--yes" | "-y" => options.yes = true,
            "--scan-dir" => {
                options.scan_dirs.push(plugins_take_path(argv, index, "--scan-dir")?);
                index += 1;
            }
            "--confirm" => {
                options.confirm = Some(plugins_take_value(argv, index, "--confirm")?);
                index += 1;
            }
            value if value.starts_with('-') => return Err(plugins_flag_like_value(value)),
            value => plugins_parse_positional(&mut options, value)?,
        }
        index += 1;
    }
    if options.action.is_none() {
        options.action = Some(PluginsAction::Ls);
    }
    Ok(options)
}

fn plugins_parse_positional(options: &mut PluginsOptions, value: &str) -> Result<(), String> {
    if options.action.is_none() {
        options.action = Some(match value {
            "ls" | "list" => PluginsAction::Ls,
            "info" => PluginsAction::Info,
            "remove" | "rm" => PluginsAction::Remove,
            "nuke" => PluginsAction::Nuke,
            "enable" => PluginsAction::Enable,
            "disable" => PluginsAction::Disable,
            "lean" => PluginsAction::Lean,
            "standard" => PluginsAction::Standard,
            "full" => PluginsAction::Full,
            other => return Err(format!("plugins: unknown subcommand {other}")),
        });
        return Ok(());
    }
    if options.target.replace(value.to_owned()).is_some() {
        return Err(format!("plugins: unexpected argument {value}"));
    }
    Ok(())
}

fn plugins_take_value(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let Some(value) = argv.get(index + 1) else { return Err(format!("plugins: {flag} requires a value")); };
    if value.starts_with('-') {
        return Err(format!("plugins: {flag} value must not start with '-'"));
    }
    Ok(value.clone())
}

fn plugins_take_path(argv: &[String], index: usize, flag: &str) -> Result<std::path::PathBuf, String> {
    Ok(std::path::PathBuf::from(plugins_take_value(argv, index, flag)?))
}

fn plugins_flag_like_value(value: &str) -> String {
    format!("plugins: {value:?} is not a supported flag")
}

fn plugins_scan_dirs(options: &PluginsOptions) -> Vec<std::path::PathBuf> {
    if options.scan_dirs.is_empty() {
        maw_plugin_manifest::scan_dirs()
    } else {
        options.scan_dirs.clone()
    }
}

fn plugins_discover(
    roots: &[std::path::PathBuf],
    disabled: &[String],
    profile: Option<&str>,
) -> maw_plugin_manifest::DiscoverPackagesReport {
    let options = maw_plugin_manifest::DiscoverPackagesOptions {
        scan_dirs: roots.to_vec(),
        disabled_plugins: disabled.to_vec(),
        runtime_version: maw_plugin_manifest::runtime_sdk_version(),
        use_cache: false,
    };
    maw_plugin_manifest::discover_packages_with_profile(&options, |plugins| {
        let allowed = plugins
            .iter()
            .filter(|plugin| plugins_profile_includes(profile, plugin.tier))
            .map(|plugin| plugin.name.clone())
            .collect::<std::collections::BTreeSet<_>>();
        profile.map(|_| allowed)
    })
}

fn plugins_profile_includes(profile: Option<&str>, tier: maw_plugin_manifest::PluginTier) -> bool {
    match profile {
        Some("lean") => tier == maw_plugin_manifest::PluginTier::Core,
        Some("standard") => tier != maw_plugin_manifest::PluginTier::Extra,
        _ => true,
    }
}

fn plugins_render_ls(
    plugins: &[maw_plugin_manifest::LoadedPlugin],
    warnings: &[String],
    options: &PluginsOptions,
) -> String {
    if options.json {
        return plugins_render_ls_json(plugins, warnings);
    }
    let rows = plugins_visible_rows(plugins, options);
    if rows.is_empty() {
        return "no plugins installed\n".to_owned();
    }
    if !options.verbose {
        return plugins_render_ls_compact(&rows);
    }
    plugins_render_ls_verbose(&rows)
}

fn plugins_visible_rows<'a>(
    plugins: &'a [maw_plugin_manifest::LoadedPlugin],
    options: &PluginsOptions,
) -> Vec<PluginsRow<'a>> {
    let mut rows = plugins
        .iter()
        .filter(|plugin| options.all || !plugin.disabled)
        .map(PluginsRow::new)
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| (plugins_tier_order(row.tier), row.name.to_owned()));
    rows
}

fn plugins_render_ls_compact(rows: &[PluginsRow<'_>]) -> String {
    let active = rows.iter().filter(|row| !row.disabled).count();
    let disabled = rows.len() - active;
    format!("{} plugin{} ({} active, {} disabled)\n", rows.len(), if rows.len() == 1 { "" } else { "s" }, active, disabled)
}

fn plugins_render_ls_verbose(rows: &[PluginsRow<'_>]) -> String {
    let mut out = String::new();
    for row in rows {
        let _ = writeln!(out, "{}\t{}\t{}\t{}\t{}", row.name, row.version, row.tier.as_str(), if row.disabled { "disabled" } else { "enabled" }, row.dir);
    }
    out
}

fn plugins_render_ls_json(plugins: &[maw_plugin_manifest::LoadedPlugin], warnings: &[String]) -> String {
    let rows = plugins.iter().map(plugins_plugin_json).collect::<Vec<_>>().join(",");
    format!("{{\"command\":\"plugins\",\"kind\":\"ls\",\"plugins\":[{rows}],\"warnings\":{}}}\n", json_string_array(warnings))
}

fn plugins_info(plugins: &[maw_plugin_manifest::LoadedPlugin], options: &PluginsOptions) -> Result<String, String> {
    let name = plugins_required_target(options)?;
    let plugin = plugins_find(plugins, &name)?;
    if options.json {
        return Ok(format!("{{\"command\":\"plugins\",\"kind\":\"info\",\"plugin\":{}}}\n", plugins_plugin_json(plugin)));
    }
    Ok(format!("{} v{} ({})\n  tier: {}\n  status: {}\n  dir: {}\n", plugin.manifest.name, plugin.manifest.version, plugin.kind.as_str(), plugins_effective_tier(&plugin.manifest).as_str(), if plugin.disabled { "disabled" } else { "enabled" }, path_string(&plugin.dir)))
}

fn plugins_enable(
    root: &std::path::Path,
    plugins: &[maw_plugin_manifest::LoadedPlugin],
    options: &PluginsOptions,
) -> Result<String, String> {
    let name = plugins_required_target(options)?;
    plugins_find(plugins, &name)?;
    let mut disabled = plugins_read_disabled(root);
    disabled.retain(|item| item != &name);
    plugins_write_disabled(root, &disabled)?;
    Ok(plugins_render_state_change("enable", &name, options))
}

fn plugins_disable(
    root: &std::path::Path,
    plugins: &[maw_plugin_manifest::LoadedPlugin],
    options: &PluginsOptions,
) -> Result<String, String> {
    let name = plugins_required_target(options)?;
    plugins_find(plugins, &name)?;
    let mut disabled = plugins_read_disabled(root);
    if !disabled.contains(&name) {
        disabled.push(name.clone());
        disabled.sort();
    }
    plugins_write_disabled(root, &disabled)?;
    Ok(plugins_render_state_change("disable", &name, options))
}

fn plugins_set_profile(
    root: &std::path::Path,
    plugins: &[maw_plugin_manifest::LoadedPlugin],
    profile: &str,
    options: &PluginsOptions,
) -> Result<String, String> {
    let selected = plugins
        .iter()
        .filter(|plugin| plugins_profile_includes(Some(profile), plugins_effective_tier(&plugin.manifest)))
        .map(|plugin| plugin.manifest.name.clone())
        .collect::<Vec<_>>();
    plugins_write_profile(root, profile)?;
    if options.json {
        return Ok(format!("{{\"command\":\"plugins\",\"kind\":\"profile\",\"profile\":{},\"plugins\":{}}}\n", json_string(profile), json_string_array(&selected)));
    }
    Ok(format!("plugins profile {profile}: {} selected\n", selected.len()))
}

fn plugins_remove(
    root: &std::path::Path,
    plugins: &[maw_plugin_manifest::LoadedPlugin],
    options: &PluginsOptions,
    nuke: bool,
) -> Result<String, String> {
    let name = plugins_required_target(options)?;
    let plugin = plugins_find(plugins, &name)?;
    plugins_confirm_destructive(options, &name)?;
    let target = plugins_validate_delete_target(root, &plugin.dir)?;
    std::fs::remove_dir_all(&target).map_err(|error| format!("plugins: remove failed: {error}"))?;
    let mut disabled = plugins_read_disabled(root);
    disabled.retain(|item| item != &name);
    plugins_write_disabled(root, &disabled)?;
    let kind = if nuke { "nuke" } else { "remove" };
    Ok(plugins_render_remove(kind, &name, &target, options))
}

fn plugins_render_state_change(action: &str, name: &str, options: &PluginsOptions) -> String {
    if options.json {
        format!("{{\"command\":\"plugins\",\"kind\":{},\"plugin\":{}}}\n", json_string(action), json_string(name))
    } else {
        format!("plugins {action}: {name}\n")
    }
}

fn plugins_render_remove(action: &str, name: &str, path: &std::path::Path, options: &PluginsOptions) -> String {
    if options.json {
        format!("{{\"command\":\"plugins\",\"kind\":{},\"plugin\":{},\"removedDir\":{}}}\n", json_string(action), json_string(name), json_string(&path_string(path)))
    } else {
        format!("plugins {action}: removed {name} ({})\n", path.display())
    }
}

fn plugins_required_target(options: &PluginsOptions) -> Result<String, String> {
    let Some(target) = options.target.as_ref() else { return Err("plugins: target plugin name is required".to_owned()); };
    plugins_validate_name(target)?;
    Ok(target.clone())
}

fn plugins_validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.starts_with('-') || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("plugins: target rejected by #67 guard".to_owned());
    }
    if !name.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-') {
        return Err("plugins: target must be a plugin slug".to_owned());
    }
    Ok(())
}

fn plugins_confirm_destructive(options: &PluginsOptions, name: &str) -> Result<(), String> {
    if options.yes || options.confirm.as_deref() == Some(name) {
        return Ok(());
    }
    Err(format!("plugins: refusing destructive action for {name}; rerun with --yes or --confirm {name}"))
}

fn plugins_find<'a>(
    plugins: &'a [maw_plugin_manifest::LoadedPlugin],
    name: &str,
) -> Result<&'a maw_plugin_manifest::LoadedPlugin, String> {
    plugins
        .iter()
        .find(|plugin| plugin.manifest.name == name)
        .ok_or_else(|| format!("plugins: plugin not found: {name}"))
}

fn plugins_validate_delete_target(
    root: &std::path::Path,
    target: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let root = root.canonicalize().map_err(|error| format!("plugins: root: {error}"))?;
    let target = target.canonicalize().map_err(|error| format!("plugins: target: {error}"))?;
    if target == root || !target.starts_with(&root) || target.file_name().is_none() {
        return Err("plugins: delete target rejected by #67 guard".to_owned());
    }
    Ok(target)
}

fn plugins_disabled_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".disabled.json")
}

fn plugins_profile_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".profile.json")
}

fn plugins_read_disabled(root: &std::path::Path) -> Vec<String> {
    plugins_read_string_array(&plugins_disabled_path(root), "disabled")
}

fn plugins_write_disabled(root: &std::path::Path, disabled: &[String]) -> Result<(), String> {
    plugins_write_json(root, &plugins_disabled_path(root), &format!("{{\"disabled\":{}}}\n", json_string_array(disabled)))
}

fn plugins_read_profile(root: &std::path::Path) -> Option<String> {
    let raw = std::fs::read_to_string(plugins_profile_path(root)).ok()?;
    serde_json::from_str::<serde_json::Value>(&raw).ok()?.get("profile")?.as_str().map(str::to_owned)
}

fn plugins_write_profile(root: &std::path::Path, profile: &str) -> Result<(), String> {
    plugins_write_json(root, &plugins_profile_path(root), &format!("{{\"profile\":{}}}\n", json_string(profile)))
}

fn plugins_write_json(root: &std::path::Path, path: &std::path::Path, text: &str) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|error| format!("plugins: create root failed: {error}"))?;
    std::fs::write(path, text).map_err(|error| format!("plugins: write failed: {error}"))
}

fn plugins_read_string_array(path: &std::path::Path, key: &str) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(path) else { return Vec::new(); };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else { return Vec::new(); };
    value.get(key).and_then(serde_json::Value::as_array).map_or_else(Vec::new, |items| {
        let mut out = items.iter().filter_map(serde_json::Value::as_str).map(str::to_owned).collect::<Vec<_>>();
        out.sort();
        out.dedup();
        out
    })
}

fn plugins_plugin_json(plugin: &maw_plugin_manifest::LoadedPlugin) -> String {
    format!("{{\"name\":{},\"version\":{},\"kind\":{},\"tier\":{},\"weight\":{},\"disabled\":{},\"dir\":{},\"command\":{},\"api\":{},\"capabilities\":{}}}", json_string(&plugin.manifest.name), json_string(&plugin.manifest.version), json_string(plugin.kind.as_str()), json_string(plugins_effective_tier(&plugin.manifest).as_str()), plugin.manifest.weight.unwrap_or(50), plugin.disabled, json_string(&path_string(&plugin.dir)), plugins_optional_json(plugin.manifest.cli.as_ref().map(|cli| cli.command.as_str())), plugins_optional_json(plugin.manifest.api.as_ref().map(|api| api.path.as_str())), json_string_array(&plugin.manifest.capabilities.clone().unwrap_or_default()))
}

fn plugins_optional_json(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), json_string)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PluginsRow<'a> {
    name: &'a str,
    version: &'a str,
    tier: maw_plugin_manifest::PluginTier,
    dir: String,
    disabled: bool,
}

impl<'a> PluginsRow<'a> {
    fn new(plugin: &'a maw_plugin_manifest::LoadedPlugin) -> Self {
        Self {
            name: &plugin.manifest.name,
            version: &plugin.manifest.version,
            tier: plugins_effective_tier(&plugin.manifest),
            dir: path_string(&plugin.dir),
            disabled: plugin.disabled,
        }
    }
}

fn plugins_effective_tier(manifest: &maw_plugin_manifest::PluginManifest) -> maw_plugin_manifest::PluginTier {
    manifest.tier.unwrap_or_else(|| plugins_weight_to_tier(manifest.weight.unwrap_or(50)))
}

fn plugins_weight_to_tier(weight: u64) -> maw_plugin_manifest::PluginTier {
    if weight < 10 {
        maw_plugin_manifest::PluginTier::Core
    } else if weight < 50 {
        maw_plugin_manifest::PluginTier::Standard
    } else {
        maw_plugin_manifest::PluginTier::Extra
    }
}

fn plugins_tier_order(tier: maw_plugin_manifest::PluginTier) -> u8 {
    match tier {
        maw_plugin_manifest::PluginTier::Core => 0,
        maw_plugin_manifest::PluginTier::Standard => 1,
        maw_plugin_manifest::PluginTier::Extra => 2,
    }
}

#[cfg(test)]
mod plugins_tests {
    use super::{plugins_run_command, DISPATCH_101};

    fn plugins_strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn plugins_temp(label: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "maw-rs-plugins-{label}-{}-{stamp}",
            std::process::id()
        ))
    }

    fn plugins_seed(root: &std::path::Path, name: &str, tier: &str) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).expect("plugin dir");
        std::fs::write(dir.join("index.ts"), "export default function handler() {}\n").expect("entry");
        std::fs::write(
            dir.join("plugin.json"),
            format!(r#"{{"name":"{name}","version":"1.0.0","sdk":"^0.1.0","entry":"index.ts","tier":"{tier}","cli":{{"command":"{name}"}},"description":"{name} plugin"}}"#),
        )
        .expect("manifest");
    }

    #[test]
    fn plugins_dispatch_registers_native_command() {
        assert_eq!(DISPATCH_101.len(), 1);
        assert_eq!(DISPATCH_101[0].command, "plugins");
    }

    #[test]
    fn plugins_ls_and_info_use_manifest_crate_hermetically() {
        let root = plugins_temp("ls");
        plugins_seed(&root, "alpha", "core");
        plugins_seed(&root, "beta", "standard");
        let output = plugins_run_command(&plugins_strings(&["ls", "--scan-dir", root.to_str().unwrap(), "--json"]));
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert!(output.stdout.contains("\"alpha\""));
        assert!(output.stdout.contains("\"beta\""));
        let info = plugins_run_command(&plugins_strings(&["info", "alpha", "--scan-dir", root.to_str().unwrap()]));
        assert_eq!(info.code, 0, "{}", info.stderr);
        assert!(info.stdout.contains("alpha v1.0.0"));
    }

    #[test]
    fn plugins_enable_disable_and_profiles_are_registry_only() {
        let root = plugins_temp("profile");
        plugins_seed(&root, "alpha", "core");
        plugins_seed(&root, "beta", "extra");
        let disabled = plugins_run_command(&plugins_strings(&["disable", "beta", "--scan-dir", root.to_str().unwrap()]));
        assert_eq!(disabled.code, 0, "{}", disabled.stderr);
        let listed = plugins_run_command(&plugins_strings(&["ls", "--all", "--scan-dir", root.to_str().unwrap(), "--json"]));
        assert!(listed.stdout.contains("\"disabled\":true"));
        let lean = plugins_run_command(&plugins_strings(&["lean", "--scan-dir", root.to_str().unwrap(), "--json"]));
        assert_eq!(lean.code, 0, "{}", lean.stderr);
        assert!(lean.stdout.contains("\"profile\":\"lean\""));
    }

    #[test]
    fn plugins_remove_requires_confirm_and_validates_target() {
        let root = plugins_temp("remove");
        plugins_seed(&root, "alpha", "core");
        let refused = plugins_run_command(&plugins_strings(&["remove", "alpha", "--scan-dir", root.to_str().unwrap()]));
        assert_ne!(refused.code, 0);
        assert!(root.join("alpha").exists());
        let bad = plugins_run_command(&plugins_strings(&["remove", "--bad", "--scan-dir", root.to_str().unwrap(), "--yes"]));
        assert_ne!(bad.code, 0);
        let removed = plugins_run_command(&plugins_strings(&["remove", "alpha", "--scan-dir", root.to_str().unwrap(), "--confirm", "alpha"]));
        assert_eq!(removed.code, 0, "{}", removed.stderr);
        assert!(!root.join("alpha").exists());
    }
}
