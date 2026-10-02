//! Selection uses native wire-box geometry or an overlay on the selected model's faces.
use super::{BlockEntityVertex, CrackShape, crack::FACE_OFFSET};
use bevy::{math::Vec3, prelude::Resource, render::extract_resource::ExtractResource};
use std::sync::Arc;

// R:l/LevelRendererPlayer.cpp:11653,11827,12038; Lens data 0x10dd71d9c/0x10dd71ec0.
const OUTLINE_ANGULAR_WIDTH: f32 = 0.003;
const HIGHLIGHT_COLOR: [f32; 4] = [0.65, 0.65, 0.65, 1.0];
// Negative UV selects the untextured overlay branch, outside any atlas coordinates.
const UNTEXTURED_UV: [f32; 2] = [-1.0; 2];

/// The selected block's native pick bounds and already-resolved model faces.
#[derive(Clone, Debug)]
pub struct BlockSelectionTarget {
    pub block: [i32; 3],
    pub bounds: [[f32; 3]; 2],
    pub shape: CrackShape,
}

#[derive(Clone, Debug, Default, Resource, ExtractResource)]
pub struct BlockSelectionFrame {
    pub revision: u64,
    pub outline: Arc<[BlockEntityVertex]>,
    pub highlight: Arc<[BlockEntityVertex]>,
}

impl BlockSelectionFrame {
    /// Publishes the current pick, or clears it when gameplay has no valid target.
    pub fn update(
        &mut self,
        target: Option<&BlockSelectionTarget>,
        eye: Vec3,
        forward: Vec3,
        outline: bool,
    ) {
        let mut wire = Vec::new();
        let mut surface = Vec::new();
        if let Some(target) =
            target.filter(|target| target.bounds.iter().flatten().all(|v| v.is_finite()))
        {
            let [min, max] = target.bounds;
            let (min, max) = (Vec3::from_array(min), Vec3::from_array(max));
            if min.cmplt(max).all() && eye.is_finite() && forward.is_finite() {
                let corners = box_corners(min, max);
                if outline {
                    let width = OUTLINE_ANGULAR_WIDTH
                        / ((eye.distance((min + max) * 0.5) - 2.0) * 0.5).clamp(1.0, 3.0);
                    for index in 0..8 {
                        for axis in 0..3 {
                            let other = index ^ (1 << axis);
                            if index < other {
                                line(
                                    &mut wire,
                                    corners[index],
                                    corners[other],
                                    eye,
                                    forward,
                                    width,
                                );
                            }
                        }
                    }
                } else {
                    match &target.shape {
                        CrackShape::Cube => {
                            for face in box_faces(corners) {
                                quad(&mut surface, face, HIGHLIGHT_COLOR);
                            }
                        }
                        CrackShape::Quads(quads) => {
                            let block = Vec3::from_array(target.block.map(|v| v as f32));
                            for face in quads.iter() {
                                let corners =
                                    face.corners.map(|corner| block + Vec3::from_array(corner));
                                let normal = (corners[1] - corners[0])
                                    .cross(corners[2] - corners[0])
                                    .normalize_or_zero();
                                let outward = if normal.dot(
                                    corners.iter().sum::<Vec3>() * 0.25 - block - Vec3::splat(0.5),
                                ) < 0.0
                                {
                                    -normal
                                } else {
                                    normal
                                };
                                quad(
                                    &mut surface,
                                    corners.map(|corner| corner + outward * FACE_OFFSET),
                                    HIGHLIGHT_COLOR,
                                );
                            }
                        }
                    }
                }
            }
        }
        if self.outline.as_ref() != wire || self.highlight.as_ref() != surface {
            self.outline = wire.into();
            self.highlight = surface.into();
            self.revision = self.revision.wrapping_add(1);
        }
    }
}

/// Returns bit-indexed corners for the selected shape's bounding box.
fn box_corners(min: Vec3, max: Vec3) -> [Vec3; 8] {
    std::array::from_fn(|index| {
        Vec3::new(
            if index & 1 == 0 { min.x } else { max.x },
            if index & 2 == 0 { min.y } else { max.y },
            if index & 4 == 0 { min.z } else { max.z },
        )
    })
}

/// Emits all outward-wound faces of a box, slightly in front of its backing model.
fn box_faces(corners: [Vec3; 8]) -> [[Vec3; 4]; 6] {
    let center = corners.iter().sum::<Vec3>() * 0.125;
    [
        [3, 2, 0, 1],
        [6, 7, 5, 4],
        [7, 3, 1, 5],
        [2, 6, 4, 0],
        [7, 6, 2, 3],
        [1, 0, 4, 5],
    ]
    .map(|face| {
        let face = face.map(|index| corners[index]);
        let mut normal = (face[1] - face[0])
            .cross(face[2] - face[0])
            .normalize_or_zero();
        if normal.dot(face.iter().sum::<Vec3>() * 0.25 - center) < 0.0 {
            normal = -normal;
        }
        face.map(|corner| corner + normal * FACE_OFFSET)
    })
}

/// Keeps wire edges camera-facing, including edges nearly parallel to the view direction.
fn line(
    output: &mut Vec<BlockEntityVertex>,
    a: Vec3,
    b: Vec3,
    eye: Vec3,
    forward: Vec3,
    width: f32,
) {
    let direction = (b - a).normalize_or_zero();
    let mut side = forward.cross(direction).normalize_or_zero();
    if side == Vec3::ZERO {
        side = (a - eye).cross(direction).normalize_or_zero();
    }
    if side == Vec3::ZERO {
        return;
    }
    let expand = |point: Vec3, sign: f32| {
        let ray = point - eye;
        let normalized = ray.normalize_or_zero();
        let facing = normalized.dot(forward).abs().max(f32::EPSILON);
        eye + (normalized / facing + side * width * sign).normalize_or_zero() * ray.length()
    };
    quad(
        output,
        [
            expand(a, -1.0),
            expand(b, -1.0),
            expand(b, 1.0),
            expand(a, 1.0),
        ],
        [0.0, 0.0, 0.0, 1.0],
    );
}

/// Packs one untextured quad into the shared storage-buffer triangle format.
fn quad(output: &mut Vec<BlockEntityVertex>, corners: [Vec3; 4], color: [f32; 4]) {
    output.extend([0, 1, 2, 0, 2, 3].map(|index| BlockEntityVertex {
        position: corners[index].to_array(),
        uv: UNTEXTURED_UV,
        color,
    }));
}

/// Supplies binding-compatible white pixels when the optional block-entity atlas is absent.
pub(super) fn fallback_atlas() -> &'static Arc<super::BlockEntityAtlasImage> {
    static ATLAS: std::sync::OnceLock<Arc<super::BlockEntityAtlasImage>> =
        std::sync::OnceLock::new();
    ATLAS.get_or_init(|| {
        Arc::new(super::BlockEntityAtlasImage {
            identity: [0; 32],
            size: [1; 2],
            static_height: 1,
            static_rgba8: Arc::from([255; 4]),
        })
    })
}
