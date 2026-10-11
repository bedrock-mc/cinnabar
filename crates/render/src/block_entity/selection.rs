//! Selection uses native wire-box geometry or an overlay on the selected model's faces.
use super::{BlockEntityVertex, CrackShape};
use bevy::{math::Vec3, prelude::Resource, render::extract_resource::ExtractResource};
use render_api::BLOCK_OVERLAY_FACE_OFFSET as FACE_OFFSET;
use std::sync::Arc;

const HIGHLIGHT_COLOR: [f32; 4] = [0.65, 0.65, 0.65, 1.0];
// Negative UV selects the untextured overlay branch, outside any atlas coordinates.
const UNTEXTURED_UV: [f32; 2] = [-1.0; 2];
pub const BLOCK_SELECTION_VERTICES_PER_EDGE: u32 = 6;

/// The selected block's native pick bounds and already-resolved model faces.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockSelectionTarget {
    pub block: [i32; 3],
    pub bounds: [[f32; 3]; 2],
    pub shape: CrackShape,
}

#[derive(Clone, Debug, Default, Resource, ExtractResource)]
#[extract_app(bevy::render::RenderApp)]
pub struct BlockSelectionFrame {
    pub revision: u64,
    /// Unexpanded endpoint pairs; the GPU supplies screen-space stroke coverage.
    pub outline: Arc<[BlockEntityVertex]>,
    pub highlight: Arc<[BlockEntityVertex]>,
    target: Option<BlockSelectionTarget>,
    outline_mode: bool,
}

impl BlockSelectionFrame {
    /// Publishes the current pick, or clears it when gameplay has no valid target.
    pub fn update(&mut self, target: Option<&BlockSelectionTarget>, outline: bool) {
        let target = target.filter(|target| {
            target.bounds.iter().flatten().all(|v| v.is_finite())
                && Vec3::from_array(target.bounds[0])
                    .cmplt(Vec3::from_array(target.bounds[1]))
                    .all()
        });
        if self.target.as_ref() == target && self.outline_mode == outline {
            return;
        }
        self.target = target.cloned();
        self.outline_mode = outline;
        let mut wire = Vec::new();
        let mut surface = Vec::new();
        if let Some(target) = target {
            let [min, max] = target.bounds;
            let (min, max) = (Vec3::from_array(min), Vec3::from_array(max));
            let corners = box_corners(min, max);
            if outline {
                for index in 0..8 {
                    for axis in 0..3 {
                        let other = index ^ (1 << axis);
                        if index < other {
                            wire.extend([corners[index], corners[other]].map(|point| {
                                BlockEntityVertex {
                                    position: point.to_array(),
                                    uv: UNTEXTURED_UV,
                                    color: [0.0, 0.0, 0.0, 1.0],
                                    ..Default::default()
                                }
                            }));
                        }
                    }
                }
            } else {
                match &target.shape {
                    CrackShape::Cube => {
                        for face in box_faces(corners) {
                            quad(&mut surface, face, HIGHLIGHT_COLOR, [0.0; 3]);
                        }
                    }
                    CrackShape::Quads(quads) => {
                        let block = Vec3::from_array(target.block.map(|v| v as f32));
                        for face in quads.iter() {
                            let (corners, normal) = face.overlay_geometry();
                            quad(
                                &mut surface,
                                corners.map(|corner| block + Vec3::from_array(corner)),
                                HIGHLIGHT_COLOR,
                                normal,
                            );
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

/// Packs one untextured quad into the shared storage-buffer triangle format.
fn quad(
    output: &mut Vec<BlockEntityVertex>,
    corners: [Vec3; 4],
    color: [f32; 4],
    normal: [f32; 3],
) {
    output.extend([0, 1, 2, 0, 2, 3].map(|index| BlockEntityVertex {
        position: corners[index].to_array(),
        uv: UNTEXTURED_UV,
        color,
        normal,
        ..Default::default()
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
