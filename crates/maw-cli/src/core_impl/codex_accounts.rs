const DISPATCH_273: &[DispatcherEntry] = &[DispatcherEntry {
    command: "codex",
    handler: Handler::Sync(codex_run_command),
}];

const CODEX_USAGE_273: &str = "usage: maw codex accounts [--json] [--free] [--slots N]";
const CODEX_PANE_FORMAT_273: &str =
    "#{pane_id}|||#{session_name}|||#{window_name}|||#{pane_title}|||#{pane_pid}|||#{pane_tty}";

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexAccountsOptions273 {
    json: bool,
    free_only: bool,
    slots: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexProcess273 {
    pid: u32,
    codex_home: String,
    tmux_pane: Option<String>,
    tty: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexPane273 {
    pane: String,
    session: String,
    window: String,
    title: String,
    pid: Option<u32>,
    tty: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexAccountOwner273 {
    user: String,
    session: Option<String>,
    pane: Option<String>,
    pid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexAccountRow273 {
    slot: usize,
    home: String,
    owner: Option<CodexAccountOwner273>,
}

fn codex_run_command(argv: &[String]) -> CliOutput {
    if wants_help(argv, &[]) {
        return help_output(CODEX_USAGE_273);
    }
    match codex_accounts_parse(argv) {
        Ok(options) => codex_accounts_run(&options),
        Err((code, message)) => CliOutput {
            code,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn codex_accounts_run(options: &CodexAccountsOptions273) -> CliOutput {
    let root = codex_accounts_team_root();
    let panes = codex_accounts_tmux_panes();
    let processes = codex_accounts_processes();
    let rows = codex_accounts_rows(&root, options.slots, options.free_only, &processes, &panes);
    CliOutput {
        code: 0,
        stdout: if options.json {
            codex_accounts_json(&rows)
        } else {
            codex_accounts_table(&rows)
        },
        stderr: String::new(),
    }
}

fn codex_accounts_parse(argv: &[String]) -> Result<CodexAccountsOptions273, (i32, String)> {
    let Some((subcommand, rest)) = argv.split_first() else {
        return Err((2, CODEX_USAGE_273.to_owned()));
    };
    if subcommand != "accounts" {
        return Err((
            2,
            format!("codex: unknown subcommand '{subcommand}'\n  {CODEX_USAGE_273}"),
        ));
    }
    let mut options = CodexAccountsOptions273 {
        json: false,
        free_only: false,
        slots: 5,
    };
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--help" | "-h" => return Err((0, CODEX_USAGE_273.to_owned())),
            "--json" => options.json = true,
            "--free" => options.free_only = true,
            "--slots" => {
                index += 1;
                let value = rest
                    .get(index)
                    .ok_or_else(|| (2, "codex accounts: missing --slots value".to_owned()))?;
                options.slots = codex_accounts_parse_slots(value)?;
            }
            value if value.starts_with("--slots=") => {
                options.slots = codex_accounts_parse_slots(&value[8..])?;
            }
            "--" => {
                return Err((
                    2,
                    "codex accounts: -- separator is not supported".to_owned(),
                ))
            }
            value if value.starts_with('-') => {
                return Err((2, format!("codex accounts: unknown flag '{value}'")));
            }
            value => return Err((2, format!("codex accounts: unexpected argument '{value}'"))),
        }
        index += 1;
    }
    Ok(options)
}

fn codex_accounts_parse_slots(value: &str) -> Result<usize, (i32, String)> {
    value
        .parse::<usize>()
        .ok()
        .filter(|slots| *slots > 0)
        .ok_or_else(|| {
            (
                2,
                "codex accounts: --slots must be a positive integer".to_owned(),
            )
        })
}

fn codex_accounts_team_root() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from)
        .join(".codex-team")
}

fn codex_accounts_rows(
    root: &std::path::Path,
    slots: usize,
    free_only: bool,
    processes: &[CodexProcess273],
    panes: &[CodexPane273],
) -> Vec<CodexAccountRow273> {
    (1..=slots)
        .filter_map(|slot| {
            let home = root.join(slot.to_string());
            let owner = codex_accounts_owner_for_home(&home, processes, panes);
            if free_only && owner.is_some() {
                return None;
            }
            Some(CodexAccountRow273 {
                slot,
                home: home.display().to_string(),
                owner,
            })
        })
        .collect()
}

fn codex_accounts_owner_for_home(
    home: &std::path::Path,
    processes: &[CodexProcess273],
    panes: &[CodexPane273],
) -> Option<CodexAccountOwner273> {
    processes
        .iter()
        .filter(|process| codex_accounts_home_matches(&process.codex_home, home))
        .map(|process| {
            let pane = codex_accounts_match_pane(process, panes);
            (pane.is_none(), process.pid, process, pane)
        })
        .min_by_key(|(missing_pane, pid, _, _)| (*missing_pane, *pid))
        .map(|(_, _, process, pane)| codex_accounts_owner_from_process(process, pane))
}

fn codex_accounts_owner_from_process(
    process: &CodexProcess273,
    pane: Option<&CodexPane273>,
) -> CodexAccountOwner273 {
    let user = pane.map_or_else(
        || format!("pid:{}", process.pid),
        |pane| {
            if pane.window.trim().is_empty() {
                pane.title.clone()
            } else {
                pane.window.clone()
            }
        },
    );
    CodexAccountOwner273 {
        user,
        session: pane.map(|pane| pane.session.clone()),
        pane: pane
            .map(|pane| pane.pane.clone())
            .or_else(|| process.tmux_pane.clone()),
        pid: process.pid,
    }
}

fn codex_accounts_match_pane<'a>(
    process: &CodexProcess273,
    panes: &'a [CodexPane273],
) -> Option<&'a CodexPane273> {
    if let Some(tmux_pane) = process.tmux_pane.as_deref() {
        if let Some(pane) = panes.iter().find(|pane| pane.pane == tmux_pane) {
            return Some(pane);
        }
    }
    if let Some(tty) = process.tty.as_deref() {
        if let Some(pane) = panes.iter().find(|pane| {
            pane.tty
                .as_deref()
                .is_some_and(|pane_tty| codex_accounts_tty_matches(tty, pane_tty))
        }) {
            return Some(pane);
        }
    }
    panes.iter().find(|pane| pane.pid == Some(process.pid))
}

fn codex_accounts_home_matches(value: &str, home: &std::path::Path) -> bool {
    let expanded = codex_accounts_expand_home(value);
    expanded == home || codex_accounts_canonical(&expanded) == codex_accounts_canonical(home)
}

fn codex_accounts_expand_home(value: &str) -> std::path::PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        return std::env::var_os("HOME")
            .map_or_else(|| std::path::PathBuf::from("~"), std::path::PathBuf::from)
            .join(rest);
    }
    std::path::PathBuf::from(value)
}

fn codex_accounts_canonical(path: &std::path::Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn codex_accounts_tty_matches(process_tty: &str, pane_tty: &str) -> bool {
    let process = codex_accounts_normalize_tty(process_tty);
    let pane = codex_accounts_normalize_tty(pane_tty);
    !process.is_empty() && process == pane
}

fn codex_accounts_normalize_tty(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "?" || trimmed == "??" {
        return String::new();
    }
    trimmed
        .strip_prefix("/dev/")
        .unwrap_or(trimmed)
        .trim_start_matches("tty")
        .to_owned()
}

fn codex_accounts_table(rows: &[CodexAccountRow273]) -> String {
    let mut stdout = String::from("slot  status  user          session   pane\n");
    for row in rows {
        let status = if row.owner.is_some() { "busy" } else { "free" };
        let user = row.owner.as_ref().map_or("—", |owner| owner.user.as_str());
        let session = row
            .owner
            .as_ref()
            .and_then(|owner| owner.session.as_deref())
            .unwrap_or("—");
        let pane = row
            .owner
            .as_ref()
            .and_then(|owner| owner.pane.as_deref())
            .unwrap_or("—");
        let _ = writeln!(
            stdout,
            "{:<4}  {:<6}  {:<12}  {:<8}  {}",
            row.slot, status, user, session, pane
        );
    }
    stdout
}

fn codex_accounts_json(rows: &[CodexAccountRow273]) -> String {
    let items = rows
        .iter()
        .map(|row| {
            let status = if row.owner.is_some() { "busy" } else { "free" };
            let pid = row
                .owner
                .as_ref()
                .map_or_else(|| "null".to_owned(), |owner| owner.pid.to_string());
            format!(
                "{{\"slot\":{},\"status\":{},\"home\":{},\"user\":{},\"session\":{},\"pane\":{},\"pid\":{}}}",
                row.slot,
                json_string(status),
                json_string(&row.home),
                codex_accounts_json_option(row.owner.as_ref().map(|owner| owner.user.as_str())),
                codex_accounts_json_option(row.owner.as_ref().and_then(|owner| owner.session.as_deref())),
                codex_accounts_json_option(row.owner.as_ref().and_then(|owner| owner.pane.as_deref())),
                pid
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{{\"slots\":[{items}]}}\n")
}

fn codex_accounts_json_option(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), json_string)
}

fn codex_accounts_tmux_panes() -> Vec<CodexPane273> {
    let mut runner = maw_tmux::CommandTmuxRunner::new();
    let raw = maw_tmux::TmuxRunner::run(
        &mut runner,
        "list-panes",
        &[
            "-a".to_owned(),
            "-F".to_owned(),
            CODEX_PANE_FORMAT_273.to_owned(),
        ],
    );
    raw.map_or_else(|_| Vec::new(), |raw| codex_accounts_parse_tmux_panes(&raw))
}

fn codex_accounts_parse_tmux_panes(raw: &str) -> Vec<CodexPane273> {
    raw.lines()
        .filter_map(|line| {
            let parts = line.split("|||").collect::<Vec<_>>();
            if parts.len() != 6 || parts[0].trim().is_empty() {
                return None;
            }
            Some(CodexPane273 {
                pane: parts[0].to_owned(),
                session: parts[1].to_owned(),
                window: parts[2].to_owned(),
                title: parts[3].to_owned(),
                pid: parts[4].parse::<u32>().ok(),
                tty: (!parts[5].trim().is_empty()).then(|| parts[5].to_owned()),
            })
        })
        .collect()
}

fn codex_accounts_processes() -> Vec<CodexProcess273> {
    let mut processes = codex_accounts_processes_from_proc();
    processes.extend(codex_accounts_processes_from_ps());
    processes.sort_by_key(|process| process.pid);
    processes.dedup_by_key(|process| process.pid);
    processes
}

fn codex_accounts_processes_from_proc() -> Vec<CodexProcess273> {
    let proc_root = std::path::Path::new("/proc");
    let Ok(entries) = std::fs::read_dir(proc_root) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let pid = entry.file_name().to_string_lossy().parse::<u32>().ok()?;
            let env = std::fs::read(entry.path().join("environ")).ok()?;
            let codex_home = codex_accounts_env_bytes_value(&env, "CODEX_HOME")?;
            Some(CodexProcess273 {
                pid,
                codex_home,
                tmux_pane: codex_accounts_env_bytes_value(&env, "TMUX_PANE"),
                tty: std::fs::read_link(entry.path().join("fd/0"))
                    .ok()
                    .map(|path| path.display().to_string()),
            })
        })
        .collect()
}

fn codex_accounts_env_bytes_value(bytes: &[u8], key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    bytes.split(|byte| *byte == 0).find_map(|item| {
        let text = std::str::from_utf8(item).ok()?;
        text.strip_prefix(&prefix)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn codex_accounts_processes_from_ps() -> Vec<CodexProcess273> {
    let output = std::process::Command::new("ps").arg("auxeww").output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    codex_accounts_parse_ps(&raw)
}

fn codex_accounts_parse_ps(raw: &str) -> Vec<CodexProcess273> {
    raw.lines()
        .skip(1)
        .filter(|line| line.contains("CODEX_HOME="))
        .filter_map(codex_accounts_parse_ps_line)
        .collect()
}

fn codex_accounts_parse_ps_line(line: &str) -> Option<CodexProcess273> {
    let fields = line.split_whitespace().collect::<Vec<_>>();
    let pid = fields.get(1)?.parse::<u32>().ok()?;
    let codex_home = codex_accounts_env_text_value(line, "CODEX_HOME")?;
    Some(CodexProcess273 {
        pid,
        codex_home,
        tmux_pane: codex_accounts_env_text_value(line, "TMUX_PANE"),
        tty: fields.get(6).map(|value| (*value).to_owned()),
    })
}

fn codex_accounts_env_text_value(line: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let value = &rest[..end];
    (!value.is_empty()).then(|| value.to_owned())
}

#[cfg(test)]
mod codex_accounts_tests273 {
    use super::{
        codex_accounts_json, codex_accounts_parse, codex_accounts_parse_ps,
        codex_accounts_parse_tmux_panes, codex_accounts_rows, dispatcher_status, run_cli,
        CodexProcess273, DispatchKind,
    };

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn codex_accounts_dispatch_registers_codex_surface() {
        assert_eq!(dispatcher_status("codex"), DispatchKind::Native);
        let output = run_cli(&strings(&["codex", "bogus"]));
        assert_eq!(output.code, 2);
        assert!(output.stderr.contains("usage: maw codex accounts"));
    }

    #[test]
    fn codex_accounts_table_marks_busy_by_codex_home_and_tmux_pane() {
        let root = std::path::Path::new("/home/nat/.codex-team");
        let panes = codex_accounts_parse_tmux_panes(
            "%11|||81-kru32|||kru32-coder-1|||Coder 1|||4100|||/dev/ttys011\n",
        );
        let processes = vec![CodexProcess273 {
            pid: 4200,
            codex_home: "/home/nat/.codex-team/1".to_owned(),
            tmux_pane: Some("%11".to_owned()),
            tty: Some("s011".to_owned()),
        }];
        let rows = codex_accounts_rows(root, 2, false, &processes, &panes);
        assert_eq!(
            super::codex_accounts_table(&rows),
            "slot  status  user          session   pane\n1     busy    kru32-coder-1  81-kru32  %11\n2     free    —             —         —\n"
        );
    }

    #[test]
    fn codex_accounts_json_and_free_filter_are_machine_readable() {
        let root = std::path::Path::new("/home/nat/.codex-team");
        let processes = vec![CodexProcess273 {
            pid: 4200,
            codex_home: "/home/nat/.codex-team/1".to_owned(),
            tmux_pane: None,
            tty: None,
        }];
        let rows = codex_accounts_rows(root, 3, true, &processes, &[]);
        let json = codex_accounts_json(&rows);
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value["slots"].as_array().expect("array").len(), 2);
        assert_eq!(value["slots"][0]["slot"], 2);
        assert_eq!(value["slots"][0]["status"], "free");
        assert!(value["slots"][0]["pid"].is_null());
    }

    #[test]
    fn codex_accounts_ps_parser_reads_codex_home_tmux_pane_and_tty() {
        let raw = "USER PID %CPU %MEM VSZ RSS TT STAT STARTED TIME COMMAND\n\
nat 123 0.0 0.0 1 1 s042 S 7PM 0:00 codex CODEX_HOME=/Users/nat/.codex-team/3 TMUX_PANE=%207\n";
        let processes = codex_accounts_parse_ps(raw);
        assert_eq!(processes.len(), 1);
        assert_eq!(processes[0].pid, 123);
        assert_eq!(processes[0].codex_home, "/Users/nat/.codex-team/3");
        assert_eq!(processes[0].tmux_pane.as_deref(), Some("%207"));
        assert_eq!(processes[0].tty.as_deref(), Some("s042"));
    }

    #[test]
    fn codex_accounts_parse_rejects_bad_slots_and_positionals() {
        assert!(codex_accounts_parse(&strings(&["accounts", "--slots", "0"])).is_err());
        assert!(codex_accounts_parse(&strings(&["accounts", "extra"])).is_err());
        let parsed = codex_accounts_parse(&strings(&["accounts", "--json", "--free", "--slots=9"]))
            .expect("parse");
        assert!(parsed.json);
        assert!(parsed.free_only);
        assert_eq!(parsed.slots, 9);
    }
}
