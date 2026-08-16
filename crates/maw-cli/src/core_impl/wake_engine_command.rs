// Building the command line that actually starts the agent.
//
// An engine may be a real binary (claude, codex) or a config alias that expands
// to a whole shell command, optionally with env assignments in front and a
// resume subcommand that has to be injected after the binary rather than
// appended. This works out the final string, and what to warn about when the
// shape is one it cannot fully verify.

/// One resolved launch line plus the `commands` key it came from.
///
/// The key is not cosmetic: the `commands.<key>-resume` and
/// `commands.<key>-channels` contracts hang off it, and both `wake` and
/// `workon` have to agree on which key won rather than each re-deriving it
/// (#761).
#[derive(Debug, Clone, PartialEq, Eq)]
struct WakeCommandResolution {
    key: String,
    command: String,
}

/// Resolve a window's launch line through merged maw config `commands`.
///
/// maw-js matched the tmux WINDOW name against the `commands` map
/// (`command-logic.ts`), and the Rust port never carried that over: wake looked
/// only at `-e` → `wake.engine` → `commands.default`, so every per-agent entry
/// on a real fleet was dead config and each `maw wake` silently launched
/// `commands.default` — wrong model, no per-agent flags, no warning (#761).
/// `workon` resolved its own near-copy of the same lookup, so the two drifted;
/// they now share this function.
///
/// Precedence, highest first:
/// 1. `-e <engine>` when `commands.<engine>` exists — an explicit, configured engine.
/// 2. `commands.<window>` — the per-agent entry, keyed exactly as the window is named.
/// 3. `commands.<oracle>-oracle` / `commands.<oracle>` — the same entry under the
///    other window-naming convention (#771/T4527). maw-js named oracle windows
///    `<oracle>-oracle` and real fleet configs are keyed that way, while windows
///    maw-rs created itself carry the bare oracle name; normalizing the window
///    name to its oracle identity and trying both spellings (case-insensitively)
///    makes the lookup naming-agnostic without renaming anybody's tmux windows.
/// 4. a glob entry matching the window name (`prefix*` / `*suffix`).
/// 5. `-e <engine>` taken literally — an explicit engine with no `commands` entry
///    is still the binary the caller named. Falling through to `wake.engine` /
///    `commands.default` here would launch a DIFFERENT engine than the flag asked
///    for, which is the silent-wrong-engine failure #682 fixed.
/// 6. `wake.engine`, then the legacy `defaultEngine` alias (#682), through
///    `commands` when it has an entry and literally otherwise.
/// 7. `commands.default`.
/// 8. the caller's built-in fallback binary, likewise through `commands` first.
///
/// Resume no longer pins codex (#615): `--resume`/`wake.resume` resume whichever
/// engine this picks, so a repo committed to claude resumes claude, while a bare
/// `maw wake X --resume` with no engine config still lands on codex via tier 8.
fn wake_resolve_command_from_config(
    config: &serde_json::Value,
    window_name: &str,
    engine: Option<&str>,
    wake_engine: Option<&str>,
    builtin: &str,
) -> WakeCommandResolution {
    if let Some(engine) = engine {
        if let Some(command) = wake_config_command(config, engine) {
            return WakeCommandResolution { key: engine.to_owned(), command };
        }
    }
    if let Some(command) = wake_config_command(config, window_name) {
        return WakeCommandResolution { key: window_name.to_owned(), command };
    }
    for candidate in wake_oracle_command_keys(window_name) {
        if let Some((key, command)) = wake_config_command_entry(config, &candidate) {
            return WakeCommandResolution { key, command };
        }
    }
    if let Some((key, command)) = wake_config_glob_command(config, window_name) {
        return WakeCommandResolution { key, command };
    }
    if let Some(engine) = engine {
        return WakeCommandResolution { key: engine.to_owned(), command: engine.to_owned() };
    }
    if let Some(engine) = wake_engine {
        let command = wake_config_command(config, engine).unwrap_or_else(|| engine.to_owned());
        return WakeCommandResolution { key: engine.to_owned(), command };
    }
    if let Some(command) = wake_config_command(config, "default") {
        return WakeCommandResolution { key: "default".to_owned(), command };
    }
    let command = wake_config_command(config, builtin).unwrap_or_else(|| builtin.to_owned());
    WakeCommandResolution { key: builtin.to_owned(), command }
}

/// The alternate `commands` keys one window can legitimately be configured
/// under: its oracle identity spelled with and without the `-oracle` suffix
/// (#771). Lowercased, because the identity's case comes from a repo directory
/// name (`Colophon-Oracle`) while config keys are written in lower case. The
/// window's own literal name is dropped — tier 2 already tried it.
fn wake_oracle_command_keys(window_name: &str) -> Vec<String> {
    let stem = wake_oracle_stem(window_name);
    if stem.is_empty() {
        return Vec::new();
    }
    let stem = stem.to_lowercase();
    let mut keys = vec![format!("{stem}-oracle"), stem];
    keys.retain(|key| key.as_str() != window_name);
    keys
}

/// maw-js `matchGlob` parity (`command-logic.ts`): a `commands` key with a
/// leading or trailing `*` matches a family of window names (`agent*`,
/// `*-oracle`). `default` is the explicit last-resort key, never a glob.
fn wake_config_glob_command(config: &serde_json::Value, name: &str) -> Option<(String, String)> {
    config.get("commands").and_then(serde_json::Value::as_object).and_then(|commands| {
        commands.iter().find_map(|(pattern, value)| {
            if pattern == "default" || pattern == name || !wake_match_command_glob(pattern, name) {
                return None;
            }
            value
                .as_str()
                .map(str::trim)
                .filter(|command| !command.is_empty())
                .map(|command| (pattern.clone(), command.to_owned()))
        })
    })
}

fn wake_match_command_glob(pattern: &str, name: &str) -> bool {
    pattern.strip_prefix('*').is_some_and(|suffix| name.ends_with(suffix))
        || pattern.strip_suffix('*').is_some_and(|prefix| name.starts_with(prefix))
}

/// Legacy top-level `defaultEngine` (#682) — a non-empty string names the
/// engine directly, exactly like `wake.engine`. Kept as a named helper so the
/// alias is greppable from the issue and so `maw config` can report that the
/// key is live rather than dead.
fn wake_config_default_engine_alias(config: &serde_json::Value) -> Option<String> {
    config
        .get(CONFIG_LEGACY_DEFAULT_ENGINE_KEY)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Committed wake-defaults block — the `wake` object in merged config:
/// `{ "engine": string, "resume": bool, "channels": bool, "prompt": string }`.
/// Read through the same dir-aware merge as `commands`, so a repo layer
/// (`.maw/maw.config.<NN>.json`) overrides user config per the usual weights.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct WakeConfigDefaults {
    engine: Option<String>,
    resume: bool,
    channels: bool,
    prompt: Option<String>,
}

fn wake_config_defaults(config: &serde_json::Value) -> WakeConfigDefaults {
    let block = config.get("wake");
    let text = |key: &str| {
        block
            .and_then(|wake| wake.get(key))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let flag = |key: &str| {
        block
            .and_then(|wake| wake.get(key))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    };
    WakeConfigDefaults {
        engine: text("engine"),
        resume: flag("resume"),
        channels: flag("channels"),
        prompt: text("prompt"),
    }
}

/// Build the in-pane launch line: `MAW_SESSION_WINDOW=<window> <engine>` —
/// work-parity (#601), bare engine and NOT `exec`, so the shell survives
/// engine exit.
///
/// The old echoed `cd DIR && { …; printf … }` wrapper is gone: the pane
/// already starts inside the repo via tmux `-c` at session/window creation
/// (`wake_apply` verifies the path exists before creating anything, and the
/// native `wake_new_session`/`wake_new_window` re-validate it), and launch
/// failure is caught Rust-side by the pane poll
/// (`wake_confirm_engine_launch`, #580) instead of an in-pane printf.
/// `cwd` stays a parameter because engine/defaults config is resolved
/// dir-aware against the resolved repo path (#600).
fn wake_command(window: &str, cwd: &std::path::Path, options: &WakeOptionsNative) -> (String, Vec<String>) {
    let config = merged_config_value_in_dir(cwd);
    let defaults = wake_config_defaults(&config);
    let wake_engine = defaults.engine.clone().or_else(|| wake_config_default_engine_alias(&config));
    let resolution =
        wake_resolve_command_from_config(&config, window, options.engine.as_deref(), wake_engine.as_deref(), "codex");
    let engine = resolution.key;
    // Wake-defaults precedence: explicit CLI flag > repo-layer config > user
    // config > built-in. `resume`/`channels` are presence-false CLI booleans —
    // `false` means "flag not passed", so a config default fills in only then
    // (there is no negative flag); `--fresh` is the explicit opt-out for a
    // configured `wake.resume`. `prompt` tracks presence via `Option`.
    let resume = options.resume || (defaults.resume && !options.fresh);
    let channels = options.channels || defaults.channels;
    let mut warnings = Vec::new();
    let mut engine_command =
        wake_engine_launch_command(&engine, resolution.command, &config, resume, options.engine_command.as_deref(), &mut warnings);
    if channels { wake_apply_channels(&mut engine_command, &engine, &config, resume, &mut warnings); }
    if let Some(prompt) = options.prompt.as_deref().or(defaults.prompt.as_deref()) { let _ = write!(engine_command, " {}", wake_shell_quote(prompt)); }
    (format!("MAW_SESSION_WINDOW={} {engine_command}", wake_shell_quote(window)), warnings)
}

/// Resume-aware launch line (#615). Precedence when resume is in effect:
/// 1. `commands.<engine>-resume` — the existing fleet convention: a COMPLETE
///    replacement command line (not a suffix), dir-aware via
///    `merged_config_value_in_dir` so repos can commit their own form.
/// 2. Engine-family fallback on the resolved command's binary token:
///    claude → append ` --continue`; codex/omx → inject the `resume`
///    subcommand directly after the binary (`codex resume <flags>` is valid).
/// 3. Unknown binary — keep the historical naive ` resume` append, but warn
///    so it is no longer silent.
fn wake_engine_launch_command(
    engine: &str,
    resolved: String,
    config: &serde_json::Value,
    resume: bool,
    engine_command: Option<&str>,
    warnings: &mut Vec<String>,
) -> String {
    // 0. `commands.<engine>-resume` stays first even against `--engine-cmd`
    //    (#738 review): the `-resume` key is an operator's EXPLICIT resume
    //    contract, while a charter `engines:` line only encodes cold start —
    //    silently discarding the `-resume` command would be the same
    //    config-that-looks-live-but-isn't failure #682 fixed. The bypass is
    //    warned so neither side is overridden silently.
    if resume {
        if let Some(command) = wake_config_command(config, &format!("{engine}-resume")) {
            if engine_command.is_some() {
                warnings.push(format!(
                    "wake: commands.{engine}-resume overrides the charter engine command for resume"
                ));
            }
            return workon_prefix_zai_pool(config, command);
        }
    }
    // 1. `--engine-cmd` — a caller-supplied launch line (a team charter's
    //    `engines:` entry, #738) outranks the rest of the `commands.*` map for
    //    cold start; without a `-resume` key, resume falls to the family-based
    //    form below, derived from the charter's own binary.
    let command = workon_prefix_zai_pool(config, engine_command.map_or(resolved, str::to_owned));
    if !resume {
        return command;
    }
    match wake_engine_binary(&command) {
        Some("claude") => format!("{command} --continue"),
        Some("codex" | "omx") => wake_inject_resume_subcommand(&command).unwrap_or_else(|| format!("{command} resume")),
        _ => {
            warnings.push(format!(
                "wake: resume requested but engine '{engine}' has no known resume form — appending ' resume' naively; define commands.{engine}-resume with the full resume launch line"
            ));
            format!("{command} resume")
        }
    }
}

/// Channels flag is claude-only (#615): honor a `commands.<engine>-channels`
/// full-replacement entry first (skipped when a resume line is already in
/// effect — replacing it would drop the resume form), then append the claude
/// plugin flag only for claude-family binaries, warning otherwise instead of
/// handing a foreign flag to codex/omx.
fn wake_apply_channels(
    engine_command: &mut String,
    engine: &str,
    config: &serde_json::Value,
    resume: bool,
    warnings: &mut Vec<String>,
) {
    if !resume {
        if let Some(command) = wake_config_command(config, &format!("{engine}-channels")) {
            *engine_command = workon_prefix_zai_pool(config, command);
            return;
        }
    }
    if wake_engine_binary(engine_command) == Some("claude") {
        engine_command.push_str(" --channels plugin:discord@claude-plugins-official");
    } else {
        warnings.push(format!(
            "wake: channels requested but engine '{engine}' is not claude-family — skipping the claude-only --channels flag; define commands.{engine}-channels for an engine-specific line"
        ));
    }
}

/// A non-empty `commands.<name>` entry from merged config, if present.
fn wake_config_command(config: &serde_json::Value, name: &str) -> Option<String> {
    config
        .get("commands")
        .and_then(|commands| commands.get(name))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Same as `wake_config_command`, but falls back to a case-insensitive key
/// scan and reports the key it actually matched. Only the derived
/// oracle-identity keys (#771) go through this: the matched key, not the probe,
/// is what the `-resume`/`-channels` contracts are keyed on, so a config that
/// spells its entry `Colophon-Oracle` keeps `Colophon-Oracle-resume` reachable.
fn wake_config_command_entry(config: &serde_json::Value, name: &str) -> Option<(String, String)> {
    if let Some(command) = wake_config_command(config, name) {
        return Some((name.to_owned(), command));
    }
    config.get("commands").and_then(serde_json::Value::as_object).and_then(|commands| {
        commands.iter().find_map(|(key, value)| {
            if !key.eq_ignore_ascii_case(name) {
                return None;
            }
            value
                .as_str()
                .map(str::trim)
                .filter(|command| !command.is_empty())
                .map(|command| (key.clone(), command.to_owned()))
        })
    })
}

/// Byte span of the engine binary token in a resolved command line: the first
/// whitespace-delimited word that is neither a `VAR=value` env assignment nor
/// the shell `command` builtin.
fn wake_engine_binary_span(command: &str) -> Option<(usize, usize)> {
    let mut start = 0;
    while start < command.len() {
        let rest = &command[start..];
        start += rest.len() - rest.trim_start().len();
        if start >= command.len() {
            return None;
        }
        let rest = &command[start..];
        let end = start + rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = &command[start..end];
        if !wake_is_env_assignment(word) && word != "command" {
            return Some((start, end));
        }
        start = end;
    }
    None
}

fn wake_is_env_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else { return false };
    !name.is_empty()
        && !name.starts_with(|ch: char| ch.is_ascii_digit())
        && name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Basename of the engine binary token — decides the resume/channels family.
fn wake_engine_binary(command: &str) -> Option<&str> {
    wake_engine_binary_span(command).map(|(start, end)| {
        let word = &command[start..end];
        word.rsplit('/').next().unwrap_or(word)
    })
}

/// `codex resume <flags>` — the subcommand goes right after the binary token.
fn wake_inject_resume_subcommand(command: &str) -> Option<String> {
    wake_engine_binary_span(command).map(|(_, end)| {
        let mut out = command.to_owned();
        out.insert_str(end, " resume");
        out
    })
}

fn wake_shell_quote(value: &str) -> String {
    if value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '=')) { return value.to_owned(); }
    format!("'{}'", value.replace('\'', "'\\''"))
}
