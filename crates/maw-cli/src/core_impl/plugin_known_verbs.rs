/// Known extracted verbs: CLI commands that maw provides through fleet
/// plugins rather than native handlers. When such a verb resolves to no
/// native handler and no installed plugin, the dispatcher prints an
/// actionable install hint instead of the bare `unknown command` exit 2
/// (issue #522 defense 4 — a missing plugin must never read as a typo).
///
/// Rows: (cli verb, plugin name, `Soul-Brews-Studio/maw-plugins`
/// `packages/` dir). The plugins themselves live in the external
/// `Soul-Brews-Studio/maw-plugins` monorepo (extracted from this repo's
/// former `fleet-plugins/` on 2026-07-15, repo split phase 1); the parity
/// test in `crates/maw-cli/tests/plugin_missing_verb_cli.rs` is `#[ignore]`d
/// until it is repointed at that repo's manifests (repo-split test rework).
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

/// A known plugin verb resolved to its install source + pin — the
/// programmatic "verb → known source + sha256" lookup (mawx WI-2,
/// spec §2.2 step 3b/3c). The missing-verb install-hint printer
/// (`missing_plugin_output`) is one consumer; `maw x` resolution (WI-8)
/// is the next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPluginSource {
    /// The CLI verb that resolved (e.g. `menubar`).
    pub verb: String,
    /// The plugin that provides the verb (e.g. `maw-menubar`).
    pub plugin_name: String,
    /// Canonical install source in `maw plugin install` grammar
    /// (`github:` prefix already stripped; `path:` prefix preserved).
    /// `None` when a `plugins.lock` pin recorded no source.
    pub source: Option<String>,
    /// The `plugins.lock` sha256 pin (`sha256:<64hex>` form). `None` for
    /// baked fleet-table rows, which carry no pin.
    pub sha256: Option<String>,
}

impl ResolvedPluginSource {
    /// The actionable `maw plugin install …` line for this resolution.
    #[must_use]
    pub fn install_hint(&self) -> String {
        match (&self.source, &self.sha256) {
            (Some(source), _) if source.starts_with("path:") => {
                format!("maw plugin install {}", source.trim_start_matches("path:"))
            }
            (Some(source), Some(sha256)) => {
                format!("maw plugin install {source} --sha256 {sha256}")
            }
            (Some(source), None) => format!("maw plugin install {source}"),
            (None, Some(sha256)) => format!(
                "maw plugin install <source-of {}> --sha256 {sha256}",
                self.plugin_name
            ),
            (None, None) => format!("maw plugin install <source-of {}>", self.plugin_name),
        }
    }
}

/// Normalize a `plugins.lock` source string into `maw plugin install`
/// grammar: the `github:` prefix is implicit there, so strip it; every
/// other form (`path:`, bare `owner/repo@ref`) passes through unchanged.
fn canonical_lock_source(source: Option<&str>) -> Option<String> {
    source.map(|value| value.strip_prefix("github:").unwrap_or(value).to_owned())
}

/// Resolve `verb` against the known extracted verbs: the baked
/// fleet-plugins table first, then this machine's `plugins.lock` pins
/// (a lock entry means this machine expects the plugin; the verb defaults
/// to the plugin name).
#[must_use]
pub fn resolve_plugin_source(verb: &str) -> Option<ResolvedPluginSource> {
    if let Some((_, plugin, dir)) = KNOWN_FLEET_PLUGIN_VERBS
        .iter()
        .find(|(known_verb, _, _)| *known_verb == verb)
    {
        return Some(ResolvedPluginSource {
            verb: verb.to_owned(),
            plugin_name: (*plugin).to_owned(),
            source: Some(format!("Soul-Brews-Studio/maw-plugins/packages/{dir}")),
            sha256: None,
        });
    }
    let entry = read_plugin_lock_entry_full(verb).ok().flatten()?;
    Some(ResolvedPluginSource {
        verb: verb.to_owned(),
        plugin_name: verb.to_owned(),
        source: canonical_lock_source(entry.source.as_deref()),
        sha256: Some(entry.sha256),
    })
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
///
/// This printer is ONE consumer of [`ResolvedPluginSource`]; the struct is
/// the reusable resolution result (mawx WI-2).
fn missing_plugin_output(command: &str, discovery_warnings: &[String]) -> Option<CliOutput> {
    let resolved = resolve_plugin_source(command)?;
    let needle = format!("'{}'", resolved.plugin_name);
    if let Some(refusal) = discovery_warnings
        .iter()
        .find(|warning| warning.contains(&needle))
    {
        return Some(CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!(
                "maw-rs: verb '{command}' is provided by plugin '{}' but the plugin refused to load:\n{refusal}\n",
                resolved.plugin_name
            ),
        });
    }
    Some(CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "maw-rs: verb '{command}' is provided by plugin '{}', which is not installed on this machine\n  install: {}\n",
            resolved.plugin_name,
            resolved.install_hint()
        ),
    })
}

#[cfg(test)]
mod known_verb_tests {
    use super::*;

    #[test]
    fn fleet_table_resolves_verbs_to_sources_and_install_hints() {
        let resolved = resolve_plugin_source("menubar").expect("menubar known");
        assert_eq!(
            resolved,
            ResolvedPluginSource {
                verb: "menubar".to_owned(),
                plugin_name: "maw-menubar".to_owned(),
                source: Some("Soul-Brews-Studio/maw-plugins/packages/maw-menubar".to_owned()),
                sha256: None,
            }
        );
        assert_eq!(
            resolved.install_hint(),
            "maw plugin install Soul-Brews-Studio/maw-plugins/packages/maw-menubar"
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
    fn canonical_lock_source_strips_github_prefix_and_keeps_paths() {
        assert_eq!(
            canonical_lock_source(Some("github:o/r@v1")),
            Some("o/r@v1".to_owned())
        );
        assert_eq!(
            canonical_lock_source(Some("path:/tmp/demo")),
            Some("path:/tmp/demo".to_owned())
        );
        assert_eq!(canonical_lock_source(None), None);
    }

    #[test]
    fn install_hint_covers_pinned_path_sourceless_and_unpinned_shapes() {
        let pinned = ResolvedPluginSource {
            verb: "demo".to_owned(),
            plugin_name: "demo".to_owned(),
            source: Some("o/r@v1".to_owned()),
            sha256: Some("sha256:aa".to_owned()),
        };
        assert_eq!(
            pinned.install_hint(),
            "maw plugin install o/r@v1 --sha256 sha256:aa"
        );

        let path = ResolvedPluginSource {
            source: Some("path:/tmp/demo".to_owned()),
            ..pinned.clone()
        };
        assert_eq!(path.install_hint(), "maw plugin install /tmp/demo");

        let sourceless = ResolvedPluginSource {
            source: None,
            ..pinned.clone()
        };
        assert_eq!(
            sourceless.install_hint(),
            "maw plugin install <source-of demo> --sha256 sha256:aa"
        );

        let unpinned = ResolvedPluginSource {
            source: None,
            sha256: None,
            ..pinned
        };
        assert_eq!(
            unpinned.install_hint(),
            "maw plugin install <source-of demo>"
        );
    }
}
