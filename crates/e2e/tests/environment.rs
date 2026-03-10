mod common;

#[test]
fn missing_base_url_env_skips_instead_of_panicking() {
    let missing_env = format!("OPENSESSION_TEST_MISSING_{}", uuid::Uuid::new_v4().simple());

    assert!(common::test_context_from_env(&missing_env).is_none());
}
