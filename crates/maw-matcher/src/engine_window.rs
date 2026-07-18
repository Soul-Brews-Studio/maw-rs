/// Pure tmux window metadata used by [`resolve_engine_window_target`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub index: u32,
    pub name: String,
    pub current_command: String,
}

/// A concrete session/window target selected by the resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWindowTarget {
    pub target: String,
    pub window: WindowInfo,
}

/// Typed result for bare-session engine-window resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineWindowResolution {
    /// The caller supplied an explicit numeric `:N` suffix, which always wins.
    Explicit(ResolvedWindowTarget),
    /// Exactly one non-shell window was found.
    Engine(ResolvedWindowTarget),
    /// Multiple non-shell windows require the caller to choose explicitly.
    Ambiguous { candidates: Vec<WindowInfo> },
    /// No non-shell window exists; window 0 is returned for caller policy.
    Fallback(ResolvedWindowTarget),
}

/// Resolve a session target from caller-supplied window topology.
///
/// This leaf-crate helper performs no tmux I/O. An explicit numeric window
/// suffix always wins. For a bare session, the shell-inverse predicate selects
/// the sole non-shell window; multiple candidates remain typed ambiguity
/// rather than being guessed. With no engine candidate, window 0 is returned
/// with a [`EngineWindowResolution::Fallback`] marker so the caller can decide
/// whether delivery to a shell is safe.
#[must_use]
pub fn resolve_engine_window_target(
    session_target: &str,
    windows: &[WindowInfo],
) -> EngineWindowResolution {
    if let Some(index) = explicit_window_index(session_target) {
        return EngineWindowResolution::Explicit(resolved_target(session_target, index, windows));
    }

    let candidates: Vec<WindowInfo> = windows
        .iter()
        .filter(|window| !command_is_shell(&window.current_command))
        .cloned()
        .collect();

    match candidates.as_slice() {
        [window] => EngineWindowResolution::Engine(ResolvedWindowTarget {
            target: format!("{session_target}:{}", window.index),
            window: window.clone(),
        }),
        [] => EngineWindowResolution::Fallback(resolved_target(session_target, 0, windows)),
        _ => EngineWindowResolution::Ambiguous { candidates },
    }
}

fn explicit_window_index(target: &str) -> Option<u32> {
    target.rsplit_once(':')?.1.parse().ok()
}

fn resolved_target(
    session_target: &str,
    index: u32,
    windows: &[WindowInfo],
) -> ResolvedWindowTarget {
    let session = session_target
        .rsplit_once(':')
        .filter(|(_, suffix)| suffix.parse::<u32>().is_ok())
        .map_or(session_target, |(session, _)| session);
    let window = windows
        .iter()
        .find(|window| window.index == index)
        .cloned()
        .unwrap_or_else(|| WindowInfo {
            index,
            name: String::new(),
            current_command: String::new(),
        });
    ResolvedWindowTarget {
        target: format!("{session}:{index}"),
        window,
    }
}

fn command_is_shell(command: &str) -> bool {
    let name = command.trim().trim_start_matches('-');
    let name = name.rsplit('/').next().unwrap_or(name);
    matches!(
        name,
        "" | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "dash"
            | "ash"
            | "ksh"
            | "tcsh"
            | "csh"
            | "nu"
            | "pwsh"
    )
}
