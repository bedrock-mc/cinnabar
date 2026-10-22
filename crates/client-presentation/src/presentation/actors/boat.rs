//! Boats rock natively when struck; their models carry no animation for it.

use client_world::ActorSnapshot;

/// A boat's structural integrity when undamaged; the hurt rock scales with what it has lost.
const BOAT_FULL_INTEGRITY: i64 = 40;

/// Rocks a struck boat about its native render origin by its streamed hurt time and direction.
pub(super) fn boat_hurt_tilted(
    mut rows: [[f32; 4]; 3],
    actor: &ActorSnapshot,
    alpha: f32,
) -> [[f32; 4]; 3] {
    use protocol_ids::ActorDataKey;
    if actor.entity_fact.native_class != assets::facts::entity::NativeEntityClass::Boat {
        return rows;
    }
    let integer = |key: ActorDataKey| match actor.metadata.get(&key.wire_value())? {
        protocol::ActorMetadataValue::Byte(value) => Some(i64::from(*value)),
        protocol::ActorMetadataValue::Short(value) => Some(i64::from(*value)),
        protocol::ActorMetadataValue::Int(value) => Some(i64::from(*value)),
        protocol::ActorMetadataValue::Long(value) => Some(*value),
        _ => None,
    };
    let time = integer(ActorDataKey::Hurt).unwrap_or(0) as f32 - alpha;
    let direction = integer(ActorDataKey::HurtDirection).unwrap_or(0);
    if time <= 0.0 || direction == 0 {
        return rows;
    }
    let lost = integer(ActorDataKey::StructuralIntegrity)
        .map_or(BOAT_FULL_INTEGRITY, |integrity| {
            BOAT_FULL_INTEGRITY - integrity
        });
    let damage = (lost as f32 - alpha).max(0.0);
    let angle = (direction as f32 * time * time.sin() * damage / 10.0).to_radians();
    let (sine, cosine) = angle.sin_cos();
    let height = actor.entity_fact.network_y_offset;
    for row in &mut rows {
        let (y, z) = (row[1], row[2]);
        row[1] = y * cosine + z * sine;
        row[2] = -y * sine + z * cosine;
        row[3] += (y - row[1]) * height;
    }
    rows
}

#[cfg(test)]
#[path = "boat_tests.rs"]
mod tests;
