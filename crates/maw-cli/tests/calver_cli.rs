use maw_cli::run_cli;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureRoot {
    compute_version: Vec<ComputeFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ComputeFixture {
    name: String,
    args: ComputeArgsFixture,
    tags: Vec<String>,
    package_version: String,
    expected: String,
}

#[derive(Debug, Deserialize)]
struct ComputeArgsFixture {
    stable: bool,
    channel: Option<ChannelFixture>,
    now: DatePartsFixture,
}

#[derive(Debug, Deserialize)]
struct DatePartsFixture {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
enum ChannelFixture {
    #[serde(rename = "alpha")]
    Alpha,
    #[serde(rename = "beta")]
    Beta,
}

impl ChannelFixture {
    const fn flag(self) -> &'static str {
        match self {
            Self::Alpha => "--alpha",
            Self::Beta => "--beta",
        }
    }
}

#[test]
fn calver_plan_cli_matches_maw_js_compute_version_fixtures() {
    let fixtures: FixtureRoot = serde_json::from_str(include_str!(
        "../../maw-calver/tests/fixtures/calver.fixtures.json"
    ))
    .expect("valid calver fixtures");

    for fixture in fixtures.compute_version {
        let mut argv = vec![
            "calver".to_owned(),
            "--plan-json".to_owned(),
            "--now".to_owned(),
            format!(
                "{}-{}-{}T{}:{}",
                fixture.args.now.year,
                fixture.args.now.month,
                fixture.args.now.day,
                fixture.args.now.hour,
                fixture.args.now.minute
            ),
            "--package-version".to_owned(),
            fixture.package_version.clone(),
        ];
        if fixture.args.stable {
            argv.push("--stable".to_owned());
        }
        if let Some(channel) = fixture.args.channel {
            argv.push(channel.flag().to_owned());
        }
        for tag in &fixture.tags {
            argv.push("--tag".to_owned());
            argv.push(tag.clone());
        }

        let output = run_cli(&argv);
        assert_eq!(output.code, 0, "{} stderr: {}", fixture.name, output.stderr);
        let json: serde_json::Value =
            serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
                panic!("{} invalid json: {error}\n{}", fixture.name, output.stdout)
            });
        assert_eq!(json["command"], "calver", "{}", fixture.name);
        assert_eq!(json["version"], fixture.expected, "{}", fixture.name);
        assert_eq!(
            json["packageVersion"], fixture.package_version,
            "{}",
            fixture.name
        );
        assert_eq!(
            json["tags"].as_array().expect("tags array").len(),
            fixture.tags.len(),
            "{}",
            fixture.name
        );
    }
}

#[test]
fn calver_plan_cli_reports_ghost_package_dates() {
    let argv = vec![
        "calver".to_owned(),
        "--now".to_owned(),
        "2026-4-30T9:37".to_owned(),
        "--package-version".to_owned(),
        "26.4.53".to_owned(),
    ];

    let output = run_cli(&argv);
    assert_eq!(output.code, 1);
    assert!(output.stderr.contains("ghost date"), "{}", output.stderr);
}
