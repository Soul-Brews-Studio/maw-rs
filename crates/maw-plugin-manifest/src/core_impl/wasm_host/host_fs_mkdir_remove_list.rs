impl MawWasmHost {
    fn fs_mkdir(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<FsPathArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if contains_glob_pattern(&args.path) {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "glob/wildcard filesystem paths are denied",
            );
        }
        let roots = self.roots_for("write");
        let canonical = match ensure_dir_within_roots(Path::new(&args.path), &roots) {
            Ok(path) => path,
            Err(err) => return err,
        };
        if let Err(err) = self.deny_protected_security_path(&canonical) {
            return err;
        }
        let Some(scope) = roots
            .iter()
            .find(|(_, root)| canonical.starts_with(root))
            .map(|(scope, _)| scope.clone())
        else {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "filesystem path outside declared write roots",
            );
        };
        let cap = match self.caps.require("fs", "write", Some(&scope)) {
            Ok(cap) => cap,
            Err(err) => return err,
        };
        let result =
            HostResult::ok(json!({"path": canonical.display().to_string(), "created": true}));
        self.audit(
            "maw.fs.mkdir",
            &cap,
            &canonical.display().to_string(),
            status_of(&result),
            start,
        );
        result
    }

    fn fs_remove(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<FsRemoveArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let (cap, path) = match self.secure_remove_path(&args.path) {
            Ok(value) => value,
            Err(err) => return err,
        };
        let result = match remove_bounded_path(
            &path,
            args.recursive.unwrap_or(false),
            &self.roots_for("write"),
        ) {
            Ok(removed) => {
                HostResult::ok(json!({"path": path.display().to_string(), "removed": removed}))
            }
            Err(err) => err,
        };
        self.audit(
            "maw.fs.remove",
            &cap,
            &path.display().to_string(),
            status_of(&result),
            start,
        );
        result
    }

    fn fs_list(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<FsListArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let (_cap, real) = match self.secure_path(&args.path, "read") {
            Ok(value) => value,
            Err(err) => return err,
        };
        let mut entries = Vec::new();
        let max = args.max_entries.unwrap_or(200).min(1000);
        let offset = args
            .offset
            .or_else(|| args.cursor.as_deref().and_then(|cursor| cursor.parse().ok()))
            .unwrap_or(0);
        let mut seen = 0;
        list_dir_page(
            &real,
            args.recursive.unwrap_or(false),
            args.include_dirs.unwrap_or(true),
            offset,
            max.saturating_add(1),
            &mut entries,
            &mut seen,
        );
        let has_more = entries.len() > max;
        if has_more {
            entries.truncate(max);
        }
        let next_offset = offset + entries.len();
        HostResult::ok(json!({
            "entries": entries,
            "hasMore": has_more,
            "nextOffset": has_more.then_some(next_offset),
            "nextCursor": has_more.then_some(next_offset.to_string())
        }))
    }

    fn fs_stat(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<FsPathArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let Ok((_cap, real)) = self.secure_path(&args.path, "read") else {
            return HostResult::ok(json!({"exists": false}));
        };
        let Ok(meta) = std::fs::symlink_metadata(&real) else {
            return HostResult::ok(json!({"exists": false}));
        };
        HostResult::ok(
            json!({"exists": true, "kind": file_kind(meta.file_type()), "bytes": meta.len()}),
        )
    }

}
