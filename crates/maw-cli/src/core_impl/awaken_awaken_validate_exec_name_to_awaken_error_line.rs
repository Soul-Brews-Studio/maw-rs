fn awaken_validate_exec_name(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.starts_with('-')
        || value.contains('/')
        || value.chars().any(char::is_control)
    {
        Err("awaken: executable name is not allowed".to_owned())
    } else {
        Ok(())
    }
}

fn awaken_validate_flag_name(value: &str) -> Result<(), String> {
    if !value.starts_with("--") || value.contains('=') || value.chars().any(char::is_control) {
        Err("awaken: invalid internal flag name".to_owned())
    } else {
        Ok(())
    }
}

fn awaken_validate_target_arg(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.chars().any(char::is_control)
        || value.split('/').any(|part| part == "..")
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ':' | '/'))
    {
        let mut message = String::from("awaken: ");
        message.push_str(label);
        message.push_str(" must be non-empty, unpadded, not start with '-', contain no '..' segments, and contain only safe target characters");
        Err(message)
    } else {
        Ok(())
    }
}

fn awaken_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.chars().any(char::is_control)
    {
        Err("awaken: tmux target must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

fn awaken_validate_repo_slug(value: &str, label: &str) -> Result<(), String> {
    let parts = value.split('/').collect::<Vec<_>>();
    if parts.len() != 2 {
        let mut message = String::from("awaken: ");
        message.push_str(label);
        message.push_str(" must be org/repo");
        return Err(message);
    }
    awaken_validate_repo_part(parts[0], label)?;
    awaken_validate_repo_part(parts[1], label)
}

fn awaken_validate_repo_part(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value == "."
        || value == ".."
        || value.chars().any(char::is_control)
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        let mut message = String::from("awaken: invalid ");
        message.push_str(label);
        Err(message)
    } else {
        Ok(())
    }
}

fn awaken_validate_text_arg(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
    {
        let mut message = String::from("awaken: ");
        message.push_str(label);
        message.push_str(" must be non-empty single-line text");
        Err(message)
    } else {
        Ok(())
    }
}

fn awaken_validate_trigger_arg(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
    {
        Err("awaken: --trigger must be non-empty single-line text".to_owned())
    } else {
        Ok(())
    }
}

fn awaken_child_error(action: &str, output: &AwakenProcessOutput) -> String {
    let mut message = String::from("awaken: maw ");
    message.push_str(action);
    message.push_str(" failed: ");
    message.push_str(&awaken_child_stderr(output));
    message
}

fn awaken_child_stderr(output: &AwakenProcessOutput) -> String {
    let stderr = output.stderr.trim();
    if stderr.is_empty() {
        let mut message = String::from("exit code ");
        message.push_str(&output.code.to_string());
        message
    } else {
        stderr.to_owned()
    }
}

fn awaken_error_line(error: &str) -> String {
    if error.is_empty() {
        String::new()
    } else {
        let mut line = error.to_owned();
        line.push('\n');
        line
    }
}

