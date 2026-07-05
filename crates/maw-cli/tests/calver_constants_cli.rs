use maw_cli::{run_cli, CliOutput};

#[test]
fn calver_constants_plan_json_locks_maw_js_calver_vocabulary() {
    let output = run_cli(&[
        "calver".to_owned(),
        "constants".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(
        output,
        CliOutput {
            code: 0,
            stdout: concat!(
                "{\"command\":\"calver\",\"kind\":\"constants\",",
                "\"baseFormat\":\"YY.M.D\",\"prereleaseFormat\":\"YY.M.D-channel.HHMM\",",
                "\"channels\":[\"alpha\",\"beta\"],\"defaultChannel\":\"alpha\",",
                "\"dateRules\":{\"zeroPadding\":false,\"februaryMaxDay\":29,\"yearModulo\":100},",
                "\"stamp\":{\"shape\":\"H*100+M\",\"leadingZeroes\":false,\"max\":2359},",
                "\"versionInputs\":[\"tags\",\"packageVersion\",\"now\",\"stable\",\"channel\"],",
                "\"monotonicRules\":[\"stable-uses-today-base\",\"prerelease-preserves-future-package-base\",\"roll-base-forward-when-existing-suffix-gte-stamp\",\"reject-ghost-package-date\"],",
                "\"fixtureCounts\":{\"dateBase\":3,\"hhmmStamp\":5,\"extractBaseFromVersion\":7,\"compareBases\":5,\"isValidCalendarDate\":8,\"nextCalendarBase\":3,\"maxNFromTags\":5,\"maxNFromPackageJson\":7,\"effectiveBase\":6,\"computeVersion\":10}}\n"
            )
            .to_owned(),
            stderr: String::new(),
        }
    );
}

#[test]
fn calver_constants_plan_rejects_unknown_flags() {
    let output = run_cli(&[
        "calver".to_owned(),
        "constants".to_owned(),
        "--bad".to_owned(),
    ]);

    assert_eq!(output.code, 2);
    assert!(output.stdout.is_empty());
    assert!(output
        .stderr
        .contains("calver constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs calver constants"));
}
