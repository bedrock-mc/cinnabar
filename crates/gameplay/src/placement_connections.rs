//! Neighbor-derived fence and pane states used by placement collision checks.

use assets::facts::BlockProperty as P;
use assets::{ContributorRole, ModelFamily, NetworkIdMode};
use serde_json::{Map, Value};
use sim::PaletteWorld;

use crate::{block_use::placement_cell, movement::PhysicsCollisionRegistries};

pub(crate) const CONNECTION_STATES: &[(&str, u8)] = &[
    (P::MinecraftConnectionWest.name(), 4),
    (P::MinecraftConnectionEast.name(), 5),
    (P::MinecraftConnectionNorth.name(), 2),
    (P::MinecraftConnectionSouth.name(), 3),
];

/// Resolves the same connections as rendering before selecting a registered collision shape.
pub(crate) fn states_for_neighbours(
    collisions: &PhysicsCollisionRegistries,
    mode: NetworkIdMode,
    world: &PaletteWorld<'_>,
    position: [i32; 3],
    identity: u32,
    states: &mut Map<String, Value>,
) -> Option<()> {
    let block = collisions.block_state_runtime_id(
        mode,
        assets::facts::BlockStateIdentity::Compiled(identity),
        states,
    )?;
    let center = connection_facts(collisions, mode, block)?;
    connected_states(states, center, |face| {
        let neighbor = world
            .primary_runtime_id(placement_cell(position, face))
            .ok()?;
        connection_facts(collisions, mode, neighbor)
    })
}

/// Retains the palette facts that select connection templates, including the gate's axis.
#[derive(Debug, Clone, Copy)]
struct ConnectionFacts {
    family: ModelFamily,
    role: ContributorRole,
    occludes_full_face: bool,
    links_as_unit_cube: bool,
    orientation: Option<u32>,
    nether_fence: bool,
}

/// Unclassified server blocks cannot silently count as disconnected neighbors.
fn connection_facts(
    collisions: &PhysicsCollisionRegistries,
    mode: NetworkIdMode,
    block: u32,
) -> Option<ConnectionFacts> {
    let facts = collisions.block_connection_facts(mode, block)?;
    Some(ConnectionFacts {
        family: facts.family,
        role: facts.role,
        occludes_full_face: facts.occludes_full_face,
        links_as_unit_cube: facts.links_as_unit_cube,
        orientation: facts.orientation,
        nether_fence: collisions
            .block_facts(mode, block)
            .traits
            .contains(assets::registry::BlockTraits::NETHER_BRICK_FENCE),
    })
}

/// Sets all four links while retaining the palette's typed state envelopes.
fn connected_states(
    states: &mut Map<String, Value>,
    center: ConnectionFacts,
    mut neighbor: impl FnMut(u8) -> Option<ConnectionFacts>,
) -> Option<()> {
    if !matches!(center.family, ModelFamily::Fence | ModelFamily::Pane) {
        return Some(());
    }
    if center.role != ContributorRole::Primary {
        return None;
    }
    for &(key, face) in CONNECTION_STATES {
        let connects = connects_to(center, neighbor(face)?, face)?;
        set_connection(states, key, connects)?;
    }
    Some(())
}

/// Mirrors the pane, same-material fence, perpendicular gate, full-face and unit-cube links.
fn connects_to(center: ConnectionFacts, neighbor: ConnectionFacts, face: u8) -> Option<bool> {
    if neighbor.occludes_full_face {
        return Some(true);
    }
    if neighbor.role != ContributorRole::Primary {
        return Some(false);
    }
    if neighbor.links_as_unit_cube {
        return Some(true);
    }
    match center.family {
        ModelFamily::Pane => Some(matches!(
            neighbor.family,
            ModelFamily::Pane | ModelFamily::Wall
        )),
        ModelFamily::Fence => match neighbor.family {
            ModelFamily::Fence => Some(center.nether_fence == neighbor.nether_fence),
            ModelFamily::Gate => {
                let orientation = neighbor.orientation?;
                if orientation > 3 {
                    return None;
                }
                Some(match face {
                    2 | 3 => orientation & 1 != 0,
                    4 | 5 => orientation & 1 == 0,
                    _ => return None,
                })
            }
            _ => Some(false),
        },
        _ => None,
    }
}

/// Preserves byte and boolean palette representations while replacing one connection bit.
fn set_connection(states: &mut Map<String, Value>, key: &str, connected: bool) -> Option<()> {
    let entry = states.get_mut(key)?;
    let value = match entry {
        Value::Object(typed) => typed.get_mut("value")?,
        plain => plain,
    };
    *value = match value {
        Value::Bool(_) => Value::from(connected),
        Value::Number(number) if number.as_u64().is_some() => Value::from(u8::from(connected)),
        _ => return None,
    };
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Supplies primary model facts with no full-face occlusion.
    fn facts(family: ModelFamily) -> ConnectionFacts {
        ConnectionFacts {
            family,
            role: ContributorRole::Primary,
            occludes_full_face: false,
            links_as_unit_cube: false,
            orientation: None,
            nether_fence: false,
        }
    }

    /// Reads the pinned pane's typed connection entries.
    fn connection_states() -> Map<String, Value> {
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let pane = records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:glass_pane")
            .unwrap();
        serde_json::from_str(&pane.canonical_state).unwrap()
    }

    #[test]
    fn panes_connect_to_panes_walls_and_full_face_occluders() {
        let pane = facts(ModelFamily::Pane);
        for family in [ModelFamily::Pane, ModelFamily::Wall] {
            assert_eq!(connects_to(pane, facts(family), 4), Some(true));
        }
        assert_eq!(connects_to(pane, facts(ModelFamily::Fence), 4), Some(false));
        let mut opaque = facts(ModelFamily::Cross);
        opaque.occludes_full_face = true;
        assert_eq!(connects_to(pane, opaque, 4), Some(true));
        assert_eq!(connects_to(pane, facts(ModelFamily::Cross), 4), Some(false));
    }

    #[test]
    fn panes_and_fences_connect_to_see_through_unit_cubes() {
        for center in [facts(ModelFamily::Pane), facts(ModelFamily::Fence)] {
            for family in [ModelFamily::Cube, ModelFamily::Leaves, ModelFamily::Unknown] {
                let mut cube = facts(family);
                cube.links_as_unit_cube = true;
                assert_eq!(connects_to(center, cube, 4), Some(true));
            }
        }
    }

    /// Panes link to every shipped unit cube, including the see-through ones the registry
    /// leaves without a cube model family.
    #[test]
    fn panes_link_to_shipped_see_through_cubes() {
        let protocol = assets::active_content_registry_protocol();
        let breg = assets::pinned_block_registry_bytes();
        let records = assets::read_registry_for_protocol(breg, protocol).unwrap();
        let collisions = PhysicsCollisionRegistries::from_assets(
            breg,
            &records,
            include_bytes!("../../assets/data/block-physics-v2193.bin"),
            protocol,
        )
        .unwrap();
        let facts_of = |name: &str| {
            let record = records
                .iter()
                .find(|record| record.name.as_ref() == name)
                .unwrap_or_else(|| panic!("pinned registry has {name}"));
            connection_facts(&collisions, NetworkIdMode::Sequential, record.sequential_id)
                .unwrap_or_else(|| panic!("{name} has connection facts"))
        };
        let pane = facts_of("minecraft:glass_pane");
        for name in [
            "minecraft:glass",
            "minecraft:tinted_glass",
            "minecraft:ice",
            "minecraft:frosted_ice",
            "minecraft:honey_block",
            "minecraft:slime",
            "minecraft:oak_leaves",
        ] {
            assert_eq!(connects_to(pane, facts_of(name), 4), Some(true), "{name}");
        }
        for name in ["minecraft:poppy", "minecraft:torch"] {
            assert_eq!(connects_to(pane, facts_of(name), 4), Some(false), "{name}");
        }
    }

    #[test]
    fn wood_and_nether_fences_connect_only_to_the_same_group() {
        let wood = facts(ModelFamily::Fence);
        let mut nether = wood;
        nether.nether_fence = true;
        for (center, neighbor, expected) in [
            (wood, wood, true),
            (nether, nether, true),
            (wood, nether, false),
            (nether, wood, false),
        ] {
            assert_eq!(connects_to(center, neighbor, 2), Some(expected));
        }
    }

    #[test]
    fn fence_gate_connection_uses_perpendicular_axis() {
        let fence = facts(ModelFamily::Fence);
        for orientation in 0..=3 {
            let mut gate = facts(ModelFamily::Gate);
            gate.orientation = Some(orientation);
            for face in 2..=5 {
                assert_eq!(
                    connects_to(fence, gate, face),
                    Some((orientation & 1 == 0) == (face >= 4))
                );
            }
        }
        assert_eq!(connects_to(fence, facts(ModelFamily::Gate), 2), None);
    }

    #[test]
    fn links_are_written_before_collision_identity_lookup() {
        for (linked_face, linked_key) in [
            (2, "minecraft:connection_north"),
            (3, "minecraft:connection_south"),
            (4, "minecraft:connection_west"),
            (5, "minecraft:connection_east"),
        ] {
            let mut placed = connection_states();
            connected_states(&mut placed, facts(ModelFamily::Pane), |face| {
                Some(if face == linked_face {
                    facts(ModelFamily::Pane)
                } else {
                    facts(ModelFamily::Cross)
                })
            })
            .unwrap();
            for (key, entry) in &placed {
                assert_eq!(entry["value"], u8::from(key.as_str() == linked_key));
            }
            assert!(connected_states(&mut placed, facts(ModelFamily::Pane), |_| None).is_none());
        }
    }
}
