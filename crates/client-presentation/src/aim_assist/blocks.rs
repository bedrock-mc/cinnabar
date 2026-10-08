use bevy::prelude::Vec3;
use protocol::CameraAimAssistTargetMode;
use sim::PaletteWorld;
use std::sync::Arc;

use super::frame::{MAX_BLOCK_SAMPLE_RAYS, vector};
use super::{AimAssistCandidate, AimAssistCategory, AimAssistFrame, AimAssistFrustum, TargetKind};

impl AimAssistFrame {
    /// Odd ticks sample the upper half; even ticks append the lower half to the same cache.
    pub(super) fn capture_blocks(
        &mut self,
        blocks: &PaletteWorld<'_>,
        frustum: AimAssistFrustum,
        distance: f32,
        odd: bool,
    ) {
        let (right, up, half_width, half_height) = frustum.far_plane(distance);
        let columns = (half_width.ceil() as usize).saturating_mul(2);
        let rows = half_height.ceil() as usize;
        if columns.saturating_mul(rows).saturating_mul(2) > MAX_BLOCK_SAMPLE_RAYS {
            self.skipped_queries += 1;
            return;
        }
        let row_offset = if odd { 0 } else { rows };
        let top_left =
            frustum.origin + frustum.forward * distance + right * half_width + up * half_height;
        for row in row_offset..row_offset + rows {
            for column in 0..columns {
                let end = top_left - right * column as f32 - up * row as f32;
                self.visibility_queries += 1;
                let hit = match blocks.camera_aim_ray(
                    vector(frustum.origin),
                    vector(end),
                    self.max_steps,
                    false,
                    self.target_liquids,
                ) {
                    Ok(hit) => hit,
                    Err(_) => {
                        self.skipped_queries += 1;
                        continue;
                    }
                };
                let Some(hit) = hit else {
                    continue;
                };
                if !self.blocks.iter().any(|old| {
                    old.block_pos == hit.block_pos && old.selection_bounds == hit.selection_bounds
                }) {
                    self.blocks.push(hit);
                }
            }
        }
    }

    /// Visible face centers compete with actor centers using the same score function.
    pub(super) fn select_blocks<'a>(
        &mut self,
        blocks: &PaletteWorld<'_>,
        frustum: AimAssistFrustum,
        mode: CameraAimAssistTargetMode,
        rules: AimAssistCategory<'_>,
        identifier: impl Fn(u32) -> Option<&'a str>,
        tags: impl Fn(u32) -> &'a [Arc<str>],
    ) {
        for index in 0..self.blocks.len() {
            let hit = self.blocks[index];
            if !hit.targetable {
                continue;
            }
            let Some(identifier) = identifier(hit.runtime_id) else {
                continue;
            };
            let Some(priority) = rules.block_priority(identifier, tags(hit.runtime_id)) else {
                continue;
            };
            let minimum = render_vector(hit.selection_bounds.min);
            let maximum = render_vector(hit.selection_bounds.max);
            for face in 0..6 {
                let normal = face_normal(face);
                let center = (minimum + maximum) * 0.5;
                let point = center + normal * ((maximum - minimum) * 0.5);
                if (frustum.origin - point).dot(normal) <= 0.0
                    || self.obstructed(blocks, point + normal * 0.01, frustum.origin)
                {
                    continue;
                }
                self.keep_best(frustum.select(
                    mode,
                    [AimAssistCandidate {
                        kind: TargetKind::Block {
                            position: hit.block_pos,
                            face,
                        },
                        minimum,
                        maximum,
                        point,
                        priority,
                        obstructed: false,
                    }],
                ));
            }
        }
    }
}

/// Converts world geometry into the camera's float coordinate space.
fn render_vector(value: sim::Vec3) -> Vec3 {
    Vec3::new(value.x as f32, value.y as f32, value.z as f32)
}

/// Uses the packet face order down, up, north, south, west, east.
fn face_normal(face: u8) -> Vec3 {
    match face {
        0 => Vec3::NEG_Y,
        1 => Vec3::Y,
        2 => Vec3::NEG_Z,
        3 => Vec3::Z,
        4 => Vec3::NEG_X,
        _ => Vec3::X,
    }
}
