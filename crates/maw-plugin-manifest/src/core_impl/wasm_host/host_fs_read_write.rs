impl MawWasmHost {
    fn fs_read(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<FsReadArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let (cap, real) = match self.secure_path(&args.path, "read") {
            Ok(value) => value,
            Err(err) => return err,
        };
        let max = args.max_bytes.unwrap_or(MAX_READ_BYTES).min(MAX_READ_BYTES);
        let file = match open_nofollow_existing(&real) {
            Ok(file) => file,
            Err(err) => return err,
        };
        if let Err(err) = verify_fd_path(&file, &real) {
            return err;
        }
        let mut bytes = Vec::new();
        if let Err(error) = file.take(max + 1).read_to_end(&mut bytes) {
            return HostResult::err(HostErrorCode::IoError, format!("read failed: {error}"));
        }
        if bytes.len() as u64 > max {
            return HostResult::err(HostErrorCode::IoError, "read exceeds maxBytes");
        }
        let content = if args.encoding.as_deref() == Some("base64") {
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        } else {
            match String::from_utf8(bytes.clone()) {
                Ok(text) => text,
                Err(_) => {
                    return HostResult::err(HostErrorCode::InvalidArgs, "file is not valid utf8")
                }
            }
        };
        let result = HostResult::ok(
            json!({"path": real.display().to_string(), "bytes": bytes.len(), "content": content}),
        );
        self.audit(
            "maw.fs.read",
            &cap,
            &real.display().to_string(),
            status_of(&result),
            start,
        );
        result
    }

    fn fs_write(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<FsWriteArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if args.mkdirp.unwrap_or(false) {
            // Create missing ancestors first: `secure_write_path` canonicalizes
            // the parent, which fails with NotFound until the parent exists.
            let Some(parent) = Path::new(&args.path).parent() else {
                return HostResult::err(
                    HostErrorCode::InvalidArgs,
                    "write path requires parent",
                );
            };
            if let Err(err) = self.secure_mkdirp(parent) {
                return err;
            }
        }
        let (cap, path) = match self.secure_write_path(&args.path) {
            Ok(value) => value,
            Err(err) => return err,
        };
        let bytes = if args.encoding.as_deref() == Some("base64") {
            match base64::engine::general_purpose::STANDARD.decode(&args.content) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return HostResult::err(
                        HostErrorCode::InvalidArgs,
                        format!("base64 decode failed: {error}"),
                    )
                }
            }
        } else {
            args.content.into_bytes()
        };
        let mut opts = OpenOptions::new();
        opts.write(true).custom_flags(O_NOFOLLOW_FLAG);
        match args.mode.as_deref().unwrap_or("create") {
            "create" => {
                opts.create_new(true);
            }
            "overwrite" => {
                opts.create(true).truncate(true);
            }
            "append" => {
                opts.create(true).append(true);
            }
            _ => {
                return HostResult::err(
                    HostErrorCode::InvalidArgs,
                    "mode must be create, overwrite, or append",
                )
            }
        }
        let mut file = match opts.open(&path) {
            Ok(file) => file,
            Err(error) => {
                return HostResult::err(HostErrorCode::IoError, format!("open failed: {error}"))
            }
        };
        if let Err(err) = verify_fd_under_roots(&file, &self.roots_for("write")) {
            return err;
        }
        if let Err(error) = file.write_all(&bytes) {
            return HostResult::err(HostErrorCode::IoError, format!("write failed: {error}"));
        }
        let result =
            HostResult::ok(json!({"path": path.display().to_string(), "bytes": bytes.len()}));
        self.audit(
            "maw.fs.write",
            &cap,
            &path.display().to_string(),
            status_of(&result),
            start,
        );
        result
    }

}
