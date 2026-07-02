fn parse_fuzzy_plan_args(argv: &[String]) -> Result<(bool, FuzzyPlanAction), String> {
    let mut plan_json = false;
    let mut action = None;
    let mut max_results = 3;
    let mut max_distance = 3;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "distance" | "--distance" => {
                let Some(left) = argv.get(index + 1) else {
                    return Err("fuzzy: missing distance left value".to_owned());
                };
                let Some(right) = argv.get(index + 2) else {
                    return Err("fuzzy: missing distance right value".to_owned());
                };
                action = Some(FuzzyPlanAction::Distance {
                    left: left.to_owned(),
                    right: right.to_owned(),
                });
                index += 2;
            }
            "match" | "--match" => {
                let Some(input) = argv.get(index + 1) else {
                    return Err("fuzzy: missing match input".to_owned());
                };
                action = Some(FuzzyPlanAction::Match {
                    input: input.to_owned(),
                    candidates: Vec::new(),
                    max_results,
                    max_distance,
                });
                index += 1;
            }
            "--candidate" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("fuzzy: missing --candidate value".to_owned());
                };
                action = append_fuzzy_candidate(action, value)?;
                index += 1;
            }
            "--max-results" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("fuzzy: missing --max-results value".to_owned());
                };
                max_results = parse_usize_arg(value, "fuzzy: --max-results")?;
                action = update_fuzzy_limits(action, max_results, max_distance);
                index += 1;
            }
            "--max-distance" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("fuzzy: missing --max-distance value".to_owned());
                };
                max_distance = parse_usize_arg(value, "fuzzy: --max-distance")?;
                action = update_fuzzy_limits(action, max_results, max_distance);
                index += 1;
            }
            arg => return Err(format!("fuzzy: unknown argument {arg}")),
        }
        index += 1;
    }

    action.map_or_else(
        || Err("fuzzy: expected distance or match".to_owned()),
        |action| Ok((plan_json, action)),
    )
}

fn append_fuzzy_candidate(
    action: Option<FuzzyPlanAction>,
    value: &str,
) -> Result<Option<FuzzyPlanAction>, String> {
    match action {
        Some(FuzzyPlanAction::Match {
            input,
            mut candidates,
            max_results,
            max_distance,
        }) => {
            candidates.push(value.to_owned());
            Ok(Some(FuzzyPlanAction::Match {
                input,
                candidates,
                max_results,
                max_distance,
            }))
        }
        _ => Err("fuzzy: --candidate requires match".to_owned()),
    }
}

fn update_fuzzy_limits(
    action: Option<FuzzyPlanAction>,
    max_results: usize,
    max_distance: usize,
) -> Option<FuzzyPlanAction> {
    match action {
        Some(FuzzyPlanAction::Match {
            input, candidates, ..
        }) => Some(FuzzyPlanAction::Match {
            input,
            candidates,
            max_results,
            max_distance,
        }),
        action => action,
    }
}

fn parse_usize_arg(value: &str, name: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("{name} must be a non-negative integer"))
}

fn render_fuzzy_distance(plan_json: bool, left: &str, right: &str) -> CliOutput {
    let distance = fuzzy_distance(left, right);
    CliOutput {
        code: 0,
        stdout: if plan_json {
            format!(
                "{{\"command\":\"fuzzy\",\"kind\":\"distance\",\"left\":{},\"right\":{},\"distance\":{distance}}}\n",
                json_string(left),
                json_string(right)
            )
        } else {
            format!("{distance}\n")
        },
        stderr: String::new(),
    }
}

fn render_fuzzy_match(
    plan_json: bool,
    input: &str,
    candidates: &[String],
    max_results: usize,
    max_distance: usize,
) -> CliOutput {
    let candidate_refs: Vec<&str> = candidates.iter().map(String::as_str).collect();
    let matches = fuzzy_match(input, &candidate_refs, max_results, max_distance);
    CliOutput {
        code: 0,
        stdout: if plan_json {
            format!(
                "{{\"command\":\"fuzzy\",\"kind\":\"match\",\"input\":{},\"candidates\":{},\"maxResults\":{max_results},\"maxDistance\":{max_distance},\"matches\":{}}}\n",
                json_string(input),
                json_string_array(candidates),
                json_string_array(&matches)
            )
        } else {
            format!("{}\n", matches.join("\n"))
        },
        stderr: String::new(),
    }
}

enum FuzzyPlanAction {
    Distance {
        left: String,
        right: String,
    },
    Match {
        input: String,
        candidates: Vec<String>,
        max_results: usize,
        max_distance: usize,
    },
}

fn run_fuzzy_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            arg => {
                return fuzzy_constants_usage_error(&format!("fuzzy constants: unknown arg {arg}"))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_fuzzy_constants_json()
        } else {
            "fuzzy constants algorithm=levenshtein distance-unit=utf16-code-unit defaults=max-results:3,max-distance:3\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn render_fuzzy_constants_json() -> String {
    r#"{"command":"fuzzy","action":"constants","actions":["distance","match"],"algorithm":"levenshtein","distanceUnit":"utf16-code-unit","caseHandling":"case-insensitive scoring, original output preserved","dedupe":"exact candidate string before scoring","defaultMaxResults":3,"defaultMaxDistance":3,"emptyInput":"no matches","zeroMaxResults":"no matches","emptyCandidate":"ignored","sortOrder":["distance asc","candidate lexicographic asc"],"limitFlags":["max-results","max-distance"]}
"#
    .to_owned()
}

fn fuzzy_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", fuzzy_constants_usage()),
    }
}

fn fuzzy_constants_usage() -> &'static str {
    "usage: maw-rs fuzzy constants [--plan-json]"
}

fn fuzzy_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs fuzzy distance <left> <right> [--plan-json]\n       maw-rs fuzzy match <input> [--candidate <candidate>]... [--max-results <n>] [--max-distance <n>] [--plan-json]\n       maw-rs fuzzy constants [--plan-json]\n"
        ),
    }
}

fn run_identity_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_identity_constants_plan(&argv[1..]);
    }

    let (plan_json, action) = match parse_identity_plan_args(argv) {
        Ok(parsed) => parsed,
        Err(message) => return identity_usage_error(&message),
    };
    match action {
        IdentityPlanAction::SessionName { oracle, slot } => {
            let input = CanonicalSessionNameInput { oracle, slot };
            match canonical_session_name(&input) {
                Ok(canonical) => CliOutput {
                    code: 0,
                    stdout: if plan_json {
                        render_identity_session_plan_json(&input, &canonical)
                    } else {
                        format!("{canonical}\n")
                    },
                    stderr: String::new(),
                },
                Err(error) => CliOutput {
                    code: 1,
                    stdout: String::new(),
                    stderr: format!("identity: {error}\n"),
                },
            }
        }
        IdentityPlanAction::Node { host, user } => {
            let canonical = canonical_node_identity(&host, user.as_deref());
            CliOutput {
                code: 0,
                stdout: if plan_json {
                    render_identity_node_plan_json(&host, user.as_deref(), &canonical)
                } else {
                    format!("{canonical}\n")
                },
                stderr: String::new(),
            }
        }
    }
}

fn parse_identity_plan_args(argv: &[String]) -> Result<(bool, IdentityPlanAction), String> {
    let mut plan_json = false;
    let mut action = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "session-name" | "--session-name" => {
                let Some(oracle) = argv.get(index + 1) else {
                    return Err("identity: missing session-name oracle".to_owned());
                };
                action = Some(IdentityPlanAction::SessionName {
                    oracle: oracle.to_owned(),
                    slot: None,
                });
                index += 1;
            }
            "node" | "node-identity" | "--node-identity" => {
                let Some(host) = argv.get(index + 1) else {
                    return Err("identity: missing node host".to_owned());
                };
                action = Some(IdentityPlanAction::Node {
                    host: host.to_owned(),
                    user: None,
                });
                index += 1;
            }
            "--slot" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("identity: missing --slot value".to_owned());
                };
                let Ok(slot) = value.parse::<u32>() else {
                    return Err("identity: --slot must be an integer".to_owned());
                };
                action = match action {
                    Some(IdentityPlanAction::SessionName { oracle, .. }) => {
                        Some(IdentityPlanAction::SessionName {
                            oracle,
                            slot: Some(slot),
                        })
                    }
                    _ => return Err("identity: --slot requires session-name".to_owned()),
                };
                index += 1;
            }
            "--user" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("identity: missing --user value".to_owned());
                };
                action = match action {
                    Some(IdentityPlanAction::Node { host, .. }) => Some(IdentityPlanAction::Node {
                        host,
                        user: Some(value.to_owned()),
                    }),
                    _ => return Err("identity: --user requires node-identity".to_owned()),
                };
                index += 1;
            }
            arg => return Err(format!("identity: unknown argument {arg}")),
        }
        index += 1;
    }
    action.map_or_else(
        || Err("identity: expected session-name or node-identity".to_owned()),
        |action| Ok((plan_json, action)),
    )
}

enum IdentityPlanAction {
    SessionName { oracle: String, slot: Option<u32> },
    Node { host: String, user: Option<String> },
}

fn identity_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs identity session-name <oracle> [--slot <0-99>] [--plan-json]\n       maw-rs identity node-identity <host> [--user <user>] [--plan-json]\n       maw-rs identity constants [--plan-json]\n"
        ),
    }
}

fn run_identity_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            _ => {
                return identity_constants_usage_error(&format!(
                    "identity constants: unknown argument {arg}"
                ))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_identity_constants_json()
        } else {
            "identity constants actions=session-name,node-identity slot-range=0..99 max-stem-chars=50\n"
                .to_owned()
        },
        stderr: String::new(),
    }
}

fn render_identity_constants_json() -> String {
    concat!(
        "{\"command\":\"identity\",\"kind\":\"constants\",",
        "\"actions\":[\"session-name\",\"node-identity\"],",
        "\"sessionName\":{\"suffixRemoved\":\"-oracle\",\"gitSuffixRemoved\":\".git\",\"slotRange\":[0,99],\"slotPadding\":2,\"maxStemChars\":50,\"sanitization\":[\"lowercase\",\"whitespace-to-dash\",\"ascii-alnum-dot-underscore-dash-only\",\"collapse-dot-runs\",\"trim-leading-dash-dot\",\"trim-trailing-dash-dot-run\",\"strip-leading-numeric-fleet-slot\"]},",
        "\"nodeIdentity\":{\"fallbackHost\":\"local\",\"separator\":\"@\",\"preserveAlreadyCanonical\":true,\"omitUserWhenSameAsHost\":true,\"trimInputs\":true},",
        "\"validation\":{\"reservedOracleSuffixes\":[\"-view\"]},",
        "\"fixtureCounts\":{\"canonicalSessionName\":5,\"canonicalNodeIdentity\":5}}\n"
    )
    .to_owned()
}

fn identity_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\nusage: maw-rs identity constants [--plan-json]\n"),
    }
}

fn render_identity_session_plan_json(input: &CanonicalSessionNameInput, canonical: &str) -> String {
    let mut input_fields = vec![format!("\"oracle\":{}", json_string(&input.oracle))];
    if let Some(slot) = input.slot {
        input_fields.push(format!("\"slot\":{slot}"));
    }
    format!(
        "{{\"command\":\"identity\",\"kind\":\"sessionName\",\"input\":{{{}}},\"canonical\":{}}}\n",
        input_fields.join(","),
        json_string(canonical)
    )
}

