// Building the command line that actually starts the agent.
//
// An engine may be a real binary (claude, codex) or a config alias that expands
// to a whole shell command, optionally with env assignments in front and a
// resume subcommand that has to be injected after the binary rather than
// appended. This works out the final string, and what to warn about when the
// shape is one it cannot fully verify.

/// Resolve an engine alias through merged maw config `commands` (matches
/// `workon` + maw-js): custom engines like `omx-1` expand to their full shell
/// command; real binaries (codex/claude) fall through to the literal name.
/// Fixes the fleet codex-team recipe (omx-N) that previously ran a bare `omx-1`.
/// Config is resolved dir-aware against the resolved repo path (like `workon`),
/// so committed `.maw/maw.config.<NN>.json` layers apply regardless of the
/// invoking shell's cwd (#600).
fn wake_resolve_engine_command(engine: &str, cwd: &std::path::Path) -> String {
    let config = merged_config_value_in_dir(cwd);
    let command = config
        .get("commands")
        .and_then(|commands| {
            commands
                .get(engine)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| engine.to_owned());
    workon_prefix_zai_pool(&config, command)
}

fn wake_default_engine(options: &WakeOptionsNative, cwd: &std::path::Path) -> String {
    if let Some(engine) = &options.engine {
        return engine.clone();
    }
    let config = merged_config_value_in_dir(cwd);
    let defaults = wake_config_defaults(&config);
    // Resume no longer pins codex (#615): `--resume`/`wake.resume` resume the
    // engine that resolution below picks (wake.engine → commands.default →
    // built-in codex), so a repo committed to claude resumes claude. A bare
    // `maw wake X --resume` with no engine config still lands on codex via
    // the final fallback, preserving the historical behavior where it mattered.
    if let Some(engine) = defaults.engine {
        return engine;
    }
    // `defaultEngine` is a maw-js-era top-level key that survived the port and
    // is still set in real fleet configs (#682). It used to be read by nothing,
    // so an operator who set `"defaultEngine": "claude"` silently got the
    // built-in fallback instead. It is now honoured as an alias of
    // `wake.engine`, sorting below it and above `commands.default` — an
    // explicit engine name beats a command alias.
    if let Some(engine) = wake_config_default_engine_alias(&config) {
        return engine;
    }
    config
        .get("commands")
        .and_then(|commands| commands.get("default"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map_or_else(|| "codex".to_owned(), |_| "default".to_owned())
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
    let engine = wake_default_engine(options, cwd);
    // Wake-defaults precedence: explicit CLI flag > repo-layer config > user
    // config > built-in. `resume`/`channels` are presence-false CLI booleans —
    // `false` means "flag not passed", so a config default fills in only then
    // (there is no negative flag); `--fresh` is the explicit opt-out for a
    // configured `wake.resume`. `prompt` tracks presence via `Option`.
    let resume = options.resume || (defaults.resume && !options.fresh);
    let channels = options.channels || defaults.channels;
    let mut warnings = Vec::new();
    let mut engine_command = wake_engine_launch_command(&engine, cwd, &config, resume, options.engine_command.as_deref(), &mut warnings);
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
    cwd: &std::path::Path,
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
    let command = if let Some(line) = engine_command {
        workon_prefix_zai_pool(config, line.to_owned())
    } else {
        wake_resolve_engine_command(engine, cwd)
    };
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
