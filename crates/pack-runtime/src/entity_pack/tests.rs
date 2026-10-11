use super::entity_identifier;

#[test]
fn identifier_is_read_from_the_client_entity_description() {
    let json = br#"{"minecraft:client_entity":{"description":{"identifier":"a:b"}}}"#;
    assert_eq!(entity_identifier(json).as_deref(), Some("a:b"));
    assert_eq!(entity_identifier(b"{}"), None);
}
