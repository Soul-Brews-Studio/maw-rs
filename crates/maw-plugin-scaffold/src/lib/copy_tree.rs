#[derive(Debug)]
struct TreeEntry {
    file_name: std::ffi::OsString,
    source_path: std::path::PathBuf,
    is_dir: bool,
}

fn copy_tree_inner(src: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    copy_tree_entries(fs::read_dir(src)?.map(read_tree_entry), dest)
}

fn copy_tree_entries(
    entries: impl IntoIterator<Item = io::Result<TreeEntry>>,
    dest: &Path,
) -> io::Result<()> {
    for entry in entries {
        let entry = entry?;
        if should_skip_entry(&entry.file_name) {
            continue;
        }
        let dest_path = dest.join(entry.file_name);
        if entry.is_dir {
            copy_tree_inner(&entry.source_path, &dest_path)?;
        } else {
            fs::copy(&entry.source_path, &dest_path)?;
        }
    }
    Ok(())
}

fn read_tree_entry(entry: io::Result<fs::DirEntry>) -> io::Result<TreeEntry> {
    let entry = entry?;
    tree_entry_from_parts(entry.file_name(), entry.path(), entry.file_type())
}

fn tree_entry_from_parts(
    file_name: std::ffi::OsString,
    source_path: std::path::PathBuf,
    file_type: io::Result<fs::FileType>,
) -> io::Result<TreeEntry> {
    Ok(TreeEntry {
        is_dir: file_type?.is_dir(),
        file_name,
        source_path,
    })
}

fn should_skip_entry(name: &std::ffi::OsStr) -> bool {
    matches!(name.to_str(), Some("target" | ".git" | "node_modules"))
}

fn is_valid_plugin_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
}
