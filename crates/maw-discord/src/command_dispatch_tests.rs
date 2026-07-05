use super::*;
use std::{future::Future, pin::Pin, sync::Mutex};

struct MockRest {
    calls: Mutex<Vec<String>>,
    responses: BTreeMap<String, DiscordHttpResponse>,
}

impl MockRest {
    fn new(responses: BTreeMap<String, DiscordHttpResponse>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            responses,
        }
    }
}

impl DiscordRest for MockRest {
    fn get_json<'a>(
        &'a self,
        path: &'a str,
        _token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<DiscordHttpResponse, String>> + Send + 'a>>
    {
        Box::pin(async move {
            assert!(path.starts_with('/'));
            assert!(!path.contains("://"));
            self.calls.lock().expect("calls").push(path.to_owned());
            self.responses
                .get(path)
                .cloned()
                .ok_or_else(|| format!("missing mock {path}"))
        })
    }
}

#[tokio::test]
async fn version_matches_maw_js_surface() {
    let env = DiscordEnv {
        home: PathBuf::from("/tmp/none"),
        ghq_root: PathBuf::from("/tmp/none"),
        hostname: "host.test".to_owned(),
    };
    let rest = MockRest::new(BTreeMap::new());
    let out = run_discord_command_with(&["version".to_owned()], &env, &rest).await;
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("maw discord v0.4.2"));
    assert!(out
        .stdout
        .contains("✓ guilds/channels/members/inventory <bot>  v0.4.2"));
}

#[test]
fn reqwest_client_rejects_non_host_relative_paths() {
    let client = ReqwestDiscordRest::new().expect("client");
    assert_eq!(
        client.url_for("/users/@me").expect("url"),
        "https://discord.com/api/v10/users/@me"
    );
    assert!(client.url_for("https://evil.test/users/@me").is_err());
    assert!(client.url_for("//evil.test/users/@me").is_err());
}
