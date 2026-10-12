#[test]
fn empty_clear_recipe_packet_is_published_without_generic_materialization() {
    let mut bytes = bytes::Bytes::from_static(&[13, 52, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let raw = crate::raw_packet::decode_packet_raw(&mut bytes).unwrap();
    let event = super::decode_world_raw_with(raw, 0, |_| {
        panic!("recipe ingress must not call the generic decoder")
    })
    .unwrap();
    assert!(
        matches!(event, Some(crate::WorldEvent::Inventory(_))),
        "an empty clear update must retire old recipe authority"
    );
}
