#[cfg(test)]
mod awaken_tests {
    use super::*;

    #[derive(Default)]
    struct AwakenFakeRunner {
        tty: bool,
        answer: bool,
        calls: Vec<(String, Vec<String>)>,
        outputs: Vec<AwakenProcessOutput>,
        sleeps: usize,
    }

    impl AwakenRunner for AwakenFakeRunner {
        fn awaken_stdin_is_tty(&mut self) -> bool {
            self.tty
        }
        fn awaken_ask_yes_no(&mut self, _question: &str) -> bool {
            self.answer
        }
        fn awaken_run(
            &mut self,
            program: &str,
            args: &[String],
        ) -> Result<AwakenProcessOutput, String> {
            self.calls.push((program.to_owned(), args.to_vec()));
            if self.outputs.is_empty() {
                return Ok(AwakenProcessOutput {
                    code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }
            Ok(self.outputs.remove(0))
        }
        fn awaken_sleep(&mut self, _duration: std::time::Duration) {
            self.sleeps += 1;
        }
    }

    fn awaken_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn awaken_ok(stdout: &str) -> AwakenProcessOutput {
        AwakenProcessOutput {
            code: 0,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        }
    }

    #[test]
    fn awaken_parse_real_flags_and_builds_bud_args_without_trigger() {
        let options = awaken_parse_args(&awaken_strings(&[
            "nova",
            "--from",
            "wish",
            "--repo",
            "tonkmac/maw-rs",
            "--issue",
            "132",
            "--trigger",
            "/awaken --fast",
            "--fast",
            "--split",
            "--dry-run",
            "--sync-peers",
            "--track-vault",
            "--yes",
        ]))
        .expect("parse");
        assert_eq!(options.name, "nova");
        assert_eq!(options.trigger.as_deref(), Some("/awaken --fast"));
        let bud_args = awaken_bud_args(&options).expect("bud args");
        assert_eq!(
            bud_args,
            awaken_strings(&[
                "bud",
                "nova",
                "--from",
                "wish",
                "--repo",
                "tonkmac/maw-rs",
                "--issue",
                "132",
                "--fast",
                "--split",
                "--dry-run",
                "--track-vault",
                "--sync-peers",
            ])
        );
    }

    #[test]
    fn awaken_dry_run_is_hermetic_and_matches_golden_without_real_env() {
        let mut runner = AwakenFakeRunner {
            outputs: vec![awaken_ok("bud plan\n")],
            ..AwakenFakeRunner::default()
        };
        let output = awaken_run_with_runner(
            &awaken_strings(&["nova", "--dry-run", "--trigger", "/awaken", "--yes"]),
            &mut runner,
        )
        .expect("run");
        assert_eq!(
            output,
            include_str!("../../tests/fixtures/native-awaken/awaken-dry-run.stdout")
        );
        assert_eq!(
            runner.calls,
            vec![(
                "maw".to_owned(),
                awaken_strings(&["bud", "nova", "--dry-run"])
            )]
        );
    }

    #[test]
    fn awaken_non_dry_run_waits_for_agent_then_sends_trigger() {
        let mut runner = AwakenFakeRunner {
            outputs: vec![
                awaken_ok("bud ok\n"),
                awaken_ok("%12\n"),
                awaken_ok("zsh\n"),
                awaken_ok("claude\n"),
                awaken_ok(""),
            ],
            ..AwakenFakeRunner::default()
        };
        let output = awaken_run_with_runner(
            &awaken_strings(&["nova", "--yes", "--no-trigger"]),
            &mut runner,
        )
        .expect("run");
        assert!(output.contains("--no-trigger"));
        assert_eq!(runner.calls.len(), 1);

        let mut runner = AwakenFakeRunner {
            outputs: vec![
                awaken_ok("bud ok\n"),
                awaken_ok("%12\n"),
                awaken_ok("zsh\n"),
                awaken_ok("claude\n"),
                awaken_ok("sent\n"),
            ],
            ..AwakenFakeRunner::default()
        };
        let output =
            awaken_run_with_runner(&awaken_strings(&["nova", "--yes"]), &mut runner).expect("run");
        assert!(output.contains("awakened"));
        assert_eq!(runner.sleeps, 1);
        assert_eq!(
            runner.calls[4],
            (
                "maw".to_owned(),
                awaken_strings(&["send-text", "nova", "/awaken"])
            )
        );
    }

    #[test]
    fn awaken_unresolved_target_returns_warning_success_like_js() {
        let mut runner = AwakenFakeRunner {
            outputs: vec![
                awaken_ok("bud ok\n"),
                AwakenProcessOutput {
                    code: 1,
                    stdout: String::new(),
                    stderr: "no target".to_owned(),
                },
            ],
            ..AwakenFakeRunner::default()
        };
        let output = awaken_run_with_runner(&awaken_strings(&["nova", "--yes"]), &mut runner)
            .expect("warning success");
        assert!(output.contains("could not resolve nova"));
        assert_eq!(runner.calls.len(), 2);
    }

    #[test]
    fn awaken_option_injection_guard_blocks_exec_path_and_target_values() {
        assert!(awaken_validate_exec_name("/bin/maw").is_err());
        assert!(awaken_validate_exec_name("-maw").is_err());
        assert!(awaken_validate_target_arg("-nova", "oracle name").is_err());
        assert!(awaken_validate_target_arg("../nova", "oracle name").is_err());
        assert!(awaken_parse_args(&awaken_strings(&["--repo", "-bad/repo", "nova"])).is_err());
        assert!(awaken_parse_args(&awaken_strings(&["-bad"])).is_err());
    }

    #[test]
    fn awaken_dispatcher_is_native() {
        assert_eq!(dispatcher_status("awaken"), DispatchKind::Native);
    }
}
