use maw_cli::run_cli;

#[test]
fn fuzzy_distance_plan_cli_matches_maw_js_cases() {
    for (name, left, right, expected) in [
        ("oracle to oracl", "oracle", "oracl", 1),
        ("hey to hek", "hey", "hek", 1),
        ("empty to empty", "", "", 0),
        ("abc to empty", "abc", "", 3),
        ("empty to abc", "", "abc", 3),
        ("exact plugin", "plugin", "plugin", 0),
        ("kitten sitting", "kitten", "sitting", 3),
    ] {
        let output = run_cli(&[
            "fuzzy".to_owned(),
            "distance".to_owned(),
            left.to_owned(),
            right.to_owned(),
            "--plan-json".to_owned(),
        ]);
        assert_eq!(output.code, 0, "{name}: {}", output.stderr);
        let json: serde_json::Value = serde_json::from_str(&output.stdout)
            .unwrap_or_else(|error| panic!("{name} invalid json: {error}\n{}", output.stdout));
        assert_eq!(json["command"], "fuzzy", "{name}");
        assert_eq!(json["kind"], "distance", "{name}");
        assert_eq!(json["left"], left, "{name}");
        assert_eq!(json["right"], right, "{name}");
        assert_eq!(json["distance"], expected, "{name}");
    }
}

#[test]
fn fuzzy_match_plan_cli_matches_maw_js_cases() {
    let pool = ["oracle", "plugin", "peek", "hey", "ping", "find", "fleet"];

    let top = fuzzy_match_json("oracl", &pool, 3, 3);
    assert_eq!(
        matches_from(&top).first().map(String::as_str),
        Some("oracle")
    );
    assert!(matches_from(&top).len() <= 3);

    let hey = fuzzy_match_json("hek", &pool, 3, 3);
    assert!(matches_from(&hey).contains(&"hey".to_owned()));

    assert_eq!(
        matches_from(&fuzzy_match_json("", &pool, 3, 3)),
        Vec::<String>::new()
    );
    assert_eq!(
        matches_from(&fuzzy_match_json("xyz-not-a-command", &pool, 3, 3)),
        Vec::<String>::new()
    );
    assert!(matches_from(&fuzzy_match_json("ORACL", &pool, 3, 3)).contains(&"oracle".to_owned()));

    let deduped = matches_from(&fuzzy_match_json(
        "oracle",
        &["oracle", "oracle", "oracl"],
        3,
        3,
    ));
    assert_eq!(deduped.iter().filter(|name| *name == "oracle").count(), 1);

    assert_eq!(
        matches_from(&fuzzy_match_json("cat", &["bat", "dat", "aat"], 3, 3)),
        vec!["aat", "bat", "dat"]
    );

    let case_distinct = matches_from(&fuzzy_match_json("oracle", &["Oracle", "oracle"], 3, 3));
    assert_eq!(case_distinct.len(), 2);
    assert!(case_distinct.contains(&"Oracle".to_owned()));
    assert!(case_distinct.contains(&"oracle".to_owned()));

    assert_eq!(
        matches_from(&fuzzy_match_json("oracle", &["oracle"], 0, 3)),
        Vec::<String>::new()
    );
}

fn fuzzy_match_json(
    input: &str,
    candidates: &[&str],
    max_results: usize,
    max_distance: usize,
) -> serde_json::Value {
    let mut argv = vec![
        "fuzzy".to_owned(),
        "match".to_owned(),
        input.to_owned(),
        "--max-results".to_owned(),
        max_results.to_string(),
        "--max-distance".to_owned(),
        max_distance.to_string(),
        "--plan-json".to_owned(),
    ];
    for candidate in candidates {
        argv.push("--candidate".to_owned());
        argv.push((*candidate).to_owned());
    }
    let output = run_cli(&argv);
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid fuzzy match json: {error}\n{}", output.stdout))
}

fn matches_from(json: &serde_json::Value) -> Vec<String> {
    assert_eq!(json["command"], "fuzzy");
    assert_eq!(json["kind"], "match");
    json["matches"]
        .as_array()
        .expect("matches array")
        .iter()
        .map(|value| value.as_str().expect("match string").to_owned())
        .collect()
}

#[test]
fn fuzzy_plan_rejects_bad_arguments() {
    let bad_limit = run_cli(&[
        "fuzzy".to_owned(),
        "match".to_owned(),
        "oracle".to_owned(),
        "--max-results".to_owned(),
        "many".to_owned(),
    ]);
    assert_eq!(bad_limit.code, 2);
    assert!(bad_limit.stderr.contains("--max-results must be"));

    let misplaced_candidate = run_cli(&[
        "fuzzy".to_owned(),
        "--candidate".to_owned(),
        "oracle".to_owned(),
    ]);
    assert_eq!(misplaced_candidate.code, 2);
    assert!(misplaced_candidate
        .stderr
        .contains("--candidate requires match"));
}
