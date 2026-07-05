use super::*;

#[test]
fn alpha_alias_scans_alpha_tags() {
    let tags = vec!["v26.4.27-alpha.1".to_owned(), "v26.4.27-beta.9".to_owned()];
    assert_eq!(max_alpha_from_tags("26.4.27", &tags), 1);
}

#[test]
fn base_parsing_rejects_extra_segments_and_bad_calendar_values() {
    assert_eq!(parse_base("26.5.21.1"), None);
    assert!(!is_valid_calendar_date("26.13.1"));
    assert!(!is_valid_calendar_date("26.4.31"));
    assert_eq!(
        extract_base_from_version("v26.5.21-alpha.1+build").as_deref(),
        Some("26.5.21")
    );
}
