use super::tooling_is_ready;

#[test]
fn reports_that_tooling_is_ready() {
    assert!(tooling_is_ready());
}
