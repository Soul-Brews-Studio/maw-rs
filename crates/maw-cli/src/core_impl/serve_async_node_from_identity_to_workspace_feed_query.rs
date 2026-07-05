fn node_from_identity(value: &str) -> Option<String> {
    let normalized = normalize_from_identity(value)?;
    node_from_normalized_identity(&normalized)
}

#[derive(Default, Deserialize)]
struct SendBody {
    target: Option<String>,
    text: Option<String>,
    inbox: Option<bool>,
    attachments: Option<Vec<String>>,
}

#[derive(Default, Deserialize)]
struct FeedQuery {
    limit: Option<usize>,
}

#[derive(Default)]
struct RequestReplyStore {
    entries: HashMap<String, RequestEntry>,
    next_id: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestEntry {
    correlation_id: String,
    from: String,
    to: String,
    target: String,
    message: String,
    status: String,
    reply: Option<String>,
    data: Option<Value>,
}

enum ReplyResult {
    Ok,
    NotFound,
    AlreadyReplied,
}

impl RequestReplyStore {
    fn create(&mut self, body: RequestCreateBody) -> RequestEntry {
        self.next_id = self.next_id.saturating_add(1);
        let correlation_id = format!("req-{}", self.next_id);
        let to = body.to.split(':').next().unwrap_or(&body.to).to_owned();
        let entry = RequestEntry {
            correlation_id: correlation_id.clone(),
            from: body.from.unwrap_or_else(|| "external".to_owned()),
            to,
            target: body.to,
            message: body.message,
            status: "delivered".to_owned(),
            reply: None,
            data: None,
        };
        self.entries.insert(correlation_id, entry.clone());
        entry
    }

    fn list(&self, oracle: Option<&str>, status: Option<&str>) -> Vec<RequestEntry> {
        let mut entries = self.entries.values().cloned().collect::<Vec<_>>();
        entries.sort_by(|a, b| a.correlation_id.cmp(&b.correlation_id));
        entries
            .into_iter()
            .filter(|entry| oracle.is_none_or(|oracle| entry.to == oracle))
            .filter(|entry| status.is_none_or(|status| entry.status == status))
            .collect()
    }

    fn reply(&mut self, correlation_id: &str, reply: String, data: Option<Value>) -> ReplyResult {
        let Some(entry) = self.entries.get_mut(correlation_id) else {
            return ReplyResult::NotFound;
        };
        if entry.status == "replied" {
            return ReplyResult::AlreadyReplied;
        }
        "replied".clone_into(&mut entry.status);
        entry.reply = Some(reply);
        entry.data = data;
        ReplyResult::Ok
    }
}

fn with_request_store<T>(state: &ServeState, f: impl FnOnce(&mut RequestReplyStore) -> T) -> T {
    match state.requests.lock() {
        Ok(mut store) => f(&mut store),
        Err(poisoned) => {
            let mut store = poisoned.into_inner();
            f(&mut store)
        }
    }
}

#[derive(Deserialize)]
struct MessageLedgerQuery {
    limit: Option<usize>,
    from: Option<String>,
    to: Option<String>,
    direction: Option<String>,
    state: Option<String>,
    q: Option<String>,
    json: Option<String>,
}

#[derive(Deserialize)]
struct RequestListQuery {
    oracle: Option<String>,
    status: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrustAddBody {
    sender: String,
    target: String,
    peer_key: String,
}

#[derive(Deserialize)]
struct TrustRevokeBody {
    sender: String,
    target: String,
    yes: Option<bool>,
}

#[derive(Default, Deserialize)]
struct RequestCreateBody {
    to: String,
    message: String,
    from: Option<String>,
}

#[derive(Deserialize)]
struct ReplyBody {
    reply: String,
    data: Option<Value>,
}

#[derive(Default)]
struct WorkspaceStore {
    workspaces: HashMap<String, Workspace>,
    join_codes: HashMap<String, String>,
}

struct Workspace {
    id: String,
    name: String,
    token: String,
    join_code: String,
    join_code_expires_at: u64,
    created_at: u64,
    nodes: HashSet<String>,
    agents: HashMap<String, WorkspaceAgent>,
    feed: Vec<Value>,
}

impl Workspace {
    fn new(name: String, node_id: String) -> Self {
        let created_at = unix_millis();
        let mut nodes = HashSet::new();
        nodes.insert(node_id);
        Self {
            id: format!("ws-{}", random_hex(8)),
            name,
            token: random_hex(32),
            join_code: random_hex(3),
            join_code_expires_at: created_at.saturating_add(15 * 60 * 1_000),
            created_at,
            nodes,
            agents: HashMap::new(),
            feed: Vec::new(),
        }
    }
}

struct WorkspaceAgent {
    name: String,
    node_id: String,
    status: Option<String>,
    capabilities: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceCreateBody {
    name: String,
    node_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceJoinBody {
    code: String,
    node_id: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceAgentBody {
    name: String,
    node_id: String,
    status: Option<String>,
    capabilities: Option<Value>,
}

#[derive(Default, Deserialize)]
struct WorkspaceMessageBody {
    from: String,
    text: String,
    to: Option<String>,
}

#[derive(Deserialize)]
struct WorkspaceFeedQuery {
    limit: Option<usize>,
}

