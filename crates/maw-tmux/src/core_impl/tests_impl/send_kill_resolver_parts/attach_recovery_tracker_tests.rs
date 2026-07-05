    #[test]
    fn tmux_attach_recovery_candidates_and_choices_match_maw_js() {
        let cloned_repos = vec![
            "/opt/Code/github.com/Soul-Brews-Studio/pulse-oracle".to_owned(),
            "/opt/Code/github.com/Soul-Brews-Studio/pulse-helper-oracle".to_owned(),
            "/opt/Code/github.com/Org/sleeping-oracle".to_owned(),
        ];
        assert_eq!(
            wake_arg_for_similar_oracle("pulse-oracle"),
            "pulse".to_owned()
        );
        assert_eq!(
            wake_arg_for_similar_oracle("Soul-Brews-Studio/pulse-oracle"),
            "Soul-Brews-Studio/pulse-oracle".to_owned()
        );

        let candidates = attach_recovery_candidates(
            "pulse",
            "ghost",
            "session-name",
            &[],
            &["/opt/Code/github.com/Soul-Brews-Studio/pulse-oracle".to_owned()],
        );
        assert_eq!(
            candidates,
            vec![AttachRecoveryCandidate {
                oracle: "Soul-Brews-Studio/pulse-oracle".to_owned(),
                label: "Soul-Brews-Studio/pulse-oracle".to_owned(),
            }]
        );
        assert_eq!(
            decide_attach_recovery(&candidates, false, None),
            AttachRecoveryDecision::AutoWake {
                command: SpawnCommand {
                    program: "maw".to_owned(),
                    args: vec![
                        "wake".to_owned(),
                        "Soul-Brews-Studio/pulse-oracle".to_owned(),
                        "-a".to_owned()
                    ],
                },
                label: "Soul-Brews-Studio/pulse-oracle".to_owned(),
            }
        );

        let candidates = attach_recovery_candidates(
            "44-sleeping",
            "44-sleeping",
            "fleet-stem (44-sleeping)",
            &[AttachRecoveryFleetEntry {
                session: "44-sleeping".to_owned(),
                first_window_name: Some("sleeping-oracle".to_owned()),
                repo: Some("Org/sleeping-oracle".to_owned()),
            }],
            &cloned_repos,
        );
        assert_eq!(
            candidates[0],
            AttachRecoveryCandidate {
                oracle: "sleeping".to_owned(),
                label: "sleeping-oracle (cloned)".to_owned(),
            }
        );

        let candidates =
            attach_recovery_candidates("pulse", "pulse", "session-name", &[], &cloned_repos);
        assert_eq!(candidates.len(), 2);
        assert_eq!(
            decide_attach_recovery(&candidates, false, None),
            AttachRecoveryDecision::PrintCandidates {
                candidates: candidates.clone()
            }
        );
        assert_eq!(
            decide_attach_recovery(&candidates, true, None),
            AttachRecoveryDecision::Prompt {
                candidates: candidates.clone()
            }
        );
        assert_eq!(
            decide_attach_recovery(&candidates, true, Some(2)),
            AttachRecoveryDecision::WakeChoice {
                command: SpawnCommand {
                    program: "maw".to_owned(),
                    args: vec![
                        "wake".to_owned(),
                        "Soul-Brews-Studio/pulse-helper-oracle".to_owned(),
                        "-a".to_owned()
                    ],
                }
            }
        );
        assert_eq!(
            decide_attach_recovery(&candidates, true, Some(3)),
            AttachRecoveryDecision::InvalidChoice
        );
        assert_eq!(
            decide_attach_recovery(&[], true, None),
            AttachRecoveryDecision::NoCandidates
        );
    }

    #[test]
    fn tmux_send_tracker_matches_maw_js_cooldown_and_quota_gate() {
        let mut tracker = TmuxSendTracker::default();
        assert_eq!(tracker.check("%1", 1_000, false), SendThrottle::Allowed);
        assert_eq!(
            tracker.check("%1", 1_100, false),
            SendThrottle::Cooldown { cooldown_ms: 500 }
        );
        assert_eq!(tracker.check("%1", 1_600, false), SendThrottle::Allowed);
        assert_eq!(
            tracker.get("%1"),
            Some(SendTrackerEntry {
                last_ts: 1_600,
                count: 2,
                window_start: 1_000,
            })
        );

        tracker.set(
            "%2",
            SendTrackerEntry {
                last_ts: 10_000,
                count: 100,
                window_start: 0,
            },
        );
        assert_eq!(
            tracker.check("%2", 11_000, false),
            SendThrottle::Quota {
                quota_per_minute: 100
            }
        );
        assert_eq!(tracker.check("%2", 61_001, false), SendThrottle::Allowed);
        assert_eq!(
            tracker.get("%2"),
            Some(SendTrackerEntry {
                last_ts: 61_001,
                count: 1,
                window_start: 61_001,
            })
        );

        tracker.set(
            "%3",
            SendTrackerEntry {
                last_ts: 20_000,
                count: 100,
                window_start: 0,
            },
        );
        assert_eq!(tracker.check("%3", 20_001, true), SendThrottle::Allowed);
        assert_eq!(
            tracker.get("%3"),
            Some(SendTrackerEntry {
                last_ts: 20_000,
                count: 100,
                window_start: 0,
            })
        );
    }
