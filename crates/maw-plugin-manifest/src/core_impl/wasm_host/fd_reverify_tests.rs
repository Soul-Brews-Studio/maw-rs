#[cfg(test)]
mod fd_reverify_tests {
    use super::*;
    use std::fs::{create_dir_all, rename, write, OpenOptions};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "maw-fd-reverify-{label}-{}-{nonce}",
            std::process::id()
        ));
        create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn fd_reverify_detects_open_file_path_mismatch() {
        let dir = temp_dir("path-mismatch");
        let expected = dir.join("target.txt");
        let moved = dir.join("moved.txt");
        write(&expected, "original").expect("target");

        let file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW_FLAG)
            .open(&expected)
            .expect("open target");
        rename(&expected, &moved).expect("rename opened file");
        write(&expected, "replacement").expect("replacement");

        let err = verify_fd_path(&file, &expected).expect_err("mismatch denied");
        assert!(matches!(
            err,
            HostResult::Err {
                code: HostErrorCode::CapabilityDenied,
                ..
            }
        ));
    }
}
