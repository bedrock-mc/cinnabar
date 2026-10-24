//! Standing and wall sign identities selected from one sign item.

use assets::facts::{BlockFact, BlockProperty as P, PlacementRule as R, PropertyDomain};
use serde_json::{Map, Value, json};

use crate::placement_state::{PlacementInput, integer_in, only_keys, value};

/// Counts the rotations in the pinned sign property domain.
fn sign_rotations() -> u8 {
    let PropertyDomain::Int(values) = P::GroundSignDirection.domain() else {
        unreachable!()
    };
    values.len() as u8
}

/// Chooses the wood-matched sign and its standing rotation or wall attachment face.
pub(crate) fn sign_states(
    fact: BlockFact,
    canonical: &str,
    input: PlacementInput,
) -> Option<(u32, Map<String, Value>)> {
    if !(1..=5).contains(&input.face) {
        return None;
    }
    let states = serde_json::from_str(canonical).ok()?;
    match fact.placement {
        R::StandingSign if only_keys(&states, &[P::GroundSignDirection.name()]) => {
            crate::placement_state::valid_property(fact, &states, P::GroundSignDirection)?;
        }
        R::WallSign if only_keys(&states, &[P::FacingDirection.name()]) => {
            integer_in(&states, P::FacingDirection.name(), &[2, 3, 4, 5])?;
        }
        _ => return None,
    }
    let (rule, key, direction) = if input.face == 1 {
        if !input.yaw.is_finite() {
            return None;
        }
        let rotation = ((input.yaw + 180.0) * f32::from(sign_rotations()) / 360.0 + 0.5).floor();
        if !rotation.is_finite() {
            return None;
        }
        (
            R::StandingSign,
            P::GroundSignDirection.name(),
            rotation.rem_euclid(f32::from(sign_rotations())) as u8,
        )
    } else {
        (R::WallSign, P::FacingDirection.name(), input.face)
    };
    Some((
        if rule == fact.placement {
            fact.identity?
        } else {
            fact.alternate?
        },
        serde_json::from_value(json!({key:{"type":"int","value":direction}})).ok()?,
    ))
}

/// Maps a resolved ordinary sign to the cell that supports it.
pub(crate) fn support_face(fact: BlockFact, states: &Map<String, Value>) -> Option<u8> {
    if fact.placement == R::StandingSign && only_keys(states, &[P::GroundSignDirection.name()]) {
        return Some(0);
    }
    if fact.placement == R::WallSign && only_keys(states, &[P::FacingDirection.name()]) {
        integer_in(states, P::FacingDirection.name(), &[2, 3, 4, 5])?;
        return Some(value(states, P::FacingDirection.name())?.as_u64()? as u8 ^ 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::block_fact;
    const SIGN_ROTATIONS: u8 = 16;

    /// Uses a real compiled counterpart for each ordinary sign identity.
    fn sign_states(
        name: &str,
        canonical: &str,
        input: PlacementInput,
    ) -> Option<(String, Map<String, Value>)> {
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let fact = block_fact(name);
        let (identity, states) = super::sign_states(fact, canonical, input)?;
        Some((records.get(identity as usize)?.name.to_string(), states))
    }

    /// Reads the sign kind from its pinned numeric facts.
    fn support_face(name: &str, states: &Map<String, Value>) -> Option<u8> {
        super::support_face(block_fact(name), states)
    }

    #[test]
    fn standing_rotations_cover_sixteen_steps_and_strict_rounding_boundaries() {
        let canonical = r#"{"ground_sign_direction":{"type":"int","value":0}}"#;
        for rotation in 0..SIGN_ROTATIONS {
            let yaw = rotation as f32 * 22.5 - 180.0;
            let input = PlacementInput {
                face: 1,
                yaw,
                pitch: 0.0,
                click_position: [0.5; 3],
            };
            for (offset, expected) in [
                (0.0, rotation),
                (11.249, rotation),
                (11.25, (rotation + 1) & 15),
            ] {
                let (name, states) = sign_states(
                    "minecraft:standing_sign",
                    canonical,
                    PlacementInput {
                        yaw: yaw + offset,
                        ..input
                    },
                )
                .unwrap();
                assert_eq!(name, "minecraft:standing_sign");
                assert_eq!(states["ground_sign_direction"]["value"], expected);
                assert_eq!(support_face(&name, &states), Some(0));
            }
        }
    }

    #[test]
    fn side_faces_choose_matching_wall_signs_and_ceiling_clicks_defer() {
        let canonical = r#"{"ground_sign_direction":{"type":"int","value":7}}"#;
        for face in 0..=6 {
            let input = PlacementInput {
                face,
                yaw: 0.0,
                pitch: 0.0,
                click_position: [0.5; 3],
            };
            let result = sign_states("minecraft:spruce_standing_sign", canonical, input);
            if face == 0 || face == 6 {
                assert!(result.is_none());
                continue;
            }
            let (name, states) = result.unwrap();
            if face == 1 {
                assert_eq!(name, "minecraft:spruce_standing_sign");
            } else {
                assert_eq!(name, "minecraft:spruce_wall_sign");
                assert_eq!(states["facing_direction"]["value"], face);
                assert_eq!(support_face(&name, &states), Some(face ^ 1));
            }
        }
    }
}
