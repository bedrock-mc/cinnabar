//! Resident surface occupancy and authored diffuse reflectance for spatial light transport.

use super::{CELL_SIZE, GRID_SIZE, HEADER_WORDS};
use crate::{ChunkBiomeTints, ChunkRenderInstance, ChunkTextureAssets};
use bevy::prelude::*;
use std::{collections::HashMap, sync::Arc};

const OCCUPIED_BIT: u32 = 1;
const SURFACE_BIT: u32 = 1 << 1;
const NORMAL_SHIFT: u32 = 2;
const SUBCELL_SHIFT: u32 = 8;
const SUBCELL_MASK_BITS: u32 = 0xff << SUBCELL_SHIFT;

/// Model triangles are inflated slightly in the voxelizer so a sub-block face
/// cannot disappear solely because it lies on a voxel boundary.
const MODEL_VOXEL_INFLATION: f32 = 0.08;

pub(crate) struct GeometryChunk {
    key: world::SubChunkKey,
    cubes: Arc<[meshing::PackedQuad]>,
    models: Arc<[meshing::PackedModelRef]>,
    cube_light: Arc<[meshing::PackedQuadLighting]>,
    model_light: Arc<[meshing::PackedQuadLighting]>,
    biome: meshing::PackedBiomeRecord,
    revision: u64,
}

fn same_stream<T: PartialEq>(previous: &Arc<[T]>, current: &Arc<[T]>) -> bool {
    Arc::ptr_eq(previous, current) || previous.as_ref() == current.as_ref()
}

fn same_sky(
    previous: &Arc<[meshing::PackedQuadLighting]>,
    current: &Arc<[meshing::PackedQuadLighting]>,
) -> bool {
    Arc::ptr_eq(previous, current)
        || previous.len() == current.len()
            && previous
                .iter()
                .zip(current.iter())
                .all(|(previous, current)| sky_access(Some(previous)) == sky_access(Some(current)))
}

#[derive(Resource, Default)]
pub(crate) struct IndirectGeometry {
    chunks: HashMap<Entity, GeometryChunk>,
    pub revision: u64,
}

pub(crate) fn collect_geometry(
    mut resident: ResMut<IndirectGeometry>,
    changed: Query<(Entity, &ChunkRenderInstance), Changed<ChunkRenderInstance>>,
    mut removed: RemovedComponents<ChunkRenderInstance>,
) {
    let mut dirty = false;
    for entity in removed.read() {
        dirty |= resident.chunks.remove(&entity).is_some();
    }
    for (entity, chunk) in &changed {
        let (cubes, models, cube_light, model_light) = chunk.indirect_geometry();
        if let Some(previous) = resident.chunks.get_mut(&entity)
            && previous.key == chunk.key()
            && same_stream(&previous.cubes, &cubes)
            && same_stream(&previous.models, &models)
            && same_sky(&previous.cube_light, &cube_light)
            && same_sky(&previous.model_light, &model_light)
            && previous.biome == *chunk.biome_record()
        {
            previous.cubes = cubes;
            previous.models = models;
            previous.cube_light = cube_light;
            previous.model_light = model_light;
            continue;
        }
        let revision = resident.revision.wrapping_add(1);
        resident.chunks.insert(
            entity,
            GeometryChunk {
                key: chunk.key(),
                cubes,
                models,
                cube_light,
                model_light,
                biome: chunk.biome_record().clone(),
                revision,
            },
        );
        dirty = true;
    }
    if dirty {
        resident.revision = resident.revision.wrapping_add(1);
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct Reflectance {
    pub rgb: Vec3,
    pub opacity: f32,
    pub flags: u32,
    pub sky: f32,
}

pub(super) fn palette(textures: &ChunkTextureAssets, output: &mut Vec<Reflectance>) {
    output.clear();
    for material in textures.assets().materials() {
        let mut page = material.texture.page() as usize;
        let mut layer = material.texture.layer() as usize;
        let mut color = &textures.assets().texture_pages()[page].texture;
        let mut mer = None;
        let mut lab = false;
        if let Some(authored) = textures.enhanced() {
            let address = page * assets::MAX_TEXTURE_LAYERS + layer;
            if let Some(&reference) = authored
                .texture_refs
                .get(address)
                .filter(|&&v| v != u32::MAX)
            {
                page = (reference >> 31) as usize;
                layer = (reference & 0x7ff) as usize;
                color = &authored.color_pages[page];
                mer = Some(&authored.mer_pages[page]);
                lab = reference & assets::PBR_REF_LABPBR != 0;
            }
        }
        let rgba = color
            .mips
            .last()
            .and_then(|mip| mip.rgba8.get(layer * 4..layer * 4 + 4));
        let mut result = Reflectance {
            flags: material.flags,
            ..Default::default()
        };
        if let Some(rgba) = rgba {
            result.rgb = Vec3::from_array([linear(rgba[0]), linear(rgba[1]), linear(rgba[2])]);
            result.opacity = if material.flags
                & (assets::MATERIAL_FLAG_ALPHA_CUTOUT | assets::MATERIAL_FLAG_ALPHA_BLEND)
                == 0
            {
                1.0
            } else {
                rgba[3] as f32 / 255.0
            };
            if let Some(metallic) = mer
                .and_then(|mer| mer.mips.last())
                .and_then(|mip| mip.rgba8.get(layer * 4))
            {
                let metallic = if lab {
                    if *metallic >= 230 { 1.0 } else { 0.0 }
                } else {
                    *metallic as f32 / 255.0
                };
                result.rgb *= 1.0 - metallic;
            }
        }
        output.push(result);
    }
}

impl IndirectGeometry {
    pub(super) fn region_revision(&self, origin: IVec3, dimension: Option<i32>) -> u64 {
        use std::hash::{Hash, Hasher};
        let end = origin + IVec3::from_array(GRID_SIZE.map(|v| v as i32));
        self.chunks
            .values()
            .filter(|chunk| {
                let base = IVec3::new(chunk.key.x, chunk.key.y, chunk.key.z)
                    * world::SUB_CHUNK_SIDE as i32;
                Some(chunk.key.dimension) == dimension
                    && !base.cmpge(end).any()
                    && !(base + IVec3::splat(world::SUB_CHUNK_SIDE as i32))
                        .cmple(origin)
                        .any()
            })
            .fold(0, |signature, chunk| {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                chunk.key.hash(&mut hasher);
                chunk.revision.hash(&mut hasher);
                signature ^ hasher.finish()
            })
    }
}

fn tinted(
    mut material: Reflectance,
    chunk: &GeometryChunk,
    tints: &ChunkBiomeTints,
    local: IVec3,
) -> Reflectance {
    let index = chunk.biome.tint_index_at(local.to_array()).unwrap_or(0) as usize;
    let Some(tint) = tints
        .entries()
        .get(index)
        .or_else(|| tints.entries().first())
    else {
        return material;
    };
    let flags = material.flags;
    let color = match flags & assets::MATERIAL_FLAG_TINT_MASK {
        assets::MATERIAL_FLAG_GRASS_TINT => tint.grass,
        assets::MATERIAL_FLAG_FOLIAGE_TINT => {
            if flags & assets::MATERIAL_FLAG_SEASONAL_FOLIAGE != 0 {
                tint.seasonal_foliage[assets::seasonal_foliage_palette_index(
                    flags,
                    flags & assets::MATERIAL_FLAG_EXPOSED_FOLIAGE != 0,
                )]
            } else {
                match flags & assets::MATERIAL_FLAG_FOLIAGE_CLASS_MASK {
                    assets::MATERIAL_FLAG_BIRCH_FOLIAGE => tint.birch,
                    assets::MATERIAL_FLAG_EVERGREEN_FOLIAGE => tint.evergreen,
                    assets::MATERIAL_FLAG_DRY_FOLIAGE => tint.dry_foliage,
                    _ => tint.foliage,
                }
            }
        }
        assets::MATERIAL_FLAG_WATER_TINT => tint.water,
        _ => [1.0; 3],
    };
    material.rgb *= Vec3::from_array(color);
    material
}

fn linear(value: u8) -> f32 {
    let x = value as f32 / 255.0;
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

pub(super) fn index(origin: IVec3, world: IVec3) -> Option<usize> {
    let p = world - origin;
    if p.cmplt(IVec3::ZERO).any()
        || p.cmpge(IVec3::from_array(GRID_SIZE.map(|v| v as i32)))
            .any()
    {
        return None;
    }
    Some(
        HEADER_WORDS
            + ((p.z as usize * GRID_SIZE[1] as usize + p.y as usize) * GRID_SIZE[0] as usize
                + p.x as usize),
    )
}

pub(super) fn coverage_signature(
    coverage: &crate::chunk::resident_coverage::ChunkResidentCoverage,
    origin: IVec3,
    dimension: Option<i32>,
) -> u64 {
    let Some(dimension) = dimension else {
        return 0;
    };
    let side = world::SUB_CHUNK_SIDE as i32;
    let lo = origin.div_euclid(IVec3::splat(side));
    let hi = (origin + IVec3::from_array(GRID_SIZE.map(|v| v as i32)) - IVec3::ONE)
        .div_euclid(IVec3::splat(side));
    coverage.with_keys(|keys| {
        let mut signature = 0u64;
        let mut bit = 0u32;
        for z in lo.z..=hi.z {
            for y in lo.y..=hi.y {
                for x in lo.x..=hi.x {
                    if keys.contains(&world::SubChunkKey::new(dimension, x, y, z)) {
                        signature |= 1u64 << bit;
                    }
                    bit += 1;
                }
            }
        }
        signature
    })
}

fn known_cells(
    coverage: &crate::chunk::resident_coverage::ChunkResidentCoverage,
    origin: IVec3,
    dimension: Option<i32>,
    words: &mut [[u32; 4]],
) {
    let Some(dimension) = dimension else {
        return;
    };
    let side = world::SUB_CHUNK_SIDE as i32;
    let end = origin + IVec3::from_array(GRID_SIZE.map(|v| v as i32));
    let first = origin.div_euclid(IVec3::splat(side));
    let last = (end - IVec3::ONE).div_euclid(IVec3::splat(side));
    coverage.with_keys(|keys| {
        for z in first.z..=last.z {
            for y in first.y..=last.y {
                for x in first.x..=last.x {
                    if !keys.contains(&world::SubChunkKey::new(dimension, x, y, z)) {
                        continue;
                    }
                    let base = IVec3::new(x, y, z) * side;
                    let lo = base.max(origin);
                    let hi = (base + IVec3::splat(side)).min(end);
                    for z in lo.z..hi.z {
                        for y in lo.y..hi.y {
                            for x in lo.x..hi.x {
                                if let Some(i) = index(origin, IVec3::new(x, y, z)) {
                                    words[i][0] |= 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    });
}

fn pack_color(rgb: Vec3) -> u32 {
    let c = (rgb.clamp(Vec3::ZERO, Vec3::ONE) * 255.0)
        .round()
        .as_uvec3();
    c.x | c.y << 8 | c.z << 16
}

fn normal_bits(normal: Vec3) -> u32 {
    let normal = normal.normalize_or_zero();
    let absolute = normal.abs();
    let (axis, positive) = if absolute.x >= absolute.y && absolute.x >= absolute.z {
        (0, normal.x >= 0.0)
    } else if absolute.y >= absolute.z {
        (1, normal.y >= 0.0)
    } else {
        (2, normal.z >= 0.0)
    };
    let direction = (axis * 2 + usize::from(positive)) as u32;
    (1 << direction) << NORMAL_SHIFT
}

fn face_normal(face: meshing::Face) -> Vec3 {
    match face {
        meshing::Face::NegativeX => -Vec3::X,
        meshing::Face::PositiveX => Vec3::X,
        meshing::Face::NegativeY => -Vec3::Y,
        meshing::Face::PositiveY => Vec3::Y,
        meshing::Face::NegativeZ => -Vec3::Z,
        meshing::Face::PositiveZ => Vec3::Z,
    }
}

fn surface(
    words: &mut [[u32; 4]],
    origin: IVec3,
    world: IVec3,
    material: Reflectance,
    normal: Vec3,
    subcells: u32,
) {
    if material.opacity <= 0.01 || subcells == 0 {
        return;
    }
    if let Some(i) = index(origin, world) {
        let previous = words[i];
        let candidate = [
            OCCUPIED_BIT
                | SURFACE_BIT
                | normal_bits(normal)
                | ((subcells << SUBCELL_SHIFT) & SUBCELL_MASK_BITS),
            pack_color(material.rgb),
            material.opacity.to_bits(),
            material.sky.to_bits(),
        ];
        let coverage_bits =
            (previous[0] | candidate[0]) & (SUBCELL_MASK_BITS | (63 << NORMAL_SHIFT));
        let same_material = previous[1] == candidate[1]
            && previous[2] == candidate[2]
            && previous[3] == candidate[3];
        if same_material {
            words[i][0] |= coverage_bits;
            return;
        }
        // Equal-strength overlaps use a stable tie break rather than chunk iteration order.
        if material.opacity > f32::from_bits(previous[2])
            || material.opacity == f32::from_bits(previous[2])
                && (candidate[3], candidate[1]) < (previous[3], previous[1])
        {
            words[i] = candidate;
        }
        words[i][0] |= coverage_bits;
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Geometry rebuilding consumes separate resident streams and reusable outputs."
)]
pub(super) fn rebuild(
    resident: &IndirectGeometry,
    coverage: &crate::chunk::resident_coverage::ChunkResidentCoverage,
    textures: &ChunkTextureAssets,
    tints: &ChunkBiomeTints,
    dimension: Option<i32>,
    origin: IVec3,
    colors: &[Reflectance],
    words: &mut [[u32; 4]],
) {
    known_cells(coverage, origin, dimension, words);
    let end = origin + IVec3::from_array(GRID_SIZE.map(|v| v as i32));
    for chunk in resident.chunks.values() {
        if Some(chunk.key.dimension) != dimension {
            continue;
        }
        let base = IVec3::new(chunk.key.x, chunk.key.y, chunk.key.z) * world::SUB_CHUNK_SIDE as i32;
        let chunk_end = base + IVec3::splat(world::SUB_CHUNK_SIDE as i32);
        if base.cmpge(end).any() || chunk_end.cmple(origin).any() {
            continue;
        }
        let lo = base.max(origin);
        let hi = chunk_end.min(end);
        for z in lo.z..hi.z {
            for y in lo.y..hi.y {
                for x in lo.x..hi.x {
                    if let Some(i) = index(origin, IVec3::new(x, y, z)) {
                        words[i][0] |= 1;
                    }
                }
            }
        }
        for (quad_index, quad) in chunk.cubes.iter().enumerate() {
            let cell = base + IVec3::from_array(quad.origin().map(i32::from));
            let mut material = colors
                .get(quad.material_id() as usize)
                .copied()
                .unwrap_or_default();
            material.sky = sky_access(chunk.cube_light.get(quad_index));
            for y in 0..quad.height() as i32 {
                for x in 0..quad.width() as i32 {
                    let offset = match quad.face() {
                        meshing::Face::NegativeX | meshing::Face::PositiveX => IVec3::new(0, y, x),
                        meshing::Face::NegativeY | meshing::Face::PositiveY => IVec3::new(x, 0, y),
                        _ => IVec3::new(x, y, 0),
                    };
                    surface(
                        words,
                        origin,
                        cell + offset,
                        tinted(material, chunk, tints, cell + offset - base),
                        face_normal(quad.face()),
                        0xff,
                    );
                }
            }
        }
        for reference in &*chunk.models {
            let [transform, template_id, lighting_base, mask] = reference.words();
            let Some(template) = textures
                .assets()
                .model_templates()
                .get(template_id as usize)
            else {
                continue;
            };
            let position = base.as_vec3()
                + Vec3::new(
                    (transform & 15) as f32,
                    ((transform >> 4) & 15) as f32,
                    ((transform >> 8) & 15) as f32,
                );
            for q in 0..template.quad_count.min(32) {
                if mask & (1 << q) == 0 {
                    continue;
                }
                let Some(quad) = textures
                    .assets()
                    .model_quads()
                    .get((template.quad_start + q) as usize)
                else {
                    continue;
                };
                let vertices = quad.positions.map(|p| {
                    position
                        + rotate(
                            Vec3::from_array(p.map(|v| v as f32 / 256.0)),
                            transform >> 12,
                        )
                });
                let mut material = tinted(
                    colors
                        .get(quad.material as usize)
                        .copied()
                        .unwrap_or_default(),
                    chunk,
                    tints,
                    (position - base.as_vec3()).as_ivec3(),
                );
                material.sky = sky_access(chunk.model_light.get((lighting_base + q) as usize));
                triangle(
                    words,
                    origin,
                    [vertices[0], vertices[1], vertices[2]],
                    material,
                );
                triangle(
                    words,
                    origin,
                    [vertices[0], vertices[2], vertices[3]],
                    material,
                );
            }
        }
    }
}

fn rotate(position: Vec3, transform: u32) -> Vec3 {
    let p = position - Vec3::new(0.5, 0.0, 0.5);
    let q = match transform & 3 {
        1 => Vec3::new(-p.z, p.y, p.x),
        2 => Vec3::new(-p.x, p.y, -p.z),
        3 => Vec3::new(p.z, p.y, -p.x),
        _ => p,
    };
    q + Vec3::new(0.5, 0.0, 0.5)
}

fn sky_access(light: Option<&meshing::PackedQuadLighting>) -> f32 {
    light.map_or(0.0, |light| {
        light
            .samples()
            .into_iter()
            .map(|value| ((value >> 4) & 15) as f32 / 15.0)
            .sum::<f32>()
            * 0.25
    })
}

fn triangle(words: &mut [[u32; 4]], origin: IVec3, vertices: [Vec3; 3], material: Reflectance) {
    let normal = (vertices[1] - vertices[0])
        .cross(vertices[2] - vertices[0])
        .normalize_or_zero();
    if normal == Vec3::ZERO {
        return;
    }
    let bounds_minimum = vertices[0]
        .min(vertices[1])
        .min(vertices[2])
        .floor()
        .as_ivec3();
    let bounds_maximum = vertices[0]
        .max(vertices[1])
        .max(vertices[2])
        .ceil()
        .as_ivec3()
        .max(bounds_minimum + IVec3::ONE);
    let minimum = bounds_minimum.max(origin);
    let maximum = bounds_maximum.min(origin + IVec3::from_array(GRID_SIZE.map(|v| v as i32)));
    if minimum.cmpge(maximum).any() {
        return;
    }
    for z in minimum.z..maximum.z {
        for y in minimum.y..maximum.y {
            for x in minimum.x..maximum.x {
                let cell = IVec3::new(x, y, z);
                let mut subcells = 0_u32;
                for subcell in 0..8_u32 {
                    let sub = Vec3::new(
                        (subcell & 1) as f32,
                        ((subcell >> 1) & 1) as f32,
                        ((subcell >> 2) & 1) as f32,
                    );
                    let center = cell.as_vec3() + (sub + Vec3::splat(0.5)) * 0.5;
                    if triangle_box_inflated(
                        vertices,
                        center,
                        CELL_SIZE * 0.25,
                        MODEL_VOXEL_INFLATION,
                    ) {
                        subcells |= 1 << subcell;
                    }
                }
                if subcells != 0 {
                    surface(words, origin, cell, material, normal, subcells);
                }
            }
        }
    }
}

#[cfg(test)]
fn triangle_box(vertices: [Vec3; 3], center: Vec3) -> bool {
    triangle_box_inflated(vertices, center, CELL_SIZE * 0.5, 0.0)
}

fn triangle_box_inflated(
    vertices: [Vec3; 3],
    center: Vec3,
    half_extent: f32,
    inflation: f32,
) -> bool {
    let v = vertices.map(|p| p - center);
    let edges = [v[1] - v[0], v[2] - v[1], v[0] - v[2]];
    let separates = |axis: Vec3| {
        let p = v.map(|v| v.dot(axis));
        let radius = axis.abs().element_sum() * (half_extent + inflation + 0.0001);
        p[0].min(p[1]).min(p[2]) > radius || p[0].max(p[1]).max(p[2]) < -radius
    };
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        if separates(axis) {
            return false;
        }
        for edge in edges {
            if separates(edge.cross(axis)) {
                return false;
            }
        }
    }
    !separates(edges[0].cross(edges[1]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_empty_coverage_is_local_dimension_scoped_and_removed() {
        let coverage = crate::chunk::resident_coverage::ChunkResidentCoverage::default();
        let origin = IVec3::ZERO;
        let mut words =
            vec![[0; 4]; HEADER_WORDS + GRID_SIZE.into_iter().product::<u32>() as usize];
        let air = world::SubChunkKey::new(0, 0, 0, 0);
        coverage.set(air, true);
        coverage.set(world::SubChunkKey::new(1, 1, 0, 0), true);
        known_cells(&coverage, origin, Some(0), &mut words);
        assert_eq!(words[index(origin, IVec3::splat(3)).unwrap()][0], 1);
        assert_eq!(
            words[index(origin, IVec3::new(20, 3, 3)).unwrap()][0],
            0,
            "other dimensions never become known air"
        );
        let local = coverage_signature(&coverage, origin, Some(0));
        coverage.set(world::SubChunkKey::new(0, 50, 0, 0), true);
        assert_eq!(
            coverage_signature(&coverage, origin, Some(0)),
            local,
            "distant streaming never rebuilds the local grid"
        );
        coverage.set(air, false);
        assert_ne!(coverage_signature(&coverage, origin, Some(0)), local);
        words.fill([0; 4]);
        known_cells(&coverage, origin, Some(0), &mut words);
        assert_eq!(
            words[index(origin, IVec3::splat(3)).unwrap()][0],
            0,
            "removed empty data becomes unknown"
        );
    }
    #[test]
    fn model_triangle_occupancy_preserves_empty_neighbor_cells() {
        let triangle = [
            Vec3::new(0.2, 0.2, 0.5),
            Vec3::new(0.8, 0.2, 0.5),
            Vec3::new(0.2, 0.8, 0.5),
        ];
        assert!(triangle_box(triangle, Vec3::splat(0.5)));
        assert!(!triangle_box(triangle, Vec3::new(1.5, 0.5, 0.5)));
        assert!(!triangle_box(triangle, Vec3::new(0.5, 0.5, 1.5)));
    }
    #[test]
    fn opaque_surface_cannot_be_replaced_by_partial_coverage() {
        let mut words =
            vec![[0; 4]; HEADER_WORDS + GRID_SIZE.into_iter().product::<u32>() as usize];
        surface(
            &mut words,
            IVec3::ZERO,
            IVec3::ZERO,
            Reflectance {
                rgb: Vec3::X,
                opacity: 1.0,
                flags: 0,
                sky: 1.0,
            },
            Vec3::Y,
            0xff,
        );
        surface(
            &mut words,
            IVec3::ZERO,
            IVec3::ZERO,
            Reflectance {
                rgb: Vec3::Y,
                opacity: 0.2,
                flags: 0,
                sky: 1.0,
            },
            Vec3::Y,
            0xff,
        );
        assert_eq!(words[HEADER_WORDS][1], 255);
        assert_eq!(f32::from_bits(words[HEADER_WORDS][2]), 1.0);
    }
}
