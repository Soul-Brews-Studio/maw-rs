impl<Io> TmuxLocalTransport<Io> {
    #[must_use]
    pub const fn new(io: Io) -> Self {
        Self {
            io,
            connected: false,
            message_handlers: 0,
            presence_handlers: 0,
            feed_handlers: 0,
        }
    }

    #[must_use]
    pub const fn connected(&self) -> bool {
        self.connected
    }

    pub const fn connect(&mut self) {
        self.connected = true;
    }

    pub const fn disconnect(&mut self) {
        self.connected = false;
    }

    pub const fn on_message(&mut self) {
        self.message_handlers += 1;
    }

    pub const fn on_presence(&mut self) {
        self.presence_handlers += 1;
    }

    pub const fn on_feed(&mut self) {
        self.feed_handlers += 1;
    }

    #[must_use]
    pub const fn handler_counts(&self) -> (usize, usize, usize) {
        (
            self.message_handlers,
            self.presence_handlers,
            self.feed_handlers,
        )
    }

    pub const fn publish_presence(&self) {}

    pub const fn publish_feed(&self) {}
}

impl<Io> TmuxLocalTransport<Io>
where
    Io: TmuxTransportIo,
{
    #[must_use]
    pub fn name(&self) -> &'static str {
        "tmux"
    }

    #[must_use]
    pub fn can_reach(&self, target: &TransportTarget) -> bool {
        is_local_host(target.host.as_deref())
    }

    /// Send using explicit `tmux_target` or by scanning sessions and resolving the oracle name.
    pub fn send(&mut self, target: &TransportTarget, message: &str) -> bool {
        if !self.can_reach(target) {
            return false;
        }
        let tmux_target = if let Some(tmux_target) = &target.tmux_target {
            tmux_target.clone()
        } else {
            let Ok(sessions) = self.io.list_tmux_sessions() else {
                return false;
            };
            let Some(resolved) = self.io.find_tmux_window(&sessions, &target.oracle) else {
                return false;
            };
            resolved
        };
        self.io.send_to_tmux(&tmux_target, message).is_ok()
    }

    #[must_use]
    pub const fn io(&self) -> &Io {
        &self.io
    }
}
