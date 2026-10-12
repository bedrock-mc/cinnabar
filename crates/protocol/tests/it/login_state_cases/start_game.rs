use super::*;
use valentine::bedrock::version::v1_26_51::{
    DimensionDataPacket, DimensionDataPacketDefinitionsItem,
    DimensionDefinitionGroupDimensionDefinition, EnumsGeneratorType,
};

pub(super) fn early_dimension_definition() -> Packet {
    Packet::new(DimensionDataPacket {
        definitions: vec![DimensionDataPacketDefinitionsItem {
            key: "minecraft:overworld".into(),
            value: DimensionDefinitionGroupDimensionDefinition {
                minimum_y: 0,
                height_range: 256,
                dimension_type: DimensionType { value: 3 },
                generator_type: EnumsGeneratorType::Overworld,
                ..Default::default()
            },
        }],
    })
}

#[tokio::test]
async fn named_dimension_definition_before_start_game_survives_login_in_order() {
    let (mut session, core) = joined(Script {
        dimension_definition_before_start: true,
        ..Script::default()
    })
    .await;
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
    core.finish(session).await;
}

#[tokio::test]
async fn conflicting_start_game_runtime_ids_are_rejected() {
    let error = match join(Script {
        conflicting_start: true,
        ..Script::default()
    })
    .await
    {
        Ok(_) => panic!("conflicting StartGame packets must fail"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("conflicting StartGame runtime entity ID")
    );
}
