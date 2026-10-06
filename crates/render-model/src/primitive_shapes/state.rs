//! Packet patches become immutable GPU instance values only when a field changes.

use glam::{Mat4, Quat, Vec3};
use render_api::primitive_shapes::*;

/// Shared geometry is selected by kind and the server's segment count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PrimitiveMeshKey {
    pub kind: PrimitiveShapeKind,
    pub segments: u8,
}

/// A shape's stable GPU record; expiry and attachment are resolved in the vertex shader.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PrimitiveInstance {
    pub transform: [[f32; 4]; 4],
    pub color: [f32; 4],
    /// Arrow head length, head radius, endpoint distance, and maximum render distance.
    pub data: [f32; 4],
    /// Alive, dimension, actor slot (u32::MAX means detached), and primitive kind.
    pub meta: [u32; 4],
    /// Absolute expiry in shared clock seconds; negative means no deadline.
    pub lifetime: [f32; 4],
}

/// Complete retained fields for one network id; updates never reset omitted values.
#[derive(Clone, Debug, PartialEq)]
pub struct PrimitiveState {
    pub kind: PrimitiveShapeKind,
    pub location: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: f32,
    pub color: [f32; 4],
    pub total_time_left: Option<f32>,
    pub maximum_render_distance: f32,
    pub dimension: i32,
    pub attached_actor: Option<i64>,
    pub end: [f32; 3],
    pub bounds: [f32; 3],
    pub segments: u8,
    pub head_length: f32,
    pub head_radius: f32,
    pub text: Option<PrimitiveText>,
}

impl PrimitiveState {
    /// Initializes the native creation defaults before applying the first patch.
    pub fn new(kind: PrimitiveShapeKind) -> Self {
        Self {
            kind,
            location: [0.0; 3],
            rotation: [0.0; 3],
            scale: 1.0,
            color: [1.0; 4],
            total_time_left: None,
            maximum_render_distance: -1.0,
            dimension: PRIMITIVE_ALL_DIMENSIONS,
            attached_actor: None,
            end: [0.0; 3],
            bounds: [1.0; 3],
            segments: if kind == PrimitiveShapeKind::Arrow {
                PRIMITIVE_DEFAULT_ARROW_SEGMENTS
            } else {
                PRIMITIVE_DEFAULT_SEGMENTS
            },
            head_length: 1.0,
            head_radius: 0.5,
            text: None,
        }
    }

    /// Applies optional fields while preserving the shape's original concrete kind.
    pub fn patch(&mut self, update: PrimitiveShapeUpdate) {
        if let Some(value) = update.location {
            self.location = value;
        }
        if let Some(value) = update.rotation {
            self.rotation = value;
        }
        if let Some(value) = update.scale {
            self.scale = value;
        }
        if let Some(value) = update.color {
            self.color = value;
        }
        if let Some(value) = update.total_time_left {
            self.total_time_left = (value != 0.0).then_some(value);
        }
        if let Some(value) = update.maximum_render_distance {
            self.maximum_render_distance = value;
        }
        if let Some(value) = update.dimension {
            self.dimension = value;
        }
        if let Some(value) = update.attached_actor {
            self.attached_actor = (value != -1).then_some(value);
        }
        match (self.kind, update.data) {
            (PrimitiveShapeKind::Line, PrimitiveShapeData::Line { end }) => self.end = end,
            (PrimitiveShapeKind::Box, PrimitiveShapeData::Box { bounds }) => self.bounds = bounds,
            (
                PrimitiveShapeKind::Sphere | PrimitiveShapeKind::Circle,
                PrimitiveShapeData::Segments(segments),
            ) => self.segments = segments,
            (
                PrimitiveShapeKind::Arrow,
                PrimitiveShapeData::Arrow {
                    end,
                    head_length,
                    head_radius,
                    segments,
                },
            ) => {
                if let Some(value) = end {
                    self.end = value;
                }
                if let Some(value) = head_length {
                    self.head_length = value;
                }
                if let Some(value) = head_radius {
                    self.head_radius = value;
                }
                if let Some(value) = segments {
                    self.segments =
                        value.clamp(PRIMITIVE_MIN_ARROW_SEGMENTS, PRIMITIVE_MAX_ARROW_SEGMENTS);
                }
            }
            (PrimitiveShapeKind::Text, PrimitiveShapeData::Text(text)) => self.text = Some(text),
            _ => {}
        }
    }

    /// Chooses a shared mesh without duplicating geometry for individual instances.
    pub fn mesh_key(&self) -> PrimitiveMeshKey {
        PrimitiveMeshKey {
            kind: self.kind,
            segments: match self.kind {
                PrimitiveShapeKind::Sphere
                | PrimitiveShapeKind::Circle
                | PrimitiveShapeKind::Arrow => self.segments,
                _ => 0,
            },
        }
    }

    /// Builds a GPU record only for a changed shape; lines use absolute world endpoints.
    pub fn instance(&self, actor_slot: u32) -> PrimitiveInstance {
        let location = Vec3::from_array(self.location);
        let delta = Vec3::from_array(self.end) - location;
        let length = delta.length();
        let transform = match self.kind {
            PrimitiveShapeKind::Line => Mat4::from_cols(
                Vec3::X.extend(0.0),
                Vec3::Y.extend(0.0),
                delta.extend(0.0),
                location.extend(1.0),
            ),
            PrimitiveShapeKind::Arrow => {
                let direction = delta.try_normalize().unwrap_or(Vec3::Z);
                let normal = direction.abs();
                let axis = if normal.x < normal.y {
                    if normal.x < normal.z {
                        Vec3::X
                    } else {
                        Vec3::Z
                    }
                } else if normal.y < normal.z {
                    Vec3::Y
                } else {
                    Vec3::Z
                };
                let u = direction.cross(axis);
                let v = direction.cross(u);
                Mat4::from_cols(
                    u.extend(0.0),
                    v.extend(0.0),
                    direction.extend(0.0),
                    location.extend(1.0),
                )
            }
            PrimitiveShapeKind::Box => Mat4::from_scale_rotation_translation(
                Vec3::from_array(self.bounds) * self.scale,
                Quat::IDENTITY,
                location,
            ),
            PrimitiveShapeKind::Sphere | PrimitiveShapeKind::Circle => {
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(self.scale),
                    Quat::IDENTITY,
                    location,
                )
            }
            PrimitiveShapeKind::Text => Mat4::from_scale_rotation_translation(
                Vec3::splat(self.scale),
                Quat::from_euler(
                    glam::EulerRot::XYZEx,
                    self.rotation[0].to_radians(),
                    self.rotation[1].to_radians(),
                    self.rotation[2].to_radians(),
                ),
                location,
            ),
        };
        PrimitiveInstance {
            transform: transform.to_cols_array_2d(),
            color: self.color,
            data: [
                self.head_length,
                self.head_radius,
                length,
                self.maximum_render_distance,
            ],
            meta: [1, self.dimension as u32, actor_slot, self.kind as u32],
            lifetime: [-1.0, 0.0, 0.0, 0.0],
        }
    }
}
