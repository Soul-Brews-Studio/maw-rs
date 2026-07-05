impl ServecoreThreadStore {
    #[must_use]
    pub fn servecore_default() -> Self {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
        let vars = [
            "MAW_HOME",
            "MAW_DATA_DIR",
            "MAW_XDG",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
        ]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)));
        let env = maw_xdg::MawXdgEnv::with_vars(home, vars);
        Self::servecore_with_root(maw_xdg::maw_data_path(&env, &["threads"]))
    }

    #[must_use]
    pub fn servecore_with_root(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Arc::new(root.into()),
            lock: Arc::new(Mutex::new(())),
        }
    }

    /// Create an empty maw-js-compatible thread and return its numeric id.
    ///
    /// # Errors
    /// Returns an error when participants are invalid or the thread file cannot be written.
    pub fn create_thread(&self, participants: &[String]) -> Result<u64, String> {
        let title = servecore_thread_title(participants)?;
        let record = self.servecore_create_record(&title)?;
        Ok(record.thread.id)
    }

    /// Append one maw-js-compatible message to an existing thread.
    ///
    /// # Errors
    /// Returns an error when the id, role, content, or backing file is invalid.
    pub fn append(
        &self,
        id: u64,
        role: &str,
        content: &str,
    ) -> Result<ServecoreThreadPostResult, String> {
        let _guard = self.servecore_lock();
        let mut record = self.servecore_read_record_locked(id)?;
        let message = servecore_thread_message(&record, role, content)?;
        record.messages.push(message.clone());
        self.servecore_write_record_locked(&record)?;
        Ok(servecore_thread_post_result(record.thread.id, message.id))
    }

    /// Read one maw-js-compatible thread record.
    ///
    /// # Errors
    /// Returns an error when the id is missing, invalid, too large, or invalid JSON.
    pub fn read(&self, id: u64) -> Result<ServecoreThreadRecord, String> {
        let _guard = self.servecore_lock();
        self.servecore_read_record_locked(id)
    }

    /// List thread metadata in descending id order.
    ///
    /// # Errors
    /// Returns an error when the backing thread directory cannot be read.
    pub fn list(&self) -> Result<Vec<ServecoreThreadInfo>, String> {
        let _guard = self.servecore_lock();
        self.servecore_list_locked()
    }

    /// Find-or-create an open channel thread and append one message.
    ///
    /// # Errors
    /// Returns an error when the title, role, content, or backing file is invalid.
    pub fn servecore_post_channel(
        &self,
        title: &str,
        role: &str,
        content: &str,
    ) -> Result<(ServecoreThreadPostResult, ServecoreThreadRecord), String> {
        let _guard = self.servecore_lock();
        let mut record = self
            .servecore_find_open_title_locked(title)?
            .unwrap_or_else(|| self.servecore_new_record_locked(title));
        let message = servecore_thread_message(&record, role, content)?;
        record.messages.push(message.clone());
        self.servecore_write_record_locked(&record)?;
        Ok((
            servecore_thread_post_result(record.thread.id, message.id),
            record,
        ))
    }

    fn servecore_lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn servecore_create_record(&self, title: &str) -> Result<ServecoreThreadRecord, String> {
        let _guard = self.servecore_lock();
        let record = self.servecore_new_record_locked(title);
        self.servecore_write_record_locked(&record)?;
        Ok(record)
    }

    fn servecore_new_record_locked(&self, title: &str) -> ServecoreThreadRecord {
        let id = self.servecore_next_id_locked().unwrap_or(1);
        let now = servecore_thread_now();
        ServecoreThreadRecord {
            thread: ServecoreThreadInfo {
                id,
                title: title.to_owned(),
                status: "open".to_owned(),
                created_at: now,
            },
            messages: Vec::new(),
        }
    }

    fn servecore_next_id_locked(&self) -> Result<u64, String> {
        let list = self.servecore_list_locked()?;
        Ok(list.into_iter().map(|thread| thread.id).max().unwrap_or(0) + 1)
    }

    fn servecore_find_open_title_locked(
        &self,
        title: &str,
    ) -> Result<Option<ServecoreThreadRecord>, String> {
        for thread in self.servecore_list_locked()? {
            if thread.title == title && thread.status != "closed" {
                return self.servecore_read_record_locked(thread.id).map(Some);
            }
        }
        Ok(None)
    }

    fn servecore_list_locked(&self) -> Result<Vec<ServecoreThreadInfo>, String> {
        let root = self.servecore_ensure_root()?;
        let mut items = Vec::new();
        let entries = fs::read_dir(root).map_err(|error| error.to_string())?;
        for entry in entries.flatten() {
            if items.len() >= SERVECORE_THREAD_MAX_THREADS {
                break;
            }
            if let Some(record) = self.servecore_load_entry(&entry)? {
                items.push(record.thread);
            }
        }
        items.sort_by_key(|thread| Reverse(thread.id));
        Ok(items)
    }

    fn servecore_load_entry(
        &self,
        entry: &fs::DirEntry,
    ) -> Result<Option<ServecoreThreadRecord>, String> {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            return Ok(None);
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            return Ok(None);
        };
        let id = servecore_thread_id(stem)?;
        self.servecore_read_record_locked(id).map(Some)
    }

    fn servecore_read_record_locked(&self, id: u64) -> Result<ServecoreThreadRecord, String> {
        let path = self.servecore_path_for_id(id)?;
        let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
        if metadata.len() > SERVECORE_THREAD_FILE_BYTES {
            return Err("thread file too large".to_owned());
        }
        let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
        serde_json::from_str(&raw).map_err(|error| error.to_string())
    }

    fn servecore_write_record_locked(&self, record: &ServecoreThreadRecord) -> Result<(), String> {
        let path = self.servecore_path_for_id(record.thread.id)?;
        let mut data = serde_json::to_vec_pretty(record).map_err(|error| error.to_string())?;
        data.push(b'\n');
        if data.len() as u64 > SERVECORE_THREAD_FILE_BYTES {
            return Err("thread file too large".to_owned());
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, data).map_err(|error| error.to_string())?;
        fs::rename(&tmp, path).map_err(|error| error.to_string())
    }

    fn servecore_path_for_id(&self, id: u64) -> Result<PathBuf, String> {
        let safe_id = servecore_thread_id(&id.to_string())?;
        let root = self.servecore_ensure_root()?;
        let path = root.join(format!("{safe_id}.json"));
        servecore_thread_path_inside(&root, &path)?;
        Ok(path)
    }

    fn servecore_ensure_root(&self) -> Result<PathBuf, String> {
        fs::create_dir_all(self.root.as_path()).map_err(|error| error.to_string())?;
        fs::canonicalize(self.root.as_path()).map_err(|error| error.to_string())
    }
}

fn servecore_thread_title(participants: &[String]) -> Result<String, String> {
    if participants.is_empty() || participants.len() > SERVECORE_THREAD_MAX_PARTICIPANTS {
        return Err("thread participants out of bounds".to_owned());
    }
    for participant in participants {
        servecore_thread_safe_text(participant, "participant")?;
    }
    Ok(participants.join(","))
}

