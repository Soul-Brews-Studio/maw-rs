#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use std::fs;
use std::path::Path;

const PATH_MAW_PATTERN: &str = "Command::new(\"maw\")";

#[test]
fn production_code_has_no_path_maw_subprocess_regressions() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let mut hits = Vec::new();
    scan_crate_sources(&workspace_root.join("crates"), workspace_root, &mut hits);

    assert!(
        hits.is_empty(),
        "after #420, production code must have zero PATH-maw subprocess hits; hits:\n{}",
        format_hits(&hits.iter().collect::<Vec<_>>())
    );
}

#[derive(Debug)]
struct Hit {
    rel_path: String,
    line_number: usize,
    line: String,
    enclosing_fn: Option<String>,
}

fn scan_crate_sources(crates_dir: &Path, workspace_root: &Path, hits: &mut Vec<Hit>) {
    for crate_entry in fs::read_dir(crates_dir).expect("read crates dir") {
        let crate_entry = crate_entry.expect("crate entry");
        let src_dir = crate_entry.path().join("src");
        if src_dir.is_dir() {
            scan_rs_files(&src_dir, workspace_root, hits);
        }
    }
}

fn scan_rs_files(dir: &Path, workspace_root: &Path, hits: &mut Vec<Hit>) {
    for entry in fs::read_dir(dir).expect("read source dir") {
        let entry = entry.expect("source entry");
        let path = entry.path();
        if path.is_dir() {
            scan_rs_files(&path, workspace_root, hits);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            scan_file(&path, workspace_root, hits);
        }
    }
}

fn scan_file(path: &Path, workspace_root: &Path, hits: &mut Vec<Hit>) {
    let source = fs::read_to_string(path).expect("read source file");
    let lines: Vec<_> = source.lines().collect();
    let production_mask = production_line_mask(&lines);
    let rel_path = path
        .strip_prefix(workspace_root)
        .expect("source under workspace")
        .display()
        .to_string();

    for (index, line) in lines.iter().enumerate() {
        if production_mask[index] && line.contains(PATH_MAW_PATTERN) {
            hits.push(Hit {
                rel_path: rel_path.clone(),
                line_number: index + 1,
                line: line.trim().to_owned(),
                enclosing_fn: enclosing_fn(&lines, index),
            });
        }
    }
}

fn production_line_mask(lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![true; lines.len()];
    let mut index = 0;
    while index < lines.len() {
        if lines[index].contains("cfg(test)") {
            let start = index;
            let mut item_start = index + 1;
            while item_start < lines.len() && lines[item_start].trim().is_empty() {
                item_start += 1;
            }

            let mut end = item_start;
            let mut depth = 0_usize;
            let mut saw_open_brace = false;
            while end < lines.len() {
                let line = lines[end];
                depth += line.matches('{').count();
                if line.contains('{') {
                    saw_open_brace = true;
                }
                depth = depth.saturating_sub(line.matches('}').count());
                end += 1;
                if saw_open_brace && depth == 0 {
                    break;
                }
                if !saw_open_brace && line.contains(';') {
                    break;
                }
            }

            for skipped in mask.iter_mut().take(end).skip(start) {
                *skipped = false;
            }
            index = end;
        } else {
            index += 1;
        }
    }
    mask
}

fn enclosing_fn(lines: &[&str], index: usize) -> Option<String> {
    lines[..=index].iter().rev().find_map(|line| {
        let line = line.trim_start();
        line.strip_prefix("fn ")
            .and_then(|rest| rest.split_once('('))
            .map(|(name, _)| name.to_owned())
    })
}

fn format_hits(hits: &[&Hit]) -> String {
    if hits.is_empty() {
        return "(none)".to_owned();
    }
    hits.iter()
        .map(|hit| {
            format!(
                "{}:{} [{}] {}",
                hit.rel_path,
                hit.line_number,
                hit.enclosing_fn.as_deref().unwrap_or("unknown"),
                hit.line
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
