//! CPU vertex emission for block-entity models: boxes with the entity-geometry UV
//! unwrap, free quads, and separate draw layers.

use assets::block_entity_geometry::BoxSpec;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use super::atlas::{AtlasRect, BlockEntityAtlas, TextureRef};

/// Hard ceiling on vertices per layer per frame; further quads are counted as rejected.
pub const MAX_BLOCK_ENTITY_VERTICES: usize = 393_216;
/// Packed 32-bit words per [`BlockEntityVertex`] as read by the shader.
pub const BLOCK_ENTITY_VERTEX_WORDS: usize =
    std::mem::size_of::<BlockEntityVertex>() / std::mem::size_of::<u32>();

/// Marks vertices whose lighting normal is also their geometric outward face normal.
pub const BLOCK_ENTITY_CULL_BACK_FACES: u32 = 1 << 29;
const _: () = assert!(
    BLOCK_ENTITY_CULL_BACK_FACES
        & (render_api::ACTOR_LIGHT_WORLD | render_api::ACTOR_LIGHT_DIRECTIONAL | u8::MAX as u32)
        == 0
);

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct BlockEntityVertex {
    pub position: [f32; 3],
    /// Normalized atlas coordinates.
    pub uv: [f32; 2],
    /// Model tint or portal normal/depth encoding; actor materials compose lit RGB in gamma.
    pub color: [f32; 4],
    /// Outward world normal for entity lighting or view-facing separation in scalar overlay passes.
    pub normal: [f32; 3],
    /// Packed actor light and geometric-face flags; zero retains the scalar-lit path.
    pub actor_light: u32,
}

/// Which pass a quad is drawn in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Layer {
    /// Alpha-tested and depth-writing.
    Solid,
    /// Blended over the scene without writing depth.
    Overlay,
    /// Multiplies the scene (twice source times destination) without writing depth.
    Crack,
    /// Native portal normal/depth encoding, composited in coplanar layer order.
    Portal,
}

// Directional face shade; needs native measurement against Bedrock entity lighting.
const SHADE_UP: f32 = 1.0;
const SHADE_DOWN: f32 = 0.5;
const SHADE_Z: f32 = 0.8;
const SHADE_X: f32 = 0.6;

/// Directional brightness in the shared terrain face order.
pub(super) const fn tile_face_shade(face: usize) -> f32 {
    [SHADE_X, SHADE_X, SHADE_DOWN, SHADE_UP, SHADE_Z, SHADE_Z][face]
}

pub const WHITE: [f32; 4] = [1.0; 4];

/// Accumulates each draw layer's vertices against one atlas.
#[derive(Debug)]
pub struct MeshBuilder {
    atlas_size: [f32; 2],
    pub solid: Vec<BlockEntityVertex>,
    pub overlay: Vec<BlockEntityVertex>,
    pub crack: Vec<BlockEntityVertex>,
    pub portal: Vec<BlockEntityVertex>,
    pub additive: Vec<BlockEntityVertex>,
    /// Multiplier applied to model RGB; encoded portal planes bypass lighting.
    pub light: f32,
    pub(super) actor_light: u32,
    /// Whether a required model texture was absent from the atlas.
    pub(super) missing_required_texture: bool,
    pub rejected_quads: u64,
}

impl MeshBuilder {
    #[must_use]
    pub fn new(atlas_size: [u32; 2]) -> Self {
        Self {
            atlas_size: [atlas_size[0] as f32, atlas_size[1] as f32],
            solid: Vec::new(),
            overlay: Vec::new(),
            crack: Vec::new(),
            portal: Vec::new(),
            additive: Vec::new(),
            light: 1.0,
            actor_light: 0,
            missing_required_texture: false,
            rejected_quads: 0,
        }
    }

    /// Empties every layer for a new frame against `atlas_size`, keeping the layers' capacity;
    /// the builder then emits exactly as one made by [`Self::new`].
    pub(super) fn reset(&mut self, atlas_size: [u32; 2]) {
        let mut layers = [
            std::mem::take(&mut self.solid),
            std::mem::take(&mut self.overlay),
            std::mem::take(&mut self.crack),
            std::mem::take(&mut self.portal),
            std::mem::take(&mut self.additive),
        ];
        for layer in &mut layers {
            layer.clear();
        }
        let [solid, overlay, crack, portal, additive] = layers;
        *self = Self {
            solid,
            overlay,
            crack,
            portal,
            additive,
            ..Self::new(atlas_size)
        };
    }

    /// Resolves a required model texture and records missing artwork without stopping emission.
    pub(super) fn required_texture(
        &mut self,
        atlas: &BlockEntityAtlas,
        name: &str,
        logical: [f32; 2],
    ) -> Option<TextureRef> {
        let texture = atlas.texture(name, logical);
        self.missing_required_texture |= texture.is_none();
        texture
    }

    /// Emits a box; `model` maps authoring pixels to world space.
    pub fn cuboid(
        &mut self,
        layer: Layer,
        texture: &TextureRef,
        model: Mat4,
        spec: BoxSpec,
        tint: [f32; 4],
    ) {
        self.box_faces(layer, texture, model, spec.faces(), tint);
    }

    /// Emits a box with no depth, such as a book cover, as its front and back faces moved
    /// `gap` pixels apart. Without culling the two faces would otherwise fight, and giving the
    /// box real depth would add edge faces that sample texels outside its art.
    pub fn sheet(
        &mut self,
        layer: Layer,
        texture: &TextureRef,
        model: Mat4,
        spec: BoxSpec,
        gap: f32,
        tint: [f32; 4],
    ) {
        let [sx, sy, _] = spec.size;
        let mut faces = BoxSpec::new(spec.origin, [sx, sy, 0.0], spec.uv)
            .mirrored(spec.mirror)
            .faces();
        for (face, shift) in faces.iter_mut().zip([-gap / 2.0, gap / 2.0]) {
            for corner in &mut face.0 {
                corner[2] += shift;
            }
        }
        self.box_faces(layer, texture, model, faces, tint);
    }

    /// Emits the six faces of a box with their face shades.
    fn box_faces(
        &mut self,
        layer: Layer,
        texture: &TextureRef,
        model: Mat4,
        faces: [assets::block_entity_geometry::ModelFace; 6],
        tint: [f32; 4],
    ) {
        for ((corners, texels), shade) in faces
            .into_iter()
            .zip([SHADE_Z, SHADE_Z, SHADE_X, SHADE_X, SHADE_UP, SHADE_DOWN])
        {
            if texels[2] <= 0.0 || texels[3] == 0.0 {
                continue;
            }
            let uv = texture.rect_uv(texels);
            let shade = if self.actor_light == 0 { shade } else { 1.0 };
            let color = [tint[0] * shade, tint[1] * shade, tint[2] * shade, tint[3]];
            let world = corners.map(|corner| model.transform_point3(Vec3::from_array(corner)));
            self.quad(layer, world.map(|point| point.to_array()), uv, color);
        }
    }

    /// Emits one quad from corners `[top-left, top-right, bottom-right, bottom-left]`
    /// and atlas-pixel UVs `[u0, v0, u1, v1]`.
    pub fn quad(&mut self, layer: Layer, corners: [[f32; 3]; 4], uv: [f32; 4], color: [f32; 4]) {
        let [u0, v0, u1, v1] = uv;
        self.quad_uv(
            layer,
            corners,
            [[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
            color,
        );
    }

    /// Emits one quad with an atlas-pixel UV per corner, in corner order.
    pub fn quad_uv(
        &mut self,
        layer: Layer,
        corners: [[f32; 3]; 4],
        uvs: [[f32; 2]; 4],
        color: [f32; 4],
    ) {
        self.quad_uv_colors(layer, corners, uvs, [color; 4]);
    }

    /// Emits one quad with UVs and colors per corner, preserving effect gradients.
    pub(super) fn quad_uv_colors(
        &mut self,
        layer: Layer,
        corners: [[f32; 3]; 4],
        uvs: [[f32; 2]; 4],
        colors: [[f32; 4]; 4],
    ) {
        let target = match layer {
            Layer::Solid => &mut self.solid,
            Layer::Overlay => &mut self.overlay,
            Layer::Crack => &mut self.crack,
            Layer::Portal => &mut self.portal,
        };
        if target.len() + 6 > MAX_BLOCK_ENTITY_VERTICES {
            self.rejected_quads = self.rejected_quads.saturating_add(1);
            return;
        }
        // Portal RGB encodes the plane normal; lighting would corrupt the projector.
        let light = if layer == Layer::Portal {
            1.0
        } else {
            self.light
        };
        let atlas_size = self.atlas_size;
        let normal = if self.actor_light == 0 {
            [0.0; 3]
        } else {
            let origin = Vec3::from_array(corners[0]);
            (Vec3::from_array(corners[2]) - origin)
                .cross(Vec3::from_array(corners[1]) - origin)
                .normalize_or_zero()
                .to_array()
        };
        let vertex = |corner: usize| BlockEntityVertex {
            position: corners[corner],
            uv: [
                uvs[corner][0] / atlas_size[0],
                uvs[corner][1] / atlas_size[1],
            ],
            color: [
                colors[corner][0] * light,
                colors[corner][1] * light,
                colors[corner][2] * light,
                colors[corner][3],
            ],
            normal,
            actor_light: if self.actor_light == 0 {
                0
            } else {
                self.actor_light | BLOCK_ENTITY_CULL_BACK_FACES
            },
        };
        let [first, second, third, fourth] = [vertex(0), vertex(1), vertex(2), vertex(3)];
        target.extend_from_slice(&[second, third, first, first, third, fourth]);
    }

    /// Repeats the beam tile in the shader, with a fixed six vertices for any height.
    pub(super) fn repeating_beam_quad(
        &mut self,
        corners: [[f32; 3]; 4],
        rect: AtlasRect,
        color: [f32; 4],
        scroll: f32,
    ) {
        let start = self.overlay.len();
        self.textured_quad(Layer::Overlay, corners, rect, color);
        for vertex in &mut self.overlay[start..] {
            vertex.uv[1] = -vertex.position[1] + scroll;
            vertex.normal = [
                rect.y / self.atlas_size[1],
                rect.height / self.atlas_size[1],
                0.0,
            ];
            vertex.actor_light = u32::MAX;
        }
    }

    /// A box whose faces each show one whole atlas rect, in `[west, east, down, up, north,
    /// south]` order; `min`/`max` are authoring pixels mapped through `model`.
    pub fn tile_cuboid(
        &mut self,
        layer: Layer,
        model: Mat4,
        min: [f32; 3],
        max: [f32; 3],
        rects: [AtlasRect; 6],
        tint: [f32; 4],
    ) {
        let faces = assets::block_entity_geometry::tile_box_faces([min, max]);
        for ((corners, shade), rect) in faces
            .into_iter()
            .zip((0..6).map(tile_face_shade))
            .zip(rects)
        {
            let world = corners.map(|corner| model.transform_point3(Vec3::from_array(corner)));
            self.textured_quad(
                layer,
                world.map(|point| point.to_array()),
                rect,
                [tint[0] * shade, tint[1] * shade, tint[2] * shade, tint[3]],
            );
        }
    }

    /// A quad covering `rect` (atlas pixels) with the given world corners.
    pub fn textured_quad(
        &mut self,
        layer: Layer,
        corners: [[f32; 3]; 4],
        rect: AtlasRect,
        color: [f32; 4],
    ) {
        self.quad(
            layer,
            corners,
            [rect.x, rect.y, rect.x + rect.width, rect.y + rect.height],
            color,
        );
    }
}

/// Maps authoring pixels to world space for a model centered on `center` blocks past the
/// block origin, turned `yaw_degrees` about +Y.
#[must_use]
pub fn model_matrix(block: [i32; 3], center: [f32; 3], yaw_degrees: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(
        block[0] as f32 + center[0],
        block[1] as f32 + center[1],
        block[2] as f32 + center[2],
    )) * Mat4::from_rotation_y(yaw_degrees.to_radians())
        * Mat4::from_scale(Vec3::splat(1.0 / 16.0))
}

/// The four horizontal directions a block entity can face; models front -Z at yaw 0.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Facing {
    North,
    South,
    West,
    East,
}

impl Facing {
    #[must_use]
    pub const fn yaw_degrees(self) -> f32 {
        match self {
            Self::North => 0.0,
            Self::West => 90.0,
            Self::South => 180.0,
            Self::East => 270.0,
        }
    }

    /// Bedrock `facing_direction` ids 2..=5; other ids are `None`.
    #[must_use]
    pub const fn from_facing_direction(id: i64) -> Option<Self> {
        match id {
            2 => Some(Self::North),
            3 => Some(Self::South),
            4 => Some(Self::West),
            5 => Some(Self::East),
            _ => None,
        }
    }

    #[must_use]
    pub fn from_cardinal(name: &str) -> Option<Self> {
        match name {
            "north" => Some(Self::North),
            "south" => Some(Self::South),
            "west" => Some(Self::West),
            "east" => Some(Self::East),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texture() -> TextureRef {
        TextureRef {
            rect: AtlasRect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 64.0,
            },
            logical: [64.0, 64.0],
        }
    }

    #[test]
    fn cuboid_emits_six_quads_with_entity_unwrap_uvs() {
        let mut builder = MeshBuilder::new([64, 64]);
        builder.cuboid(
            Layer::Solid,
            &texture(),
            Mat4::IDENTITY,
            BoxSpec::new([-4.0, 0.0, -4.0], [8.0; 3], [0.0, 0.0]),
            WHITE,
        );
        assert_eq!(builder.solid.len(), 36);
        // The first quad is the front face (-Z): it samples texels 8..16 on both axes.
        assert!(builder.solid[..6].iter().all(|vertex| {
            (8.0 / 64.0..=16.0 / 64.0).contains(&vertex.uv[0])
                && (8.0 / 64.0..=16.0 / 64.0).contains(&vertex.uv[1])
        }));
    }

    #[test]
    fn a_sheet_draws_only_its_two_faces_inside_its_art() {
        let mut builder = MeshBuilder::new([64, 64]);
        builder.sheet(
            Layer::Solid,
            &texture(),
            Mat4::IDENTITY,
            BoxSpec::new([0.0, 0.0, 0.0], [6.0, 10.0, 0.0], [16.0, 0.0]),
            0.01,
            WHITE,
        );
        assert_eq!(builder.solid.len(), 12, "no edge faces");
        assert!(builder.solid.iter().all(|vertex| {
            let u = vertex.uv[0] * 64.0;
            let v = vertex.uv[1] * 64.0;
            ((16.0..=22.0).contains(&u) || (22.0..=28.0).contains(&u)) && (0.0..=10.0).contains(&v)
        }));
        let depths: Vec<f32> = builder
            .solid
            .iter()
            .map(|vertex| vertex.position[2])
            .collect();
        assert!(depths[..6].iter().all(|z| (*z + 0.005).abs() < 1.0e-6));
        assert!(depths[6..].iter().all(|z| (*z - 0.005).abs() < 1.0e-6));
    }

    #[test]
    fn mirrored_sheets_keep_their_artwork_and_outward_faces() {
        let mut builder = MeshBuilder::new([64, 64]);
        builder.actor_light = render_api::ACTOR_LIGHT_WORLD;
        builder.sheet(
            Layer::Solid,
            &texture(),
            Mat4::IDENTITY,
            BoxSpec::new([0.0; 3], [6.0, 10.0, 0.0], [16.0, 0.0]).mirrored(true),
            0.01,
            WHITE,
        );
        assert_eq!(builder.solid.len(), 12);
        let front_left = builder.solid[..6]
            .iter()
            .find(|vertex| vertex.position[..2] == [0.0, 10.0])
            .unwrap();
        assert_eq!(front_left.uv, [16.0 / 64.0, 0.0]);
        for (face, normal) in builder
            .solid
            .as_chunks::<6>()
            .0
            .iter()
            .zip([Vec3::NEG_Z, Vec3::Z])
        {
            assert!(face.iter().all(|vertex| vertex.normal == normal.to_array()));
        }
    }

    #[test]
    fn facing_yaw_turns_the_front_toward_the_named_direction() {
        for (facing, expected) in [
            (Facing::North, Vec3::NEG_Z),
            (Facing::South, Vec3::Z),
            (Facing::West, Vec3::NEG_X),
            (Facing::East, Vec3::X),
        ] {
            let front = Mat4::from_rotation_y(facing.yaw_degrees().to_radians())
                .transform_vector3(Vec3::NEG_Z);
            assert!(front.abs_diff_eq(expected, 1.0e-5), "{facing:?} {front:?}");
        }
    }

    #[test]
    fn vertex_budget_rejects_instead_of_growing() {
        let mut builder = MeshBuilder::new([16, 16]);
        for _ in 0..=MAX_BLOCK_ENTITY_VERTICES / 6 {
            builder.quad(Layer::Solid, [[0.0; 3]; 4], [0.0; 4], WHITE);
        }
        assert_eq!(builder.solid.len(), MAX_BLOCK_ENTITY_VERTICES);
        assert_eq!(builder.rejected_quads, 1);
    }
}
