//! Packet bursts must start a new far-terrain visit even when the dimension returns.

use super::tests::{far_terrain_app, flat};
use super::*;
use far_terrain::ColumnSummary;

/// Commits both dimension packets before the next scene publication.
pub(super) fn round_trip(app: &mut bevy::prelude::App) {
    let mut world = app.world_mut().resource_mut::<ClientWorld>();
    let stream = world.stream.as_mut().unwrap();
    for (sequence, dimension) in [(1, 1), (2, 0)] {
        stream
            .submit(
                sequence,
                protocol::WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
                    dimension,
                    ..Default::default()
                }),
            )
            .unwrap();
    }
    assert_eq!(stream.current_dimension(), 0);
    assert_eq!(stream.form_dimension_epoch(), 2);
}

#[path = "transfer_tests/frames.rs"]
mod frames;

/// The old visit cannot survive a round trip hidden between two rendered frames.
#[test]
fn dimension_round_trip_discards_previous_far_tiles() {
    let mut app = far_terrain_app(64);
    app.update();
    let previous = ColumnPos::new(30, 0);
    app.world_mut()
        .resource_mut::<FarTerrain>()
        .cache
        .store()
        .admit(previous, &ColumnSummary::decode(&flat(96).bytes).unwrap());
    test_time::eventually("old visit has drawn far terrain", || {
        app.update();
        !app.world().resource::<FarTerrainScene>().tiles.is_empty()
    });
    round_trip(&mut app);
    app.update();
    assert!(
        app.world().resource::<FarTerrainScene>().tiles.is_empty(),
        "the first destination frame still draws the previous world's far tiles"
    );
    assert!(
        !app.world_mut()
            .resource_mut::<FarTerrain>()
            .cache
            .store()
            .contains(previous),
        "the destination inherited the old visit's summaries"
    );
}
