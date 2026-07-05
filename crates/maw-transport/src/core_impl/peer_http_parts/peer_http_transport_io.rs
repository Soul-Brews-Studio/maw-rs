impl HttpTransportIo for ReqwestHttpTransportIo {
    fn list_local_sessions(&mut self) -> Result<Vec<TmuxTransportSession>, String> {
        Ok(Vec::new())
    }

    fn get_all_sessions(
        &mut self,
        _local_sessions: &[TmuxTransportSession],
    ) -> Result<Vec<TransportSession>, String> {
        Ok(Vec::new())
    }

    fn find_target_window(&mut self, _sessions: &[TransportSession], _query: &str) -> Option<String> {
        None
    }

    fn send_peer_keys(
        &mut self,
        _source: &str,
        _target: &str,
        _message: &str,
    ) -> Result<bool, String> {
        Err("sync send_peer_keys is not supported by the async reqwest transport".to_owned())
    }

    fn post_peer_feed(
        &mut self,
        _url: &str,
        _method: &str,
        _body: &str,
        _timeout_ms: u64,
    ) -> Result<HttpPostResult, String> {
        Err("sync post_peer_feed is not supported by the async reqwest transport".to_owned())
    }

    fn timeout_for(&self, _transport: &str) -> u64 {
        self.timeout_ms
    }
}

#[derive(Debug, Deserialize)]
struct PeerSendWireResponse {
    #[serde(default)]
    ok: Option<bool>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default, rename = "lastLine")]
    last_line: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PeerWakeWireResponse {
    #[serde(default)]
    ok: Option<bool>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Build the exact v26.6.13 `/api/send` JSON body: target, text, and optional inbox.
///
/// # Errors
///
/// Returns JSON serialization errors for non-representable strings.
pub fn peer_send_body(target: &str, text: &str, inbox: Option<bool>) -> Result<String, String> {
    let target = serde_json::to_string(target).map_err(|error| error.to_string())?;
    let text = serde_json::to_string(text).map_err(|error| error.to_string())?;
    Ok(match inbox {
        Some(inbox) => format!(r#"{{"target":{target},"text":{text},"inbox":{inbox}}}"#),
        None => format!(r#"{{"target":{target},"text":{text}}}"#),
    })
}

#[cfg(test)]
mod tests {
    use super::{peer_send_body, peer_wake_body};

    #[test]
    fn peer_send_body_keeps_wire_field_order_and_optional_inbox() {
        assert_eq!(
            peer_send_body("remote-oracle", "E1 signed capture", Some(true)).unwrap(),
            r#"{"target":"remote-oracle","text":"E1 signed capture","inbox":true}"#
        );
        assert_eq!(
            peer_send_body("remote-oracle", "hello", None).unwrap(),
            r#"{"target":"remote-oracle","text":"hello"}"#
        );
    }

    #[test]
    fn peer_wake_body_keeps_wire_field_order_and_optional_task() {
        assert_eq!(
            peer_wake_body("remote-oracle", None).unwrap(),
            r#"{"target":"remote-oracle"}"#
        );
        assert_eq!(
            peer_wake_body("remote-oracle", Some("fix issue")).unwrap(),
            r#"{"target":"remote-oracle","task":"fix issue"}"#
        );
    }
}

/// Build the exact v26.6.13 `/api/wake` JSON body: target and optional task.
///
/// # Errors
///
/// Returns JSON serialization errors for non-representable strings.
pub fn peer_wake_body(target: &str, task: Option<&str>) -> Result<String, String> {
    let target = serde_json::to_string(target).map_err(|error| error.to_string())?;
    Ok(match task {
        Some(task) => {
            let task = serde_json::to_string(task).map_err(|error| error.to_string())?;
            format!(r#"{{"target":{target},"task":{task}}}"#)
        }
        None => format!(r#"{{"target":{target}}}"#),
    })
}
