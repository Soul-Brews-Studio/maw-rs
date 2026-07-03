const DISPATCH_152: &[DispatcherEntry] = &[
    DispatcherEntry { command: "serve", handler: Handler::Async(run_serve_async) },
    DispatcherEntry { command: "messages", handler: Handler::Async(run_messages_async) },
];

const MESSAGES_ENGINE_PREFIX_152: &str = "/api/message-ledger";

#[derive(Debug, Clone, PartialEq, Eq)]
enum MessagesLifecycleAction152 {
    Serve { detach: bool, engine: String, port: u16 },
    Status { engine: String },
    Stop { engine: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ServeLifecycleAction152 {
    Status,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServeLifecycleStatus152 {
    pid: Option<u32>,
    file: std::path::PathBuf,
    port: u16,
    port_probe: ServePortProbe152,
    listener_pid: Option<u32>,
    pid_process_alive: bool,
    stale_pid: bool,
    stale_pid_removed: bool,
    summary: Option<String>,
}

impl ServeLifecycleStatus152 {
    fn alive(&self) -> bool {
        self.port_probe == ServePortProbe152::Responding || self.pid_process_alive
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ServePortProbe152 {
    Responding,
    NoListener,
    Failed(String),
}

trait MessagesLifecycleHost152 {
    fn messages_pid_path(&self) -> std::path::PathBuf;
    fn messages_log_path(&self) -> std::path::PathBuf;
    fn messages_db_path(&self) -> std::path::PathBuf;
    fn messages_read_pid(&self, path: &std::path::Path) -> Option<u32>;
    fn messages_pid_alive(&self, pid: u32) -> bool;
    fn messages_remove_pid(&mut self, path: &std::path::Path) -> Result<(), String>;
    fn messages_stop_pid(&mut self, pid: u32) -> Result<(), String>;
    fn messages_serve_status(&self) -> ServeLifecycleStatus152;
}

trait ServeLifecycleHost152 {
    fn serve_pid_path(&self) -> std::path::PathBuf;
    fn serve_read_pid(&self, path: &std::path::Path) -> Option<u32>;
    fn serve_pid_alive(&self, pid: u32) -> bool;
    fn serve_process_is_maw(&self, pid: u32) -> bool;
    fn serve_process_summary(&self, pid: u32) -> String;
    fn serve_configured_port(&self) -> u16;
    fn serve_probe_port(&self, port: u16) -> ServePortProbe152;
    fn serve_listener_pid(&self, port: u16) -> Option<u32>;
    fn serve_remove_pid(&mut self, path: &std::path::Path) -> Result<(), String>;
    fn serve_stop_pid(&mut self, pid: u32) -> Result<(), String>;
}

struct MessagesSystemHost152;
struct ServeSystemHost152;

impl MessagesLifecycleHost152 for MessagesSystemHost152 {
    fn messages_pid_path(&self) -> std::path::PathBuf { messages_pid_path152() }
    fn messages_log_path(&self) -> std::path::PathBuf { messages_log_path152() }
    fn messages_db_path(&self) -> std::path::PathBuf { messages_db_path152() }
    fn messages_read_pid(&self, path: &std::path::Path) -> Option<u32> { messages_read_pid_file152(path) }
    fn messages_pid_alive(&self, pid: u32) -> bool { messages_pid_alive152(pid) }
    fn messages_remove_pid(&mut self, path: &std::path::Path) -> Result<(), String> { messages_remove_file152(path) }
    fn messages_stop_pid(&mut self, pid: u32) -> Result<(), String> { messages_signal_term152(pid) }
    fn messages_serve_status(&self) -> ServeLifecycleStatus152 { serve_status_with_host152(&ServeSystemHost152) }
}

impl ServeLifecycleHost152 for ServeSystemHost152 {
    fn serve_pid_path(&self) -> std::path::PathBuf { serve_pid_path152() }
    fn serve_read_pid(&self, path: &std::path::Path) -> Option<u32> { messages_read_pid_file152(path) }
    fn serve_pid_alive(&self, pid: u32) -> bool { messages_pid_alive152(pid) }
    fn serve_process_is_maw(&self, pid: u32) -> bool { serve_process_is_maw152(pid) }
    fn serve_process_summary(&self, pid: u32) -> String { serve_process_summary152(pid) }
    fn serve_configured_port(&self) -> u16 { serve_configured_port152() }
    fn serve_probe_port(&self, port: u16) -> ServePortProbe152 { serve_probe_port152(port) }
    fn serve_listener_pid(&self, port: u16) -> Option<u32> { serve_listener_pid152(port) }
    fn serve_remove_pid(&mut self, path: &std::path::Path) -> Result<(), String> { messages_remove_file152(path) }
    fn serve_stop_pid(&mut self, pid: u32) -> Result<(), String> { messages_signal_term152(pid) }
}

fn serve_lifecycle_subcommand152(raw_args: &[String]) -> Option<CliOutput> {
    let first = raw_args.first()?.as_str();
    if first != "status" && first != "--status" && first != "stop" { return None; }
    Some(serve_lifecycle_run152(raw_args, &mut ServeSystemHost152))
}

fn messages_lifecycle_subcommand152(raw_args: &[String]) -> Option<CliOutput> {
    let first = raw_args.first()?.as_str();
    if first != "serve" && first != "status" && first != "stop" { return None; }
    Some(messages_lifecycle_run152(raw_args, &mut MessagesSystemHost152))
}

fn messages_lifecycle_run152(raw_args: &[String], host: &mut impl MessagesLifecycleHost152) -> CliOutput {
    match messages_parse_lifecycle152(raw_args) {
        Ok(MessagesLifecycleAction152::Serve { detach, engine, port }) => messages_serve152(detach, &engine, port, host),
        Ok(MessagesLifecycleAction152::Status { engine }) => messages_status152(&engine, host),
        Ok(MessagesLifecycleAction152::Stop { engine }) => messages_stop152(&engine, host),
        Err(message) => messages_lifecycle_error152(2, &message),
    }
}

fn serve_lifecycle_run152(raw_args: &[String], host: &mut impl ServeLifecycleHost152) -> CliOutput {
    match serve_parse_lifecycle152(raw_args) {
        Ok(ServeLifecycleAction152::Status) => serve_status152(host),
        Ok(ServeLifecycleAction152::Stop) => serve_stop152(host),
        Err(message) => messages_lifecycle_error152(2, &message),
    }
}

fn messages_parse_lifecycle152(raw_args: &[String]) -> Result<MessagesLifecycleAction152, String> {
    let Some(subcommand) = raw_args.first().map(String::as_str) else { return Err(messages_usage152()); };
    match subcommand {
        "serve" => {
            let mut detach = false;
            let mut engine = messages_default_engine_url152();
            let mut port = messages_default_port152();
            let mut index = 1;
            while index < raw_args.len() {
                match raw_args[index].as_str() {
                    "--detach" => detach = true,
                    "--engine" => {
                        let value = raw_args.get(index + 1).ok_or_else(|| "messages serve: missing --engine value".to_owned())?;
                        messages_validate_engine_url152(value)?;
                        engine = messages_trim_url152(value);
                        index += 1;
                    }
                    "--port" => {
                        let value = raw_args.get(index + 1).ok_or_else(|| "messages serve: missing --port value".to_owned())?;
                        port = messages_parse_port152(value)?;
                        index += 1;
                    }
                    "--help" | "-h" => return Err(messages_usage152()),
                    value if value.starts_with('-') => return Err(format!("messages serve: unknown argument {value}")),
                    value => return Err(format!("messages serve: unexpected argument {value}")),
                }
                index += 1;
            }
            Ok(MessagesLifecycleAction152::Serve { detach, engine, port })
        }
        "status" => {
            let engine = messages_parse_engine_only152(raw_args, "messages status")?;
            Ok(MessagesLifecycleAction152::Status { engine })
        }
        "stop" => {
            let engine = messages_parse_engine_only152(raw_args, "messages stop")?;
            Ok(MessagesLifecycleAction152::Stop { engine })
        }
        _ => Err(messages_usage152()),
    }
}

fn messages_parse_engine_only152(raw_args: &[String], label: &str) -> Result<String, String> {
    let mut engine = messages_default_engine_url152();
    let mut index = 1;
    while index < raw_args.len() {
        match raw_args[index].as_str() {
            "--engine" => {
                let value = raw_args.get(index + 1).ok_or_else(|| format!("{label}: missing --engine value"))?;
                messages_validate_engine_url152(value)?;
                engine = messages_trim_url152(value);
                index += 1;
            }
            "--help" | "-h" => return Err(messages_usage152()),
            value if value.starts_with('-') => return Err(format!("{label}: unknown argument {value}")),
            value => return Err(format!("{label}: unexpected argument {value}")),
        }
        index += 1;
    }
    Ok(engine)
}

fn serve_parse_lifecycle152(raw_args: &[String]) -> Result<ServeLifecycleAction152, String> {
    let Some(first) = raw_args.first().map(String::as_str) else { return Err(serve_usage152()); };
    let action = match first {
        "status" | "--status" => ServeLifecycleAction152::Status,
        "stop" => ServeLifecycleAction152::Stop,
        _ => return Err(serve_usage152()),
    };
    if raw_args.len() > 1 { return Err(format!("serve {first}: unexpected argument {}", raw_args[1])); }
    Ok(action)
}

fn messages_serve152(detach: bool, engine: &str, port: u16, host: &impl MessagesLifecycleHost152) -> CliOutput {
    let serve = host.messages_serve_status();
    let mut out = String::new();
    if detach {
        out.push_str("maw messages serve detached: native cutover uses maw serve daemon\n");
    } else {
        out.push_str("maw messages serve: native cutover uses maw serve daemon\n");
    }
    let _ = writeln!(out, "registered: {MESSAGES_ENGINE_PREFIX_152} on {engine}");
    let _ = writeln!(out, "upstream: built-in maw serve /api/message-ledger (requested port {port})");
    let _ = writeln!(
        out,
        "serve: {}",
        if serve.alive() { "running" } else { "stopped" }
    );
    let _ = writeln!(out, "db: {}", host.messages_db_path().display());
    let _ = writeln!(out, "log: {}", host.messages_log_path().display());
    CliOutput { code: 0, stdout: out, stderr: String::new() }
}

fn messages_status152(engine: &str, host: &impl MessagesLifecycleHost152) -> CliOutput {
    let pid_path = host.messages_pid_path();
    let pid = host.messages_read_pid(&pid_path);
    let alive = pid.is_some_and(|pid| host.messages_pid_alive(pid));
    let serve = host.messages_serve_status();
    let registered = if serve.alive() { format!("{MESSAGES_ENGINE_PREFIX_152} → built-in maw serve") } else { "no".to_owned() };
    let mut out = String::new();
    let _ = writeln!(out, "maw messages serve: {}{}", if alive || serve.alive() { "running" } else { "stopped" }, pid.map_or_else(String::new, |pid| format!(" (PID {pid})")));
    let _ = writeln!(out, "engine: {engine}");
    let _ = writeln!(out, "registered: {registered}");
    let _ = writeln!(out, "db: {}", host.messages_db_path().display());
    let _ = writeln!(out, "log: {}", host.messages_log_path().display());
    if !alive && pid.is_some() && pid_path.exists() { out.push_str("note: stale pid file present\n"); }
    CliOutput { code: 0, stdout: out, stderr: String::new() }
}

fn messages_stop152(engine: &str, host: &mut impl MessagesLifecycleHost152) -> CliOutput {
    messages_validate_engine_url152(engine).expect("already validated engine");
    let pid_path = host.messages_pid_path();
    let pid = host.messages_read_pid(&pid_path);
    let mut lines = Vec::<String>::new();
    if pid.is_some_and(|pid| host.messages_pid_alive(pid)) {
        let pid = pid.expect("checked some");
        if let Err(error) = host.messages_stop_pid(pid) { return messages_lifecycle_error152(1, &format!("messages stop: {error}")); }
        lines.push(format!("sent SIGTERM to PID {pid}"));
        if let Err(error) = host.messages_remove_pid(&pid_path) { return messages_lifecycle_error152(1, &format!("messages stop: {error}")); }
        lines.push(format!("stopped PID {pid}"));
    } else {
        lines.push("maw messages serve already stopped".to_owned());
        if pid.is_some() {
            if let Err(error) = host.messages_remove_pid(&pid_path) { return messages_lifecycle_error152(1, &format!("messages stop: {error}")); }
            lines.push("removed stale pid file".to_owned());
        }
    }
    if host.messages_serve_status().alive() { lines.push(format!("native route remains served by maw serve at {MESSAGES_ENGINE_PREFIX_152}")); }
    CliOutput { code: 0, stdout: format!("{}\n", lines.join("\n")), stderr: String::new() }
}

fn serve_status152(host: &mut impl ServeLifecycleHost152) -> CliOutput {
    let mut status = serve_status_with_host152(host);
    if status.stale_pid {
        if let Err(error) = host.serve_remove_pid(&status.file) {
            return messages_lifecycle_error152(1, &format!("serve status: {error}"));
        }
        status.stale_pid_removed = true;
    }
    let stdout = serve_render_status152(&status);
    CliOutput { code: 0, stdout, stderr: String::new() }
}

fn serve_stop152(host: &mut impl ServeLifecycleHost152) -> CliOutput {
    let status = serve_status_with_host152(host);
    let pid = if status.alive() { status.listener_pid.or(status.pid.filter(|_| status.pid_process_alive)) } else { status.pid };
    let Some(pid) = pid else { return CliOutput { code: 0, stdout: "maw serve: already stopped\n".to_owned(), stderr: String::new() }; };
    if !status.alive() {
        if status.stale_pid {
            if let Err(error) = host.serve_remove_pid(&status.file) { return messages_lifecycle_error152(1, &format!("serve stop: {error}")); }
        }
        return CliOutput { code: 0, stdout: format!("maw serve: removed stale PID {pid}\n"), stderr: String::new() };
    }
    if let Err(error) = host.serve_stop_pid(pid) { return messages_lifecycle_error152(1, &format!("serve stop: {error}")); }
    if status.pid.is_some() {
        if let Err(error) = host.serve_remove_pid(&status.file) { return messages_lifecycle_error152(1, &format!("serve stop: {error}")); }
    }
    CliOutput { code: 0, stdout: format!("maw serve: stopped PID {pid}\n"), stderr: String::new() }
}

fn serve_status_with_host152(host: &impl ServeLifecycleHost152) -> ServeLifecycleStatus152 {
    let file = host.serve_pid_path();
    let pid = host.serve_read_pid(&file);
    let port = host.serve_configured_port();
    let port_probe = host.serve_probe_port(port);
    let port_responding = port_probe == ServePortProbe152::Responding;
    let listener_pid = port_responding.then(|| host.serve_listener_pid(port)).flatten();
    let pid_process_alive = pid.is_some_and(|pid| host.serve_pid_alive(pid) && host.serve_process_is_maw(pid));
    let summary = pid.filter(|_| pid_process_alive).map(|pid| host.serve_process_summary(pid));
    let stale_pid = match (pid, listener_pid) {
        (Some(pid), Some(listener_pid)) if pid != listener_pid => true,
        (Some(_), _) if !pid_process_alive => true,
        _ => false,
    };
    ServeLifecycleStatus152 {
        pid,
        file,
        port,
        port_probe,
        listener_pid,
        pid_process_alive,
        stale_pid,
        stale_pid_removed: false,
        summary,
    }
}

fn serve_render_status152(status: &ServeLifecycleStatus152) -> String {
    let stale_note = if status.stale_pid_removed { Some("stale pidfile removed") } else { None };
    if status.alive() {
        let mut evidence = Vec::new();
        if let Some(pid) = serve_status_evidence_pid152(status) {
            evidence.push(format!("pid {pid}"));
        }
        if status.port_probe == ServePortProbe152::Responding {
            evidence.push(format!(":{} responding", status.port));
        } else if status.pid_process_alive {
            evidence.push(format!(
                "maw process alive{}",
                status.summary.as_deref().unwrap_or_default()
            ));
        }
        if let Some(note) = stale_note {
            evidence.push(note.to_owned());
        }
        return format!("maw serve: running ({})\n", evidence.join(", "));
    }
    let mut evidence = vec![serve_probe_stopped_evidence152(&status.port_probe, status.port)];
    if let Some(note) = stale_note {
        evidence.push(note.to_owned());
    }
    format!("maw serve: stopped ({})\n", evidence.join("; "))
}

fn serve_status_evidence_pid152(status: &ServeLifecycleStatus152) -> Option<u32> {
    status.listener_pid.or_else(|| status.pid.filter(|_| status.pid_process_alive))
}

fn serve_probe_stopped_evidence152(probe: &ServePortProbe152, port: u16) -> String {
    match probe {
        ServePortProbe152::Responding => format!(":{port} responding"),
        ServePortProbe152::NoListener => format!("no listener on :{port}"),
        ServePortProbe152::Failed(reason) => format!("health probe failed on :{port}: {reason}"),
    }
}

fn messages_default_engine_url152() -> String {
    std::env::var("MAW_ENGINE_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{}", std::env::var("MAW_PORT").unwrap_or_else(|_| "3456".to_owned()))).trim_end_matches('/').to_owned()
}

fn messages_default_port152() -> u16 {
    std::env::var("MAW_MESSAGES_PORT").ok().and_then(|value| value.parse::<u16>().ok()).unwrap_or(0)
}

fn messages_trim_url152(value: &str) -> String { value.trim_end_matches('/').to_owned() }

fn messages_parse_port152(value: &str) -> Result<u16, String> {
    messages_validate_token152(value, "--port")?;
    value.parse::<u16>().map_err(|_| format!("invalid --port: {value}"))
}

fn messages_validate_engine_url152(value: &str) -> Result<(), String> {
    messages_validate_token152(value, "--engine")?;
    if !(value.starts_with("http://") || value.starts_with("https://")) { return Err(format!("messages: invalid --engine: {value}")); }
    Ok(())
}

fn messages_validate_token152(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value.chars().any(|ch| ch == '\0' || ch.is_control()) { return Err(format!("messages: rejected {label} value")); }
    Ok(())
}

fn messages_usage152() -> String {
    "usage: maw-rs messages [serve [--detach] [--engine URL] [--port N] | status [--engine URL] | stop [--engine URL] | --limit N --from ID --to ID --direction outbound|inbound|forwarded --state queued|delivered|failed --q text --json]".to_owned()
}

fn serve_usage152() -> String { "usage: maw-rs serve [--host 0.0.0.0] [--port <port>] [--cached-pubkey <key>] | maw-rs serve status|--status|stop".to_owned() }

fn messages_lifecycle_error152(code: i32, message: &str) -> CliOutput { CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") } }

fn messages_pid_path152() -> std::path::PathBuf { maw_state_path(&current_xdg_env(), &["engine-plugins", "messages.pid"]) }
fn messages_log_path152() -> std::path::PathBuf { maw_state_path(&current_xdg_env(), &["engine-plugins", "messages.log"]) }
fn messages_db_path152() -> std::path::PathBuf { maw_data_path(&current_xdg_env(), &["message-ledger.sqlite"]) }
fn serve_pid_path152() -> std::path::PathBuf { maw_runtime_home_dir(&current_xdg_env()).join("maw.pid") }

fn serve_configured_port152() -> u16 {
    std::env::var("MAW_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port > 0)
        .or_else(|| {
            let value = merged_config_value_for_env(&current_xdg_env());
            value
                .get("port")
                .and_then(|port| {
                    port.as_u64()
                        .and_then(|number| u16::try_from(number).ok())
                        .or_else(|| port.as_str()?.parse::<u16>().ok())
                })
                .filter(|port| *port > 0)
        })
        .unwrap_or(DEFAULT_SERVE_PORT)
}

fn messages_read_pid_file152(path: &std::path::Path) -> Option<u32> {
    let raw = std::fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') || trimmed.chars().any(|ch| ch == '\0' || ch.is_control()) { return None; }
    trimmed.parse::<u32>().ok().filter(|pid| *pid > 0)
}

fn messages_remove_file152(path: &std::path::Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove {} failed: {error}", path.display())),
    }
}

fn messages_pid_alive152(pid: u32) -> bool {
    let output = std::process::Command::new("kill").arg("-0").arg(pid.to_string()).output();
    match output {
        Ok(output) => output.status.success(),
        Err(_) => std::path::Path::new("/proc").join(pid.to_string()).exists(),
    }
}

fn messages_signal_term152(pid: u32) -> Result<(), String> {
    let output = std::process::Command::new("kill")
        .args(["-TERM", "--", &pid.to_string()])
        .output()
        .map_err(|error| format!("SIGTERM {pid} failed: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(format!("SIGTERM {pid} failed{}", if stderr.is_empty() { String::new() } else { format!(": {stderr}") }))
    }
}

fn serve_process_summary152(pid: u32) -> String {
    let output = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid=,etime=,command="])
        .output();
    let Ok(output) = output else { return String::new(); };
    if !output.status.success() { return String::new(); }
    let raw = String::from_utf8_lossy(&output.stdout);
    let Some(line) = raw.lines().last().map(str::trim).filter(|line| !line.is_empty()) else { return String::new(); };
    let parts = line.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 3 { return String::new(); }
    let elapsed = parts[1];
    let command = parts[2..].join(" ");
    format!(", uptime {elapsed}, cmd: {}", messages_truncate152(&command, 80))
}

fn serve_process_is_maw152(pid: u32) -> bool {
    let Some(command) = serve_process_command152(pid) else { return false; };
    serve_command_is_maw152(&command)
}

fn serve_process_command152(pid: u32) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

fn serve_command_is_maw152(command: &str) -> bool {
    command
        .split_whitespace()
        .next()
        .and_then(|program| std::path::Path::new(program).file_name())
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "maw" || name == "maw-rs" || name.starts_with("maw-rs-"))
}

fn serve_probe_port152(port: u16) -> ServePortProbe152 {
    use std::io::{Read as _, Write as _};

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let timeout = std::time::Duration::from_millis(250);
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(&addr, timeout) else {
        return ServePortProbe152::NoListener;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    if let Err(error) = stream.write_all(b"GET /api/health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n") {
        return ServePortProbe152::Failed(error.to_string());
    }
    let mut response = String::new();
    if let Err(error) = stream.read_to_string(&mut response) {
        return ServePortProbe152::Failed(error.to_string());
    }
    if response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200") {
        ServePortProbe152::Responding
    } else {
        ServePortProbe152::Failed("unexpected /api/health response".to_owned())
    }
}

fn serve_listener_pid152(port: u16) -> Option<u32> {
    let output = std::process::Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-t"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find_map(|line| line.parse::<u32>().ok().filter(|pid| *pid > 0))
}

fn messages_truncate152(value: &str, max: usize) -> String {
    if value.chars().count() <= max { return value.to_owned(); }
    let mut out = value.chars().take(max.saturating_sub(1)).collect::<String>();
    out.push('…');
    out
}

#[cfg(test)]
mod messages_serve_lifecycle_tests152 {
    use super::*;

    #[derive(Default)]
    struct FakeHost152 {
        messages_pid: Option<u32>,
        messages_alive: bool,
        serve_pid: Option<u32>,
        serve_alive: bool,
        serve_port_probe: Option<ServePortProbe152>,
        serve_listener_pid: Option<u32>,
        serve_port: u16,
        removed: Vec<String>,
        stopped: Vec<u32>,
    }

    impl MessagesLifecycleHost152 for FakeHost152 {
        fn messages_pid_path(&self) -> std::path::PathBuf { std::path::PathBuf::from("/tmp/maw/engine-plugins/messages.pid") }
        fn messages_log_path(&self) -> std::path::PathBuf { std::path::PathBuf::from("/tmp/maw/engine-plugins/messages.log") }
        fn messages_db_path(&self) -> std::path::PathBuf { std::path::PathBuf::from("/tmp/maw/message-ledger.sqlite") }
        fn messages_read_pid(&self, _path: &std::path::Path) -> Option<u32> { self.messages_pid }
        fn messages_pid_alive(&self, pid: u32) -> bool { self.messages_alive && Some(pid) == self.messages_pid }
        fn messages_remove_pid(&mut self, path: &std::path::Path) -> Result<(), String> { self.removed.push(path.display().to_string()); Ok(()) }
        fn messages_stop_pid(&mut self, pid: u32) -> Result<(), String> { self.stopped.push(pid); Ok(()) }
        fn messages_serve_status(&self) -> ServeLifecycleStatus152 { serve_status_with_host152(self) }
    }

    impl ServeLifecycleHost152 for FakeHost152 {
        fn serve_pid_path(&self) -> std::path::PathBuf { std::path::PathBuf::from("/tmp/maw/maw.pid") }
        fn serve_read_pid(&self, _path: &std::path::Path) -> Option<u32> { self.serve_pid }
        fn serve_pid_alive(&self, pid: u32) -> bool { self.serve_alive && Some(pid) == self.serve_pid }
        fn serve_process_is_maw(&self, pid: u32) -> bool { self.serve_alive && Some(pid) == self.serve_pid }
        fn serve_process_summary(&self, _pid: u32) -> String { ", uptime 00:01, cmd: maw serve".to_owned() }
        fn serve_configured_port(&self) -> u16 {
            if self.serve_port == 0 { DEFAULT_SERVE_PORT } else { self.serve_port }
        }
        fn serve_probe_port(&self, _port: u16) -> ServePortProbe152 {
            self.serve_port_probe.clone().unwrap_or(ServePortProbe152::NoListener)
        }
        fn serve_listener_pid(&self, _port: u16) -> Option<u32> { self.serve_listener_pid }
        fn serve_remove_pid(&mut self, path: &std::path::Path) -> Result<(), String> { self.removed.push(path.display().to_string()); Ok(()) }
        fn serve_stop_pid(&mut self, pid: u32) -> Result<(), String> { self.stopped.push(pid); Ok(()) }
    }

    fn args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn messages_status_uses_existing_serve_state() {
        let mut host = FakeHost152 { serve_pid: Some(777), serve_alive: true, ..FakeHost152::default() };
        let out = messages_lifecycle_run152(&args(&["status", "--engine", "http://127.0.0.1:3456"]), &mut host);
        assert_eq!(out.code, 0);
        assert!(out.stdout.contains("maw messages serve: running"));
        assert!(out.stdout.contains("registered: /api/message-ledger → built-in maw serve"));
        assert!(out.stderr.is_empty());
    }

    #[test]
    fn messages_stop_is_idempotent_and_cleans_stale_pid() {
        let mut host = FakeHost152 { messages_pid: Some(123), messages_alive: false, ..FakeHost152::default() };
        let out = messages_lifecycle_run152(&args(&["stop"]), &mut host);
        assert_eq!(out.code, 0);
        assert!(out.stdout.contains("maw messages serve already stopped"));
        assert!(out.stdout.contains("removed stale pid file"));
        assert!(host.stopped.is_empty());
        assert_eq!(host.removed.len(), 1);
    }

    #[test]
    fn serve_status_and_stop_follow_pid_file_state() {
        let mut host = FakeHost152 {
            serve_pid: Some(999),
            serve_alive: true,
            serve_port_probe: Some(ServePortProbe152::Responding),
            serve_listener_pid: Some(999),
            ..FakeHost152::default()
        };
        let status = serve_lifecycle_run152(&args(&["status"]), &mut host);
        assert_eq!(status.stdout, "maw serve: running (pid 999, :3456 responding)\n");
        let stop = serve_lifecycle_run152(&args(&["stop"]), &mut host);
        assert_eq!(stop.stdout, "maw serve: stopped PID 999\n");
        assert_eq!(host.stopped, vec![999]);
        assert_eq!(host.removed, vec!["/tmp/maw/maw.pid"]);
    }

    #[test]
    fn serve_status_reports_responding_port_without_pidfile() {
        let mut host = FakeHost152 {
            serve_port_probe: Some(ServePortProbe152::Responding),
            serve_listener_pid: Some(31_999),
            ..FakeHost152::default()
        };

        let status = serve_lifecycle_run152(&args(&["status"]), &mut host);

        assert_eq!(status.stdout, "maw serve: running (pid 31999, :3456 responding)\n");
        assert!(host.removed.is_empty());
    }

    #[test]
    fn serve_status_removes_stale_pid_when_no_listener() {
        let mut host = FakeHost152 {
            serve_pid: Some(123),
            serve_alive: false,
            serve_port_probe: Some(ServePortProbe152::NoListener),
            ..FakeHost152::default()
        };

        let status = serve_lifecycle_run152(&args(&["status"]), &mut host);

        assert_eq!(
            status.stdout,
            "maw serve: stopped (no listener on :3456; stale pidfile removed)\n"
        );
        assert_eq!(host.removed, vec!["/tmp/maw/maw.pid"]);
    }

    #[test]
    fn serve_status_removes_stale_pid_while_reporting_live_listener() {
        let mut host = FakeHost152 {
            serve_pid: Some(123),
            serve_alive: false,
            serve_port_probe: Some(ServePortProbe152::Responding),
            serve_listener_pid: Some(31_999),
            ..FakeHost152::default()
        };

        let status = serve_lifecycle_run152(&args(&["status"]), &mut host);

        assert_eq!(
            status.stdout,
            "maw serve: running (pid 31999, :3456 responding, stale pidfile removed)\n"
        );
        assert_eq!(host.removed, vec!["/tmp/maw/maw.pid"]);
    }

    #[test]
    fn lifecycle_rejects_injection_values() {
        let mut host = FakeHost152::default();
        let bad_engine = messages_lifecycle_run152(&args(&["status", "--engine", "--bad"]), &mut host);
        assert_eq!(bad_engine.code, 2);
        assert!(bad_engine.stderr.contains("rejected --engine"));
        let bad_port = messages_lifecycle_run152(&args(&["serve", "--port", "bad\nport"]), &mut host);
        assert_eq!(bad_port.code, 2);
        assert!(bad_port.stderr.contains("rejected --port"));
    }

    #[test]
    fn dispatch_fragment_owns_cutover_entries() {
        assert_eq!(DISPATCH_152.iter().map(|entry| entry.command).collect::<Vec<_>>(), ["serve", "messages"]);
    }
}
