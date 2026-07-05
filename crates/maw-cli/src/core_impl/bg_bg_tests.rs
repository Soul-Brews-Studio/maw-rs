#[cfg(test)]
mod bg_tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct BgCall {
        subcommand: String,
        args: Vec<String>,
    }

    #[derive(Debug, Default)]
    struct BgFakeTmux {
        calls: Vec<BgCall>,
        attach_calls: Vec<Vec<String>>,
        responses: std::collections::VecDeque<BgTmuxResult>,
    }

    impl BgFakeTmux {
        fn bg_with_responses(responses: Vec<BgTmuxResult>) -> Self {
            Self {
                responses: responses.into(),
                ..Default::default()
            }
        }
    }

    impl BgTmux for BgFakeTmux {
        fn bg_run(&mut self, subcommand: &str, args: &[String]) -> Result<BgTmuxResult, String> {
            self.calls.push(BgCall {
                subcommand: subcommand.to_owned(),
                args: args.to_vec(),
            });
            Ok(self.responses.pop_front().unwrap_or_else(bg_ok_empty))
        }

        fn bg_attach(&mut self, args: &[String]) -> Result<i32, String> {
            self.attach_calls.push(args.to_vec());
            Ok(0)
        }
    }

    fn bg_ok(stdout: &str) -> BgTmuxResult {
        BgTmuxResult {
            status: 0,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        }
    }

    fn bg_ok_empty() -> BgTmuxResult {
        bg_ok("")
    }

    fn bg_fail(stderr: &str) -> BgTmuxResult {
        BgTmuxResult {
            status: 1,
            stdout: String::new(),
            stderr: stderr.to_owned(),
        }
    }

    fn bg_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn bg_now() -> u64 {
        1_700_000_000
    }

    fn bg_not_tmux() -> bool {
        false
    }

    fn bg_in_tmux() -> bool {
        true
    }

    #[test]
    fn bg_dispatch_registers_bg() {
        assert_eq!(DISPATCH_88[0].command, "bg");
    }

    #[test]
    fn bg_spawn_builds_safe_new_session_after_has_session() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![bg_fail("missing"), bg_ok_empty()]);
        let output = bg_run(&bg_strings(&["cargo", "test", "--name", "cargo-test"]), &mut tmux, bg_now, bg_not_tmux)
            .expect("spawn");
        assert_eq!(output.0, 0);
        assert_eq!(output.1, "cargo-test\tmaw-bg-cargo-test\n");
        assert_eq!(tmux.calls[0].subcommand, "has-session");
        assert_eq!(tmux.calls[1].subcommand, "new-session");
        assert!(tmux.calls[1].args.contains(&"/bin/sh".to_owned()));
        assert!(tmux.calls[1].args.contains(&"-c".to_owned()));
    }

    #[test]
    fn bg_rejects_leading_dash_command_before_spawn() {
        let mut tmux = BgFakeTmux::default();
        let error = bg_run(&bg_strings(&["--bad"]), &mut tmux, bg_now, bg_not_tmux).expect_err("bad");
        assert!(error.1.contains("command must"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn bg_rejects_bad_name_before_tmux() {
        let mut tmux = BgFakeTmux::default();
        let error = bg_run(&bg_strings(&["echo", "hi", "--name=-bad"]), &mut tmux, bg_now, bg_not_tmux)
            .expect_err("bad name");
        assert!(error.1.contains("invalid --name"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn bg_list_formats_sessions_and_captures_last_lines() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![
            bg_ok("maw-bg-build-a1b2\t1699999940\tcargo\nmaw-bg-done-b2c3\t1699996400\tsleep\nother\t1\tsh\n"),
            bg_ok("building\n"),
            bg_ok("[done — exit 0]\n"),
        ]);
        let output = bg_run(&bg_strings(&["ls"]), &mut tmux, bg_now, bg_not_tmux).expect("ls");
        assert!(output.1.contains("build-a1b2  running  1m"));
        assert!(output.1.contains("done-b2c3   done     1h"));
        assert_eq!(tmux.calls[0].subcommand, "list-sessions");
    }

    #[test]
    fn bg_json_list_is_camel_case_like_js() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![bg_ok("maw-bg-build-a1b2\t1699999990\tread\n"), bg_ok("tail\n")]);
        let output = bg_run(&bg_strings(&["list", "--json"]), &mut tmux, bg_now, bg_not_tmux).expect("json");
        assert!(output.1.contains("\"ageSeconds\": 10"));
        assert!(output.1.contains("\"status\": \"done\""));
    }

    #[test]
    fn bg_tail_resolves_hash_suffix_and_uses_lines_guard() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![
            bg_ok("maw-bg-build-a1b2\t1\tcargo\n"),
            bg_ok("last\n"),
            bg_ok("one\ntwo\n"),
        ]);
        let output = bg_run(&bg_strings(&["tail", "a1b2", "--lines", "2"]), &mut tmux, bg_now, bg_not_tmux).expect("tail");
        assert_eq!(output.1, "one\ntwo");
        let tail = tmux.calls.last().expect("tail call");
        assert_eq!(tail.subcommand, "capture-pane");
        assert!(tail.args.contains(&"-2".to_owned()));
    }

    #[test]
    fn bg_kill_all_validates_bg_targets_before_kill() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![
            bg_ok("maw-bg-one-a111\t1\tsleep\nmaw-bg-two-b222\t1\tread\n"),
            bg_ok("done\n"),
            bg_ok("done\n"),
            bg_ok_empty(),
            bg_ok_empty(),
        ]);
        let output = bg_run(&bg_strings(&["kill", "--all"]), &mut tmux, bg_now, bg_not_tmux).expect("kill");
        assert!(output.1.contains("killed: one-a111, two-b222"));
        assert_eq!(tmux.calls.iter().filter(|call| call.subcommand == "kill-session").count(), 2);
    }

    #[test]
    fn bg_gc_dry_run_does_not_kill() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![
            bg_ok("maw-bg-old-a111\t1699900000\tsleep\nmaw-bg-new-b222\t1699999990\tcargo\n"),
            bg_ok("old done\n"),
            bg_ok("new run\n"),
        ]);
        let output = bg_run(&bg_strings(&["gc", "--dry-run", "--older-than", "1h"]), &mut tmux, bg_now, bg_not_tmux)
            .expect("gc");
        assert!(output.1.contains("would reap: old-a111"));
        assert!(output.1.contains("kept:    new-b222"));
        assert!(!tmux.calls.iter().any(|call| call.subcommand == "kill-session"));
    }

    #[test]
    fn bg_attach_switches_inside_tmux_without_real_spawn() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![bg_ok("maw-bg-one-a111\t1\tcargo\n"), bg_ok("tail\n")]);
        let output = bg_run(&bg_strings(&["attach", "one"]), &mut tmux, bg_now, bg_in_tmux).expect("attach");
        assert_eq!(output.0, 0);
        assert_eq!(tmux.attach_calls[0][0], "switch-client");
        assert_eq!(tmux.attach_calls[0][2], "maw-bg-one-a111");
    }

    #[test]
    fn bg_resolve_ambiguous_prefix_is_error_before_kill() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![
            bg_ok("maw-bg-one-a111\t1\tcargo\nmaw-bg-one-b222\t1\tcargo\n"),
            bg_ok("a\n"),
            bg_ok("b\n"),
        ]);
        let error = bg_run(&bg_strings(&["kill", "one"]), &mut tmux, bg_now, bg_not_tmux).expect_err("ambiguous");
        assert!(error.1.contains("matches 2 sessions"));
        assert!(!tmux.calls.iter().any(|call| call.subcommand == "kill-session"));
    }
}
