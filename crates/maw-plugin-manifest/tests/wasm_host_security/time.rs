#[test]
fn time_now_host_returns_wall_clock_without_a_capability() {
    let dir = temp("time-now");
    let result = call(&host(&dir, &[]), "maw.time.now", &json!({}));

    assert_eq!(result["ok"], true, "{result}");
    assert!(
        result["value"]["millis"].as_u64().is_some_and(|value| value > 0),
        "{result}"
    );
}
