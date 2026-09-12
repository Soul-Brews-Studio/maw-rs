#![allow(clippy::unwrap_used, clippy::expect_used)] // build script: fail-fast panics are conventional
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
struct CoreFile {
    name: String,
    order: Option<u32>,
    dispatch_number: Option<u32>,
    tmux_sub_number: Option<u32>,
    path: PathBuf,
}

const HEADER_SCAN_LINES: usize = 10;

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

    let parts = collect_core_files(&core_impl_dir)?;
    let mut includes = String::new();
    let mut dispatch_numbers = Vec::new();
    let mut tmux_sub_numbers = Vec::new();

    for part in &parts {
        println!("cargo:rerun-if-changed={}", part.path.display());
        writeln!(includes, "include!({:?});", part.path.display().to_string())
            .expect("write to String");

        if let Some(dispatch_number) = part.dispatch_number {
            dispatch_numbers.push(dispatch_number);
        }
        if let Some(tmux_sub_number) = part.tmux_sub_number {
            tmux_sub_numbers.push(tmux_sub_number);
        }
    }

    let mut fragments =
        String::from("#[allow(clippy::needless_borrow)]\npub(crate) const DISPATCHER_FRAGMENTS: &[&[DispatcherEntry]] = &[\n");
    for number in dispatch_numbers {
        writeln!(fragments, "    &DISPATCH_{number:02},").expect("write to String");
    }
    fragments.push_str("];\n");

    let mut tmux_fragments =
        String::from("#[allow(clippy::needless_borrow)]\npub(crate) const TMUX_SUB_FRAGMENTS: &[&[TmuxSubcommandEntry]] = &[\n");
    for number in tmux_sub_numbers {
        writeln!(tmux_fragments, "    &TMUX_SUB_{number:02},").expect("write to String");
    }
    tmux_fragments.push_str("];\n");

    fs::write(out_dir.join("parts_includes.rs"), includes)?;
    fs::write(out_dir.join("dispatch_fragments.rs"), fragments)?;
    fs::write(out_dir.join("tmux_sub_fragments.rs"), tmux_fragments)?;
    Ok(())
}

fn collect_core_files(core_impl_dir: &Path) -> io::Result<Vec<CoreFile>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(core_impl_dir)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if file_name == "mod.rs" || path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let contents = fs::read_to_string(&path)?;
        if has_noauto_header(&contents) {
            continue;
        }
        let dispatch_number = find_dispatch_const_number(&contents);
        let tmux_sub_number = find_tmux_sub_const_number(&contents);
        let order = header_order(&contents)
            .or(dispatch_number)
            .or(tmux_sub_number);
        files.push(CoreFile {
            name: file_name.to_owned(),
            order,
            dispatch_number,
            tmux_sub_number,
            path,
        });
    }

    files.sort_by(compare_core_files);
    assert_unique(
        "DISPATCH",
        files.iter().filter_map(|file| file.dispatch_number),
    );
    assert_unique(
        "TMUX_SUB",
        files.iter().filter_map(|file| file.tmux_sub_number),
    );
    Ok(files)
}

fn compare_core_files(left: &CoreFile, right: &CoreFile) -> Ordering {
    match (left.order, right.order) {
        (Some(left_order), Some(right_order)) => left_order
            .cmp(&right_order)
            .then_with(|| left.name.cmp(&right.name)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left.name.cmp(&right.name),
    }
}

fn has_noauto_header(contents: &str) -> bool {
    contents
        .lines()
        .take(HEADER_SCAN_LINES)
        .any(|line| line.trim() == "//maw:noauto")
}

fn header_order(contents: &str) -> Option<u32> {
    contents.lines().take(HEADER_SCAN_LINES).find_map(|line| {
        let rest = line.trim().strip_prefix("//maw:order")?.trim();
        if rest.is_empty() {
            return None;
        }
        rest.parse().ok()
    })
}

fn assert_unique(label: &str, numbers: impl Iterator<Item = u32>) {
    let mut seen = BTreeSet::new();
    for number in numbers {
        assert!(
            seen.insert(number),
            "duplicate core_impl {label}_{number:02}"
        );
    }
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
        .unwrap_or_else(|| env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_owned()))
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
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}
