use super::{DroppedItemModel, ItemMeshVertex, cube_mesh, native_dropped_sprite_mesh};
use glam::{Mat4, Vec3, Vec4};
use std::sync::OnceLock;

/// Covers rounding in the products and sums used by affine and clip-space transforms.
const ROUNDING_ALLOWANCE: f32 = 8.0 * f32::EPSILON;

/// Bounds of the actual uploaded item mesh, in model or world coordinates.
#[derive(Clone, Copy, Debug)]
pub struct DroppedItemBounds {
    min: Vec3,
    max: Vec3,
}

/// The current view's reverse-Z near plane and its clip-transform rounding allowance.
#[derive(Clone, Copy, Debug)]
pub struct DroppedItemCullView {
    near_plane: Vec4,
    camera_position: Vec3,
    rounding: Vec4,
}

/// Presentation frustum used only when every consumer of the shared item scene uses this view.
#[derive(Clone, Copy, Debug)]
pub struct DroppedItemFrustum {
    planes: [Vec4; 6],
    rounding: [Vec4; 6],
    camera_position: Vec3,
}

impl DroppedItemFrustum {
    /// Reads WebGPU clip half-spaces, retaining draws for singular or non-finite projections.
    pub fn new(clip_from_world: Mat4, camera_position: Vec3) -> Option<Self> {
        if !clip_from_world.is_finite()
            || !camera_position.is_finite()
            || !clip_from_world.determinant().is_finite()
            || clip_from_world.determinant() == 0.0
            || !clip_from_world.inverse().is_finite()
        {
            return None;
        }
        let [x, y, z, w] = [0, 1, 2, 3].map(|row| clip_from_world.row(row));
        let planes = [w + x, w - x, w + y, w - y, z, w - z];
        let rounding = [
            w.abs() + x.abs(),
            w.abs() + x.abs(),
            w.abs() + y.abs(),
            w.abs() + y.abs(),
            z.abs(),
            w.abs() + z.abs(),
        ]
        .map(|row| row * ROUNDING_ALLOWANCE);
        // An infinite far plane has no normal and a positive constant, so rejects nothing.
        if planes.iter().any(|plane| {
            !plane.is_finite()
                || !plane.truncate().length_squared().is_finite()
                || (plane.truncate() == Vec3::ZERO && plane.w <= 0.0)
        }) || rounding.iter().any(|row| !row.is_finite())
        {
            return None;
        }
        Some(Self {
            planes,
            rounding,
            camera_position,
        })
    }

    /// Rejects only boxes wholly outside a plane, with an allowance for clip arithmetic.
    pub fn rejects(&self, bounds: DroppedItemBounds) -> bool {
        if self.camera_position.cmpge(bounds.min).all()
            && self.camera_position.cmple(bounds.max).all()
        {
            return false;
        }
        let magnitude = bounds.min.abs().max(bounds.max.abs()).extend(1.0);
        self.planes
            .iter()
            .zip(self.rounding)
            .any(|(plane, rounding)| {
                let farthest = Vec3::from_array(std::array::from_fn(|axis| {
                    if plane[axis] >= 0.0 {
                        bounds.max[axis]
                    } else {
                        bounds.min[axis]
                    }
                }));
                let distance = plane.dot(farthest.extend(1.0));
                let error = rounding.dot(magnitude);
                distance.is_finite() && error.is_finite() && distance < -error
            })
    }
}

impl DroppedItemCullView {
    /// Reads the near half-space `clip.w - clip.z > 0`; invalid projections retain rear faces.
    pub fn new(clip_from_world: Mat4, camera_position: Vec3) -> Option<Self> {
        let near_plane = clip_from_world.row(3) - clip_from_world.row(2);
        let rounding =
            (clip_from_world.row(3).abs() + clip_from_world.row(2).abs()) * ROUNDING_ALLOWANCE;
        let length = near_plane.truncate().length_squared();
        (clip_from_world.is_finite()
            && camera_position.is_finite()
            && rounding.is_finite()
            && length.is_finite()
            && length > 0.0)
            .then_some(Self {
                near_plane,
                camera_position,
                rounding,
            })
    }
}

impl DroppedItemBounds {
    /// Derives native bounds without scanning pixels or tessellating the retained model.
    /// A square sprite encloses every aspect ratio; block bounds include all authored corners.
    pub fn from_model(model: &DroppedItemModel) -> Option<Self> {
        static CUBE: OnceLock<Option<DroppedItemBounds>> = OnceLock::new();
        static SPRITE: OnceLock<Option<DroppedItemBounds>> = OnceLock::new();
        match model {
            DroppedItemModel::Cube(_) => {
                *CUBE.get_or_init(|| Self::from_mesh(&cube_mesh([0; 6], [0; 6], 1, 1)?))
            }
            DroppedItemModel::NativeSprite(_) => *SPRITE.get_or_init(|| {
                let mut bounds =
                    Self::from_mesh(&native_dropped_sprite_mesh(1, 1, &[255; 4], 1, 0)?)?;
                // Pixel normalization and default-origin corrections each round before upload.
                bounds.min -= Vec3::splat(ROUNDING_ALLOWANCE);
                bounds.max += Vec3::splat(ROUNDING_ALLOWANCE);
                Some(bounds)
            }),
            DroppedItemModel::Block(block) => {
                Self::from_positions(block.quads.iter().flat_map(|quad| {
                    quad.positions
                        .map(|point| super::native::block_item_position(point, block.rotation))
                }))
            }
            DroppedItemModel::Vacant | DroppedItemModel::Sprite(_) => None,
        }
    }

    /// Encloses all emitted positions; an empty or non-finite mesh cannot justify culling.
    pub fn from_mesh(mesh: &[ItemMeshVertex]) -> Option<Self> {
        Self::from_positions(mesh.iter().map(|vertex| vertex.position))
    }

    /// Encloses a nonempty stream of finite model positions.
    fn from_positions(positions: impl IntoIterator<Item = [f32; 3]>) -> Option<Self> {
        let mut positions = positions.into_iter();
        let mut min = Vec3::from_array(positions.next()?);
        if !min.is_finite() {
            return None;
        }
        let mut max = min;
        for position in positions {
            let position = Vec3::from_array(position);
            if !position.is_finite() {
                return None;
            }
            min = min.min(position);
            max = max.max(position);
        }
        Some(Self { min, max })
    }

    /// Transforms the box by intervals, padding for rounding in the shader's affine dot products.
    pub fn transformed(self, rows: [[f32; 4]; 3]) -> Option<Self> {
        let magnitude = self.min.abs().max(self.max.abs()).extend(1.0);
        let intervals = rows.map(|row| {
            let row = Vec4::from_array(row);
            let first = row.truncate() * self.min;
            let second = row.truncate() * self.max;
            let error = row.abs().dot(magnitude) * ROUNDING_ALLOWANCE;
            [
                first.min(second).element_sum() + row.w - error,
                first.max(second).element_sum() + row.w + error,
            ]
        });
        if intervals.iter().flatten().any(|value| !value.is_finite()) {
            return None;
        }
        Some(Self {
            min: Vec3::from_array(intervals.map(|range| range[0].next_down())),
            max: Vec3::from_array(intervals.map(|range| range[1].next_up())),
        })
    }

    /// Requires the entire world bound beyond this view's near plane.
    /// Intersecting bounds, including ones containing the camera, retain two-sided drawing.
    pub fn allows_back_face_culling(self, view: &DroppedItemCullView) -> bool {
        if view.camera_position.cmpge(self.min).all() && view.camera_position.cmple(self.max).all()
        {
            return false;
        }
        let nearest = Vec3::from_array(std::array::from_fn(|axis| {
            if view.near_plane[axis] >= 0.0 {
                self.min[axis]
            } else {
                self.max[axis]
            }
        }));
        let distance = view.near_plane.dot(nearest.extend(1.0));
        let error = view
            .rounding
            .dot(self.min.abs().max(self.max.abs()).extend(1.0));
        distance.is_finite() && error.is_finite() && distance > error
    }
}

#[cfg(test)]
mod tests;
