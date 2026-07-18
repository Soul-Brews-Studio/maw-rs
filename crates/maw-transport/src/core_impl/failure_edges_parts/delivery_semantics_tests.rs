    struct LegacyTransport;

    impl Transport for LegacyTransport {
        fn name(&self) -> &'static str {
            "legacy"
        }

        fn connected(&self) -> bool {
            true
        }

        fn can_reach(&self, _: &TransportTarget) -> bool {
            true
        }

        fn send(&mut self, _: &TransportTarget, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }
    }

    #[test]
    fn delivery_types_round_trip_and_construct_every_variant() {
        let receipts = [
            SendReceipt::SubmittedToPane,
            SendReceipt::ReceivedAsPrompt,
            SendReceipt::Queued,
        ];
        let refusals = [
            DeliveryRefusal::RefusedBareShell {
                observed_command: "zsh".to_owned(),
            },
            DeliveryRefusal::RefusedNotIdle {
                observed: "agent working".to_owned(),
            },
            DeliveryRefusal::TargetUnknown,
            DeliveryRefusal::DeliveryFailed {
                message: "closed".to_owned(),
            },
        ];

        for receipt in receipts {
            let json = serde_json::to_string(&receipt).expect("serialize receipt");
            let decoded = serde_json::from_str::<SendReceipt>(&json).expect("decode receipt");
            assert_eq!(decoded, receipt);
        }
        for refusal in refusals {
            let json = serde_json::to_string(&refusal).expect("serialize refusal");
            let decoded =
                serde_json::from_str::<DeliveryRefusal>(&json).expect("decode refusal");
            assert_eq!(decoded, refusal);
        }
    }

    #[test]
    fn trait_object_uses_legacy_default_delivery_adapter() {
        let mut transport: Box<dyn Transport> = Box::new(LegacyTransport);
        let receipt = transport.send_delivery(
            &TransportTarget::default(),
            "brief ending in newline\n",
            "codex",
            false,
        );

        assert_eq!(receipt, Ok(SendReceipt::SubmittedToPane));
    }
