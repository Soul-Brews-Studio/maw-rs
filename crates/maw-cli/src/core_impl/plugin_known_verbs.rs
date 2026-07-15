/// Known extracted verbs: CLI commands that maw provides through fleet
/// plugins rather than native handlers. When such a verb resolves to no
/// native handler and no installed plugin, the dispatcher prints an
/// actionable install hint instead of the bare `unknown command` exit 2
/// (issue #522 defense 4 — a missing plugin must never read as a typo).
///
/// Rows: (cli verb, plugin name, fleet-plugins/ dir). Kept in lockstep with
/// `fleet-plugins/*/plugin.json` by the parity test in
/// `crates/maw-cli/tests/plugin_missing_verb_cli.rs`.
pub const KNOWN_FLEET_PLUGIN_VERBS: &[(&str, &str, &str)] = &[
    ("atlas", "atlas", "atlas"),
    ("cross-team-queue", "cross-team-queue", "cross-team-queue"),
    ("hermes", "hermes", "hermes"),
    ("menubar", "maw-menubar", "maw-menubar"),
    ("p2p-share", "p2p-share", "p2p-share"),
    ("share", "share", "share"),
    ("squad", "squad", "squad"),
    ("team", "team", "team"),
];

struct KnownExtractedVerb {
    plugin_name: String,
    install_hint: String,
}

/// Resolve `command` against the known extracted verbs: the baked
/// fleet-plugins table first, then this machine's `plugins.lock` pins
/// (a lock entry means this machine expects the plugin; the verb defaults
/// to the plugin name).
fn known_extracted_verb(command: &str) -> Option<KnownExtractedVerb> {
    if let Some((_, plugin, dir)) = KNOWN_FLEET_PLUGIN_VERBS
        .iter()
        .find(|(verb, _, _)| *verb == command)
    {
        return Some(KnownExtractedVerb {
            plugin_name: (*plugin).to_owned(),
            install_hint: format!("maw plugin install Soul-Brews-Studio/maw-rs/fleet-plugins/{dir}"),
        });
    }
    let entry = read_plugin_lock_entry_full(command).ok().flatten()?;
    Some(KnownExtractedVerb {
        plugin_name: command.to_owned(),
        install_hint: plugin_lock_install_hint(command, entry.source.as_deref(), &entry.sha256),
    })
}

fn plugin_lock_install_hint(name: &str, source: Option<&str>, sha256: &str) -> String {
    let source = source.map(|value| value.strip_prefix("github:").unwrap_or(value).to_owned());
    match source {
        Some(source) if source.starts_with("path:") => {
            format!("maw plugin install {}", source.trim_start_matches("path:"))
        }
        Some(source) => format!("maw plugin install {source} --sha256 {sha256}"),
        None => format!("maw plugin install <source-of {name}> --sha256 {sha256}"),
    }
}

/// Loud fallthrough for a verb with no native handler and no runnable plugin.
///
/// - If discovery REFUSED a plugin that provides this known verb (sdk floor,
///   hash mismatch, unbuilt/missing artifact), surface that refusal verbatim
///   (exit 1) — never silently degrade to `unknown command`.
/// - If the verb is known-extracted but nothing is installed, print the
///   actionable install hint (exit 2).
/// - Anything else (true typo) returns `None` and keeps the existing
///   `unknown command` path.
fn missing_plugin_output(command: &str, discovery_warnings: &[String]) -> Option<CliOutput> {
    let known = known_extracted_verb(command)?;
    let needle = format!("'{}'", known.plugin_name);
    if let Some(refusal) = discovery_warnings
        .iter()
        .find(|warning| warning.contains(&needle))
    {
        return Some(CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!(
                "maw-rs: verb '{command}' is provided by plugin '{}' but the plugin refused to load:\n{refusal}\n",
                known.plugin_name
            ),
        });
    }
    Some(CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "maw-rs: verb '{command}' is provided by plugin '{}', which is not installed on this machine\n  install: {}\n",
            known.plugin_name, known.install_hint
        ),
    })
}

#[cfg(test)]
mod known_verb_tests {
    use super::*;

    #[test]
    fn fleet_table_resolves_verbs_to_install_hints() {
        let verb = known_extracted_verb("menubar").expect("menubar known");
        assert_eq!(verb.plugin_name, "maw-menubar");
        assert_eq!(
            verb.install_hint,
            "maw plugin install Soul-Brews-Studio/maw-rs/fleet-plugins/maw-menubar"
        );
    }

    #[test]
    fn refusal_warning_wins_over_missing_message() {
        let warnings = vec![
            "plugin 'atlas' artifact hash mismatch — refusing to load.".to_owned(),
        ];
        let output = missing_plugin_output("atlas", &warnings).expect("known verb");
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("refused to load"));
        assert!(output.stderr.contains("hash mismatch"));
    }

    #[test]
    fn lock_hint_strips_github_prefix_and_keeps_paths() {
        assert_eq!(
            plugin_lock_install_hint("demo", Some("github:o/r@v1"), "sha256:aa"),
            "maw plugin install o/r@v1 --sha256 sha256:aa"
        );
        assert_eq!(
            plugin_lock_install_hint("demo", Some("path:/tmp/demo"), "sha256:aa"),
            "maw plugin install /tmp/demo"
        );
        assert_eq!(
            plugin_lock_install_hint("demo", None, "sha256:aa"),
            "maw plugin install <source-of demo> --sha256 sha256:aa"
        );
    }
}
