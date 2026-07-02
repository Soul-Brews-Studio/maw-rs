const DISPATCH_119: &[DispatcherEntry] = &[DispatcherEntry { command: "tonk", handler: Handler::Sync(tonk_run_command) }];

const TONK_SIG: &str = "\n\n— Tonk 🌿 (AI · ไม่ใช่คน)";
const TONK_USAGE: &str = "🌿 maw tonk — Tonk Oracle (Active Student)\n\n  maw tonk say [name]    Hello, student style\n  maw tonk status        Identity + role + host\n  maw tonk gh ...        Reusable GitHub wrapper (Discussions)\n  maw tonk help          This view";
const TONK_GH_USAGE: &str = "🌿 maw tonk gh — reusable GitHub wrapper (Discussions)\n\n  maw tonk gh whoami\n  maw tonk gh discuss read   <owner/repo> <num>\n  maw tonk gh discuss create <owner/repo> --title \"<t>\" --file <path> [--category \"<name>\"]\n  maw tonk gh discuss post   <owner/repo> <num> --file <path> | --text \"<body>\"\n  maw tonk gh discuss reply  <owner/repo> <num> <commentId> --file <path> | --text \"<body>\"\n\n  (bodies auto-signed with Oracle attribution unless --raw)";
const TONK_FAKE_GH_ENV: &str = "MAW_RS_TONK_FAKE_GH";

#[derive(Debug, Clone, PartialEq, Eq)]
struct TonkGhOutput { stdout: String }

trait TonkGhRunner {
    fn tonk_gh(&mut self, args: &[String]) -> Result<TonkGhOutput, String>;
}

struct TonkSystemGhRunner;

impl TonkGhRunner for TonkSystemGhRunner {
    fn tonk_gh(&mut self, args: &[String]) -> Result<TonkGhOutput, String> {
        let output = std::process::Command::new("gh")
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|error| format!("gh exec failed: {error}"))?;
        if !output.status.success() {
            return Err(format!("gh exited {}", output.status.code().unwrap_or(1)));
        }
        Ok(TonkGhOutput { stdout: String::from_utf8_lossy(&output.stdout).trim().to_owned() })
    }
}

struct TonkEnvFakeGhRunner;

impl TonkGhRunner for TonkEnvFakeGhRunner {
    fn tonk_gh(&mut self, args: &[String]) -> Result<TonkGhOutput, String> {
        tonk_fake_gh(args).map(|stdout| TonkGhOutput { stdout })
    }
}

fn tonk_run_command(argv: &[String]) -> CliOutput {
    if std::env::var_os(TONK_FAKE_GH_ENV).is_some() {
        tonk_run_with_runner(argv, &mut TonkEnvFakeGhRunner)
    } else {
        tonk_run_with_runner(argv, &mut TonkSystemGhRunner)
    }
}

fn tonk_run_with_runner<R: TonkGhRunner>(argv: &[String], runner: &mut R) -> CliOutput {
    match tonk_render(argv, runner) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: format!("🌿 gh error: {}\n", tonk_redact_error(&message)), stderr: String::new() },
    }
}

fn tonk_render<R: TonkGhRunner>(argv: &[String], runner: &mut R) -> Result<String, String> {
    let sub = argv.first().map_or("help", String::as_str);
    if !matches!(sub, "gh" | "say" | "status") {
        return Ok(format!("{TONK_USAGE}\n"));
    }
    match sub {
        "gh" => tonk_gh_command(runner, argv),
        "say" => Ok(tonk_say(argv.get(1).map_or("world", String::as_str))),
        "status" => Ok(tonk_status()),
        _ => unreachable!("filtered above"),
    }
}

fn tonk_say(name: &str) -> String {
    format!("🌿 Tonk Oracle: Hello, {name}!\n   มาเรียน ถามมาก ฟังมาก พูดน้อย\n")
}

fn tonk_status() -> String {
    "🌿 Tonk Oracle — Active Student\n   role:   Student Oracle — ที่นี่มาเรียน ไม่ได้มาสอน\n   human:  TK (@tonkmac)\n   model:  Claude Opus 4.8 (1M context)\n   born:   2026-06-07\n   note:   AI — ไม่ใช่คน (Rule 6)\n".to_owned()
}

fn tonk_gh_command<R: TonkGhRunner>(runner: &mut R, args: &[String]) -> Result<String, String> {
    match args.get(1).map(String::as_str) {
        Some("whoami") => tonk_gh_whoami(runner),
        Some("discuss") => tonk_gh_discuss(runner, args),
        _ => Ok(format!("{TONK_GH_USAGE}\n")),
    }
}

fn tonk_gh_whoami<R: TonkGhRunner>(runner: &mut R) -> Result<String, String> {
    let out = runner.tonk_gh(&tonk_vec(&["api", "user", "--jq", ".login + \" (\" + (.name // \"?\") + \")\""]))?;
    Ok(format!("{}\n", out.stdout))
}

fn tonk_gh_discuss<R: TonkGhRunner>(runner: &mut R, args: &[String]) -> Result<String, String> {
    let verb = args.get(2).map_or("", String::as_str);
    let (owner, name) = tonk_split_repo(args.get(3).map_or("", String::as_str))?;
    match verb {
        "create" => tonk_discuss_create(runner, args, &owner, &name),
        "read" => tonk_discuss_read(runner, args, &owner, &name),
        "post" => tonk_discuss_post(runner, args, &owner, &name),
        "reply" => tonk_discuss_reply(runner, args, &owner, &name),
        _ => Ok(format!("{TONK_GH_USAGE}\n")),
    }
}

fn tonk_discuss_create<R: TonkGhRunner>(runner: &mut R, args: &[String], owner: &str, name: &str) -> Result<String, String> {
    let title = tonk_flag_value(args, "--title")?.ok_or_else(|| "need --title \"<thread title>\"".to_owned())?;
    tonk_validate_field("title", &title)?;
    let category = tonk_flag_value(args, "--category")?;
    if let Some(value) = category.as_deref() { tonk_validate_field("category", value)?; }
    let body = tonk_body_from_flags(args)?;
    let repo = tonk_repo_with_categories(runner, owner, name)?;
    let cat = tonk_pick_category(&repo.categories, category.as_deref())?;
    let mutation = "mutation($r:ID!,$c:ID!,$t:String!,$b:String!){createDiscussion(input:{repositoryId:$r,categoryId:$c,title:$t,body:$b}){discussion{url number}}}";
    let out = runner.tonk_gh(&tonk_graphql_args(&[("query", mutation), ("r", &repo.id), ("c", &cat.id), ("t", &title), ("b", &body)]))?;
    let json: serde_json::Value = serde_json::from_str(&out.stdout).map_err(|error| error.to_string())?;
    let discussion = &json["data"]["createDiscussion"]["discussion"];
    Ok(format!("✅ new thread #{} in [{}] → {}\n", tonk_json_display(&discussion["number"]), cat.name, tonk_json_str(&discussion["url"])?))
}

fn tonk_discuss_read<R: TonkGhRunner>(runner: &mut R, args: &[String], owner: &str, name: &str) -> Result<String, String> {
    let num = tonk_discussion_number(args.get(4).map_or("", String::as_str))?;
    let query = "query($o:String!,$n:String!,$d:Int!){repository(owner:$o,name:$n){discussion(number:$d){title body comments(first:50){nodes{author{login} body url}}}}}";
    let out = runner.tonk_gh(&tonk_graphql_args(&[("query", query), ("o", owner), ("n", name), ("d", &num.to_string())]))?;
    let json: serde_json::Value = serde_json::from_str(&out.stdout).map_err(|error| error.to_string())?;
    let disc = &json["data"]["repository"]["discussion"];
    if disc.is_null() { return Err(format!("discussion #{num} not found (private? no access?)")); }
    tonk_render_discussion(disc)
}

fn tonk_discuss_post<R: TonkGhRunner>(runner: &mut R, args: &[String], owner: &str, name: &str) -> Result<String, String> {
    let num = tonk_discussion_number(args.get(4).map_or("", String::as_str))?;
    let (id, title) = tonk_discussion_id(runner, owner, name, num)?;
    let body = tonk_body_from_flags(args)?;
    let mutation = "mutation($d:ID!,$b:String!){addDiscussionComment(input:{discussionId:$d,body:$b}){comment{url}}}";
    let out = runner.tonk_gh(&tonk_graphql_args(&[("query", mutation), ("d", &id), ("b", &body)]))?;
    let json: serde_json::Value = serde_json::from_str(&out.stdout).map_err(|error| error.to_string())?;
    Ok(format!("✅ posted to \"{title}\" → {}\n", tonk_json_str(&json["data"]["addDiscussionComment"]["comment"]["url"])?))
}

fn tonk_discuss_reply<R: TonkGhRunner>(runner: &mut R, args: &[String], owner: &str, name: &str) -> Result<String, String> {
    let num = tonk_discussion_number(args.get(4).map_or("", String::as_str))?;
    let reply_to = args.get(5).ok_or_else(|| "need <commentId> to reply to (get it from `discuss read --json`)".to_owned())?;
    tonk_validate_graphql_id("commentId", reply_to)?;
    let (id, _) = tonk_discussion_id(runner, owner, name, num)?;
    let body = tonk_body_from_flags(&args[1..])?;
    let mutation = "mutation($d:ID!,$r:ID!,$b:String!){addDiscussionComment(input:{discussionId:$d,replyToId:$r,body:$b}){comment{url}}}";
    let out = runner.tonk_gh(&tonk_graphql_args(&[("query", mutation), ("d", &id), ("r", reply_to), ("b", &body)]))?;
    let json: serde_json::Value = serde_json::from_str(&out.stdout).map_err(|error| error.to_string())?;
    Ok(format!("✅ replied → {}\n", tonk_json_str(&json["data"]["addDiscussionComment"]["comment"]["url"])?))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TonkRepoCategories { id: String, categories: Vec<TonkCategory> }

#[derive(Debug, Clone, PartialEq, Eq)]
struct TonkCategory { id: String, name: String }

fn tonk_repo_with_categories<R: TonkGhRunner>(runner: &mut R, owner: &str, name: &str) -> Result<TonkRepoCategories, String> {
    let query = "query($o:String!,$n:String!){repository(owner:$o,name:$n){id discussionCategories(first:25){nodes{id name}}}}";
    let out = runner.tonk_gh(&tonk_graphql_args(&[("query", query), ("o", owner), ("n", name)]))?;
    let json: serde_json::Value = serde_json::from_str(&out.stdout).map_err(|error| error.to_string())?;
    let repo = &json["data"]["repository"];
    if repo.is_null() || repo["id"].is_null() { return Err(format!("repo {owner}/{name} not found (private? no access?)")); }
    Ok(TonkRepoCategories { id: tonk_json_str(&repo["id"])? .to_owned(), categories: tonk_categories(repo)? })
}

fn tonk_categories(repo: &serde_json::Value) -> Result<Vec<TonkCategory>, String> {
    let nodes = repo["discussionCategories"]["nodes"].as_array().ok_or_else(|| "repo has no discussion categories (Discussions enabled?)".to_owned())?;
    if nodes.is_empty() { return Err("repo has no discussion categories (Discussions enabled?)".to_owned()); }
    nodes.iter().map(|node| Ok(TonkCategory { id: tonk_json_str(&node["id"])? .to_owned(), name: tonk_json_str(&node["name"])? .to_owned() })).collect()
}

fn tonk_pick_category(categories: &[TonkCategory], want: Option<&str>) -> Result<TonkCategory, String> {
    if let Some(want) = want.filter(|value| !value.is_empty()) {
        return categories.iter().find(|cat| cat.name.eq_ignore_ascii_case(want)).cloned().ok_or_else(|| format!("category \"{want}\" not found · available: {}", categories.iter().map(|cat| cat.name.as_str()).collect::<Vec<_>>().join(", ")));
    }
    categories.first().cloned().ok_or_else(|| "repo has no discussion categories (Discussions enabled?)".to_owned())
}

fn tonk_discussion_id<R: TonkGhRunner>(runner: &mut R, owner: &str, name: &str, num: u64) -> Result<(String, String), String> {
    let query = "query($o:String!,$n:String!,$d:Int!){repository(owner:$o,name:$n){discussion(number:$d){id title}}}";
    let out = runner.tonk_gh(&tonk_graphql_args(&[("query", query), ("o", owner), ("n", name), ("d", &num.to_string())]))?;
    let json: serde_json::Value = serde_json::from_str(&out.stdout).map_err(|error| error.to_string())?;
    let discussion = &json["data"]["repository"]["discussion"];
    if discussion.is_null() || discussion["id"].is_null() { return Err(format!("discussion #{num} not found in {owner}/{name} (private? no access?)")); }
    Ok((tonk_json_str(&discussion["id"])? .to_owned(), tonk_json_str(&discussion["title"])? .to_owned()))
}

fn tonk_render_discussion(disc: &serde_json::Value) -> Result<String, String> {
    let mut out = String::new();
    writeln!(out, "📋 {}\n", tonk_json_str(&disc["title"])?).expect("write string");
    writeln!(out, "{}", disc["body"].as_str().unwrap_or("(no body)")).expect("write string");
    let comments = disc["comments"]["nodes"].as_array().cloned().unwrap_or_default();
    writeln!(out, "\n── {} comment(s) ──", comments.len()).expect("write string");
    for comment in comments {
        writeln!(out, "\n👤 {}:\n{}", comment["author"]["login"].as_str().unwrap_or(""), comment["body"].as_str().unwrap_or("")).expect("write string");
    }
    Ok(out)
}

fn tonk_body_from_flags(args: &[String]) -> Result<String, String> {
    let file_idx = args.iter().position(|arg| arg == "--file");
    let text_idx = args.iter().position(|arg| arg == "--text");
    let mut body = if let Some(index) = file_idx {
        let path = args.get(index + 1).ok_or_else(|| "need --file <path> or --text \"<body>\"".to_owned())?;
        tonk_validate_file_arg(path)?;
        std::fs::read_to_string(path).map_err(|error| error.to_string())?
    } else if let Some(index) = text_idx {
        args.get(index + 1).cloned().ok_or_else(|| "need --file <path> or --text \"<body>\"".to_owned())?
    } else {
        return Err("need --file <path> or --text \"<body>\"".to_owned());
    };
    if !args.iter().any(|arg| arg == "--raw") && !body.contains("Tonk 🌿") { body.push_str(TONK_SIG); }
    Ok(body)
}

fn tonk_split_repo(slug: &str) -> Result<(String, String), String> {
    let mut parts = slug.split('/');
    let owner = parts.next().unwrap_or("");
    let name = parts.next().unwrap_or("");
    if owner.is_empty() || name.is_empty() || parts.next().is_some() { return Err(format!("bad repo \"{slug}\" — want owner/repo")); }
    tonk_validate_repo_part("owner", owner)?;
    tonk_validate_repo_part("repo", name)?;
    Ok((owner.to_owned(), name.to_owned()))
}

fn tonk_discussion_number(raw: &str) -> Result<u64, String> {
    tonk_validate_field("discussion", raw)?;
    raw.parse::<u64>().ok().filter(|number| *number > 0).ok_or_else(|| "need discussion number".to_owned())
}

fn tonk_flag_value(args: &[String], flag: &str) -> Result<Option<String>, String> {
    if let Some(index) = args.iter().position(|arg| arg == flag) {
        return args.get(index + 1).cloned().map(Some).ok_or_else(|| format!("missing {flag} value"));
    }
    Ok(None)
}

fn tonk_validate_repo_part(label: &str, value: &str) -> Result<(), String> {
    if !tonk_safe_atom(value) { return Err(format!("tonk gh: invalid {label} '{value}'")); }
    Ok(())
}

fn tonk_validate_graphql_id(label: &str, value: &str) -> Result<(), String> {
    if !tonk_safe_atom(value) { return Err(format!("tonk gh: invalid {label} '{value}'")); }
    Ok(())
}

fn tonk_validate_file_arg(value: &str) -> Result<(), String> {
    if tonk_empty_dash_control(value) || value.chars().any(char::is_whitespace) { return Err(format!("tonk gh: invalid --file '{value}'")); }
    Ok(())
}

fn tonk_validate_field(label: &str, value: &str) -> Result<(), String> {
    if tonk_empty_dash_control(value) { return Err(format!("tonk gh: invalid {label}")); }
    Ok(())
}

fn tonk_safe_atom(value: &str) -> bool {
    !tonk_empty_dash_control(value) && value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'))
}

fn tonk_empty_dash_control(value: &str) -> bool {
    value.is_empty() || value.trim() != value || value.starts_with('-') || value.chars().any(char::is_control)
}

fn tonk_graphql_args(fields: &[(&str, &str)]) -> Vec<String> {
    let mut args = vec!["api".to_owned(), "graphql".to_owned()];
    for (key, value) in fields {
        args.push(if *key == "query" { "-f".to_owned() } else { "-F".to_owned() });
        args.push(format!("{key}={value}"));
    }
    args
}

fn tonk_vec(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

fn tonk_json_str(value: &serde_json::Value) -> Result<&str, String> {
    value.as_str().ok_or_else(|| "unexpected gh JSON shape".to_owned())
}

fn tonk_json_display(value: &serde_json::Value) -> String {
    value.as_u64().map_or_else(|| value.to_string(), |number| number.to_string())
}

fn tonk_redact_error(message: &str) -> String {
    let lowered = message.to_ascii_lowercase();
    if lowered.contains("token") || lowered.contains("secret") || lowered.contains("authorization") { "gh failed (redacted)".to_owned() } else { message.to_owned() }
}

fn tonk_fake_gh(args: &[String]) -> Result<String, String> {
    if args == tonk_vec(&["api", "user", "--jq", ".login + \" (\" + (.name // \"?\") + \")\""]) { return Ok("tonk (Tonk Oracle)".to_owned()); }
    let query = tonk_fake_field(args, "query").unwrap_or_default();
    if query.contains("discussionCategories") { return Ok(r#"{"data":{"repository":{"id":"R_repo","discussionCategories":{"nodes":[{"id":"CAT_general","name":"General"},{"id":"CAT_workshop","name":"Workshop"}]}}}}"#.to_owned()); }
    if query.contains("discussion(number:$d){title body comments") { return Ok(r#"{"data":{"repository":{"discussion":{"title":"Tonk thread","body":"Welcome body","comments":{"nodes":[{"author":{"login":"alice"},"body":"first","url":"https://example.invalid/comment/1"}]}}}}}"#.to_owned()); }
    if query.contains("discussion(number:$d){id title}") { return Ok(r#"{"data":{"repository":{"discussion":{"id":"D_thread","title":"Tonk thread"}}}}"#.to_owned()); }
    if query.contains("createDiscussion") { return Ok(r#"{"data":{"createDiscussion":{"discussion":{"url":"https://example.invalid/discussions/7","number":7}}}}"#.to_owned()); }
    if query.contains("addDiscussionComment") { return Ok(r#"{"data":{"addDiscussionComment":{"comment":{"url":"https://example.invalid/comment/9"}}}}"#.to_owned()); }
    Err(format!("fake gh has no fixture for {}", args.join(" ")))
}

fn tonk_fake_field(args: &[String], name: &str) -> Option<String> {
    args.iter().find_map(|arg| arg.strip_prefix(&format!("{name}=")).map(str::to_owned))
}

#[cfg(test)]
mod tonk_tests {
    use super::*;

    #[derive(Default)]
    struct TonkFakeRunner { calls: Vec<Vec<String>> }

    impl TonkGhRunner for TonkFakeRunner {
        fn tonk_gh(&mut self, args: &[String]) -> Result<TonkGhOutput, String> {
            self.calls.push(args.to_vec());
            tonk_fake_gh(args).map(|stdout| TonkGhOutput { stdout })
        }
    }

    fn tonk_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn tonk_dispatch_fragment_owns_tonk() { assert_eq!(DISPATCH_119[0].command, "tonk"); }

    #[test]
    fn tonk_help_status_say_match_plugin_surface() {
        let mut fake = TonkFakeRunner::default();
        assert!(tonk_run_with_runner(&[], &mut fake).stdout.contains("maw tonk gh ..."));
        assert!(tonk_run_with_runner(&tonk_args(&["status"]), &mut fake).stdout.contains("Active Student"));
        assert!(tonk_run_with_runner(&tonk_args(&["say", "TK"]), &mut fake).stdout.contains("Hello, TK!"));
    }

    #[test]
    fn tonk_mutations_are_hermetic_fake_runner_paths() {
        let mut fake = TonkFakeRunner::default();
        let create = tonk_run_with_runner(&tonk_args(&["gh", "discuss", "create", "tonkmac/maw-rs", "--title", "Hello", "--category", "Workshop", "--text", "Body"]), &mut fake);
        assert_eq!(create.code, 0);
        assert!(create.stdout.contains("new thread #7"));
        let post = tonk_run_with_runner(&tonk_args(&["gh", "discuss", "post", "tonkmac/maw-rs", "7", "--text", "Body"]), &mut fake);
        assert_eq!(post.code, 0);
        let reply = tonk_run_with_runner(&tonk_args(&["gh", "discuss", "reply", "tonkmac/maw-rs", "7", "COMMENT_1", "--text", "Body"]), &mut fake);
        assert_eq!(reply.code, 0);
        assert!(fake.calls.iter().any(|call| call.iter().any(|arg| arg.contains("createDiscussion"))));
        assert_eq!(fake.calls.iter().filter(|call| call.iter().any(|arg| arg.contains("addDiscussionComment"))).count(), 2);
    }

    #[test]
    fn tonk_gh_uses_argv_vec_without_shell_tokens() {
        let mut fake = TonkFakeRunner::default();
        let out = tonk_run_with_runner(&tonk_args(&["gh", "discuss", "post", "tonkmac/maw-rs", "7", "--text", "Body"]), &mut fake);
        assert_eq!(out.code, 0);
        for call in fake.calls {
            assert_eq!(call[0], "api");
            assert!(!call.iter().any(|arg| matches!(arg.as_str(), "sh" | "-c")));
        }
    }

    #[test]
    fn tonk_rejects_injected_repo_title_category_discussion() {
        let mut fake = TonkFakeRunner::default();
        assert_ne!(tonk_run_with_runner(&tonk_args(&["gh", "discuss", "read", "-bad/repo", "1"]), &mut fake).code, 0);
        assert_ne!(tonk_run_with_runner(&tonk_args(&["gh", "discuss", "read", "org/repo", "-1"]), &mut fake).code, 0);
        assert_ne!(tonk_run_with_runner(&tonk_args(&["gh", "discuss", "create", "org/repo", "--title", "-bad", "--text", "Body"]), &mut fake).code, 0);
        assert_ne!(tonk_run_with_runner(&tonk_args(&["gh", "discuss", "create", "org/repo", "--title", "Ok", "--category", "bad\ncat", "--text", "Body"]), &mut fake).code, 0);
    }

    #[test]
    fn tonk_redacts_secret_errors() {
        assert_eq!(tonk_redact_error("stderr had token ghp_123"), "gh failed (redacted)");
    }
}
