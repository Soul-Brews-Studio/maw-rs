//maw:noauto
#[test]
fn private_attach_renderers_cover_live_action_variants() {
    let switch = TmuxAttachAction::SwitchClient {
        session: "50-mawjs".to_owned(),
    };
    assert!(
        render_attach_plan_json("50-mawjs", "50-mawjs", &switch, false)
            .contains("\"action\":\"switch-client\"")
    );
    assert_eq!(
        attach_command_args(&switch, true),
        vec!["attach", "-r", "-t", "50-mawjs"]
    );

    let attach = TmuxAttachAction::Attach {
        session: "50-thclaws".to_owned(),
    };
    assert!(
        render_attach_plan_json("50-thclaws", "50-thclaws", &attach, false)
            .contains("\"action\":\"attach\"")
    );
    assert_eq!(
        attach_command_args(&attach, true),
        vec!["attach", "-r", "-t", "50-thclaws"]
    );

    let recover = TmuxAttachAction::Recover {
        session: "ghost".to_owned(),
    };
    assert_eq!(
        attach_command_args(&recover, false),
        vec!["attach", "-t", "ghost"]
    );
}
