//! The per-frame view uniform and the colour tables' GPU words.

use bevy::{camera::primitives::Frustum, render::view::ExtractedView};
use lod::colour::TintClass;
use render_scene::{
    AtmosphereFrame,
    distant_terrain::{DistantTerrainColourTables, ResidentColumnMask},
};

use super::layout::{LodView, WATER_ALPHA, shade_uniform};

/// The view's eye in blocks when it is an unmirrored perspective view distant terrain can
/// draw into.
pub(crate) fn perspective_eye(view: &ExtractedView) -> Option<[f64; 3]> {
    let projection = view.clip_from_view;
    let perspective = projection.x_axis.w == 0.0
        && projection.y_axis.w == 0.0
        && projection.z_axis.w != 0.0
        && projection.w_axis.w == 0.0;
    let unmirrored = projection.determinant() > 0.0
        && view.world_from_view.affine().matrix3.determinant() > 0.0
        && !view.invert_culling;
    (perspective && unmirrored).then(|| view.world_from_view.translation().as_dvec3().to_array())
}

/// What [`lod_view`] reads besides the view.
pub(crate) struct ViewInputs<'a> {
    pub(crate) eye: [f64; 3],
    pub(crate) depth_size: [u32; 2],
    /// Mips of the pyramid; zero turns Hi-Z off.
    pub(crate) hiz_mips: u32,
    /// Section slots, cluster slots, translucent order entries, cluster capacity.
    pub(crate) counts: [u32; 4],
    pub(crate) resident: Option<&'a ResidentColumnMask>,
    pub(crate) atmosphere: &'a AtmosphereFrame,
    pub(crate) tables: &'a PackedTables,
}

/// The [`LodView`] of `view`: matrices and planes relative to the integer eye, so far
/// sections keep their precision.
pub(crate) fn lod_view(
    view: &ExtractedView,
    frustum: &Frustum,
    inputs: &ViewInputs<'_>,
) -> LodView {
    let world_from_view = view.world_from_view.to_matrix().as_dmat4();
    let clip_from_world = view
        .clip_from_world
        .map(|matrix| matrix.as_dmat4())
        .unwrap_or_else(|| view.clip_from_view.as_dmat4() * world_from_view.inverse());
    let floor = inputs.eye.map(f64::floor);
    let m = clip_from_world.to_cols_array_2d();
    let translation: [f64; 4] = std::array::from_fn(|row| {
        m[0][row] * floor[0] + m[1][row] * floor[1] + m[2][row] * floor[2] + m[3][row]
    });
    let columns = [m[0], m[1], m[2], translation];
    let planes = std::array::from_fn(|index| {
        let plane = frustum.half_spaces[index].normal_d().as_dvec4().to_array();
        let d = plane[3] + plane[0] * floor[0] + plane[1] * floor[1] + plane[2] * floor[2];
        [plane[0] as f32, plane[1] as f32, plane[2] as f32, d as f32]
    });
    let resident = inputs.resident.map_or([0; 4], |mask| {
        [
            mask.centre[0],
            mask.centre[1],
            ResidentColumnMask::RADIUS,
            1,
        ]
    });
    let fog = inputs.atmosphere.fog_color();
    LodView {
        clip_from_rel: columns.map(|column| column.map(|value| value as f32)),
        planes,
        camera: [floor[0] as i32, floor[1] as i32, floor[2] as i32, 0],
        camera_frac: [
            (inputs.eye[0] - floor[0]) as f32,
            (inputs.eye[1] - floor[1]) as f32,
            (inputs.eye[2] - floor[2]) as f32,
            0.0,
        ],
        viewport: view.viewport.as_vec4().to_array(),
        depth: [
            inputs.depth_size[0],
            inputs.depth_size[1],
            inputs.hiz_mips,
            u32::from(inputs.hiz_mips != 0),
        ],
        counts: inputs.counts,
        resident,
        fog: [fog[0], fog[1], fog[2], inputs.atmosphere.fog_start()],
        fog_end: [inputs.atmosphere.fog_end(), WATER_ALPHA, 0.0, 0.0],
        shade: shade_uniform(),
        tables: inputs.tables.info,
        tables_extra: [inputs.tables.records, 0, 0, 0],
    }
}

/// The colour tables as one word array: face colours by material and face, then each raw
/// biome id's record, then each record's linear tints by class as `f32` bits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PackedTables {
    pub(crate) words: Vec<u32>,
    /// Material entries, raw biome ids, the first raw id word, the first record word.
    pub(crate) info: [u32; 4],
    /// Tint records; zero leaves faces untinted.
    pub(crate) records: u32,
}

impl PackedTables {
    /// Packs `colours`.
    pub(crate) fn new(colours: &DistantTerrainColourTables) -> Self {
        let mut words: Vec<u32> = colours
            .materials
            .entries()
            .iter()
            .flatten()
            .map(|colour| colour.bits())
            .collect();
        let materials = colours.materials.len() as u32;
        let raw_start = words.len() as u32;
        let (raw, records) = colours.biomes.as_deref().map_or((0, 0), |tints| {
            words.extend_from_slice(tints.by_raw_id());
            (tints.by_raw_id().len() as u32, tints.records().len() as u32)
        });
        let records_start = words.len() as u32;
        if let Some(tints) = colours.biomes.as_deref() {
            for record in tints.records() {
                for class in TintClass::ALL {
                    words.extend(record[class.index()].map(f32::to_bits));
                }
            }
        }
        if words.is_empty() {
            words.push(0);
        }
        Self {
            words,
            info: [materials, raw, raw_start, records_start],
            records,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lod::{
        colour::{BiomeTints, FaceColour, MaterialColours},
        geometry::Face,
        voxel::MaterialId,
    };

    use super::*;

    #[test]
    fn tables_pack_materials_raw_ids_and_tint_records() {
        let mut materials = MaterialColours::new();
        let stone = FaceColour::from_srgb([90, 91, 92], TintClass::None, 0);
        let grass = FaceColour::from_srgb([10, 200, 10], TintClass::Grass, 31);
        let mut faces = [stone; 6];
        faces[Face::PosY.index()] = grass;
        assert!(materials.push(faces));
        let mut record = [[0.5_f32; 3]; TintClass::COUNT];
        record[TintClass::Grass.index()] = [0.1, 0.8, 0.2];
        let tints = BiomeTints::new(Box::new([0, 0, 0]), Box::new([record]));
        let packed = PackedTables::new(&DistantTerrainColourTables {
            materials: Arc::new(materials),
            biomes: Some(Arc::new(tints)),
        });
        let [count, raw, raw_start, records_start] = packed.info;
        assert_eq!(count, 2);
        let material = MaterialId::FIRST_BLOCK.raw() as usize;
        assert_eq!(
            packed.words[material * 6 + Face::PosY.index()],
            grass.bits()
        );
        assert_eq!(
            packed.words[material * 6 + Face::NegX.index()],
            stone.bits()
        );
        assert_eq!(raw, 3);
        assert_eq!(&packed.words[raw_start as usize..][..3], &[0, 0, 0]);
        assert_eq!(packed.records, 1);
        let grass_at = records_start as usize + TintClass::Grass.index() * 3;
        assert_eq!(f32::from_bits(packed.words[grass_at + 1]), 0.8);
        let none_at = records_start as usize + TintClass::None.index() * 3;
        assert_eq!(
            f32::from_bits(packed.words[none_at]),
            1.0,
            "untinted stays white"
        );
    }
}
