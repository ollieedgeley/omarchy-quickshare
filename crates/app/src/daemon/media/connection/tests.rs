use super::normalized_endpoint_name;

#[test]
fn endpoint_name_uses_trimmed_hostname_or_fallback() {
    assert_eq!(
        normalized_endpoint_name(Some("omarchy-macbook\n")),
        "omarchy-macbook"
    );
    assert_eq!(normalized_endpoint_name(Some(" \n")), "Omarchy");
    assert_eq!(normalized_endpoint_name(None), "Omarchy");
}
