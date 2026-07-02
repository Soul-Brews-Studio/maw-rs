const DISPATCH_99: &[DispatcherEntry] = &[DispatcherEntry {
    command: "completions",
    handler: Handler::Sync(completions_run_command),
}];

const COMPLETIONS_HELP: &str = "usage: maw completions <commands|oracles|windows|zsh|bash|fish>\n\nGenerate maw shell completions or dynamic completion data.\n\nInstall examples:\n  zsh:  mkdir -p ~/.zsh/completions && maw completions zsh > ~/.zsh/completions/_maw\n        add to ~/.zshrc before compinit: fpath=(~/.zsh/completions $fpath)\n  bash: maw completions bash > ~/.maw-completion.bash\n        add to ~/.bashrc: source ~/.maw-completion.bash\n  fish: mkdir -p ~/.config/fish/completions && maw completions fish > ~/.config/fish/completions/maw.fish\n\nData subcommands:\n  commands   command names for first-position completion\n  oracles    oracle names from fleet configs\n  windows    tmux window/session names from fleet configs";

const COMPLETIONS_ZSH: &str = r#"#compdef maw

_maw_dynamic_words() {
  local out
  out=(${(f)"$(maw completions "$1" 2>/dev/null)"})
  print -l -- ${out[@]}
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

_maw() {
  local curcontext="$curcontext" state line
  typeset -A opt_args

  _arguments -C \
    '1:command:->cmd' \
    '*::arg:->args'

  case $state in
    cmd)
      local -a commands oracles all
      commands=(${(f)"$(maw completions commands 2>/dev/null)"})
      oracles=(${(f)"$(maw completions oracles 2>/dev/null)"})
      all=(${commands[@]})
      for o in ${oracles[@]}; do all+=("$o:Oracle (peek/send shorthand)"); done
      _describe 'command' all
      ;;
    args)
      case $line[1] in
        peek|see|a|attach|bring|b|hey|send|tell|done|finish)
          _maw_windows
          ;;
        wake|about|info)
          _maw_oracles
          ;;
        completions)
          _values 'completion mode' commands oracles windows zsh bash fish --help
          ;;
        plugin|plugins)
          _values 'plugin action' ls list enable disable info standard full lean nuke
          ;;
        team|t)
          _values 'team action' create new plan preflight load spawn-from spawn bring send msg shutdown down resume lives list ls status add tasks done assign delete rm invite oracle-invite oracle-remove members enter
          ;;
        serve)
          _message 'port (default: 3456)'
          ;;
        *)
          _message 'argument'
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
    completions)
      words="commands oracles windows zsh bash fish --help"
      ;;
    plugin|plugins)
      words="ls list enable disable info standard full lean nuke"
      ;;
    team|t)
      words="create new plan preflight load spawn-from spawn bring send msg shutdown down resume lives list ls status add tasks done assign delete rm invite oracle-invite oracle-remove members enter"
      ;;
    *)
      words=""
      ;;
  esac
  COMPREPLY=( $(compgen -W "$words" -- "$cur") )
}
complete -F _maw_complete maw"#;

const COMPLETIONS_FISH: &str = r"# maw fish completion
complete -c maw -f -n '__fish_use_subcommand' -a '(maw completions commands 2>/dev/null)'
complete -c maw -f -n '__fish_seen_subcommand_from wake about info' -a '(maw completions oracles 2>/dev/null)'
complete -c maw -f -n '__fish_seen_subcommand_from peek see a attach bring b hey send tell done finish' -a '(maw completions windows 2>/dev/null)'
complete -c maw -f -n '__fish_seen_subcommand_from completions' -a 'commands oracles windows zsh bash fish --help'
complete -c maw -f -n '__fish_seen_subcommand_from team t' -a 'create new plan preflight load spawn-from spawn bring send msg shutdown down resume lives list ls status add tasks done assign delete rm invite oracle-invite oracle-remove members enter'";

fn completions_run_command(argv: &[String]) -> CliOutput {
    match completions_parse_mode(argv).and_then(completions_render_mode) {
        Ok(stdout) => completions_ok(&stdout),
        Err(message) if message.is_empty() => completions_ok(COMPLETIONS_HELP),
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn completions_parse_mode(argv: &[String]) -> Result<&str, String> {
    let Some(mode) = argv.first().map(String::as_str) else { return Err(String::new()); };
    if argv.len() > 1 { return Err("completions: expected exactly one subcommand".to_owned()); }
    if matches!(mode, "--help" | "-h" | "help") { return Err(String::new()); }
    if mode == "--" || mode.starts_with('-') { return Err("completions: subcommand must not start with '-' or be '--'".to_owned()); }
    Ok(mode)
}

fn completions_render_mode(mode: &str) -> Result<String, String> {
    match mode {
        "commands" => Ok(completions_commands().join(" ")),
        "oracles" => Ok(completions_targets(CompletionsTargetKind::Oracles).join("\n")),
        "windows" => Ok(completions_targets(CompletionsTargetKind::Windows).join("\n")),
        "fleet" => Ok("init ls renumber validate sync".to_owned()),
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
    use super::{completions_commands, completions_is_safe_target, completions_run_command, DISPATCH_99};

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
        assert!(output.stdout.contains("complete -F _maw_complete maw\n"));
    }

    #[test]
    fn completions_shell_modes_cover_fish_and_zsh() {
        let fish = completions_run_command(&completions_args(&["fish"]));
        let zsh = completions_run_command(&completions_args(&["zsh"]));
        assert!(fish.stdout.contains("# maw fish completion"));
        assert!(fish.stdout.contains("__fish_use_subcommand"));
        assert!(zsh.stdout.contains("#compdef maw"));
        assert!(zsh.stdout.contains("_maw_oracles"));
    }

    #[test]
    fn completions_commands_list_uses_native_dispatch_registry() {
        let commands = completions_commands();
        assert!(commands.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(commands.contains(&"completions"));
        assert!(commands.contains(&"serve"));
        assert!(commands.contains(&"run"));
        assert!(!commands.iter().any(|command| command.starts_with('-') || command.starts_with("__")));
        let output = completions_run_command(&completions_args(&["commands"]));
        assert_eq!(output.stdout, format!("{}\n", commands.join(" ")));
    }

    #[test]
    fn completions_rejects_bad_shell_and_option_injection() {
        let bad = completions_run_command(&completions_args(&["powershell"]));
        assert_eq!(bad.code, 1);
        assert!(bad.stderr.contains("unknown completion mode: powershell"));
        let flag = completions_run_command(&completions_args(&["--", "bash"]));
        assert_eq!(flag.code, 1);
        assert!(flag.stderr.contains("expected exactly one subcommand"));
        assert!(!completions_is_safe_target("-bad"));
        assert!(!completions_is_safe_target("bad target"));
    }
}
