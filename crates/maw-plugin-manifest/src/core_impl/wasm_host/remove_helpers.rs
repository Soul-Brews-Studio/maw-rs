fn remove_bounded_path(
    path: &Path,
    recursive: bool,
    roots: &BTreeMap<String, PathBuf>,
) -> Result<bool, HostResult<Value>> {
    if !roots.values().any(|root| path.starts_with(root)) {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "filesystem path outside declared write roots",
        ));
    }
    let meta = std::fs::symlink_metadata(path).map_err(|error| {
        HostResult::err(
            if error.kind() == std::io::ErrorKind::NotFound {
                HostErrorCode::NotFound
            } else {
                HostErrorCode::IoError
            },
            format!("stat failed: {error}"),
        )
    })?;
    let file_type = meta.file_type();
    if file_type.is_symlink() {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "symlink deletion is denied",
        ));
    }
    if file_type.is_file() {
        let file = open_nofollow_existing(path)?;
        verify_fd_path(&file, path)?;
        drop(file);
        std::fs::remove_file(path).map_err(|error| {
            HostResult::err(
                HostErrorCode::IoError,
                format!("remove file failed: {error}"),
            )
        })?;
        return Ok(true);
    }
    if file_type.is_dir() {
        if !recursive {
            std::fs::remove_dir(path).map_err(|error| {
                HostResult::err(
                    HostErrorCode::IoError,
                    format!("remove dir failed: {error}"),
                )
            })?;
            return Ok(true);
        }
        remove_bounded_dir_recursive(path, roots)?;
        return Ok(true);
    }
    Err(HostResult::err(
        HostErrorCode::CapabilityDenied,
        "device/special file deletion denied",
    ))
}

fn remove_bounded_dir_recursive(
    path: &Path,
    roots: &BTreeMap<String, PathBuf>,
) -> Result<(), HostResult<Value>> {
    if !roots.values().any(|root| path.starts_with(root)) {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "filesystem path outside declared write roots",
        ));
    }
    for entry in std::fs::read_dir(path).map_err(|error| {
        HostResult::err(HostErrorCode::IoError, format!("read dir failed: {error}"))
    })? {
        let entry = entry.map_err(|error| {
            HostResult::err(
                HostErrorCode::IoError,
                format!("read dir entry failed: {error}"),
            )
        })?;
        let child = entry.path();
        if !child.starts_with(path) || !roots.values().any(|root| child.starts_with(root)) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "filesystem path outside declared write roots",
            ));
        }
        let meta = std::fs::symlink_metadata(&child).map_err(|error| {
            HostResult::err(HostErrorCode::IoError, format!("stat failed: {error}"))
        })?;
        let file_type = meta.file_type();
        if file_type.is_symlink() {
            std::fs::remove_file(&child).map_err(|error| {
                HostResult::err(
                    HostErrorCode::IoError,
                    format!("remove symlink failed: {error}"),
                )
            })?;
        } else if file_type.is_dir() {
            remove_bounded_dir_recursive(&child, roots)?;
        } else if file_type.is_file() {
            let file = open_nofollow_existing(&child)?;
            verify_fd_path(&file, &child)?;
            drop(file);
            std::fs::remove_file(&child).map_err(|error| {
                HostResult::err(
                    HostErrorCode::IoError,
                    format!("remove file failed: {error}"),
                )
            })?;
        } else {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "device/special file deletion denied",
            ));
        }
    }
    std::fs::remove_dir(path).map_err(|error| {
        HostResult::err(
            HostErrorCode::IoError,
            format!("remove dir failed: {error}"),
        )
    })
}
