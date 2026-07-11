const DISPATCH_99: &[DispatcherEntry] = &[DispatcherEntry {
    command: "completions",
    handler: Handler::Sync(completions_run_command),
}];

const COMPLETIONS_HELP: &str = "usage: maw completions <commands [--describe]|subs <command>|fleet|oracle|squads|oracles|windows|zsh|bash|fish>\n\nGenerate shell completion scripts and dynamic completion data for maw.\n\nInstall examples:\n  zsh:  mkdir -p ~/.zsh/completions && maw completions zsh > ~/.zsh/completions/_maw\n        add to ~/.zshrc before compinit: fpath=(~/.zsh/completions $fpath)\n  bash: maw completions bash > ~/.maw-completion.bash\n        add to ~/.bashrc: source ~/.maw-completion.bash\n  fish: mkdir -p ~/.config/fish/completions && maw completions fish > ~/.config/fish/completions/maw.fish\n\nData subcommands:\n  commands             command names for first-position completion\n  commands --describe  one name<TAB>description line per command, sourced from usage strings\n  subs <command>       second-position words for <command>, derived from its usage string\n  fleet                fleet subcommand names (alias for: subs fleet)\n  oracle               oracle subcommand names (alias for: subs oracle)\n  squads               squad names from fleet roster files\n  oracles              oracle names from fleet configs\n  windows              tmux window/session names from fleet configs";

const COMPLETIONS_ZSH: &str = r#"#compdef maw

_maw_subs() {
  local -a subs
  subs=(${(f)"$(maw completions subs "$1" 2>/dev/null)"})
  (( ${#subs} )) || return 1
  compadd -- ${subs[@]}
}

_maw_oracles() {
  local -a oracles
  oracles=(${(f)"$(maw completions oracles 2>/dev/null)"})
  _describe 'oracle' oracles
}

_maw_windows() {
  local -a windows
  windows=(${(f)"$(maw completions windows 2>/dev/null)"})
  _describe 'window' windows
}

_maw_squads() {
  local -a squads
  squads=(${(f)"$(maw completions squads 2>/dev/null)"})
  _describe 'fleet squad' squads
}

_maw_commands() {
  local line
  local -a lines commands oracles
  lines=(${(f)"$(maw completions commands --describe 2>/dev/null)"})
  for line in ${lines[@]}; do
    [[ -n $line ]] || continue
    if [[ $line == *$'\t'* ]]; then
      commands+=("${line%%$'\t'*}:${line#*$'\t'}")
    else
      commands+=("$line")
    fi
  done
  for line in ${(f)"$(maw completions oracles 2>/dev/null)"}; do
    [[ -n $line ]] && oracles+=("${line}:Oracle (peek/send shorthand)")
  done
  _describe -t oracle-shorthand 'oracle shorthand' oracles
  _describe -t maw-commands 'maw commands' commands
}

_maw() {
  local curcontext="$curcontext" state line
  typeset -A opt_args

  # Labeled groups (oracle shorthand vs maw commands) need group-name + format
  # styles; add maw-scoped defaults only when the user has none of their own.
  local _maw_style
  zstyle -s ":completion:${curcontext}:descriptions" format _maw_style ||
    zstyle ':completion:*:*:maw:*:descriptions' format '%B%d%b'
  zstyle -s ":completion:${curcontext}:" group-name _maw_style ||
    zstyle ':completion:*:*:maw:*' group-name ''

  _arguments -C \
    '1:command:->cmd' \
    '*::arg:->args'

  case $state in
    cmd)
      _maw_commands
      ;;
    args)
      case $line[1] in
        peek|see|a|attach|bring|b|hey|send|tell|done|finish)
          _maw_windows
          ;;
        wake|about|info)
          _maw_oracles
          ;;
        fleet)
          if (( CURRENT == 2 )); then
            _maw_subs fleet
          elif (( CURRENT == 3 )); then
            case $words[2] in
              show|status|wake|sleep|token) _maw_squads ;;
            esac
          fi
          ;;
        oracle)
          if (( CURRENT == 2 )); then
            _maw_subs oracle
          elif [[ $words[2] == recruit ]]; then
            (( CURRENT == 3 )) && _maw_squads
            (( CURRENT == 4 )) && _maw_oracles
          fi
          ;;
        serve)
          _message 'port (default: 3456)'
          ;;
        *)
          if (( CURRENT == 2 )); then
            _maw_subs "$line[1]" || _message 'argument'
          else
            _message 'argument'
          fi
          ;;
      esac
      ;;
  esac
}

_maw "$@""#;

const COMPLETIONS_BASH: &str = r#"# maw bash completion
_maw_complete() {
  local cur cmd words
  COMPREPLY=()
  cur="${COMP_WORDS[COMP_CWORD]}"

  if [[ $COMP_CWORD -eq 1 ]]; then
    words="$(maw completions commands 2>/dev/null)"
    COMPREPLY=( $(compgen -W "$words" -- "$cur") )
    return 0
  fi

  cmd="${COMP_WORDS[1]}"
  case "$cmd" in
    peek|see|a|attach|bring|b|hey|send|tell|done|finish)
      words="$(maw completions windows 2>/dev/null)"
      ;;
    wake|about|info)
      words="$(maw completions oracles 2>/dev/null)"
      ;;
    fleet)
      if [[ $COMP_CWORD -eq 2 ]]; then
        words="$(maw completions subs fleet 2>/dev/null)"
      elif [[ $COMP_CWORD -eq 3 ]]; then
        case "${COMP_WORDS[2]}" in
          show|status|wake|sleep|token) words="$(maw completions squads 2>/dev/null)" ;;
        esac
      fi
      ;;
    oracle)
      if [[ $COMP_CWORD -eq 2 ]]; then
        words="$(maw completions subs oracle 2>/dev/null)"
      elif [[ ${COMP_WORDS[2]} == recruit && $COMP_CWORD -eq 3 ]]; then
        words="$(maw completions squads 2>/dev/null)"
      elif [[ ${COMP_WORDS[2]} == recruit && $COMP_CWORD -eq 4 ]]; then
        words="$(maw completions oracles 2>/dev/null)"
      fi
      ;;
    *)
      words=""
      if [[ $COMP_CWORD -eq 2 ]]; then
        words="$(maw completions subs "$cmd" 2>/dev/null)"
      fi
      ;;
  esac
  COMPREPLY=( $(compgen -W "$words" -- "$cur") )
}
complete -F _maw_complete maw"#;

const COMPLETIONS_FISH: &str = r"# maw fish completion
complete -c maw -f -n '__fish_use_subcommand' -a '(maw completions commands --describe 2>/dev/null)'
complete -c maw -f -n '__fish_use_subcommand' -a '(maw completions oracles 2>/dev/null)' -d 'Oracle (peek/send shorthand)'
complete -c maw -f -n '__fish_seen_subcommand_from wake about info' -a '(maw completions oracles 2>/dev/null)'
complete -c maw -f -n '__fish_seen_subcommand_from peek see a attach bring b hey send tell done finish' -a '(maw completions windows 2>/dev/null)'
complete -c maw -f -n 'test (count (commandline -opc)) -eq 2' -a '(maw completions subs (commandline -opc)[2] 2>/dev/null)'
complete -c maw -f -n 'test (count (commandline -opc)) -eq 3; and contains -- (commandline -opc)[2] fleet; and contains -- (commandline -opc)[3] show status wake sleep token' -a '(maw completions squads 2>/dev/null)'
complete -c maw -f -n 'test (count (commandline -opc)) -eq 3; and contains -- (commandline -opc)[2] oracle; and contains -- (commandline -opc)[3] recruit' -a '(maw completions squads 2>/dev/null)'
complete -c maw -f -n 'test (count (commandline -opc)) -eq 4; and contains -- (commandline -opc)[2] oracle; and contains -- (commandline -opc)[3] recruit' -a '(maw completions oracles 2>/dev/null)'";

fn completions_run_command(argv: &[String]) -> CliOutput {
    match completions_parse_request(argv).and_then(completions_render_request) {
        Ok(stdout) => completions_ok(&stdout),
        Err(message) if message.is_empty() => completions_ok(COMPLETIONS_HELP),
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionsRequest<'a> {
    Mode(&'a str),
    CommandsDescribed,
    Subs(&'a str),
}

fn completions_parse_request(argv: &[String]) -> Result<CompletionsRequest<'_>, String> {
    let Some(mode) = argv.first().map(String::as_str) else { return Err(String::new()); };
    match (mode, argv.get(1).map(String::as_str)) {
        ("commands", Some("--describe")) if argv.len() == 2 => return Ok(CompletionsRequest::CommandsDescribed),
        ("subs", Some(command)) if argv.len() == 2 => {
            if !completions_is_safe_target(command) {
                return Err(format!("completions: invalid command name: {command}"));
            }
            return Ok(CompletionsRequest::Subs(command));
        }
        ("subs", None) => return Err("completions: subs expects exactly one command name".to_owned()),
        _ => {}
    }
    if argv.len() > 1 { return Err("completions: expected exactly one subcommand".to_owned()); }
    if matches!(mode, "--help" | "-h" | "help") { return Err(String::new()); }
    if mode == "--" || mode.starts_with('-') { return Err("completions: subcommand must not start with '-' or be '--'".to_owned()); }
    Ok(CompletionsRequest::Mode(mode))
}

fn completions_render_request(request: CompletionsRequest<'_>) -> Result<String, String> {
    match request {
        CompletionsRequest::CommandsDescribed => Ok(completions_commands_described().join("\n")),
        CompletionsRequest::Subs(command) => Ok(completions_subs(command).join("\n")),
        CompletionsRequest::Mode(mode) => completions_render_mode(mode),
    }
}

fn completions_render_mode(mode: &str) -> Result<String, String> {
    match mode {
        // Newline-joined: zsh `${(f)...}` and fish command substitution split on newlines only.
        "commands" => Ok(completions_commands().join("\n")),
        "oracles" => Ok(completions_targets(CompletionsTargetKind::Oracles).join("\n")),
        "windows" => Ok(completions_targets(CompletionsTargetKind::Windows).join("\n")),
        "squads" => Ok(completions_squads().join("\n")),
        "fleet" => Ok(completions_subs("fleet").join("\n")),
        "oracle" => Ok(completions_subs("oracle").join("\n")),
        "pulse" => Ok("add ls list".to_owned()),
        "zsh" => Ok(COMPLETIONS_ZSH.to_owned()),
        "bash" => Ok(COMPLETIONS_BASH.to_owned()),
        "fish" => Ok(COMPLETIONS_FISH.to_owned()),
        _ => Err(format!("{COMPLETIONS_HELP}\nunknown completion mode: {mode}")),
    }
}

fn completions_commands() -> Vec<&'static str> {
    let mut commands = native_dispatch_commands()
        .into_iter()
        .filter(|command| completions_is_public_command(command))
        .collect::<Vec<_>>();
    commands.sort_unstable();
    commands.dedup();
    commands
}

fn completions_is_public_command(command: &str) -> bool {
    !command.is_empty() && !command.starts_with('-') && !command.starts_with("__")
}

// ---- usage-derived completion data (#309) ----
//
// One registry maps dispatcher commands to the usage strings the commands
// themselves print. Descriptions and second-position words are *derived* from
// those strings, so completion data cannot drift from the documented surface;
// commands without an entry simply fall back to name-only completion.

fn completions_usage_sources() -> Vec<(&'static str, String)> {
    vec![
        ("absorb", ABSORB_USAGE.to_owned()),
        ("activity", ACTIVITY_USAGE.to_owned()),
        ("art", ARTIFACTMGR_USAGE.to_owned()),
        ("audit", AUDIT_USAGE.to_owned()),
        ("awake", AWAKE_USAGE.to_owned()),
        ("codex", CODEX_USAGE_273.to_owned()),
        ("completions", COMPLETIONS_HELP.to_owned()),
        ("consent", consent_help_135()),
        ("done", DONE_USAGE.to_owned()),
        ("fleet", fleet_usage()),
        ("follow", FOLLOW_USAGE.to_owned()),
        ("inbox", INBOX_USAGE.to_owned()),
        ("join", JOIN_USAGE.to_owned()),
        ("kill", KILL_USAGE.to_owned()),
        ("layout", LAYOUT_USAGE.to_owned()),
        ("more", MORE_USAGE.to_owned()),
        ("new", NEW_USAGE.to_owned()),
        ("notify", NOTIFY_USAGE.to_owned()),
        ("oracle", ORACLE_USAGE.to_owned()),
        ("oracle-workon", ORACLEWORKON_USAGE.to_owned()),
        ("plugin", PLUGIN_USAGE.to_owned()),
        ("plugin-artifact", PLUGINARTIFACT_USAGE.to_owned()),
        ("plugin-policy", POLICY_USAGE.to_owned()),
        ("plugins", PLUGINS_USAGE.to_owned()),
        ("policy", POLICY_USAGE.to_owned()),
        ("preflight", PREFLIGHT_USAGE.to_owned()),
        ("promote", PROMOTE_USAGE.to_owned()),
        ("scaffold", SCAFFOLD_USAGE.to_owned()),
        ("setup", SETUP_USAGE.to_owned()),
        ("snapshots", SNAPSHOTS_USAGE.to_owned()),
        ("soul-sync", SOULSYNC_USAGE.to_owned()),
        ("split", SPLIT_USAGE.to_owned()),
        ("stop", STOP_USAGE.to_owned()),
        ("stream", STREAM_USAGE.to_owned()),
        ("t", TEAM_USAGE.to_owned()),
        ("talk-to", TALKTO_USAGE.to_owned()),
        ("team", TEAM_USAGE.to_owned()),
        ("tile", TILE_USAGE.to_owned()),
        ("user-setup", USERSETUP_USAGE.to_owned()),
        ("wave", WAVE_USAGE.to_owned()),
        ("zai", ZAI_USAGE.to_owned()),
    ]
}

fn completions_usage_for(command: &str) -> Option<String> {
    completions_usage_sources()
        .into_iter()
        .find_map(|(name, usage)| (name == command).then_some(usage))
}

/// `commands --describe` payload: `name<TAB>description` per command, where the
/// description is the command's usage synopsis line; name-only when unknown.
fn completions_commands_described() -> Vec<String> {
    completions_commands()
        .into_iter()
        .map(|command| {
            match completions_usage_for(command).map(|usage| completions_usage_description(&usage)) {
                Some(description) if !description.is_empty() => format!("{command}\t{description}"),
                _ => command.to_owned(),
            }
        })
        .collect()
}

/// First non-empty usage line with any `usage:` prefix dropped and inner
/// whitespace collapsed.
fn completions_usage_description(usage: &str) -> String {
    usage
        .lines()
        .map(|line| {
            line.trim_start_matches("usage:")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .find(|line| !line.is_empty())
        .unwrap_or_default()
}

/// Second-position completion words for `command`, derived from its usage
/// string: for every synopsis line (`maw <command> ...` / `maw-rs <command> ...`)
/// take the token group right after the command name — bare words become
/// candidates directly, `<a|b|...>` / `[a|b|...]` groups contribute the first
/// word of each alternative.
fn completions_subs(command: &str) -> Vec<String> {
    let Some(usage) = completions_usage_for(command) else { return Vec::new(); };
    let mut words = std::collections::BTreeSet::new();
    for line in usage.lines() {
        if let Some(remainder) = completions_synopsis_remainder(line) {
            completions_collect_synopsis_words(remainder, &mut words);
        }
    }
    words.into_iter().collect()
}

/// Text after the first word-boundary `maw <word> ` / `maw-rs <word> ` on the line.
fn completions_synopsis_remainder(line: &str) -> Option<&str> {
    for (index, _) in line.match_indices("maw") {
        let at_boundary = index == 0 || line[..index].chars().next_back().is_some_and(char::is_whitespace);
        if !at_boundary { continue; }
        let rest = &line[index + 3..];
        let rest = rest.strip_prefix("-rs").unwrap_or(rest);
        let Some(rest) = rest.strip_prefix(' ') else { continue; };
        let rest = rest.trim_start();
        return rest
            .find(char::is_whitespace)
            .map(|position| rest[position..].trim_start())
            .filter(|value| !value.is_empty());
    }
    None
}

fn completions_collect_synopsis_words(remainder: &str, words: &mut std::collections::BTreeSet<String>) {
    for alternative in completions_split_alternatives(remainder) {
        let alternative = completions_strip_binary_prefix(alternative.trim());
        let Some(chunk) = completions_first_chunk(alternative) else { continue; };
        if let Some(inner) = completions_bracket_inner(chunk) {
            let inner_alternatives = completions_split_alternatives(inner);
            if inner_alternatives.len() < 2 { continue; }
            for inner_alternative in inner_alternatives {
                if let Some(word) = completions_leading_word(inner_alternative) {
                    words.insert(word);
                }
            }
        } else if let Some(word) = completions_leading_word(alternative) {
            words.insert(word);
        }
    }
}

/// Split on `|` and standalone ` or ` at bracket depth 0.
fn completions_split_alternatives(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0_usize;
    for (index, ch) in text.char_indices() {
        match ch {
            '<' | '[' => depth += 1,
            '>' | ']' => depth = depth.saturating_sub(1),
            '|' if depth == 0 && index >= start => {
                pieces.push(&text[start..index]);
                start = index + 1;
            }
            ' ' if depth == 0 && index >= start && text[index..].starts_with(" or ") => {
                pieces.push(&text[start..index]);
                start = index + 4;
            }
            _ => {}
        }
    }
    pieces.push(&text[start..]);
    pieces
}

/// Drop a leading `maw <command> ` / `maw-rs <command> ` repeated inside an alternative.
fn completions_strip_binary_prefix(alternative: &str) -> &str {
    let trimmed = alternative.trim_start();
    let Some(rest) = trimmed.strip_prefix("maw-rs ").or_else(|| trimmed.strip_prefix("maw ")) else {
        return alternative;
    };
    let rest = rest.trim_start();
    rest.find(char::is_whitespace)
        .map_or("", |position| rest[position..].trim_start())
}

/// First bracket-balanced group (`<...>` / `[...]`) or whitespace token.
fn completions_first_chunk(alternative: &str) -> Option<&str> {
    let alternative = alternative.trim_start();
    if alternative.starts_with('<') || alternative.starts_with('[') {
        completions_balanced_group(alternative)
    } else {
        alternative.split_whitespace().next()
    }
}

fn completions_balanced_group(text: &str) -> Option<&str> {
    let mut depth = 0_usize;
    for (index, ch) in text.char_indices() {
        match ch {
            '<' | '[' => depth += 1,
            '>' | ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 { return Some(&text[..=index]); }
            }
            _ => {}
        }
    }
    None
}

fn completions_bracket_inner(chunk: &str) -> Option<&str> {
    (chunk.len() >= 2 && (chunk.starts_with('<') || chunk.starts_with('['))).then(|| &chunk[1..chunk.len() - 1])
}

/// First whitespace token when it is a plain word or `--flag`; placeholders and
/// punctuation yield nothing.
fn completions_leading_word(alternative: &str) -> Option<String> {
    let token = alternative.split_whitespace().next()?;
    let word = token.strip_prefix("--").unwrap_or(token);
    let valid = !word.is_empty()
        && !word.starts_with('-')
        && word.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    valid.then(|| token.to_owned())
}

// Squadron squad names from fleet roster files — reuses the #291 roster parsing
// (fleet_load_entries + fleet_roster_squad_name), no separate (de)serialization.
fn completions_squads() -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    for entry in fleet_load_entries() {
        if let Some(name) = fleet_roster_squad_name(&entry) {
            if completions_is_safe_target(&name) {
                names.insert(name);
            }
        }
    }
    names.into_iter().collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionsTargetKind { Oracles, Windows }

fn completions_targets(kind: CompletionsTargetKind) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    for session in load_native_fleet() {
        for window in session.windows {
            completions_insert_target(&mut names, kind, &window.name);
        }
    }
    names.into_iter().collect()
}

fn completions_insert_target(names: &mut std::collections::BTreeSet<String>, kind: CompletionsTargetKind, name: &str) {
    if !completions_is_safe_target(name) { return; }
    match kind {
        CompletionsTargetKind::Oracles if name.ends_with("-oracle") => {
            names.insert(name.trim_end_matches("-oracle").to_owned());
        }
        CompletionsTargetKind::Windows => {
            names.insert(name.to_owned());
        }
        CompletionsTargetKind::Oracles => {}
    }
}

fn completions_is_safe_target(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('-') && !value.chars().any(char::is_whitespace)
}

fn completions_ok(body: &str) -> CliOutput {
    CliOutput { code: 0, stdout: format!("{body}\n"), stderr: String::new() }
}

#[cfg(test)]
mod completions_tests {
    use super::{
        completions_commands, completions_commands_described, completions_is_safe_target,
        completions_run_command, completions_subs, completions_usage_sources, env_test_lock,
        fleet_default_options, fleet_roster_intercept, fleet_set_command, native_dispatch_commands,
        EnvVarRestore, DISPATCH_99, ORACLE_USAGE,
    };

    fn completions_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn completions_dispatch_registers_single_native_command() {
        assert_eq!(DISPATCH_99.len(), 1);
        assert_eq!(DISPATCH_99[0].command, "completions");
    }

    #[test]
    fn completions_bash_script_matches_dynamic_command_contract() {
        let output = completions_run_command(&completions_args(&["bash"]));
        assert_eq!(output.code, 0);
        assert!(output.stderr.is_empty());
        assert!(output.stdout.starts_with("# maw bash completion\n"));
        assert!(output.stdout.contains("maw completions commands 2>/dev/null"));
        assert!(output.stdout.contains("maw completions subs fleet 2>/dev/null"));
        assert!(output.stdout.contains("maw completions subs oracle 2>/dev/null"));
        assert!(output.stdout.contains("maw completions subs \"$cmd\" 2>/dev/null"));
        assert!(output.stdout.contains("maw completions squads 2>/dev/null"));
        assert!(output.stdout.contains("show|status|wake|sleep|token"));
        assert!(output.stdout.contains("complete -F _maw_complete maw\n"));
    }

    #[test]
    fn completions_shell_modes_cover_fish_and_zsh() {
        let fish = completions_run_command(&completions_args(&["fish"]));
        let zsh = completions_run_command(&completions_args(&["zsh"]));
        assert!(fish.stdout.contains("# maw fish completion"));
        assert!(fish.stdout.contains("__fish_use_subcommand"));
        assert!(fish.stdout.contains("maw completions commands --describe 2>/dev/null"));
        assert!(fish.stdout.contains("maw completions subs (commandline -opc)[2] 2>/dev/null"));
        assert!(fish.stdout.contains("maw completions squads 2>/dev/null"));
        assert!(fish.stdout.contains("-d 'Oracle (peek/send shorthand)'"));
        assert!(zsh.stdout.contains("#compdef maw"));
        assert!(zsh.stdout.contains("maw completions commands --describe 2>/dev/null"));
        assert!(zsh.stdout.contains("_maw_subs"));
        assert!(zsh.stdout.contains("'oracle shorthand'"));
        assert!(zsh.stdout.contains("'maw commands'"));
        assert!(zsh.stdout.contains("_maw_squads"));
    }

    #[test]
    fn completions_commands_list_uses_native_dispatch_registry() {
        let commands = completions_commands();
        assert!(commands.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(commands.contains(&"completions"));
        assert!(commands.contains(&"serve"));
        assert!(commands.contains(&"run"));
        assert!(commands.contains(&"fleet"), "fleet must stay in the dispatcher-derived list");
        assert!(commands.contains(&"oracle"), "oracle must stay in the dispatcher-derived list");
        assert!(!commands.iter().any(|command| command.starts_with('-') || command.starts_with("__")));
        let output = completions_run_command(&completions_args(&["commands"]));
        assert_eq!(output.stdout, format!("{}\n", commands.join("\n")));
    }

    #[test]
    fn completions_usage_registry_commands_stay_dispatchable() {
        let commands = native_dispatch_commands();
        for (name, usage) in completions_usage_sources() {
            assert!(commands.contains(&name), "usage registry names unknown command: {name}");
            assert!(!usage.is_empty(), "usage registry has empty usage for: {name}");
        }
    }

    #[test]
    fn completions_fleet_and_oracle_subcommands_stay_dispatchable() {
        let fleet_words = completions_subs("fleet");
        for probe in ["add", "create", "join", "token", "wake", "sleep", "consolidate"] {
            assert!(fleet_words.contains(&probe.to_owned()), "fleet usage lost {probe}");
        }
        for word in &fleet_words {
            let mut options = fleet_default_options();
            let mut seen = false;
            let parsed = fleet_set_command(&mut options, &mut seen, word).is_ok();
            let intercepted = fleet_roster_intercept(&completions_args(&[word])).is_some();
            // "token" is routed before fleet_parse_args (see run_fleet_command).
            assert!(parsed || intercepted || word == "token", "fleet completion {word} is not dispatchable");
        }
        let oracle_words = completions_subs("oracle");
        assert_eq!(
            oracle_words,
            ["about", "get-nickname", "ls", "prune", "recruit", "register", "scan", "search", "set-nickname"],
            "oracle words must track ORACLE_USAGE"
        );
        for word in &oracle_words {
            assert!(ORACLE_USAGE.contains(word.as_str()), "oracle completion {word} missing from ORACLE_USAGE");
        }
        let fleet = completions_run_command(&completions_args(&["fleet"]));
        assert_eq!(fleet.stdout, format!("{}\n", fleet_words.join("\n")));
        let oracle = completions_run_command(&completions_args(&["oracle"]));
        assert_eq!(oracle.stdout, format!("{}\n", oracle_words.join("\n")));
        let subs_fleet = completions_run_command(&completions_args(&["subs", "fleet"]));
        assert_eq!(subs_fleet.stdout, fleet.stdout, "subs fleet is the same data as the fleet mode");
    }

    #[test]
    fn completions_subs_derives_second_level_words_from_usage_strings() {
        assert_eq!(completions_subs("zai"), ["mon", "status", "test"]);
        assert_eq!(completions_subs("codex"), ["accounts"]);
        assert_eq!(completions_subs("policy"), ["--constants", "--default-active", "--weight", "constants"]);
        assert_eq!(completions_subs("consent"), ["approve", "list", "list-trust", "reject", "trust", "untrust"]);
        assert_eq!(completions_subs("art"), ["attach", "get", "init", "ls", "write"]);
        assert_eq!(completions_subs("more"), ["codex", "status"]);
        assert_eq!(completions_subs("user-setup"), ["projects"]);
        let team = completions_subs("team");
        for probe in ["create", "status", "spawn", "spawn-from", "resume", "shutdown", "tasks", "invite"] {
            assert!(team.contains(&probe.to_owned()), "team usage lost {probe}");
        }
        assert_eq!(completions_subs("t"), team, "t alias derives the same team words");
        let plugin = completions_subs("plugin");
        for probe in ["ls", "install", "build", "dev", "init"] {
            assert!(plugin.contains(&probe.to_owned()), "plugin usage lost {probe}");
        }
        let own = completions_subs("completions");
        for probe in ["commands", "subs", "fleet", "oracle", "squads", "zsh", "bash", "fish"] {
            assert!(own.contains(&probe.to_owned()), "completions usage lost {probe}");
        }
        // Placeholder-only synopses contribute nothing; uncovered commands are empty.
        assert!(completions_subs("kill").is_empty(), "kill has no literal subcommands");
        assert!(completions_subs("hey").is_empty(), "uncovered commands yield no subs");
        let output = completions_run_command(&completions_args(&["subs", "policy"]));
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert_eq!(output.stdout, "--constants\n--default-active\n--weight\nconstants\n");
    }

    #[test]
    fn completions_commands_describe_emits_tab_separated_usage_lines() {
        let described = completions_commands_described();
        let commands = completions_commands();
        assert_eq!(described.len(), commands.len());
        for (line, command) in described.iter().zip(&commands) {
            assert!(
                line == command || line.starts_with(&format!("{command}\t")),
                "describe line must be `name` or `name<TAB>description`: {line}"
            );
            assert!(line.matches('\t').count() <= 1, "single tab separator only: {line}");
        }
        assert!(described.contains(&"codex\tmaw codex accounts [--json] [--free] [--slots N]".to_owned()));
        let consent = described.iter().find(|line| line.starts_with("consent\t")).expect("consent is described");
        assert!(consent.contains("list pending requests"), "consent description from first non-empty usage line");
        let hey = described.iter().find(|line| *line == "hey").expect("uncovered commands stay name-only");
        assert_eq!(hey, "hey");
        let output = completions_run_command(&completions_args(&["commands", "--describe"]));
        assert_eq!(output.stdout, format!("{}\n", described.join("\n")));
    }

    #[test]
    fn completions_squads_lists_roster_squad_names_from_fleet_files() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let root = std::env::temp_dir().join(format!("maw-rs-completions-squads-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _restore = ["HOME", "MAW_HOME", "MAW_CONFIG_DIR", "MAW_STATE_DIR", "MAW_CACHE_DIR", "GHQ_ROOT"].map(EnvVarRestore::capture);
        std::env::remove_var("MAW_HOME");
        for (key, dir) in [("HOME", "home"), ("MAW_CONFIG_DIR", "config"), ("MAW_STATE_DIR", "state"), ("MAW_CACHE_DIR", "cache"), ("GHQ_ROOT", "ghq")] {
            std::env::set_var(key, root.join(dir));
        }
        let fleet_dir = root.join("state/fleet");
        std::fs::create_dir_all(&fleet_dir).expect("fleet dir");
        std::fs::write(fleet_dir.join("01-3e.json"), r#"{"name":"01-3e","squadName":"3e","windows":[],"members":[{"handle":"atlas"}]}"#).expect("roster");
        std::fs::write(fleet_dir.join("05-ccdc.json"), r#"{"name":"05-ccdc","windows":[],"members":[]}"#).expect("members-only roster");
        std::fs::write(fleet_dir.join("03-alpha.json"), r#"{"name":"03-alpha","windows":[]}"#).expect("legacy file");
        let output = completions_run_command(&completions_args(&["squads"]));
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert_eq!(output.stdout, "3e\nccdc\n", "squadName wins, members-only falls back to stem, legacy excluded");
    }

    #[test]
    fn completions_rejects_bad_shell_and_option_injection() {
        let bad = completions_run_command(&completions_args(&["powershell"]));
        assert_eq!(bad.code, 1);
        assert!(bad.stderr.contains("unknown completion mode: powershell"));
        let flag = completions_run_command(&completions_args(&["--", "bash"]));
        assert_eq!(flag.code, 1);
        assert!(flag.stderr.contains("expected exactly one subcommand"));
        let missing = completions_run_command(&completions_args(&["subs"]));
        assert_eq!(missing.code, 1);
        assert!(missing.stderr.contains("subs expects exactly one command name"));
        let injected = completions_run_command(&completions_args(&["subs", "--describe"]));
        assert_eq!(injected.code, 1);
        assert!(injected.stderr.contains("invalid command name"));
        let extra = completions_run_command(&completions_args(&["subs", "team", "x"]));
        assert_eq!(extra.code, 1);
        assert!(extra.stderr.contains("expected exactly one subcommand"));
        let described_typo = completions_run_command(&completions_args(&["commands", "--nope"]));
        assert_eq!(described_typo.code, 1);
        assert!(described_typo.stderr.contains("expected exactly one subcommand"));
        assert!(!completions_is_safe_target("-bad"));
        assert!(!completions_is_safe_target("bad target"));
    }
}
