impl MawWasmHost {
    fn peer_send(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<PeerSendArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let key = match self.secret_ref(args.peer_key_ref.as_deref()) {
            Ok(key) => key,
            Err(err) => return err,
        };
        let url = match Url::parse(&args.peer_url) {
            Ok(url) => url,
            Err(error) => {
                return HostResult::err(
                    HostErrorCode::InvalidArgs,
                    format!("invalid peerUrl: {error}"),
                )
            }
        };
        let host = url.host_str().unwrap_or_default();
        if let Err(err) = self
            .caps
            .require("peer", "send", None)
            .and_then(|_| self.caps.require(url.scheme(), "", Some(host)))
        {
            let _ = err;
        }
        if !self.caps.contains("peer", "send", None)
            || !self.caps.contains("net", url.scheme(), Some(host))
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "peer send capability denied",
            );
        }
        let req = PeerSendRequest {
            peer_url: args.peer_url,
            target: args.target,
            text: args.text,
            inbox: args.inbox,
            from: args.from,
            peer_key: key,
            timestamp: args.timestamp.unwrap_or(0),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ())
            .ok();
        let Some(rt) = rt else {
            return HostResult::err(HostErrorCode::NetworkError, "tokio runtime failed");
        };
        let client = match ReqwestHttpTransportIo::new(self.http_timeout_ms) {
            Ok(client) => client,
            Err(error) => return HostResult::err(HostErrorCode::NetworkError, error),
        };
        match rt.block_on(client.send_peer(&req)) {
            Ok(resp) => HostResult::ok(
                json!({"ok": resp.ok, "status": resp.status, "state": resp.state, "target": resp.target, "lastLine": resp.last_line, "error": resp.error}),
            ),
            Err(error) => HostResult::err(HostErrorCode::NetworkError, error),
        }
    }

    fn peer_wake(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<PeerWakeArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let key = match self.secret_ref(args.peer_key_ref.as_deref()) {
            Ok(key) => key,
            Err(err) => return err,
        };
        let url = Url::parse(&args.peer_url).map_err(|_| ()).ok();
        let Some(url) = url else {
            return HostResult::err(HostErrorCode::InvalidArgs, "invalid peerUrl");
        };
        let host = url.host_str().unwrap_or_default();
        if !self.caps.contains("peer", "wake", None)
            || !self.caps.contains("net", url.scheme(), Some(host))
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "peer wake capability denied",
            );
        }
        let req = PeerWakeRequest {
            peer_url: args.peer_url,
            target: args.target,
            task: args.task,
            from: args.from,
            peer_key: key,
            timestamp: args.timestamp.unwrap_or(0),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ())
            .ok();
        let Some(rt) = rt else {
            return HostResult::err(HostErrorCode::NetworkError, "tokio runtime failed");
        };
        let client = match ReqwestHttpTransportIo::new(self.http_timeout_ms) {
            Ok(client) => client,
            Err(error) => return HostResult::err(HostErrorCode::NetworkError, error),
        };
        match rt.block_on(client.wake_peer(&req)) {
            Ok(resp) => HostResult::ok(
                json!({"ok": resp.ok, "status": resp.status, "target": resp.target, "error": resp.error}),
            ),
            Err(error) => HostResult::err(HostErrorCode::NetworkError, error),
        }
    }

}
