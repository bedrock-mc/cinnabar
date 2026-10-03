#[test]
fn explicit_terminal_and_bootstrap_hook_order_is_preserved() {
    let source = include_str!("../../runtime/network.rs");
    for label in [
        "NetworkControlEvent::Failed {",
        "NetworkControlEvent::Transferred {",
        "NetworkControlEvent::Stopped {",
    ] {
        let tail = &source[source.find(label).unwrap()..];
        assert!(
            tail.find("clear_local_abilities(&mut player_runtime);")
                .unwrap()
                < tail.find("movement.deactivate();").unwrap()
        );
    }
    let start = source
        .find("BootstrapGenerationDisposition::Expected =>")
        .unwrap();
    let tail = &source[start
        ..source
            .find("NetworkControlEvent::SubChunkRequestSent {")
            .unwrap()];
    assert!(
        tail.find("BootstrapGenerationDisposition::Stale => continue")
            .unwrap()
            < tail
                .find("clear_local_abilities(&mut player_runtime);")
                .unwrap()
    );
    assert!(
        tail.find("clear_local_abilities(&mut player_runtime);")
            .unwrap()
            < tail.find("publish_bootstrap_inventory(").unwrap()
    );
    assert!(
        tail.find("bind_local_abilities(").unwrap()
            > tail.find("publish_equipment_identity(").unwrap()
    );
    assert!(tail.contains("client_world.fatal_error.is_none()"));
}
