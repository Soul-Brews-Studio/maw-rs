#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ExecRunArgs {
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    env: Option<BTreeMap<String, String>>,
    stdin: Option<String>,
    timeout_ms: Option<u64>,
    #[serde(default)]
    allow_non_zero: bool,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FsReadArgs {
    path: String,
    encoding: Option<String>,
    max_bytes: Option<u64>,
    offset: Option<u64>,
}
#[derive(Debug, Deserialize)]
struct FsPathArgs {
    #[serde(alias = "target")]
    path: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FsRemoveArgs {
    path: String,
    recursive: Option<bool>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FsWriteArgs {
    path: String,
    content: String,
    encoding: Option<String>,
    mode: Option<String>,
    mkdirp: Option<bool>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FsListArgs {
    path: String,
    recursive: Option<bool>,
    max_entries: Option<usize>,
    offset: Option<usize>,
    cursor: Option<String>,
    include_dirs: Option<bool>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigGetArgs {
    key: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathsGetArgs {
    name: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigSetArgs {
    key: String,
    value: Value,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsentReadArgs {
    view: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HttpArgs {
    method: String,
    url: String,
    headers: Option<BTreeMap<String, String>>,
    body: Option<String>,
    timeout_ms: Option<u64>,
    follow_redirects: Option<bool>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalserverArgs {
    method: String,
    path: Option<String>,
    url: Option<String>,
    headers: Option<BTreeMap<String, String>>,
    body: Option<String>,
    timeout_ms: Option<u64>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NetFetchArgs {
    endpoint: String,
    method: Option<String>,
    path: String,
    query: Option<BTreeMap<String, String>>,
    body: Option<String>,
    timeout_ms: Option<u64>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeerSendArgs {
    peer_url: String,
    target: String,
    text: String,
    inbox: Option<bool>,
    from: String,
    federation_token_ref: Option<String>,
    peer_key_ref: Option<String>,
    timestamp: Option<i64>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeerWakeArgs {
    peer_url: String,
    target: String,
    task: Option<String>,
    from: String,
    federation_token_ref: Option<String>,
    peer_key_ref: Option<String>,
    timestamp: Option<i64>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TmuxCaptureArgs {
    target: String,
    lines: Option<u32>,
    strip_ansi: Option<bool>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TmuxSendArgs {
    target: String,
    keys: Vec<String>,
    literal: Option<bool>,
    enter: Option<bool>,
    allow_destructive: Option<bool>,
    force: Option<bool>,
    allow_ai_pane: Option<bool>,
}
#[derive(Debug, Deserialize)]
struct TmuxRunArgs {
    target: String,
    text: String,
}
#[derive(Debug, Deserialize)]
struct TmuxEnterArgs {
    target: String,
    count: Option<u32>,
}
#[derive(Debug, Deserialize)]
struct TmuxTagsWriteArgs {
    target: String,
    title: Option<String>,
    meta: Option<BTreeMap<String, String>>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SshExecArgs {
    host: String,
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
    stdin: Option<String>,
    timeout_ms: Option<u64>,
}
#[derive(Debug, Deserialize)]
struct SshTmuxCaptureArgs {
    host: String,
    target: String,
    lines: Option<u32>,
}
#[derive(Debug, Deserialize)]
struct SshTmuxSendArgs {
    host: String,
    target: String,
    keys: Vec<String>,
}
