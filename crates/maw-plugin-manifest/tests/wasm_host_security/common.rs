use std::fs::{create_dir_all, read_to_string, write};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{parse_manifest, HostErrorCode, MawWasmHost, PluginManifest};
use serde_json::{json, Value};

fn temp(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-wasm-host-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("temp dir");
    std::fs::canonicalize(&dir).unwrap_or(dir)
}

fn manifest(dir: &Path, caps: &[&str]) -> PluginManifest {
    write(dir.join("plugin.wasm"), b"\0asm\x01\0\0\0").expect("wasm");
    parse_manifest(
        &json!({
            "name": "secure-plugin",
            "version": "1.0.0",
            "sdk": "*",
            "entry": { "kind": "wasm", "path": "plugin.wasm", "export": "handle" },
            "capabilities": caps,
        })
        .to_string(),
        dir,
    )
    .expect("manifest")
}

fn host(dir: &Path, caps: &[&str]) -> MawWasmHost {
    let manifest = manifest(dir, caps);
    wasm_host_from_manifest(dir, manifest)
}

fn endpoint_host(dir: &Path, caps: &[&str], endpoints: Value) -> MawWasmHost {
    endpoint_secret_host(dir, caps, endpoints, None)
}

fn endpoint_secret_host(
    dir: &Path,
    caps: &[&str],
    endpoints: Value,
    secrets: Option<Value>,
) -> MawWasmHost {
    write(dir.join("plugin.wasm"), b"\0asm\x01\0\0\0").expect("wasm");
    let mut raw = json!({
        "name": "secure-plugin",
        "version": "1.0.0",
        "sdk": "*",
        "entry": { "kind": "wasm", "path": "plugin.wasm", "export": "handle" },
        "capabilities": caps,
    });
    raw.as_object_mut()
        .expect("manifest object")
        .insert("endpoints".to_owned(), endpoints);
    if let Some(secrets) = secrets {
        raw.as_object_mut()
            .expect("manifest object")
            .insert("secrets".to_owned(), secrets);
    }
    let manifest = parse_manifest(&raw.to_string(), dir).expect("manifest");
    wasm_host_from_manifest(dir, manifest)
}

fn wasm_host_from_manifest(dir: &Path, manifest: PluginManifest) -> MawWasmHost {
    let loaded = maw_plugin_manifest::LoadedPlugin {
        manifest,
        dir: dir.to_path_buf(),
        wasm_path: dir.join("plugin.wasm"),
        entry_path: None,
        wasm_export: "handle".to_owned(),
        kind: maw_plugin_manifest::LoadedPluginKind::Wasm,
        disabled: false,
    };
    MawWasmHost::new(&loaded).with_fs_root("sandbox", dir)
}

fn call(host: &MawWasmHost, name: &str, args: &Value) -> Value {
    serde_json::from_str(&host.handle_json(name, &args.to_string())).expect("host result json")
}

fn running_as_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .is_some_and(|uid| uid.trim() == "0")
}

fn spawn_localserver_once(body: &'static str) -> String {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind localserver");
    let addr = listener.local_addr().expect("localserver addr");
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept localserver request");
        let mut buf = [0_u8; 1024];
        let _ = stream.read(&mut buf);
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write localserver response");
    });
    format!("http://127.0.0.1:{}", addr.port())
}
