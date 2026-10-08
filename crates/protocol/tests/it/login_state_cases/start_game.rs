use super::*;

pub(super) fn early_dimension_definition() -> McpePacket {
    McpePacket::from(jolyne::valentine::DimensionDataPacket {
        definitions: vec![jolyne::valentine::DimensionDataPacketDefinitionsItem {
            key: "minecraft:overworld".into(),
            value: jolyne::valentine::DimensionDefinitionGroupDimensionDefinition {
                minimum_y: 0,
                height_range: 256,
                dimension_type: DimensionType { value: 3 },
                generator_type: jolyne::valentine::EnumsGeneratorType::Overworld,
                ..Default::default()
            },
        }],
    })
}

#[tokio::test]
async fn named_dimension_definition_before_start_game_survives_login_in_order() {
    let transport =
        ScriptTransport::new(CompressionMode::Deflate, SpawnOrder::RadiusThenSpawn, false);
    transport
        .script
        .lock()
        .unwrap()
        .dimension_definition_before_start = true;
    let (mut session, _) = LoginSequence::connect_transport(transport, "RustClient")
        .await
        .expect("scripted login with an early dimension definition");
    let first = session.recv_world_event(0).await.unwrap();
    let WorldEvent::DimensionHeights(heights) = first else {
        panic!("the dimension definition must precede deferred world traffic, got {first:?}");
    };
    assert_eq!(heights[0].name.as_ref(), "minecraft:overworld");
    assert_eq!(heights[0].dimension, 3);
    assert_eq!(heights[0].minimum_y, 0);
    assert_eq!(heights[0].height_range, 256);
    assert_eq!(heights[0].generator, 1);
    assert!(matches!(
        session.recv_world_event(0).await.unwrap(),
        WorldEvent::SetTime(protocol::SetTimeEvent { time: 12_345 })
    ));
}

#[tokio::test]
async fn conflicting_start_game_runtime_ids_are_rejected() {
    let transport =
        ScriptTransport::new(CompressionMode::Deflate, SpawnOrder::RadiusThenSpawn, true);
    let error = match LoginSequence::connect_transport(transport, "RustClient").await {
        Ok(_) => panic!("conflicting StartGame packets must fail"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("conflicting StartGame runtime entity ID")
    );
}

#[tokio::test]
async fn unadvertised_optional_resource_pack_stack_does_not_block_login() {
    let transport = ScriptTransport::new_with_pack_stack(
        CompressionMode::Deflate,
        SpawnOrder::RadiusThenSpawn,
        false,
        true,
    );
    let (_, game_data) = LoginSequence::connect_transport(transport, "RustClient")
        .await
        .expect("an unavailable optional pack must not block login");
    assert_eq!(game_data.start_game.runtime_id.actor_runtime_id, RUNTIME_ID);
}
