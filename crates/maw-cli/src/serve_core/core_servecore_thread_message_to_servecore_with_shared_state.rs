fn servecore_thread_message(
    record: &ServecoreThreadRecord,
    role: &str,
    content: &str,
) -> Result<ServecoreThreadMessage, String> {
    servecore_thread_safe_text(role, "role")?;
    servecore_thread_safe_content(content)?;
    Ok(ServecoreThreadMessage {
        id: record.messages.last().map_or(1, |message| message.id + 1),
        role: role.to_owned(),
        content: content.to_owned(),
        created_at: servecore_thread_now(),
    })
}

fn servecore_thread_safe_text(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') {
        return Err(format!("thread {label} must be safe"));
    }
    if value.contains("..") || value.contains('/') || value.chars().any(char::is_control) {
        return Err(format!("thread {label} must be safe"));
    }
    Ok(())
}

fn servecore_thread_safe_content(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > SERVECORE_THREAD_MAX_TEXT_BYTES {
        return Err("thread content out of bounds".to_owned());
    }
    if value.bytes().any(|byte| byte == 0) {
        return Err("thread content contains nul".to_owned());
    }
    Ok(())
}

fn servecore_thread_id(value: &str) -> Result<u64, String> {
    if value.is_empty() || value == "--" || value.starts_with('-') {
        return Err("thread id must be numeric".to_owned());
    }
    if value.contains("..") || value.chars().any(char::is_control) {
        return Err("thread id must be numeric".to_owned());
    }
    if value.bytes().any(|byte| matches!(byte, b'/' | b'\\')) {
        return Err("thread id must be numeric".to_owned());
    }
    value
        .parse::<u64>()
        .map_err(|_| "thread id must be numeric".to_owned())
}

fn servecore_thread_path_inside(root: &Path, path: &Path) -> Result<(), String> {
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("thread path escapes root".to_owned());
    }
    if !path.starts_with(root) {
        return Err("thread path escapes root".to_owned());
    }
    Ok(())
}

fn servecore_thread_post_result(thread_id: u64, message_id: u64) -> ServecoreThreadPostResult {
    ServecoreThreadPostResult {
        thread_id,
        message_id,
        status: "ok".to_owned(),
    }
}

fn servecore_thread_now() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    format!("epoch-ms:{ms}")
}

#[derive(Clone, Debug, Default)]
pub struct ServecoreLifecycle {
    modules: Arc<Vec<ServecoreLifecycleModule>>,
    api_routers: Arc<BTreeSet<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServecoreLifecycleModule {
    pub name: String,
    pub weight: i32,
}

impl ServecoreLifecycle {
    #[must_use]
    pub fn servecore_from_profile(
        modules: Vec<ServecoreLifecycleModule>,
        api_routers: &[String],
    ) -> Self {
        let mut sorted = modules;
        sorted.sort_by(|left, right| {
            left.weight
                .cmp(&right.weight)
                .then(left.name.cmp(&right.name))
        });
        Self {
            modules: Arc::new(sorted),
            api_routers: Arc::new(api_routers.iter().cloned().collect()),
        }
    }

    #[must_use]
    pub fn servecore_enabled_modules(&self) -> Vec<String> {
        self.modules
            .iter()
            .filter(|module| self.api_routers.is_empty() || self.api_routers.contains(&module.name))
            .map(|module| module.name.clone())
            .collect()
    }
}

#[derive(Default)]
pub struct ServecoreRouteRegistry {
    seen: BTreeSet<ServecoreRouteKey>,
    routes: Vec<ServecoreRouteKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ServecoreRouteKey {
    method: Method,
    path: String,
}

impl ServecoreRouteRegistry {
    /// Register one HTTP route.
    ///
    /// # Errors
    /// Returns an error when the path is invalid or the method/path pair is already registered.
    pub fn servecore_register(&mut self, method: Method, path: &str) -> Result<(), String> {
        servecore_validate_path(path)?;
        let key = ServecoreRouteKey {
            method,
            path: path.to_owned(),
        };
        if !self.seen.insert(key.clone()) {
            return Err(format!(
                "serve-core: duplicate route {} {}",
                key.method, key.path
            ));
        }
        self.routes.push(key);
        Ok(())
    }

    #[must_use]
    pub fn servecore_routes(&self) -> &[ServecoreRouteKey] {
        &self.routes
    }
}

#[derive(Default)]
pub struct ServecoreWsRegistry {
    handlers: BTreeMap<String, ServecoreWsKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServecoreWsKind {
    Engine,
    Pty,
    Tmux,
}

impl ServecoreWsRegistry {
    /// Register one websocket upgrade path.
    ///
    /// # Errors
    /// Returns an error when the path is invalid or already registered.
    pub fn servecore_register_ws(&mut self, path: &str) -> Result<(), String> {
        self.servecore_register_ws_kind(path, ServecoreWsKind::Engine)
    }

    /// Register one websocket upgrade path with its stream kind.
    ///
    /// # Errors
    /// Returns an error when the path is invalid or already registered.
    pub fn servecore_register_ws_kind(
        &mut self,
        path: &str,
        kind: ServecoreWsKind,
    ) -> Result<(), String> {
        servecore_validate_path(path)?;
        if self.handlers.insert(path.to_owned(), kind).is_some() {
            return Err(format!("serve-core: duplicate ws route {path}"));
        }
        Ok(())
    }

    #[must_use]
    pub fn servecore_paths(&self) -> Vec<String> {
        self.handlers.keys().cloned().collect()
    }

    #[must_use]
    pub fn servecore_handlers(&self) -> Vec<(String, ServecoreWsKind)> {
        self.handlers
            .iter()
            .map(|(path, kind)| (path.clone(), *kind))
            .collect()
    }
}

pub fn servecore_with_shared_state<S>(router: Router<S>, state: ServecoreSharedState) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.layer(Extension(Arc::new(state)))
}

