// Pure peer source resolution ported from maw-js `peer-sources.ts`.
//
// This crate does not perform network discovery. Callers pass already-fetched
// scout discovery data, keeping the fixture-tested policy deterministic.

use std::{
    collections::BTreeMap,
    error::Error,
    fmt::Write,
    fs, io,
    path::{Path, PathBuf},
};

use maw_xdg::{maw_state_path, MawXdgEnv};
use serde::{Deserialize, Serialize};

/// Peer source mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerSourceMode {
    Config,
    Scout,
    Both,
}

impl PeerSourceMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Scout => "scout",
            Self::Both => "both",
        }
    }
}

/// Peer target source kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerSourceKind {
    Config,
    Scout,
}

impl PeerSourceKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Scout => "scout",
        }
    }
}

/// Named peer from config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedPeerConfig {
    pub name: String,
    pub url: String,
}

/// Minimal maw config shape needed for peer source resolution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeerConfig {
    pub peers: Vec<String>,
    pub named_peers: Vec<NamedPeerConfig>,
}

/// Resolved peer target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerTarget {
    pub name: Option<String>,
    pub url: String,
    pub source: PeerSourceKind,
    pub node: Option<String>,
    pub oracle: Option<String>,
}

/// Scout discovery row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveryRow {
    pub node: Option<String>,
    pub oracle: Option<String>,
    pub host: Option<String>,
    pub locators: Vec<String>,
}

/// Discovery response supplied by runtime IO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryResult {
    Ok { peers: Vec<DiscoveryRow> },
    Err { error: String, hint: Option<String> },
}

/// Peer source resolver result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSourceResult {
    pub mode: PeerSourceMode,
    pub peers: Vec<PeerTarget>,
    pub warnings: Vec<String>,
    /// Number of discovery fetches the JS implementation would perform.
    pub fetch_calls: usize,
}

/// Parse a peer source mode value.
#[must_use]
pub fn parse_peer_source_mode(
    value: Option<&str>,
    fallback: PeerSourceMode,
) -> Option<PeerSourceMode> {
    match value {
        None | Some("") => Some(fallback),
        Some("config") => Some(PeerSourceMode::Config),
        Some("scout") => Some(PeerSourceMode::Scout),
        Some("both") => Some(PeerSourceMode::Both),
        Some(_) => None,
    }
}

/// Return configured peer targets with flat peers before named peers, deduped by URL.
#[must_use]
pub fn configured_peer_targets(config: &PeerConfig) -> Vec<PeerTarget> {
    let flat = config.peers.iter().map(|url| PeerTarget {
        name: None,
        url: url.clone(),
        source: PeerSourceKind::Config,
        node: None,
        oracle: None,
    });
    let named = config.named_peers.iter().map(|peer| PeerTarget {
        name: Some(peer.name.clone()),
        url: peer.url.clone(),
        source: PeerSourceKind::Config,
        node: None,
        oracle: None,
    });
    dedupe_peer_targets(flat.chain(named).collect())
}

/// Resolve config/scout peer sources from deterministic inputs.
#[must_use]
pub fn resolve_peer_sources(
    config: &PeerConfig,
    mode: PeerSourceMode,
    discoveries: Option<&DiscoveryResult>,
) -> PeerSourceResult {
    let config_peers = if mode == PeerSourceMode::Scout {
        Vec::new()
    } else {
        configured_peer_targets(config)
    };
    let mut warnings = Vec::new();
    let mut scout_peers = Vec::new();
    let mut fetch_calls = 0;

    if matches!(mode, PeerSourceMode::Scout | PeerSourceMode::Both) {
        fetch_calls = 1;
        match discoveries {
            Some(DiscoveryResult::Ok { peers }) => {
                scout_peers = peers.iter().filter_map(discovered_peer_target).collect();
            }
            Some(DiscoveryResult::Err { error, hint }) => {
                warnings.push(format_scout_warning(error, hint.as_deref()));
            }
            None => warnings.push("scout unavailable (missing_discoveries)".to_owned()),
        }
    }

    let peers = if mode == PeerSourceMode::Scout {
        scout_peers
    } else {
        let mut combined = config_peers;
        combined.extend(scout_peers);
        combined
    };

    PeerSourceResult {
        mode,
        peers: dedupe_peer_targets(peers),
        warnings,
        fetch_calls,
    }
}

/// Dedupe peer targets by URL after trimming trailing slashes.
#[must_use]
pub fn dedupe_peer_targets(peers: Vec<PeerTarget>) -> Vec<PeerTarget> {
    let mut seen: Vec<String> = Vec::new();
    let mut merged = Vec::new();
    for peer in peers {
        let key = peer_key(&peer.url);
        if seen.iter().any(|existing| existing == &key) {
            continue;
        }
        seen.push(key);
        merged.push(peer);
    }
    merged
}

fn discovered_peer_target(peer: &DiscoveryRow) -> Option<PeerTarget> {
    let url = peer.locators.iter().find(|locator| is_http_url(locator))?;
    Some(PeerTarget {
        name: peer.node.clone().or_else(|| peer.host.clone()),
        url: url.clone(),
        source: PeerSourceKind::Scout,
        node: peer.node.clone(),
        oracle: peer.oracle.clone(),
    })
}

fn is_http_url(value: &str) -> bool {
    let lower = value.to_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn peer_key(url: &str) -> String {
    url.trim_end_matches('/').to_owned()
}

