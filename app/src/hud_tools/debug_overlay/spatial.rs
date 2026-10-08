//! Player coordinates, heading and the authoritative block-selection ray.

use bevy::prelude::Vec3;
use chunk_pipeline::WorldStream;

use super::{Column, DebugContext, Lines, LocalPlayerFrameCarrier, LocalViewPose};
use std::fmt::Write;

/// Diagnostic inspection reach, independent of the server's interaction reach.
pub(super) const TARGET_RANGE_BLOCKS: f64 = 20.0;
const MAX_BLOCK_STATES: usize = 10;

/// Retains parsed target properties until the canonical block state changes.
#[derive(Default)]
pub(super) struct BlockStates {
    source: String,
    rows: Vec<String>,
    spare: Vec<String>,
}

impl BlockStates {
    /// Parses a new state once and formats cached rows into the sampled column.
    pub(super) fn append(&mut self, lines: &mut Column<'_>, states: &str) {
        if self.source != states {
            self.source.clear();
            self.source.push_str(states);
            let mut rows = Column::new(std::mem::take(&mut self.rows), &mut self.spare);
            append_block_states(&mut rows, states);
            self.rows = rows.finish();
        }
        for row in &self.rows {
            lines.push(row);
        }
    }
}

impl DebugContext<'_, '_> {
    pub(super) fn append_position(
        &self,
        lines: &mut Lines<'_>,
        stream: &WorldStream,
        block_states: &mut BlockStates,
    ) {
        #[cfg(feature = "tracy")]
        let _zone = bevy::log::info_span!("ui.f3.spatial").entered();
        // CameraPose can be several blocks away in third person. XYZ is always
        // the subject's feet, and facing/raycast use its authoritative eye.
        let origins = player_origins(&self.frame, self.view.as_deref());
        let Some((feet, eye, direction)) = origins else {
            lines.left.push("XYZ: unavailable (waiting for player)");
            return;
        };
        lines.left.push("");
        append_coordinates(&mut lines.left, feet, direction);
        let (block, sky) = stream.light_level_at(eye.to_array());
        lines.left.push(format_args!(
            "Client Light: {} ({sky} sky, {block} block)",
            block.max(sky)
        ));
        lines.left.push_with(|line| {
            line.push_str("Biome: ");
            let Some(id) = stream.camera_biome_id(eye.to_array()) else {
                line.push_str("unavailable");
                return Ok(());
            };
            let definitions = stream.biome_definitions_snapshot();
            if let Some(definition) = definitions.iter().find(|definition| {
                definition
                    .biome_id
                    .is_some_and(|biome| u32::from(biome) == id)
            }) {
                line.push_str(&definition.name);
                Ok(())
            } else {
                write!(line, "id {id}")
            }
        });
        let block_distance = self.append_target(lines, stream, eye, direction, block_states);
        super::entities::append_target_entity(lines, stream, eye, direction, block_distance);
    }

    fn append_target(
        &self,
        lines: &mut Lines<'_>,
        stream: &WorldStream,
        eye: Vec3,
        direction: Vec3,
        block_states: &mut BlockStates,
    ) -> Option<f64> {
        let Some(collisions) = self.collisions.as_deref() else {
            lines
                .right
                .push("Targeted Block: unavailable (no registry)");
            return None;
        };
        let world = sim::PaletteWorld::new(
            stream.collision_store(),
            collisions.registry(stream.network_id_mode()),
            stream.current_dimension(),
        );
        let vector = |value: Vec3| {
            sim::Vec3::new(f64::from(value.x), f64::from(value.y), f64::from(value.z))
        };
        let hit = world.camera_visibility_ray(vector(eye), vector(direction), TARGET_RANGE_BLOCKS);
        lines.right.push("");
        let hit = match hit {
            Ok(Some(hit)) => hit,
            Ok(None) => {
                lines.right.push(format_args!(
                    "Targeted Block: none within {TARGET_RANGE_BLOCKS:.0} blocks"
                ));
                return None;
            }
            Err(_) => {
                lines
                    .right
                    .push("Targeted Block: unavailable (unloaded terrain)");
                return None;
            }
        };
        let [x, y, z] = hit.block_pos;
        lines
            .right
            .push(format_args!("Targeted Block: {x}, {y}, {z}"));
        lines.right.push(
            collisions
                .block_identifier(stream.network_id_mode(), hit.runtime_id)
                .unwrap_or("unregistered block"),
        );
        lines.right.push(format_args!(
            "Runtime ID: {} ({:?})",
            hit.runtime_id,
            stream.network_id_mode()
        ));
        lines.right.push(format_args!(
            "Face: {} | distance: {:.2} blocks",
            face_name(hit.face),
            hit.distance
        ));
        if let Some(states) =
            collisions.block_canonical_state(stream.network_id_mode(), hit.runtime_id)
        {
            block_states.append(&mut lines.right, states);
        }
        Some(hit.distance)
    }
}

pub(super) fn append_coordinates(lines: &mut Column<'_>, position: Vec3, direction: Vec3) {
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
        format_args!("XYZ: {x:.3} / {y:.5} / {z:.3}"),
        format_args!("Block: {} {} {}", block[0], block[1], block[2]),
        format_args!(
            "Chunk: {} {} {} in {} {} {}",
            within[0], within[1], within[2], chunk[0], chunk[1], chunk[2]
        ),
        format_args!("Facing: {heading} ({axis}) ({yaw:.1} / {pitch:.1})"),
    ]);
}

pub(super) fn append_block_states(lines: &mut Column<'_>, states: &str) {
    if let Ok(states) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(states) {
        for (name, value) in states.iter().take(MAX_BLOCK_STATES) {
            lines.push_with(|line| {
                write!(line, "{name}: ")?;
                if let Some(text) = value.as_str() {
                    line.push_str(text);
                    Ok(())
                } else {
                    write!(line, "{value}")
                }
            });
        }
        if states.len() > MAX_BLOCK_STATES {
            lines.push(format_args!(
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
