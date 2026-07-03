use maw_plugin_manifest::{
    parse_api, parse_artifact, parse_capabilities, parse_capability_namespaces, parse_cli,
    parse_cron, parse_dependencies, parse_engine, parse_hooks, parse_module, parse_target,
    parse_tier, parse_transport, ApiMethod, CliFlagKind, HookPolicy, PluginTarget, PluginTier,
};
use serde_json::json;

#[test]
fn parse_cli_rejects_malformed_cli_shapes_and_preserves_optional_fields() {
    assert_eq!(parse_cli(&json!({})).expect("missing cli is valid"), None);

    let parsed = parse_cli(&json!({
        "cli": {
            "command": "demo",
            "aliases": ["d"],
            "help": "hi",
            "flags": { "verbose": "boolean" }
        }
    }))
    .expect("valid cli")
    .expect("cli present");
    assert_eq!(parsed.command, "demo");
    assert_eq!(parsed.aliases, Some(vec!["d".to_owned()]));
    assert_eq!(parsed.help, Some("hi".to_owned()));
    assert_eq!(
        parsed.flags.expect("flags present").get("verbose"),
        Some(&CliFlagKind::Boolean)
    );

    expect_error(&json!({ "cli": [] }), "plugin.json: cli must be an object");
    expect_error(
        &json!({ "cli": { "command": "" } }),
        "plugin.json: cli.command must be a non-empty string",
    );
    expect_error(
        &json!({ "cli": { "command": "x", "aliases": [1] } }),
        "plugin.json: cli.aliases must be an array of strings",
    );
    expect_error(
        &json!({ "cli": { "command": "x", "flags": [] } }),
        "plugin.json: cli.flags must be an object",
    );
    expect_error(
        &json!({ "cli": { "command": "x", "flags": { "bad": "object" } } }),
        "plugin.json: cli.flags[\"bad\"] must be \"boolean\", \"string\", or \"number\"",
    );
}

#[test]
fn parse_api_rejects_malformed_api_objects() {
    assert_eq!(parse_api(&json!({})).expect("missing api is valid"), None);

    let parsed = parse_api(&json!({
        "api": { "path": "/api/demo", "methods": ["GET", "POST"] }
    }))
    .expect("valid api")
    .expect("api present");
    assert_eq!(parsed.path, "/api/demo");
    assert_eq!(parsed.methods, vec![ApiMethod::Get, ApiMethod::Post]);

    assert_eq!(ApiMethod::Get.as_str(), "GET");
    assert_eq!(CliFlagKind::Number.as_str(), "number");

    expect_api_error(&json!({ "api": [] }), "plugin.json: api must be an object");
    expect_api_error(
        &json!({ "api": { "path": "", "methods": ["GET"] } }),
        "plugin.json: api.path must be a non-empty string",
    );
    expect_api_error(
        &json!({ "api": { "path": "/api/demo", "methods": ["PUT"] } }),
        "plugin.json: api.methods must be an array",
    );
}

#[test]
fn parse_hooks_validates_lifecycle_hook_branches() {
    assert_eq!(
        parse_hooks(&json!({})).expect("missing hooks is valid"),
        None
    );

    let parsed = parse_hooks(&json!({
        "hooks": {
            "wake": { "script": "wake.ts", "handler": "onWake", "ensures": ["db"], "policy": "best-effort" },
            "sleep": {},
            "serve": {}
        }
    }))
    .expect("valid hooks")
    .expect("hooks present");
    let wake = parsed.wake.expect("wake present");
    assert_eq!(wake.script, Some("wake.ts".to_owned()));
    assert_eq!(wake.handler, Some("onWake".to_owned()));
    assert_eq!(wake.ensures, Some(vec!["db".to_owned()]));
    assert_eq!(wake.policy, Some(HookPolicy::BestEffort));
    assert_eq!(HookPolicy::FailFast.as_str(), "fail-fast");
    assert!(parsed.sleep.is_some());
    assert!(parsed.serve.is_some());

    expect_hooks_error(
        &json!({ "hooks": { "wake": [] } }),
        "plugin.json: hooks.wake must be an object",
    );
    expect_hooks_error(
        &json!({ "hooks": { "wake": { "script": "" } } }),
        "plugin.json: hooks.wake.script must be a non-empty string",
    );
    expect_hooks_error(
        &json!({ "hooks": { "sleep": { "handler": "" } } }),
        "plugin.json: hooks.sleep.handler must be a non-empty string",
    );
    expect_hooks_error(
        &json!({ "hooks": { "serve": { "ensures": [""] } } }),
        "plugin.json: hooks.serve.ensures must be an array of non-empty strings",
    );
    expect_hooks_error(
        &json!({ "hooks": { "wake": { "policy": "hard" } } }),
        "plugin.json: hooks.wake.policy must be",
    );
    expect_hooks_error(
        &json!({ "hooks": [] }),
        "plugin.json: hooks must be an object",
    );
    expect_hooks_error(
        &json!({ "hooks": { "on": [1] } }),
        "plugin.json: hooks.on must be an array of strings",
    );
    expect_hooks_error(
        &json!({ "hooks": { "gate": [1] } }),
        "plugin.json: hooks.gate must be an array of strings",
    );
    expect_hooks_error(
        &json!({ "hooks": { "filter": "not-array" } }),
        "plugin.json: hooks.filter must be an array of strings",
    );
}

#[test]
fn parse_cron_module_and_transport_reject_malformed_sections() {
    assert_eq!(parse_cron(&json!({})).expect("missing cron is valid"), None);
    let cron = parse_cron(&json!({ "cron": { "schedule": "* * * * *", "handler": "tick" } }))
        .expect("valid cron")
        .expect("cron present");
    assert_eq!(cron.schedule, "* * * * *");
    assert_eq!(cron.handler, Some("tick".to_owned()));
    expect_cron_error(
        &json!({ "cron": [] }),
        "plugin.json: cron must be an object",
    );
    expect_cron_error(
        &json!({ "cron": { "schedule": "" } }),
        "plugin.json: cron.schedule must be a non-empty string",
    );
    expect_cron_error(
        &json!({ "cron": { "schedule": "* * * * *", "handler": 1 } }),
        "plugin.json: cron.handler must be a string",
    );

    assert_eq!(
        parse_module(&json!({})).expect("missing module is valid"),
        None
    );
    let module = parse_module(&json!({ "module": { "exports": ["thing"], "path": "./mod.ts" } }))
        .expect("valid module")
        .expect("module present");
    assert_eq!(module.exports, vec!["thing".to_owned()]);
    assert_eq!(module.path, "./mod.ts");
    expect_module_error(
        &json!({ "module": [] }),
        "plugin.json: module must be an object",
    );
    expect_module_error(
        &json!({ "module": { "exports": [], "path": "./mod.ts" } }),
        "plugin.json: module.exports must be a non-empty array of strings",
    );
    expect_module_error(
        &json!({ "module": { "exports": ["thing"], "path": "" } }),
        "plugin.json: module.path must be a non-empty string",
    );

    assert_eq!(
        parse_transport(&json!({})).expect("missing transport is valid"),
        None
    );
    let transport = parse_transport(&json!({ "transport": { "peer": false } }))
        .expect("valid transport")
        .expect("transport present");
    assert_eq!(transport.peer, Some(false));
    expect_transport_error(
        &json!({ "transport": [] }),
        "plugin.json: transport must be an object",
    );
    expect_transport_error(
        &json!({ "transport": { "peer": "yes" } }),
        "plugin.json: transport.peer must be a boolean",
    );
}

#[test]
fn parse_engine_rejects_malformed_serve_process_metadata() {
    assert_eq!(
        parse_engine(&json!({})).expect("missing engine is valid"),
        None
    );
    assert_eq!(
        parse_engine(&json!({ "engine": {} }))
            .expect("valid empty engine")
            .expect("engine present")
            .serve,
        None
    );

    let engine = parse_engine(&json!({
        "engine": {
            "serve": {
                "command": "bun run serve",
                "prefix": "/api/demo",
                "health": "/health",
                "events": ["MessageSend"],
                "eventPath": "/events"
            }
        }
    }))
    .expect("valid engine")
    .expect("engine present");
    let serve = engine.serve.expect("serve present");
    assert_eq!(serve.command, Some("bun run serve".to_owned()));
    assert_eq!(serve.prefix, Some("/api/demo".to_owned()));
    assert_eq!(serve.health, Some("/health".to_owned()));
    assert_eq!(serve.events, Some(vec!["MessageSend".to_owned()]));
    assert_eq!(serve.event_path, Some("/events".to_owned()));

    expect_engine_error(
        &json!({ "engine": [] }),
        "plugin.json: engine must be an object",
    );
    expect_engine_error(
        &json!({ "engine": { "serve": [] } }),
        "plugin.json: engine.serve must be an object",
    );
    expect_engine_error(
        &json!({ "engine": { "serve": { "command": "" } } }),
        "plugin.json: engine.serve.command must be a non-empty string",
    );
    expect_engine_error(
        &json!({ "engine": { "serve": { "prefix": "/demo" } } }),
        "plugin.json: engine.serve.prefix must start with /api/",
    );
    expect_engine_error(
        &json!({ "engine": { "serve": { "health": "health" } } }),
        "plugin.json: engine.serve.health must be an absolute path",
    );
    expect_engine_error(
        &json!({ "engine": { "serve": { "eventPath": "events" } } }),
        "plugin.json: engine.serve.eventPath must be an absolute path",
    );
    expect_engine_error(
        &json!({ "engine": { "serve": { "events": [""] } } }),
        "plugin.json: engine.serve.events must be an array of non-empty strings",
    );
}

#[test]
fn parse_dependencies_artifact_and_tier_cover_compact_and_invalid_shapes() {
    assert_eq!(
        parse_dependencies(&json!({})).expect("missing dependencies is valid"),
        None
    );
    let deps = parse_dependencies(&json!({ "dependencies": ["trace", "dig"] }))
        .expect("valid compact dependencies")
        .expect("dependencies present");
    assert_eq!(
        deps.plugins,
        Some(vec!["trace".to_owned(), "dig".to_owned()])
    );
    let empty_deps = parse_dependencies(&json!({ "dependencies": {} }))
        .expect("valid empty dependencies")
        .expect("dependencies present");
    assert_eq!(empty_deps.plugins, None);
    expect_dependencies_error(
        &json!({ "dependencies": "trace" }),
        "plugin.json: dependencies must be an object or array of plugin names",
    );
    expect_dependencies_error(
        &json!({ "dependencies": { "plugins": ["Bad Name"] } }),
        "plugin.json: dependencies.plugins must be an array of plugin names",
    );

    assert_eq!(
        parse_artifact(&json!({})).expect("missing artifact is valid"),
        None
    );
    let artifact =
        parse_artifact(&json!({ "artifact": { "path": "dist/index.js", "sha256": null } }))
            .expect("valid artifact null sha")
            .expect("artifact present");
    assert_eq!(artifact.path, "dist/index.js");
    assert_eq!(artifact.sha256, None);
    let artifact =
        parse_artifact(&json!({ "artifact": { "path": "dist/index.js", "sha256": "abc" } }))
            .expect("valid artifact sha")
            .expect("artifact present");
    assert_eq!(artifact.sha256, Some("abc".to_owned()));
    expect_artifact_error(
        &json!({ "artifact": [] }),
        "plugin.json: artifact must be an object",
    );
    expect_artifact_error(
        &json!({ "artifact": { "path": "" } }),
        "plugin.json: artifact.path must be a non-empty string",
    );
    expect_artifact_error(
        &json!({ "artifact": { "path": "dist/index.js", "sha256": 1 } }),
        "plugin.json: artifact.sha256 must be a string or null",
    );

    assert_eq!(parse_tier(&json!({})).expect("missing tier is valid"), None);
    assert_eq!(
        parse_tier(&json!({ "tier": "core" })).expect("valid tier"),
        Some(PluginTier::Core)
    );
    assert_eq!(PluginTier::Extra.as_str(), "extra");
    expect_tier_error(&json!({ "tier": "primary" }), "plugin.json: tier must be");
}

#[test]
fn target_and_capability_validators_cover_valid_invalid_and_warning_branches() {
    assert_eq!(
        parse_target(&json!({})).expect("missing target is valid"),
        None
    );
    assert_eq!(
        parse_target(&json!({ "target": "js" })).expect("valid target"),
        Some(PluginTarget::Js)
    );
    assert_eq!(
        parse_target(&json!({ "target": "wasm" })).expect("valid wasm target"),
        Some(PluginTarget::Wasm)
    );
    assert_eq!(PluginTarget::Js.as_str(), "js");
    assert_eq!(PluginTarget::Wasm.as_str(), "wasm");
    expect_target_error(
        &json!({ "target": 1 }),
        "plugin.json: target must be a string",
    );
    expect_target_error(
        &json!({ "target": "python" }),
        "plugin.json: unknown target",
    );

    assert_eq!(
        parse_capability_namespaces(&json!({})).expect("missing namespaces is valid"),
        None
    );
    assert_eq!(
        parse_capability_namespaces(
            &json!({ "capabilityNamespaces": ["messages", "messages", "storage"] })
        )
        .expect("valid namespaces"),
        Some(vec!["messages".to_owned(), "storage".to_owned()])
    );
    expect_capability_namespaces_error(
        &json!({ "capabilityNamespaces": ["Bad Name"] }),
        "plugin.json: capabilityNamespaces must be an array of slug strings",
    );

    assert_eq!(
        parse_capabilities(&json!({}), &[]).expect("missing capabilities is valid"),
        None
    );
    let capabilities = parse_capabilities(
        &json!({ "capabilities": ["sdk:identity", "messages:ledger"] }),
        &["messages"],
    )
    .expect("valid capabilities")
    .expect("capabilities present");
    assert_eq!(
        capabilities.capabilities,
        vec!["sdk:identity".to_owned(), "messages:ledger".to_owned()]
    );
    assert!(capabilities.warnings.is_empty());
    expect_capabilities_error(
        &json!({ "capabilities": [1] }),
        "plugin.json: capabilities must be an array of strings",
    );

    let capabilities = parse_capabilities(&json!({ "capabilities": ["unknown:thing"] }), &[])
        .expect("unknown namespaces warn")
        .expect("capabilities present");
    assert_eq!(capabilities.capabilities, vec!["unknown:thing".to_owned()]);
    assert!(capabilities
        .warnings
        .join("\n")
        .contains("unknown capability namespace"));
}

#[test]
fn transport_engine_and_namespace_defaults_match_maw_js() {
    assert_eq!(
        parse_transport(&json!({ "transport": {} }))
            .expect("empty transport")
            .expect("transport present")
            .peer,
        None
    );
    assert_eq!(
        parse_transport(&json!({ "transport": { "peer": true } }))
            .expect("peer true")
            .expect("transport present")
            .peer,
        Some(true)
    );

    let empty_serve = parse_engine(&json!({ "engine": { "serve": {} } }))
        .expect("empty serve")
        .expect("engine present")
        .serve
        .expect("serve present");
    assert_eq!(empty_serve.command, None);
    let partial_serve = parse_engine(
        &json!({ "engine": { "serve": { "prefix": "/api/plugin", "health": "/health" } } }),
    )
    .expect("partial serve")
    .expect("engine present")
    .serve
    .expect("serve present");
    assert_eq!(partial_serve.prefix, Some("/api/plugin".to_owned()));
    assert_eq!(partial_serve.health, Some("/health".to_owned()));
    assert_eq!(
        parse_engine(&json!({ "engine": { "serve": { "events": [] } } }))
            .expect("empty events")
            .expect("engine present")
            .serve
            .expect("serve present")
            .events,
        Some(Vec::new())
    );
    expect_engine_error(
        &json!({ "engine": { "serve": { "prefix": 1 } } }),
        "plugin.json: engine.serve.prefix must start with /api/",
    );
    expect_engine_error(
        &json!({ "engine": { "serve": { "events": "MessageSend" } } }),
        "plugin.json: engine.serve.events must be an array of non-empty strings",
    );

    assert_eq!(
        parse_capability_namespaces(&json!({ "capabilityNamespaces": [] }))
            .expect("empty namespaces"),
        Some(Vec::new())
    );
    assert_eq!(
        parse_capability_namespaces(
            &json!({ "capabilityNamespaces": ["custom", "custom", "x-1"] })
        )
        .expect("dedup namespaces"),
        Some(vec!["custom".to_owned(), "x-1".to_owned()])
    );
    expect_capability_namespaces_error(
        &json!({ "capabilityNamespaces": "custom" }),
        "plugin.json: capabilityNamespaces must be an array of slug strings",
    );
    expect_capability_namespaces_error(
        &json!({ "capabilityNamespaces": ["Custom"] }),
        "plugin.json: capabilityNamespaces must be an array of slug strings",
    );
}

#[test]
fn capability_dependency_artifact_tier_and_late_hook_defaults_match_maw_js() {
    let caps = parse_capabilities(
        &json!({ "capabilities": ["sdk", "sdk:identity", "custom", "custom:thing"] }),
        &["custom"],
    )
    .expect("known and declared capabilities")
    .expect("capabilities present");
    assert!(caps.warnings.is_empty());
    assert_eq!(
        caps.capabilities,
        vec![
            "sdk".to_owned(),
            "sdk:identity".to_owned(),
            "custom".to_owned(),
            "custom:thing".to_owned()
        ]
    );
    let caps = parse_capabilities(
        &json!({ "capabilities": ["mystery", "unknown:value"] }),
        &["custom"],
    )
    .expect("unknown capability warnings")
    .expect("capabilities present");
    assert_eq!(caps.warnings.len(), 2);
    assert!(caps.warnings[0].contains("unknown capability namespace \"mystery\" in \"mystery\""));
    assert!(
        caps.warnings[1].contains("unknown capability namespace \"unknown\" in \"unknown:value\"")
    );
    assert!(caps.warnings[1].contains("custom"));

    assert_eq!(
        parse_dependencies(&json!({ "dependencies": [] }))
            .expect("empty compact deps")
            .expect("dependencies present")
            .plugins,
        Some(Vec::new())
    );
    assert_eq!(
        parse_dependencies(&json!({ "dependencies": { "plugins": ["trace", "x-1"] } }))
            .expect("object deps")
            .expect("dependencies present")
            .plugins,
        Some(vec!["trace".to_owned(), "x-1".to_owned()])
    );
    expect_artifact_error(
        &json!({ "artifact": { "path": "dist/plugin.js" } }),
        "plugin.json: artifact.sha256 must be a string or null",
    );
    assert_eq!(
        parse_tier(&json!({ "tier": "standard" })).expect("standard tier"),
        Some(PluginTier::Standard)
    );
    expect_tier_error(
        &json!({ "tier": 1 }),
        "plugin.json: tier must be \"core\", \"standard\", or \"extra\" (got 1)",
    );

    let hooks = parse_hooks(&json!({ "hooks": { "gate": [], "filter": ["Clean"], "on": ["MessageSend"], "late": ["After"] } }))
        .expect("default hook arrays")
        .expect("hooks present");
    assert_eq!(hooks.gate, Some(Vec::new()));
    assert_eq!(hooks.filter, Some(vec!["Clean".to_owned()]));
    assert_eq!(hooks.on, Some(vec!["MessageSend".to_owned()]));
    assert_eq!(hooks.late, Some(vec!["After".to_owned()]));
    expect_hooks_error(
        &json!({ "hooks": { "late": [1] } }),
        "plugin.json: hooks.late must be an array of strings",
    );
}

