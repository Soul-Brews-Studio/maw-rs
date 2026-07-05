fn scaffold_parse_args(argv: &[String], usage: &str) -> Result<ScaffoldOptionsNative, String> {
    let mut language = ScaffoldLanguageNative::Rust;
    let mut dest = None::<std::path::PathBuf>;
    let mut dry_run = false;
    let mut name = None::<String>;
    let mut index = 0_usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" | "help" => return Err(usage.to_owned()),
            "--" => return Err("scaffold: -- separator is not allowed".to_owned()),
            "--rust" => language = ScaffoldLanguageNative::Rust,
            "--as" | "--assemblyscript" => language = ScaffoldLanguageNative::AssemblyScript,
            "--dry-run" => dry_run = true,
            "--dest" => { dest = Some(scaffold_path_value(argv, &mut index, "--dest")?); }
            value if value.starts_with("--dest=") => dest = Some(scaffold_validate_path(&value["--dest=".len()..])?),
            value if value.starts_with('-') => return Err(scaffold_flag_like(value)),
            value => scaffold_set_name(&mut name, value)?,
        }
        index += 1;
    }
    let name = name.ok_or_else(|| usage.to_owned())?;
    scaffold_validate_name(&name)?;
    let dest = dest.unwrap_or_else(|| std::path::PathBuf::from(&name));
    Ok(ScaffoldOptionsNative { name, dest, language, dry_run })
}

fn scaffold_set_name(slot: &mut Option<String>, value: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(SCAFFOLD_USAGE.to_owned());
    }
    if value.starts_with('-') {
        return Err(scaffold_flag_like(value));
    }
    *slot = Some(value.to_owned());
    Ok(())
}

fn scaffold_path_value(argv: &[String], index: &mut usize, flag: &str) -> Result<std::path::PathBuf, String> {
    let Some(value) = argv.get(*index + 1) else { return Err(format!("scaffold: {flag} requires a value")); };
    *index += 1;
    scaffold_validate_path(value)
}

fn scaffold_validate_name(name: &str) -> Result<(), String> {
    if name == "--" || name.starts_with('-') {
        return Err("scaffold name must not start with '-'".to_owned());
    }
    if let Some(error) = validate_plugin_name(name) {
        return Err(format!("scaffold: invalid plugin name: {error}"));
    }
    Ok(())
}

fn scaffold_validate_path(value: &str) -> Result<std::path::PathBuf, String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') || value.contains('\0') {
        return Err("scaffold path must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.split('/').any(|part| part == "..") {
        return Err("scaffold path must not contain .. segments".to_owned());
    }
    Ok(std::path::PathBuf::from(value))
}

fn scaffold_flag_like(value: &str) -> String {
    format!("\"{value}\" looks like a flag, not a scaffold name.\n  {SCAFFOLD_USAGE}")
}

fn scaffold_apply(options: &ScaffoldOptionsNative) -> Result<String, String> {
    scaffold_validate_destination(&options.dest)?;
    if options.dry_run {
        return Ok(scaffold_render_plan(options));
    }
    match options.language {
        ScaffoldLanguageNative::Rust => scaffold_write_rust(options)?,
        ScaffoldLanguageNative::AssemblyScript => scaffold_write_as(options)?,
    }
    Ok(scaffold_render_created(options))
}

fn scaffold_validate_destination(path: &std::path::Path) -> Result<(), String> {
    let display = path.display().to_string();
    scaffold_validate_path(&display)?;
    if path.exists() {
        return Err(format!("scaffold: destination exists: {}", path.display()));
    }
    Ok(())
}

fn scaffold_render_plan(options: &ScaffoldOptionsNative) -> String {
    format!("scaffold plan: create {} plugin {} at {}\n", scaffold_language_name(options.language), options.name, options.dest.display())
}

fn scaffold_render_created(options: &ScaffoldOptionsNative) -> String {
    format!("created {} plugin {} at {}\n", scaffold_language_name(options.language), options.name, options.dest.display())
}

fn scaffold_language_name(language: ScaffoldLanguageNative) -> &'static str {
    match language {
        ScaffoldLanguageNative::Rust => "rust",
        ScaffoldLanguageNative::AssemblyScript => "assemblyscript",
    }
}

fn scaffold_write_rust(options: &ScaffoldOptionsNative) -> Result<(), String> {
    std::fs::create_dir_all(options.dest.join("src")).map_err(|error| format!("scaffold: create rust dirs: {error}"))?;
    std::fs::write(options.dest.join("Cargo.toml"), scaffold_rust_cargo(&options.name)).map_err(|error| format!("scaffold: write Cargo.toml: {error}"))?;
    std::fs::write(options.dest.join("src/lib.rs"), scaffold_rust_lib()).map_err(|error| format!("scaffold: write src/lib.rs: {error}"))?;
    std::fs::write(options.dest.join("README.md"), scaffold_readme(&options.name, "Rust")).map_err(|error| format!("scaffold: write README.md: {error}"))?;
    std::fs::write(options.dest.join("plugin.json"), build_manifest_json(&options.name, ScaffoldLanguage::Rust)).map_err(|error| format!("scaffold: write plugin.json: {error}"))?;
    Ok(())
}

fn scaffold_write_as(options: &ScaffoldOptionsNative) -> Result<(), String> {
    std::fs::create_dir_all(options.dest.join("assembly")).map_err(|error| format!("scaffold: create as dirs: {error}"))?;
    std::fs::write(options.dest.join("package.json"), scaffold_as_package(&options.name)).map_err(|error| format!("scaffold: write package.json: {error}"))?;
    std::fs::write(options.dest.join("assembly/index.ts"), scaffold_as_index()).map_err(|error| format!("scaffold: write assembly/index.ts: {error}"))?;
    std::fs::write(options.dest.join("README.md"), scaffold_readme(&options.name, "AssemblyScript")).map_err(|error| format!("scaffold: write README.md: {error}"))?;
    std::fs::write(options.dest.join("plugin.json"), build_manifest_json(&options.name, ScaffoldLanguage::AssemblyScript)).map_err(|error| format!("scaffold: write plugin.json: {error}"))?;
    Ok(())
}

fn scaffold_rust_cargo(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n")
}

fn scaffold_rust_lib() -> &'static str {
    "#[no_mangle]\npub extern \"C\" fn maw_plugin_entry() -> i32 { 0 }\n"
}

fn scaffold_as_package(name: &str) -> String {
    format!("{{\n  \"name\": \"{name}\",\n  \"version\": \"0.1.0\",\n  \"scripts\": {{\"build\": \"asc assembly/index.ts --target release\"}}\n}}\n")
}

fn scaffold_as_index() -> &'static str {
    "export function mawPluginEntry(): i32 { return 0; }\n"
}

fn scaffold_readme(name: &str, language: &str) -> String {
    format!("# {name}\n\n{language} maw plugin scaffold.\n")
}

fn new_run_command(argv: &[String]) -> CliOutput {
    match new_parse_args(argv) {
        Ok(options) => match scaffold_apply(&options) {
            Ok(stdout) => CliOutput { code: 0, stdout: new_relabel_stdout(&stdout), stderr: String::new() },
            Err(message) => new_error(&message),
        },
        Err(message) => new_error(&message),
    }
}

fn new_error(message: &str) -> CliOutput {
    CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") }
}

fn new_parse_args(argv: &[String]) -> Result<ScaffoldOptionsNative, String> {
    scaffold_parse_args(argv, NEW_USAGE).map_err(|message| message.replace("scaffold", "new"))
}

fn new_relabel_stdout(stdout: &str) -> String {
    stdout.replace("scaffold plan:", "new plan:").replace("created", "created new")
}

fn promote_run_command(argv: &[String]) -> CliOutput {
    let mut tmux = PromoteSystemTmuxNative;
    promote_run_command_with(argv, &mut tmux)
}

fn promote_run_command_with(argv: &[String], tmux: &mut impl PromoteTmuxNative) -> CliOutput {
    match promote_parse_args(argv).and_then(|options| promote_execute(&options, tmux)) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => promote_error(&message),
    }
}

fn promote_error(message: &str) -> CliOutput {
    CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}
") }
}

fn promote_parse_args(argv: &[String]) -> Result<PromoteOptionsNative, String> {
    let mut target = None::<String>;
    let mut as_session = None::<String>;
    let mut attach = false;
    let mut force = false;
    let mut index = 0_usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" | "help" => return Err(PROMOTE_USAGE.to_owned()),
            "--" => return Err("promote: -- separator is not allowed".to_owned()),
            "--attach" => attach = true,
            "--force" => force = true,
            "--as" => as_session = Some(promote_take_session_value(argv, &mut index, "--as")?),
            value if value.starts_with("--as=") => as_session = Some(promote_validate_session_name(&value["--as=".len()..], "--as")?),
            value if value.starts_with('-') => return Err(promote_flag_like(value)),
            value => promote_set_target(&mut target, value)?,
        }
        index += 1;
    }
    Ok(PromoteOptionsNative { target: target.ok_or_else(|| PROMOTE_USAGE.to_owned())?, as_session, attach, force })
}

fn promote_take_session_value(argv: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    let Some(value) = argv.get(*index + 1) else { return Err(format!("promote: {flag} requires a value")); };
    *index += 1;
    promote_validate_session_name(value, flag)
}

fn promote_set_target(slot: &mut Option<String>, value: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(PROMOTE_USAGE.to_owned());
    }
    *slot = Some(promote_validate_target(value, "target")?);
    Ok(())
}

