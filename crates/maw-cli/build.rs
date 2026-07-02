use std::collections::{BTreeSet, HashSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);
const GIT_COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Ordered manifest of the `core_impl` fragment files (one file name per line).
/// This is the single source of truth for which fragments are compiled in and in
/// what order the dispatch/tmux fragment arrays are assembled — decoupled from the
/// file names, so fragments can be renamed to semantic names without touching
/// dispatch behaviour.
const MANIFEST_FILE: &str = "parts.order";

fn main() {
    if let Err(error) = generate() {
        panic!("failed to generate maw-cli core includes: {error}");
    }
}

fn generate() -> io::Result<()> {
    emit_build_info();

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set"));
    let core_impl_dir = manifest_dir.join("src").join("core_impl");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR set"));

    println!("cargo:rerun-if-changed=src/core_impl");
    println!("cargo:rerun-if-changed=src/core_impl/{MANIFEST_FILE}");

    let ordered = read_manifest(&core_impl_dir)?;
    validate_membership(&core_impl_dir, &ordered)?;

    let mut includes = String::new();
    let mut dispatch_numbers = Vec::new();
    let mut tmux_sub_numbers = Vec::new();
    let mut seen_dispatch = HashSet::new();
    let mut seen_tmux = HashSet::new();

    for file_name in &ordered {
        let path = core_impl_dir.join(file_name);
        println!("cargo:rerun-if-changed={}", path.display());
        writeln!(includes, "include!({:?});", path.display().to_string()).expect("write to String");

        let contents = fs::read_to_string(&path)?;
        if let Some(dispatch_number) = find_dispatch_const_number(&contents) {
            assert!(
                seen_dispatch.insert(dispatch_number),
                "duplicate DISPATCH_{dispatch_number:02} const (declared again in {file_name})"
            );
            dispatch_numbers.push(dispatch_number);
        }
        if let Some(tmux_sub_number) = find_tmux_sub_const_number(&contents) {
            assert!(
                seen_tmux.insert(tmux_sub_number),
                "duplicate TMUX_SUB_{tmux_sub_number:02} const (declared again in {file_name})"
            );
            tmux_sub_numbers.push(tmux_sub_number);
        }
    }

    let mut fragments = String::from(
        "#[allow(clippy::needless_borrow)]\npub(crate) const DISPATCHER_FRAGMENTS: &[&[DispatcherEntry]] = &[\n",
    );
    for number in dispatch_numbers {
        writeln!(fragments, "    &DISPATCH_{number:02},").expect("write to String");
    }
    fragments.push_str("];\n");

    let mut tmux_fragments = String::from(
        "#[allow(clippy::needless_borrow)]\npub(crate) const TMUX_SUB_FRAGMENTS: &[&[TmuxSubcommandEntry]] = &[\n",
    );
    for number in tmux_sub_numbers {
        writeln!(tmux_fragments, "    &TMUX_SUB_{number:02},").expect("write to String");
    }
    tmux_fragments.push_str("];\n");

    fs::write(out_dir.join("parts_includes.rs"), includes)?;
    fs::write(out_dir.join("dispatch_fragments.rs"), fragments)?;
    fs::write(out_dir.join("tmux_sub_fragments.rs"), tmux_fragments)?;
    Ok(())
}

/// Read `parts.order` into the ordered list of fragment file names.
/// Blank lines and `#` comments are ignored; duplicate entries are rejected.
fn read_manifest(core_impl_dir: &Path) -> io::Result<Vec<String>> {
    let contents = fs::read_to_string(core_impl_dir.join(MANIFEST_FILE))?;
    let mut ordered = Vec::new();
    let mut seen = HashSet::new();
    for raw in contents.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        assert!(
            seen.insert(line.to_owned()),
            "duplicate entry {line} in {MANIFEST_FILE}"
        );
        ordered.push(line.to_owned());
    }
    Ok(ordered)
}

/// Ensure the manifest exactly describes the fragment set: every `.rs` in `core_impl`
/// (except `mod.rs` and files pulled in by a nested `include!`, e.g.
/// `attach_private_tests.rs`) is listed exactly once, and every listed entry is a
/// real top-level fragment. Catches a new part added without a manifest entry, a
/// stale entry, or an accidental double-include.
fn validate_membership(core_impl_dir: &Path, ordered: &[String]) -> io::Result<()> {
    let listed: HashSet<&str> = ordered.iter().map(String::as_str).collect();

    let mut nested = HashSet::new();
    for file_name in ordered {
        let path = core_impl_dir.join(file_name);
        assert!(
            path.is_file(),
            "{MANIFEST_FILE} lists {file_name} but that file does not exist"
        );
        for included in nested_includes(&fs::read_to_string(&path)?) {
            nested.insert(included);
        }
    }

    let mut on_disk = BTreeSet::new();
    for entry in fs::read_dir(core_impl_dir)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !has_rs_extension(name) || name == "mod.rs" || nested.contains(name) {
            continue;
        }
        on_disk.insert(name.to_owned());
    }

    for name in &on_disk {
        assert!(
            listed.contains(name.as_str()),
            "{name} exists in core_impl but is missing from {MANIFEST_FILE}"
        );
    }
    for name in ordered {
        assert!(
            on_disk.contains(name),
            "{MANIFEST_FILE} lists {name}, which is not a top-level fragment (nested-included or absent)"
        );
    }
    Ok(())
}

/// File names referenced by a bare same-directory `include!("name.rs")`.
fn nested_includes(contents: &str) -> Vec<String> {
    contents
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("include!(\"")?;
            let path = &rest[..rest.find("\")")?];
            if path.contains('/') || !has_rs_extension(path) {
                return None;
            }
            Some(path.to_owned())
        })
        .collect()
}

/// True if `name` has a `.rs` extension (case-insensitive).
fn has_rs_extension(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

fn find_dispatch_const_number(contents: &str) -> Option<u32> {
    contents.lines().find_map(dispatch_const_number_from_line)
}

fn find_tmux_sub_const_number(contents: &str) -> Option<u32> {
    contents.lines().find_map(tmux_sub_const_number_from_line)
}

fn dispatch_const_number_from_line(line: &str) -> Option<u32> {
    let line = line.trim_start();
    let rest = line
        .strip_prefix("const ")
        .or_else(|| line.strip_prefix("pub const "))
        .or_else(|| line.strip_prefix("pub(crate) const "))?;
    let rest = rest.strip_prefix("DISPATCH_")?;
    let digits_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits_len == 0 || !rest[digits_len..].starts_with(':') {
        return None;
    }
    rest[..digits_len].parse().ok()
}

fn tmux_sub_const_number_from_line(line: &str) -> Option<u32> {
    let line = line.trim_start();
    let rest = line
        .strip_prefix("const ")
        .or_else(|| line.strip_prefix("pub const "))
        .or_else(|| line.strip_prefix("pub(crate) const "))?;
    let rest = rest.strip_prefix("TMUX_SUB_")?;
    let digits_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits_len == 0 || !rest[digits_len..].starts_with(':') {
        return None;
    }
    rest[..digits_len].parse().ok()
}

fn emit_build_info() {
    println!("cargo:rerun-if-env-changed=MAW_BUILD_VERSION");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");
    println!("cargo:rerun-if-changed=../../.git/packed-refs");
    println!(
        "cargo:rustc-env=MAW_BUILD_VERSION={}",
        resolve_build_version()
    );
    println!(
        "cargo:rustc-env=MAW_RS_GIT_HASH={}",
        git_output(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_owned())
    );
    println!(
        "cargo:rustc-env=MAW_RS_BUILD_DATE={}",
        git_output(&["log", "-1", "--format=%ci"]).unwrap_or_else(|| "unknown".to_owned())
    );
}

fn resolve_build_version() -> String {
    let value = env::var("MAW_BUILD_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| git_output(&["describe", "--tags", "--always", "--dirty"]))
        .unwrap_or_else(|| "unknown".to_owned())
        .trim()
        .to_owned();
    strip_leading_v(value)
}

fn strip_leading_v(value: String) -> String {
    if let Some(stripped) = value.strip_prefix('v') {
        stripped.to_owned()
    } else {
        value
    }
}

fn git_output(args: &[&str]) -> Option<String> {
    command_output_with_timeout("git", args, GIT_COMMAND_TIMEOUT)
}

fn command_output_with_timeout(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait().ok()? {
            Some(status) if status.success() => {
                return child
                    .wait_with_output()
                    .ok()
                    .and_then(|output| String::from_utf8(output.stdout).ok())
                    .map(|value| value.trim().to_owned())
                    .filter(|value| !value.is_empty());
            }
            Some(_) => return None,
            None if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => thread::sleep(GIT_COMMAND_POLL_INTERVAL),
        }
    }
}
