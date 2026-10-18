//! Render-only joints for classic cuboid skins; unusual authored models stay intact.
use assets::{
    EntityGeometryBone, EntityGeometryCube, EntityGeometryFaceUv, EntityGeometryFaceUvs,
    EntityGeometryScalar as Scalar, EntityGeometryUv, SkinGeometry,
};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex, OnceLock};

/// Geometry and immutable vertices prepared together for the emote skeleton.
#[derive(Clone)]
pub(super) struct Model {
    pub geometry: Arc<SkinGeometry>,
    pub mesh: render_model::ActorRigGeometry,
    pub bounds: Arc<[Option<BoneBounds>]>,
    /// Original bone index for each appended joint, in mesh order.
    sources: Arc<[u32]>,
}

impl Model {
    /// Hides generated pieces only when their own source cuboid is hidden in this draw.
    pub(super) fn hidden_bones(&self, hidden: &Arc<[u32]>) -> Arc<[u32]> {
        let original_count = self.geometry.bones.len() - self.sources.len();
        let mut generated = self
            .sources
            .iter()
            .enumerate()
            .filter_map(|(index, source)| {
                hidden
                    .contains(source)
                    .then_some((original_count + index) as u32)
            })
            .peekable();
        if generated.peek().is_none() {
            return Arc::clone(hidden);
        }
        hidden.iter().copied().chain(generated).collect()
    }
}

/// Local bounds derived from the prepared vertices, including skin-layer inflation.
#[derive(Clone, Copy, Debug)]
pub(super) struct BoneBounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// Retains contact bounds once per prepared mesh rather than scanning vertices each frame.
fn contact_bounds(mesh: &render_model::ActorRigGeometry) -> Arc<[Option<BoneBounds>]> {
    let mut bounds: Vec<Option<BoneBounds>> = vec![None; mesh.bone_pivots.len()];
    for vertex in mesh.vertices.iter() {
        let index = vertex.bone_index as usize;
        let point = std::array::from_fn(|axis| {
            (vertex.position[axis] - mesh.bone_pivots[index][axis]) * 16.0
        });
        let bounds = bounds[index].get_or_insert(BoneBounds {
            min: point,
            max: point,
        });
        for (axis, value) in point.iter().enumerate() {
            bounds.min[axis] = bounds.min[axis].min(*value);
            bounds.max[axis] = bounds.max[axis].max(*value);
        }
    }
    bounds.into()
}

type Entry = ([u8; 32], Option<Model>);
mod full_body;
/// Covers every selected player and persona layer without an eight-skin cache thrashing during shared playback.
const MAX_MODELS: usize =
    render_model::MAX_RENDERED_PLAYERS * (1 + render_api::MAX_SKIN_ANIMATION_LAYERS) + 1;

/// Counts retained prepared vertex and pivot bytes against the actor catalog's existing bound.
fn mesh_bytes(model: &Option<Model>) -> usize {
    model.as_ref().map_or(0, |model| {
        std::mem::size_of_val(model.mesh.vertices.as_ref())
            + std::mem::size_of_val(model.mesh.bone_pivots.as_ref())
            + std::mem::size_of_val(model.bounds.as_ref())
            + std::mem::size_of_val(model.sources.as_ref())
    })
}

/// Reuses the exact mesh for each articulated geometry across playback samples.
pub(super) fn model(source: &Arc<SkinGeometry>) -> Option<Model> {
    static CACHE: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Mutex::default).lock().ok()?;
    if let Some((_, model)) = cache.iter().find(|(digest, _)| *digest == source.digest) {
        return model.clone();
    }
    let model = build(source);
    let bytes = mesh_bytes(&model);
    if bytes > render_model::MAX_ACTOR_CATALOG_VERTEX_BYTES {
        return None;
    }
    let mut retained: usize = cache.iter().map(|(_, model)| mesh_bytes(model)).sum();
    while cache.len() >= MAX_MODELS
        || retained + bytes > render_model::MAX_ACTOR_CATALOG_VERTEX_BYTES
    {
        retained -= mesh_bytes(&cache.remove(0).1);
    }
    cache.push((source.digest, model.clone()));
    model
}

/// Splits supported cuboids and prepares a mesh with each generated joint's source bone.
fn build(source: &SkinGeometry) -> Option<Model> {
    // Only classic skins with unrotated cuboid limbs can be split without
    // changing authored bind rotations, polygon meshes or arbitrary proportions.
    if source.poly_meshes.iter().any(Option::is_some) {
        return None;
    }
    let legs: Vec<_> = ["leftleg", "rightleg"]
        .into_iter()
        .map(|name| {
            source
                .bones
                .iter()
                .position(|bone| bone.name.eq_ignore_ascii_case(name))
        })
        .collect::<Option<_>>()?;
    let mut model = source.clone();
    let mut bones = source.bones.to_vec();
    let mut added = Vec::new();
    let mut sources = Vec::new();
    for (index, bone) in source.bones.iter().enumerate() {
        let leg = legs
            .iter()
            .copied()
            .find(|leg| descendant(source, index, *leg));
        let Some(leg) = leg else { continue };
        let pivot = source.bones[leg].pivot?;
        let height = pivot[1].get();
        let foot_height = height / 6.0;
        if !(8.0..=16.0).contains(&height)
            || bone
                .rotation
                .is_some_and(|v| v.iter().any(|v| v.get() != 0.0))
            || bone
                .bind_pose_rotation
                .is_some_and(|v| v.iter().any(|v| v.get() != 0.0))
            || bone.binding.is_some()
            || !bone.texture_meshes.is_empty()
        {
            return None;
        }
        let mut upper = Vec::new();
        let mut lower = Vec::new();
        let mut feet = Vec::new();
        for cube in &bone.cubes {
            if cube.rotation.iter().any(|v| v.get() != 0.0) {
                return None;
            }
            let overlap = height / 12.0;
            let (top, _) = split(cube, height * 0.5 - overlap)?;
            let (_, bottom) = split(cube, height * 0.5 + overlap)?;
            // Put the ankle inside the foot volume, rather than on its exposed
            // top face. A rotated shin otherwise opens a wedge between the two
            // rigid cuboids even though their joint centers remain connected.
            // Reuse the original skin rows in the overlap; the sole stays intact.
            let (_, foot) = split(&bottom, foot_height * 2.0)?;
            let (bottom, _) = split(&bottom, foot_height)?;
            upper.push(top);
            lower.push(bottom);
            feet.push(foot);
        }
        if index == leg && lower.is_empty() {
            return None;
        }
        if lower.is_empty() {
            continue;
        }
        bones[index].cubes = upper.into();
        let mut shin: EntityGeometryBone = bone.clone();
        shin.name = format!("{}.cinnabar_knee", bone.name).into();
        shin.parent = Some(if index == leg {
            bone.name.clone()
        } else {
            format!("{}.cinnabar_knee", source.bones[leg].name).into()
        });
        shin.pivot = Some([pivot[0], Scalar::new(height * 0.5)?, pivot[2]]);
        shin.cubes = lower.into();
        let mut foot = shin.clone();
        foot.name = format!("{}.cinnabar_ankle", bone.name).into();
        foot.parent = Some(if index == leg {
            shin.name.clone()
        } else {
            format!("{}.cinnabar_ankle", source.bones[leg].name).into()
        });
        foot.pivot = Some([pivot[0], Scalar::new(foot_height)?, pivot[2]]);
        foot.cubes = feet.into();
        added.push(shin);
        added.push(foot);
        sources.extend([index as u32; 2]);
    }
    bones.extend(added);
    if bones.len() > assets::MAX_SKIN_GEOMETRY_BONES {
        return None;
    }
    model.poly_meshes = vec![None; bones.len()].into();
    model.bones = bones.into();
    full_body::extend(source, &mut model, &mut sources)?;
    let mut digest = Sha256::new();
    digest.update(source.digest);
    digest.update(b"cinnabar:render-only-knees-and-ankles:v3");
    digest.update(b"cinnabar:elbows-wrists-spine:v2");
    model.digest = digest.finalize().into();
    let mesh = render_model::skin_geometry(&model, render_model::DIAGNOSTIC_RIG_ID).ok()?;
    Some(Model {
        bounds: contact_bounds(&mesh),
        geometry: Arc::new(model),
        mesh,
        sources: sources.into(),
    })
}

/// Reports whether a bone belongs to a limb subtree.
fn descendant(source: &SkinGeometry, index: usize, leg: usize) -> bool {
    let mut current = index;
    for _ in 0..source.bones.len() {
        if current == leg {
            return true;
        }
        let Some(parent) = source.bones[current].parent.as_ref() else {
            return false;
        };
        let Some(index) = source
            .bones
            .iter()
            .position(|bone| bone.name.eq_ignore_ascii_case(parent))
        else {
            return false;
        };
        current = index;
    }
    false
}

/// Splits one cuboid at a joint while cropping its original texture faces.
fn split(cube: &EntityGeometryCube, knee: f32) -> Option<(EntityGeometryCube, EntityGeometryCube)> {
    let bottom = cube.origin[1].get();
    let top = bottom + cube.size[1].get();
    if !(bottom < knee && knee < top) {
        return None;
    }
    let faces = faces(cube)?;
    let mut upper = cube.clone();
    let mut lower = cube.clone();
    upper.origin[1] = Scalar::new(knee)?;
    upper.size[1] = Scalar::new(top - knee)?;
    lower.size[1] = Scalar::new(knee - bottom)?;
    let upper_fraction = (top - knee) / (top - bottom);
    upper.uv = EntityGeometryUv::Faces(crop(&faces, 0.0, upper_fraction)?);
    lower.uv = EntityGeometryUv::Faces(crop(&faces, upper_fraction, 1.0)?);
    Some((upper, lower))
}

/// Resolves box or authored UVs to explicit face coordinates.
fn faces(cube: &EntityGeometryCube) -> Option<EntityGeometryFaceUvs> {
    let [x, y, z] = cube.size.map(Scalar::get);
    let face = |uv: [f32; 2], size: [f32; 2]| -> Option<EntityGeometryFaceUv> {
        Some(EntityGeometryFaceUv {
            uv: [Scalar::new(uv[0])?, Scalar::new(uv[1])?],
            uv_size: Some([Scalar::new(size[0])?, Scalar::new(size[1])?]),
        })
    };
    match &cube.uv {
        EntityGeometryUv::Box(uv) => {
            let [u, v] = uv.map(Scalar::get);
            let [x, y, z] = [x, y, z].map(f32::trunc);
            Some(EntityGeometryFaceUvs {
                north: face([u + z, v + z], [x, y]),
                south: face([u + z + x + z, v + z], [x, y]),
                east: face([u, v + z], [z, y]),
                west: face([u + z + x, v + z], [z, y]),
                up: face([u + z, v], [x, z]),
                down: face([u + z + x, v], [x, z]),
            })
        }
        EntityGeometryUv::Faces(authored) => {
            let mut result = authored.clone();
            for (face, size) in [
                (&mut result.north, [x, y]),
                (&mut result.south, [x, y]),
                (&mut result.east, [z, y]),
                (&mut result.west, [z, y]),
                (&mut result.up, [x, z]),
                (&mut result.down, [x, z]),
            ] {
                if let Some(face) = face {
                    face.uv_size = face
                        .uv_size
                        .or(Some([Scalar::new(size[0])?, Scalar::new(size[1])?]));
                }
            }
            Some(result)
        }
    }
}

/// Crops side faces and samples interior rows for newly exposed joint caps.
fn crop(faces: &EntityGeometryFaceUvs, start: f32, end: f32) -> Option<EntityGeometryFaceUvs> {
    let mut result = faces.clone();
    for face in [
        &mut result.north,
        &mut result.south,
        &mut result.east,
        &mut result.west,
    ]
    .into_iter()
    .flatten()
    {
        let size = face.uv_size?;
        face.uv[1] = Scalar::new(face.uv[1].get() + start * size[1].get())?;
        face.uv_size = Some([size[0], Scalar::new((end - start) * size[1].get())?]);
    }
    // The new joint caps use the leg's knee row, never the original boot sole.
    let joint_cap = |fraction: f32| -> Option<EntityGeometryFaceUv> {
        let mut face = faces.north.as_ref()?.clone();
        let size = face.uv_size?;
        let row = fraction * size[1].get() - size[1].get().signum() * 0.5;
        face.uv[1] = Scalar::new(face.uv[1].get() + row)?;
        face.uv_size = Some([size[0], Scalar::new(size[1].get().signum())?]);
        Some(face)
    };
    if end < 1.0 {
        result.down = joint_cap(end);
    }
    if start > 0.0 {
        result.up = joint_cap(start);
    }
    Some(result)
}

#[cfg(test)]
mod tests;
