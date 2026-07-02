const DISPATCH_35: &[DispatcherEntry] = &[
    DispatcherEntry { command: "contacts", handler: Handler::Sync(run_contacts_command) },
    DispatcherEntry { command: "contact", handler: Handler::Sync(run_contacts_command) },
];

fn run_contacts_command(args: &[String]) -> CliOutput {
    match run_contacts_command_impl(args) {
        Ok(output) => output,
        Err(error) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("contacts: {error}\n"),
        },
    }
}

fn run_contacts_command_impl(args: &[String]) -> Result<CliOutput, String> {
    let sub = args.first().map(|value| value.to_ascii_lowercase());
    match sub.as_deref() {
        Some("add") if args.get(1).is_some() => contacts_add(&args[1], &args[2..]),
        Some("rm" | "remove") => {
            let Some(name) = args.get(1) else {
                return Ok(CliOutput {
                    code: 1,
                    stdout: String::new(),
                    stderr: "usage: maw contacts rm <name>\n".to_owned(),
                });
            };
            contacts_rm(name)
        }
        _ => contacts_ls(),
    }
}

fn contacts_ls() -> Result<CliOutput, String> {
    let data = load_contacts_file()?;
    let contacts = data
        .get("contacts")
        .and_then(serde_json::Value::as_object);
    let Some(contacts) = contacts else {
        return Ok(CliOutput {
            code: 0,
            stdout: "\u{1b}[90mno contacts\u{1b}[0m\n".to_owned(),
            stderr: String::new(),
        });
    };

    let active = contacts
        .iter()
        .filter(|(_, contact)| !contact.get("retired").and_then(serde_json::Value::as_bool).unwrap_or(false))
        .collect::<Vec<_>>();

    if active.is_empty() {
        return Ok(CliOutput {
            code: 0,
            stdout: "\u{1b}[90mno contacts\u{1b}[0m\n".to_owned(),
            stderr: String::new(),
        });
    }

    let mut stdout = format!("\n\u{1b}[36mCONTACTS\u{1b}[0m ({}):\n\n", active.len());
    for (name, contact) in active {
        let maw = contact_string(contact, "maw")
            .map(|value| format!("maw: \u{1b}[33m{value}\u{1b}[0m"));
        let thread = contact_string(contact, "thread")
            .map(|value| format!("thread: \u{1b}[90m{value}\u{1b}[0m"));
        let inbox = contact_string(contact, "inbox")
            .map(|value| format!("inbox: \u{1b}[90m{value}\u{1b}[0m"));
        let repo = contact_string(contact, "repo")
            .map(|value| format!("repo: \u{1b}[90m{value}\u{1b}[0m"));
        let notes = contact_string(contact, "notes")
            .map(|value| format!("\u{1b}[90m\"{value}\"\u{1b}[0m"));
        let parts = [maw, thread, inbox, repo, notes]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("    ");
        let _ = writeln!(stdout, "  \u{1b}[32m{name:<12}\u{1b}[0m  {parts}");
    }
    stdout.push('\n');

    Ok(CliOutput {
        code: 0,
        stdout,
        stderr: String::new(),
    })
}

fn contacts_add(name: &str, args: &[String]) -> Result<CliOutput, String> {
    let mut data = load_contacts_file()?;
    ensure_contacts_root(&mut data);
    let contact = data
        .get_mut("contacts")
        .and_then(serde_json::Value::as_object_mut)
        .expect("contacts root ensured")
        .entry(name.to_owned())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !contact.is_object() {
        *contact = serde_json::Value::Object(serde_json::Map::new());
    }
    let contact = contact.as_object_mut().expect("contact object ensured");
    for flag in ["--maw", "--thread", "--inbox", "--repo", "--notes"] {
        if let Some(value) = parse_contacts_string_flag(args, flag)? {
            contact.insert(flag.trim_start_matches("--").to_owned(), serde_json::Value::String(value));
        }
    }
    contact.remove("retired");
    save_contacts_file(&mut data)?;
    Ok(CliOutput {
        code: 0,
        stdout: format!("\u{1b}[32m✓\u{1b}[0m contact \u{1b}[33m{name}\u{1b}[0m saved\n"),
        stderr: String::new(),
    })
}

fn contacts_rm(name: &str) -> Result<CliOutput, String> {
    let mut data = load_contacts_file()?;
    let contact = data
        .get_mut("contacts")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|contacts| contacts.get_mut(name));
    let Some(contact) = contact else {
        return Ok(CliOutput {
            code: 0,
            stdout: String::new(),
            stderr: format!("\u{1b}[31merror\u{1b}[0m: contact '{name}' not found\n"),
        });
    };
    if !contact.is_object() {
        *contact = serde_json::Value::Object(serde_json::Map::new());
    }
    contact
        .as_object_mut()
        .expect("contact object ensured")
        .insert("retired".to_owned(), serde_json::Value::Bool(true));
    save_contacts_file(&mut data)?;
    Ok(CliOutput {
        code: 0,
        stdout: format!("\u{1b}[32m✓\u{1b}[0m contact \u{1b}[33m{name}\u{1b}[0m retired\n"),
        stderr: String::new(),
    })
}

fn contact_string<'a>(contact: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    contact.get(key).and_then(serde_json::Value::as_str).filter(|value| !value.is_empty())
}

fn parse_contacts_string_flag(args: &[String], flag: &str) -> Result<Option<String>, String> {
    let prefix = format!("{flag}=");
    let mut index = 0;
    let mut found = None;
    while index < args.len() {
        let arg = &args[index];
        if arg == flag {
            let Some(value) = args.get(index + 1) else {
                return Err(format!("missing value for {flag}"));
            };
            found = Some(value.clone());
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix(&prefix) {
            found = Some(value.to_owned());
        }
        index += 1;
    }
    Ok(found)
}

fn load_contacts_file() -> Result<serde_json::Value, String> {
    let path = contacts_file_path()?;
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Ok(empty_contacts_file());
    };
    serde_json::from_str(&raw).map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn save_contacts_file(data: &mut serde_json::Value) -> Result<(), String> {
    ensure_contacts_root(data);
    data.as_object_mut()
        .expect("contacts file object ensured")
        .insert("updated".to_owned(), serde_json::Value::String(contacts_now_iso8601()));
    let psi = resolve_contacts_psi_path()?;
    std::fs::create_dir_all(&psi).map_err(|error| format!("failed to create {}: {error}", psi.display()))?;
    let path = psi.join("contacts.json");
    let json = serde_json::to_string_pretty(data)
        .map_err(|error| format!("failed to serialize contacts: {error}"))?;
    std::fs::write(&path, format!("{json}\n"))
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn empty_contacts_file() -> serde_json::Value {
    serde_json::json!({
        "contacts": {},
        "updated": contacts_now_iso8601(),
    })
}

fn ensure_contacts_root(data: &mut serde_json::Value) {
    if !data.is_object() {
        *data = serde_json::Value::Object(serde_json::Map::new());
    }
    let object = data.as_object_mut().expect("object ensured");
    if !object.get("contacts").is_some_and(serde_json::Value::is_object) {
        object.insert(
            "contacts".to_owned(),
            serde_json::Value::Object(serde_json::Map::new()),
        );
    }
    if !object.get("updated").is_some_and(serde_json::Value::is_string) {
        object.insert("updated".to_owned(), serde_json::Value::String(contacts_now_iso8601()));
    }
}

fn contacts_file_path() -> Result<std::path::PathBuf, String> {
    Ok(resolve_contacts_psi_path()?.join("contacts.json"))
}

fn resolve_contacts_psi_path() -> Result<std::path::PathBuf, String> {
    if let Some(psi_path) = load_config_psi_path() {
        return Ok(std::path::PathBuf::from(psi_path));
    }
    let cwd = std::env::current_dir().map_err(|error| format!("failed to read cwd: {error}"))?;
    let greek_psi = cwd.join("ψ");
    if greek_psi.exists() {
        return Ok(greek_psi);
    }
    Ok(cwd.join("psi"))
}

fn load_config_psi_path() -> Option<String> {
    let env = real_xdg_env();
    let path = maw_config_path(&env, &["maw.config.json"]);
    let raw = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    value
        .get("psiPath")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn contacts_now_iso8601() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = i64::try_from(duration.as_secs()).unwrap_or(i64::MAX);
    let millis = duration.subsec_millis();
    let (year, month, day, hour, minute, second) = unix_seconds_to_utc(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn unix_seconds_to_utc(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = u32::try_from(seconds_of_day / 3_600).unwrap_or(0);
    let minute = u32::try_from((seconds_of_day % 3_600) / 60).unwrap_or(0);
    let second = u32::try_from(seconds_of_day % 60).unwrap_or(0);
    (year, month, day, hour, minute, second)
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}
