//! Player coordinates, heading and the authoritative block-selection ray.

use bevy::prelude::Vec3;
use chunk_pipeline::WorldStream;

use super::{DebugContext, DebugLines, LocalPlayerFrameCarrier, LocalViewPose};

/// Diagnostic inspection reach, independent of the server's interaction reach.
const TARGET_RANGE_BLOCKS: f64 = 20.0;
const MAX_BLOCK_STATES: usize = 10;

impl DebugContext<'_, '_> {
    pub(super) fn append_position(&self, lines: &mut DebugLines, stream: &WorldStream) {
        // CameraPose can be several blocks away in third person. XYZ is always
        // the subject's feet, and facing/raycast use its authoritative eye.
        let origins = player_origins(&self.frame, self.view.as_deref());
        let Some((feet, eye, direction)) = origins else {
            lines
                .left
                .push("XYZ: unavailable (waiting for player)".to_owned());
            return;
        };
        lines.left.push(String::new());
        append_coordinates(&mut lines.left, feet, direction);
        let (block, sky) = stream.light_level_at(eye.to_array());
        lines.left.push(format!(
            "Client Light: {} ({sky} sky, {block} block)",
            block.max(sky)
        ));
        let biome = stream.camera_biome_id(eye.to_array()).map(|id| {
            stream
                .biome_definitions_snapshot()
                .iter()
                .find(|definition| {
                    definition
                        .biome_id
                        .is_some_and(|biome| u32::from(biome) == id)
                })
                .map_or_else(
                    || format!("id {id}"),
                    |definition| definition.name.to_string(),
                )
        });
        lines.left.push(format!(
            "Biome: {}",
            biome.as_deref().unwrap_or("unavailable")
        ));
        self.append_target(lines, stream, eye, direction);
    }

    fn append_target(
        &self,
        lines: &mut DebugLines,
        stream: &WorldStream,
        eye: Vec3,
        direction: Vec3,
    ) {
        let Some(collisions) = self.collisions.as_deref() else {
            lines
                .right
                .push("Targeted Block: unavailable (no registry)".to_owned());
            return;
        };
        let world = sim::PaletteWorld::new(
            stream.collision_store(),
            collisions.registry(stream.network_id_mode()),
            stream.current_dimension(),
        );
        let vector = |value: Vec3| {
            sim::Vec3::new(f64::from(value.x), f64::from(value.y), f64::from(value.z))
        };
        let hit = world.block_interaction_ray_current(
            vector(eye),
            vector(direction),
            TARGET_RANGE_BLOCKS,
        );
        lines.right.push(String::new());
        let hit = match hit {
            Ok(Some(hit)) => hit,
            Ok(None) => {
                lines.right.push(format!(
                    "Targeted Block: none within {TARGET_RANGE_BLOCKS:.0} blocks"
                ));
                return;
            }
            Err(_) => {
                lines
                    .right
                    .push("Targeted Block: unavailable (unloaded terrain)".to_owned());
                return;
            }
        };
        let [x, y, z] = hit.block_pos;
        lines.right.push(format!("Targeted Block: {x}, {y}, {z}"));
        lines.right.push(
            collisions
                .block_identifier(stream.network_id_mode(), hit.runtime_id)
                .unwrap_or("unregistered block")
                .to_owned(),
        );
        lines.right.push(format!(
            "Runtime ID: {} ({:?})",
            hit.runtime_id,
            stream.network_id_mode()
        ));
        lines.right.push(format!(
            "Face: {} | distance: {:.2} blocks",
            face_name(hit.face),
            hit.distance
        ));
        if let Some(states) =
            collisions.block_canonical_state(stream.network_id_mode(), hit.runtime_id)
        {
            append_block_states(&mut lines.right, states);
        }
    }
}

pub(super) fn append_coordinates(lines: &mut Vec<String>, position: Vec3, direction: Vec3) {
    let [x, y, z] = position.to_array();
    let block = [x, y, z].map(|value| value.floor() as i32);
    // Shared meshing query width is the sub-chunk edge, rather than another literal.
    let edge = meshing::biome_lattice::BIOME_QUERY_SIDE;
    let chunk = block.map(|value| value.div_euclid(edge));
    let within = block.map(|value| value.rem_euclid(edge));
    let yaw = (-direction.x).atan2(direction.z).to_degrees() + 0.0;
    let pitch = -direction.y.clamp(-1.0, 1.0).asin().to_degrees() + 0.0;
    let (heading, axis) = facing(direction);
    lines.extend([
        format!("XYZ: {x:.3} / {y:.5} / {z:.3}"),
        format!("Block: {} {} {}", block[0], block[1], block[2]),
        format!(
            "Chunk: {} {} {} in {} {} {}",
            within[0], within[1], within[2], chunk[0], chunk[1], chunk[2]
        ),
        format!("Facing: {heading} ({axis}) ({yaw:.1} / {pitch:.1})"),
    ]);
}

pub(super) fn append_block_states(lines: &mut Vec<String>, states: &str) {
    if let Ok(states) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(states) {
        for (name, value) in states.iter().take(MAX_BLOCK_STATES) {
            let value = value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned);
            lines.push(format!("{name}: {value}"));
        }
        if states.len() > MAX_BLOCK_STATES {
            lines.push(format!(
                "... {} more states",
                states.len() - MAX_BLOCK_STATES
            ));
        }
    }
}

fn face_name(face: u8) -> &'static str {
    ["down", "up", "north", "south", "west", "east"]
        .get(usize::from(face))
        .copied()
        .unwrap_or("unknown")
}

pub(super) fn facing(direction: Vec3) -> (&'static str, &'static str) {
    if direction.x.abs() > direction.z.abs() {
        if direction.x > 0.0 {
            ("east", "+X")
        } else {
            ("west", "-X")
        }
    } else if direction.z > 0.0 {
        ("south", "+Z")
    } else {
        ("north", "-Z")
    }
}

pub(super) fn player_origins(
    frame: &LocalPlayerFrameCarrier,
    view: Option<&LocalViewPose>,
) -> Option<(Vec3, Vec3, Vec3)> {
    frame
        .snapshot()
        .map(|frame| (frame.feet(), frame.eye(), frame.direction()))
        .or_else(|| {
            view.map(|view| {
                (
                    view.feet_translation(),
                    view.eye_translation(),
                    view.rotation() * Vec3::NEG_Z,
                )
            })
        })
}
