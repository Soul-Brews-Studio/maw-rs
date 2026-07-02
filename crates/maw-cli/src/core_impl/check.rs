const DISPATCH_137: &[DispatcherEntry] = &[DispatcherEntry {
    command: "check",
    handler: Handler::Sync(run_check_command),
}];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckToolCategory133 {
    Required,
    Optional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CheckToolDefinition133 {
    name: &'static str,
    required: bool,
    category: CheckToolCategory133,
    install_url: &'static str,
    notes: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CheckToolStatus133 {
    definition: CheckToolDefinition133,
    present: bool,
    version: Option<String>,
}

const CHECK_TOOLS_133: &[CheckToolDefinition133] = &[
    CheckToolDefinition133 { name: "bun", required: true, category: CheckToolCategory133::Required, install_url: "https://bun.sh", notes: None },
    CheckToolDefinition133 { name: "gh", required: true, category: CheckToolCategory133::Required, install_url: "https://cli.github.com", notes: None },
    CheckToolDefinition133 { name: "ghq", required: true, category: CheckToolCategory133::Required, install_url: "https://github.com/x-motemen/ghq#install", notes: None },
    CheckToolDefinition133 { name: "git", required: true, category: CheckToolCategory133::Required, install_url: "https://git-scm.com/downloads", notes: None },
    CheckToolDefinition133 { name: "tmux", required: true, category: CheckToolCategory133::Required, install_url: "https://github.com/tmux/tmux/wiki/Installing", notes: None },
    CheckToolDefinition133 { name: "uv", required: false, category: CheckToolCategory133::Optional, install_url: "https://docs.astral.sh/uv/getting-started/installation/", notes: None },
    CheckToolDefinition133 { name: "uvx", required: false, category: CheckToolCategory133::Optional, install_url: "https://docs.astral.sh/uv/", notes: Some("provided by uv") },
];

fn run_check_command(argv: &[String]) -> CliOutput {
    let subcommand = argv.first().map_or("tools", String::as_str);
    let stdout = check_run(subcommand);
    CliOutput { code: 0, stdout, stderr: String::new() }
}

fn check_run(subcommand: &str) -> String {
    if subcommand != "tools" {
        return format!("unknown subcommand: {subcommand}\nusage: maw check [tools]\n");
    }

    let results = CHECK_TOOLS_133
        .iter()
        .copied()
        .map(check_tool_status)
        .collect::<Vec<_>>();
    check_render_tools(&results)
}

fn check_tool_status(definition: CheckToolDefinition133) -> CheckToolStatus133 {
    let (present, version) = check_probe_tool(definition.name);
    CheckToolStatus133 { definition, present, version }
}

fn check_probe_tool(name: &str) -> (bool, Option<String>) {
    if name == "uvx" {
        let which = check_run_tool_probe("which", &["uvx"]);
        if !which.present {
            return (false, None);
        }
        let uv = check_run_tool_probe("uv", &["--version"]);
        return (true, check_extract_version(&uv.output));
    }

    let flag = if name == "tmux" { "-V" } else { "--version" };
    let result = check_run_tool_probe(name, &[flag]);
    (result.present, check_extract_version(&result.output))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CheckToolProbe133 {
    present: bool,
    output: String,
}

fn check_run_tool_probe(program: &str, args: &[&str]) -> CheckToolProbe133 {
    let Ok(mut child) = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    else {
        return CheckToolProbe133 { present: false, output: String::new() };
    };

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                let output = child.wait_with_output().ok();
                let stdout = output
                    .as_ref()
                    .map(|out| String::from_utf8_lossy(&out.stdout).to_string())
                    .unwrap_or_default();
                let stderr = output
                    .as_ref()
                    .map(|out| String::from_utf8_lossy(&out.stderr).to_string())
                    .unwrap_or_default();
                return CheckToolProbe133 { present: true, output: format!("{stdout}{stderr}") };
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return CheckToolProbe133 { present: false, output: String::new() };
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return CheckToolProbe133 { present: false, output: String::new() };
            }
        }
    }
}

fn check_extract_version(output: &str) -> Option<String> {
    let bytes = output.as_bytes();
    let mut index = 0_usize;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'.' {
            continue;
        }
        index += 1;
        if index >= bytes.len() || !bytes[index].is_ascii_digit() {
            continue;
        }
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'.' {
            let patch_dot = index;
            index += 1;
            if index < bytes.len() && bytes[index].is_ascii_digit() {
                while index < bytes.len() && bytes[index].is_ascii_digit() {
                    index += 1;
                }
            } else {
                index = patch_dot;
            }
        }
        return Some(output[start..index].to_owned());
    }
    None
}

fn check_render_tools(results: &[CheckToolStatus133]) -> String {
    const GREEN: &str = "\x1b[32m";
    const RED: &str = "\x1b[31m";
    const DIM: &str = "\x1b[90m";
    const RESET: &str = "\x1b[0m";

    let mut out = "\nmaw check tools\n\n".to_owned();
    out.push_str("Required:\n");
    for tool in results.iter().filter(|tool| tool.definition.category == CheckToolCategory133::Required) {
        if tool.present {
            let version = tool.version.as_ref().map_or(String::new(), |version| format!("  {version}"));
            let _ = writeln!(out, "  {GREEN}✓{RESET} {:<8}{version}", tool.definition.name);
        } else {
            let _ = writeln!(out, "  {RED}✗{RESET} {:<8}  {DIM}not installed{RESET}", tool.definition.name);
        }
    }

    out.push_str("\nOptional (Python plugins):\n");
    for tool in results.iter().filter(|tool| tool.definition.category == CheckToolCategory133::Optional) {
        if tool.present {
            let version = tool.version.as_ref().map_or(String::new(), |version| format!("  {version}"));
            let notes = tool.definition.notes.map_or(String::new(), |notes| format!("  {DIM}({notes}){RESET}"));
            let _ = writeln!(out, "  {GREEN}✓{RESET} {:<8}{version}{notes}", tool.definition.name);
        } else {
            let _ = writeln!(out, "  {RED}✗{RESET} {:<8}  {DIM}not installed{RESET}", tool.definition.name);
        }
    }

    let missing = results.iter().filter(|tool| !tool.present).collect::<Vec<_>>();
    if !missing.is_empty() {
        out.push_str("\nMissing:\n");
        for tool in &missing {
            let _ = writeln!(out, "  {RED}✗{RESET} {:<16}  {}", tool.definition.name, tool.definition.install_url);
        }
    }

    let req_ok = results.iter().filter(|tool| tool.definition.required && tool.present).count();
    let opt_ok = results.iter().filter(|tool| !tool.definition.required && tool.present).count();
    let missing_summary = if missing.is_empty() { "0 missing".to_owned() } else { format!("{RED}{} missing{RESET}", missing.len()) };
    let _ = writeln!(out, "\n{req_ok} required ✓  ·  {opt_ok} optional ✓  ·  {missing_summary}");
    out.push('\n');
    out
}
