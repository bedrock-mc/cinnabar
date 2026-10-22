//! A struck boat rocks about its native render origin, scaled by its lost integrity.

use super::*;
use crate::presentation::actors::rig_world_from_actor;
use protocol::ActorMetadataValue;
use protocol_ids::ActorDataKey;

fn boat(metadata: &[(ActorDataKey, i32)]) -> ActorSnapshot {
    let mut actor = super::super::glide_tests::player([0.0; 3], 0);
    actor.entity_fact = assets::facts::entity::EntityFacts::pinned()
        .get(assets::facts::VanillaEntity::Boat.identifier());
    for (key, value) in metadata {
        actor
            .metadata
            .insert(key.wire_value(), ActorMetadataValue::Int(*value));
    }
    actor
}

fn transform(rows: &[[f32; 4]; 3], point: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| {
        rows[row][0] * point[0] + rows[row][1] * point[1] + rows[row][2] * point[2] + rows[row][3]
    })
}

#[test]
fn struck_boats_rock_about_their_render_origin() {
    let rows = rig_world_from_actor([1.0, 2.0, 3.0], 30.0, 1.0);
    assert_eq!(boat_hurt_tilted(rows, &boat(&[]), 0.5), rows);
    let struck = boat(&[
        (ActorDataKey::Hurt, 10),
        (ActorDataKey::HurtDirection, 1),
        (ActorDataKey::StructuralIntegrity, 30),
    ]);
    let tilted = boat_hurt_tilted(rows, &struck, 0.5);
    let height = struck.entity_fact.network_y_offset;
    assert!(height > 0.0);
    let origin = [0.0, height, 0.0];
    let [before, after] = [rows, tilted].map(|rows| transform(&rows, origin));
    assert!(before.iter().zip(after).all(|(a, b)| (a - b).abs() < 1e-5));
    let (time, damage) = (10.0_f32 - 0.5, 10.0_f32 - 0.5);
    let angle = (time * time.sin() * damage / 10.0).to_radians();
    let up = transform(&tilted, [0.0, height + 1.0, 0.0]);
    let lift = up[1] - after[1];
    assert!(
        (lift - angle.cos()).abs() < 1e-4,
        "{lift} vs {}",
        angle.cos()
    );
}
