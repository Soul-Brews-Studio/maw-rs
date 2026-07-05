async fn servecore_ws_handle_frame(
    socket: &mut WebSocket,
    state: &ServecoreSharedState,
    kind: ServecoreWsKind,
    target: Option<&str>,
    config: &modules::ws::WsConfig,
    frame: Message,
) -> bool {
    match frame {
        Message::Text(text) => {
            if text.len() > config.max_frame_bytes {
                return servecore_ws_send(socket, Message::Close(None), config.send_timeout)
                    .await
                    .is_ok();
            }
            if let Some(reply) = state.engine.servecore_ws_text(kind, &text, target) {
                return servecore_ws_send(socket, Message::Text(reply), config.send_timeout)
                    .await
                    .is_ok();
            }
            true
        }
        Message::Binary(bytes) => {
            if bytes.len() > config.max_frame_bytes {
                return servecore_ws_send(socket, Message::Close(None), config.send_timeout)
                    .await
                    .is_ok();
            }
            if let Some(reply) = state.engine.servecore_ws_binary(kind, &bytes, target) {
                return servecore_ws_send(socket, Message::Binary(reply), config.send_timeout)
                    .await
                    .is_ok();
            }
            true
        }
        Message::Ping(bytes) => {
            servecore_ws_send(socket, Message::Pong(bytes), config.send_timeout)
                .await
                .is_ok()
        }
        Message::Pong(_) => true,
        Message::Close(frame) => {
            let _ = servecore_ws_send(socket, Message::Close(frame), config.send_timeout).await;
            false
        }
    }
}

async fn servecore_ws_send(
    socket: &mut WebSocket,
    message: Message,
    timeout: Duration,
) -> Result<(), ()> {
    tokio::time::timeout(timeout, socket.send(message))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}

fn servecore_ws_target(query: Option<&str>) -> Option<&str> {
    query?
        .split('&')
        .filter_map(|part| part.split_once('='))
        .find_map(|(key, value)| (key == "target" || key == "session").then_some(value))
}

fn servecore_ws_connection_guard(max_connections: usize) -> Option<ServecoreWsConnectionGuard> {
    let mut current = SERVECORE_WS_CONNECTIONS.load(Ordering::Relaxed);
    loop {
        if current >= max_connections {
            return None;
        }
        match SERVECORE_WS_CONNECTIONS.compare_exchange_weak(
            current,
            current + 1,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => return Some(ServecoreWsConnectionGuard),
            Err(actual) => current = actual,
        }
    }
}

struct ServecoreWsConnectionGuard;

impl Drop for ServecoreWsConnectionGuard {
    fn drop(&mut self) {
        SERVECORE_WS_CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod thread_store_tests {
    use super::*;

    fn threadstore_temp(name: &str) -> PathBuf {
        let mut root = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        root.push(format!(
            "maw-rs-core-threadstore-{name}-{}-{nanos}",
            std::process::id()
        ));
        root
    }

    #[test]
    fn servecore_thread_store_create_append_read_list() {
        let store = ServecoreThreadStore::servecore_with_root(threadstore_temp("crud"));
        let id = store
            .create_thread(&["channel:alpha".to_owned()])
            .expect("create");
        let first = store.append(id, "claude", "hello").expect("append");
        let second = store.append(id, "claude", "again").expect("append2");
        assert_eq!(first.thread_id, id);
        assert_eq!(first.message_id, 1);
        assert_eq!(second.message_id, 2);
        let record = store.read(id).expect("read");
        assert_eq!(record.thread.title, "channel:alpha");
        assert_eq!(record.messages.len(), 2);
        let list = store.list().expect("list");
        assert_eq!(list[0].id, id);
    }

    #[test]
    fn servecore_thread_store_rejects_traversal_and_injection() {
        let store = ServecoreThreadStore::servecore_with_root(threadstore_temp("guard"));
        assert!(store.create_thread(&["../bad".to_owned()]).is_err());
        assert!(store.create_thread(&["-bad".to_owned()]).is_err());
        assert!(servecore_thread_id("../../1").is_err());
        assert!(servecore_thread_id("-1").is_err());
        assert!(servecore_thread_id("1\n").is_err());
    }

    #[test]
    fn servecore_thread_store_concurrent_append_no_corrupt() {
        let store = ServecoreThreadStore::servecore_with_root(threadstore_temp("concurrent"));
        let id = store
            .create_thread(&["channel:alpha".to_owned()])
            .expect("create");
        let handles = (0..8)
            .map(|index| {
                let store = store.clone();
                std::thread::spawn(move || {
                    let text = format!("message-{index}");
                    store.append(id, "claude", &text).expect("append");
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().expect("join");
        }
        let record = store.read(id).expect("read");
        assert_eq!(record.messages.len(), 8);
        let ids = record
            .messages
            .iter()
            .map(|message| message.id)
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), 8);
    }
}

#[cfg(test)]
mod tests {
    include!("core_tests/01_fake_orchestrator_to_servecore_orchestrato_8ee176.rs");
    include!("core_tests/02_servecore_simple_work_5f45e7_to_servecore_advanced_f57706.rs");
    include!("core_tests/03_servecore_advanced_sh_632737_to_servecore_rejects_t_4d3245.rs");
    include!("core_tests/04_servecore_spawn_test_server_to_servecore_auth_test_app.rs");
    include!("core_tests/05_servecore_auth_request_to_servecore_ws_rejects_850010.rs");
    include!("core_tests/06_servecore_ws_idle_tim_05fbde.rs");
}
