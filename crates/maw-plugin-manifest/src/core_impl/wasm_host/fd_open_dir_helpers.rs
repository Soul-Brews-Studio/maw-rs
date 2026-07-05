fn open_nofollow_existing(path: &Path) -> Result<File, HostResult<Value>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_FLAG)
        .open(path)
        .map_err(|error| {
            HostResult::err(HostErrorCode::IoError, format!("open failed: {error}"))
        })?;
    let meta = file.metadata().map_err(|error| {
        HostResult::err(HostErrorCode::IoError, format!("metadata failed: {error}"))
    })?;
    if !meta.file_type().is_file() {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "device/special file denied",
        ));
    }
    Ok(file)
}
fn open_dir_nofollow(path: &Path) -> Result<File, HostResult<Value>> {
    // O_NOFOLLOW on the final component: if `path` was swapped for a symlink,
    // this open fails (ELOOP) rather than following it out of the sandbox.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW_FLAG)
        .open(path)
        .map_err(|error| {
            HostResult::err(
                HostErrorCode::CapabilityDenied,
                format!("open dir failed: {error}"),
            )
        })?;
    let meta = file.metadata().map_err(|error| {
        HostResult::err(HostErrorCode::IoError, format!("metadata failed: {error}"))
    })?;
    if !meta.file_type().is_dir() {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "expected directory",
        ));
    }
    Ok(file)
}

/// Ensure `dir` (and any missing ancestors) exist, creating each level safely
/// inside the declared write `roots`. Mirrors the existing fs hardening: never
/// follows a symlink, never escapes a root, and re-verifies every created
/// component through `O_NOFOLLOW` + fd realpath (the "filesystem race detected"
/// TOCTOU check). Returns the canonical path of the ensured directory.
fn ensure_dir_within_roots(
    dir: &Path,
    roots: &BTreeMap<String, PathBuf>,
) -> Result<PathBuf, HostResult<Value>> {
    if roots.is_empty() {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "filesystem path outside declared write roots",
        ));
    }
    // Walk up to the deepest existing ancestor, collecting the missing tail.
    // symlink_metadata is lstat: it does not follow symlinks.
    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    let mut base = dir.to_path_buf();
    loop {
        if std::fs::symlink_metadata(&base).is_ok() {
            break;
        }
        let Some(name) = base.file_name() else {
            // Terminates in `..`, `/`, or is empty: refuse rather than guess.
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "unsafe mkdir path component",
            ));
        };
        missing.push(name.to_owned());
        let Some(parent) = base.parent() else {
            return Err(HostResult::err(
                HostErrorCode::InvalidArgs,
                "mkdir path requires parent",
            ));
        };
        base = parent.to_path_buf();
    }
    // Anchor: the deepest existing ancestor must canonicalize to a real path
    // that stays under a declared write root (this catches a symlinked ancestor
    // escaping the sandbox, since canonicalize resolves every symlink).
    let mut current = canonicalize_checked_path(&base)?;
    if deny_special_path(&current) {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "special filesystem path denied",
        ));
    }
    if !roots.values().any(|root| current.starts_with(root)) {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "filesystem path outside declared write roots",
        ));
    }
    // Create each missing level one component at a time. `current` is always a
    // canonical (symlink-free) directory under a root, so only the single new
    // leaf is untrusted — and O_NOFOLLOW + realpath re-check guards it.
    for name in missing.iter().rev() {
        let comp = name.to_string_lossy();
        if comp.is_empty() || comp == "." || comp == ".." || comp.contains('/') {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "unsafe mkdir path component",
            ));
        }
        let next = current.join(name);
        match std::fs::create_dir(&next) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(HostResult::err(
                    HostErrorCode::IoError,
                    format!("mkdir failed: {error}"),
                ))
            }
        }
        let opened = open_dir_nofollow(&next)?;
        let real = fd_real_path(&opened)?;
        if deny_special_path(&real) || !roots.values().any(|root| real.starts_with(root)) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "filesystem race detected",
            ));
        }
        current = real;
    }
    Ok(current)
}

fn verify_fd_path(file: &File, expected: &Path) -> Result<(), HostResult<Value>> {
    let actual = fd_real_path(file)?;
    if actual == expected {
        Ok(())
    } else {
        Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "filesystem race detected",
        ))
    }
}

fn verify_fd_under_roots(
    file: &File,
    roots: &BTreeMap<String, PathBuf>,
) -> Result<(), HostResult<Value>> {
    let actual = fd_real_path(file)?;
    if roots.values().any(|root| actual.starts_with(root)) {
        Ok(())
    } else {
        Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "filesystem race detected",
        ))
    }
}

#[cfg(target_os = "linux")]
fn fd_real_path(file: &File) -> Result<PathBuf, HostResult<Value>> {
    std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).map_err(|error| {
        HostResult::err(
            HostErrorCode::IoError,
            format!("fd reverify failed: {error}"),
        )
    })
}

#[cfg(target_os = "macos")]
fn fd_real_path(file: &File) -> Result<PathBuf, HostResult<Value>> {
    rustix::fs::getpath(file.as_fd())
        .map(|path| PathBuf::from(OsString::from_vec(path.into_bytes())))
        .map_err(|error| {
            HostResult::err(
                HostErrorCode::IoError,
                format!("fd reverify failed: {error}"),
            )
        })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn fd_real_path(_file: &File) -> Result<PathBuf, HostResult<Value>> {
    Err(HostResult::err(
        HostErrorCode::IoError,
        "fd reverify unsupported on this platform",
    ))
}
