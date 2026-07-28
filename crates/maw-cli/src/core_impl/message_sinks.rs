// Everywhere a delivered message gets written down.
//
// One send fans out to several records: the audit jsonl, the maw-log jsonl, an
// MQTT topic and a sqlite ledger. They are behind one trait and one registry so
// a send either records everywhere or the failure is visible -- and so the two
// jsonl writers can be held byte-compatible with maw-js, which other tools still
// parse.

struct MessageSinkRecord<'a> {
    command: &'a str,
    audit_args: &'a [String],
    normalized_from: Option<&'a str>,
    sender_oracle: &'a str,
    to: &'a str,
    msg: &'a str,
    route: &'a str,
    signature: Option<&'a MessageSignature>,
}

#[derive(Debug)]
struct MessageSignature;

trait MessageSink {
    fn record(&self, record: &MessageSinkRecord<'_>);
}

struct AuditJsonlSink;

struct MawLogJsonlSink;

struct MqttMessageSink;

struct MessageLedgerSqliteSink;

fn message_sink_registry() -> Vec<Box<dyn MessageSink>> {
    vec![
        Box::new(AuditJsonlSink),
        Box::new(MawLogJsonlSink),
        Box::new(MqttMessageSink),
        Box::new(MessageLedgerSqliteSink),
    ]
}

impl MessageSink for AuditJsonlSink {
    fn record(&self, record: &MessageSinkRecord<'_>) {
        send_write_js_audit_record(record.command, record.audit_args);
    }
}

impl MessageSink for MawLogJsonlSink {
    fn record(&self, record: &MessageSinkRecord<'_>) {
        if let Some(from) = record.normalized_from {
            send_write_js_maw_log_record(from, record.to, record.msg, record.route);
        }
    }
}

impl MessageSink for MqttMessageSink {
    fn record(&self, record: &MessageSinkRecord<'_>) {
        send_publish_mqtt_message(record);
    }
}

impl MessageSink for MessageLedgerSqliteSink {
    fn record(&self, record: &MessageSinkRecord<'_>) {
        if let Some(from) = record.normalized_from {
            send_write_message_ledger_record(record, from);
        }
    }
}

fn send_write_js_audit_record(command: &str, audit_args: &[String]) {
    let row = serde_json::json!({
        "ts": cli_dispatch_now_iso(),
        "cmd": command,
        "args": audit_args,
        "user": send_audit_user(),
        "pid": std::process::id(),
    });
    send_append_jsonl(&audit_jsonl_path(&real_xdg_env()), &row);
}

fn send_write_js_maw_log_record(from: &str, to: &str, msg: &str, route: &str) {
    let row = serde_json::json!({
        "ts": cli_dispatch_now_iso(),
        "from": from,
        "to": to,
        "msg": msg.chars().take(500).collect::<String>(),
        "host": send_hostname(),
        "route": route,
    });
    send_append_jsonl(&maw_data_path(&real_xdg_env(), &["maw-log.jsonl"]), &row);
}

fn send_append_jsonl(path: &std::path::Path, row: &serde_json::Value) {
    let _ = append_jsonl_atomic(path, row);
}

fn send_audit_user() -> String {
    std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_else(|_| "unknown".to_owned())
}

fn send_hostname() -> String {
    std::env::var("HOSTNAME").ok().filter(|value| !value.is_empty()).unwrap_or_else(|| "unknown".to_owned())
}

fn send_publish_mqtt_message(record: &MessageSinkRecord<'_>) {
    let Some(from) = record.normalized_from else { return; };
    let value = merged_config_value_for_env(&real_xdg_env());
    let broker = value
        .get("mqttPublish")
        .and_then(|mqtt| mqtt.get("broker"))
        .and_then(serde_json::Value::as_str)
        .filter(|broker| !broker.is_empty());
    let Some(broker) = broker else { return; };
    let Some(node) = value.get("node").and_then(serde_json::Value::as_str) else { return; };
    let payload = serde_json::json!({
        "event": "message",
        "oracle": record.sender_oracle,
        "host": node,
        "message": record.msg,
        "ts": cli_dispatch_now_millis(),
        "data": {"from": from, "to": record.to, "route": record.route},
    })
    .to_string();
    for topic in [
        format!("maw/v1/oracle/{}/feed", record.sender_oracle),
        format!("maw/v1/node/{node}/feed"),
    ] {
        let _ = std::process::Command::new("mosquitto_pub")
            .args(["-L", broker, "-t", &topic, "-m", &payload])
            .output();
    }
}

fn send_write_message_ledger_record(record: &MessageSinkRecord<'_>, from: &str) {
    if std::env::var("MAW_MESSAGE_LEDGER_DISABLE").ok().as_deref() == Some("1") {
        return;
    }
    let path = maw_data_path(&real_xdg_env(), &["message-ledger.sqlite"]);
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() { return; }
    } else {
        return;
    }
    let ts = cli_dispatch_now_iso();
    let id = format!("{}:{}:{}:{}", ts, from, record.to, record.route);
    let sql = format!(
        "{} INSERT OR REPLACE INTO messages (id, ts, direction, state, channel, route, from_id, to_id, target, peer_url, text, error, last_line, signed) VALUES ({}, {}, 'outbound', 'delivered', 'hey', {}, {}, {}, {}, NULL, {}, NULL, NULL, {});",
        send_message_ledger_schema_sql(),
        send_sqlite_quote(&id),
        send_sqlite_quote(&ts),
        send_sqlite_quote(record.route),
        send_sqlite_quote(from),
        send_sqlite_quote(record.to),
        send_sqlite_quote(record.to),
        send_sqlite_quote(record.msg),
        i32::from(record.signature.is_some()),
    );
    let _ = std::process::Command::new("sqlite3").arg(path).arg(sql).output();
}

fn send_message_ledger_schema_sql() -> &'static str {
    "CREATE TABLE IF NOT EXISTS messages (id TEXT PRIMARY KEY, ts TEXT NOT NULL, direction TEXT NOT NULL, state TEXT NOT NULL, channel TEXT NOT NULL, route TEXT NOT NULL, from_id TEXT NOT NULL, to_id TEXT NOT NULL, target TEXT, peer_url TEXT, text TEXT NOT NULL, error TEXT, last_line TEXT, signed INTEGER NOT NULL DEFAULT 0); CREATE INDEX IF NOT EXISTS idx_messages_ts ON messages(ts); CREATE INDEX IF NOT EXISTS idx_messages_from ON messages(from_id); CREATE INDEX IF NOT EXISTS idx_messages_to ON messages(to_id); CREATE INDEX IF NOT EXISTS idx_messages_direction ON messages(direction); CREATE INDEX IF NOT EXISTS idx_messages_state ON messages(state); PRAGMA busy_timeout=1000;"
}

fn send_sqlite_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
