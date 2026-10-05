//! Rig geometry built from the runtime entity catalog: bone merging, names, and pivots.
use std::sync::Arc;

use assets::{EntityGeometryBone, MAX_ENTITY_GEOMETRY_CUBES, RuntimeEntityAssets};

use super::{
    ActorRigGeometry, ActorRigGeometryError, EntityRigId, MAX_ACTOR_RIG_VERTICES,
    MAX_RENDER_BONES_PER_ACTOR,
};

#[cfg(test)]
#[path = "inherited_cube_tests.rs"]
mod inherited_cube_tests;
#[cfg(test)]
#[path = "quadruped_geometry_tests.rs"]
mod quadruped_geometry_tests;
#[cfg(test)]
#[path = "skin_geometry_tests.rs"]
mod skin_geometry_tests;

pub fn geometry_from_runtime_assets(
    assets: &RuntimeEntityAssets,
    binding_index: usize,
) -> Result<ActorRigGeometry, ActorRigGeometryError> {
    let binding = assets
        .rig_geometries()
        .get(binding_index)
        .ok_or(ActorRigGeometryError::InvalidAssetGeometry)?;
    geometry_from_geometry_index(
        assets,
        binding.geometry as usize,
        EntityRigId(
            u32::try_from(binding_index)
                .map_err(|_| ActorRigGeometryError::InvalidAssetGeometry)?,
        ),
    )
}

/// Rig geometry of one catalog geometry under `id`, for equipment layers.
#[must_use]
pub fn equipment_geometry(
    assets: &RuntimeEntityAssets,
    geometry_index: usize,
    id: EntityRigId,
) -> Option<ActorRigGeometry> {
    geometry_from_geometry_index(assets, geometry_index, id).ok()
}

/// Rig geometry for every binding of a session pack catalog, under pack rig ids;
/// an unbuildable binding is omitted so its actors take the missing-rig route.
pub fn pack_geometries(assets: &RuntimeEntityAssets) -> Vec<ActorRigGeometry> {
    let mut geometries: Vec<_> = (0..assets.rig_geometries().len())
        .filter_map(|binding| {
            let id = super::pack_rig_id(u32::try_from(binding).ok()?);
            let geometry = assets.rig_geometries().get(binding)?.geometry as usize;
            geometry_from_geometry_index(assets, geometry, id).ok()
        })
        .collect();
    geometries.extend(super::layer_geometries(assets, super::pack_rig_id(0)));
    geometries
}

/// Rig geometry of catalog geometry `geometry_index` under `id`.
pub fn entity_geometry(
    assets: &RuntimeEntityAssets,
    geometry_index: usize,
    id: EntityRigId,
) -> Result<ActorRigGeometry, ActorRigGeometryError> {
    geometry_from_geometry_index(assets, geometry_index, id)
}

pub fn geometry_from_geometry_index(
    assets: &RuntimeEntityAssets,
    geometry_index: usize,
    id: EntityRigId,
) -> Result<ActorRigGeometry, ActorRigGeometryError> {
    let bones = resolve_geometry_bones(assets, geometry_index)?;
    if bones.is_empty() || bones.len() > MAX_RENDER_BONES_PER_ACTOR {
        return Err(ActorRigGeometryError::BoneCount);
    }
    let mut vertices = Vec::new();
    for (bone_index, bone) in bones.iter().enumerate() {
        if bone.never_render == Some(true) {
            continue;
        }
        for cube in &bone.cubes {
            super::geometry::append_entity_bone_cube_vertices(
                &mut vertices,
                cube,
                bone_index as u32,
                assets
                    .geometries()
                    .get(geometry_index)
                    .map(|geometry| (geometry.texture_width, geometry.texture_height))
                    .ok_or(ActorRigGeometryError::InvalidAssetGeometry)?,
                bone,
            )?;
            if vertices.len() > MAX_ACTOR_RIG_VERTICES {
                return Err(ActorRigGeometryError::CatalogCapacity);
            }
        }
    }
    let bone_pivots = bones.iter().map(bone_bind_pivot).collect::<Vec<_>>();
    ActorRigGeometry::new(id, Arc::from(vertices), Arc::from(bone_pivots))
}

/// Rig geometry of a player skin's own model under `id`. Cubes carry their resolved mirror and
/// inflate, so no bone-level values apply.
pub fn skin_geometry(
    geometry: &assets::SkinGeometry,
    id: EntityRigId,
) -> Result<ActorRigGeometry, ActorRigGeometryError> {
    let bones = &geometry.bones;
    if bones.is_empty() || bones.len() > MAX_RENDER_BONES_PER_ACTOR {
        return Err(ActorRigGeometryError::BoneCount);
    }
    let texture_size = (geometry.texture_width, geometry.texture_height);
    let mut vertices = Vec::new();
    for (bone_index, bone) in bones.iter().enumerate() {
        if bone.never_render == Some(true) {
            continue;
        }
        for cube in &bone.cubes {
            super::geometry::append_entity_bone_cube_vertices(
                &mut vertices,
                cube,
                bone_index as u32,
                texture_size,
                bone,
            )?;
            if vertices.len() > MAX_ACTOR_RIG_VERTICES {
                return Err(ActorRigGeometryError::CatalogCapacity);
            }
        }
    }
    for (bone_index, mesh) in geometry.poly_meshes.iter().enumerate() {
        if bones[bone_index].never_render == Some(true) {
            continue;
        }
        let Some(mesh) = mesh else {
            continue;
        };
        super::skin_poly_mesh::append(&mut vertices, mesh, bone_index as u32, texture_size)?;
    }
    if vertices.is_empty() {
        return Err(ActorRigGeometryError::VertexCount);
    }
    let bone_pivots = bones.iter().map(bone_bind_pivot).collect::<Vec<_>>();
    ActorRigGeometry::new(id, Arc::from(vertices), Arc::from(bone_pivots))
}

/// Bone names of a geometry in rig order, after inheritance is merged.
#[must_use]
pub fn geometry_bone_names(
    assets: &RuntimeEntityAssets,
    geometry_index: usize,
) -> Option<Vec<Box<str>>> {
    resolve_geometry_bones(assets, geometry_index)
        .ok()
        .map(|bones| bones.into_iter().map(|bone| bone.name).collect())
}

/// Bind pivot (rig frame, blocks) of every bone of a geometry, in rig order.
#[must_use]
pub fn geometry_bone_pivots(
    assets: &RuntimeEntityAssets,
    geometry_index: usize,
) -> Option<Vec<[f32; 3]>> {
    resolve_geometry_bones(assets, geometry_index)
        .ok()
        .map(|bones| bones.iter().map(bone_bind_pivot).collect())
}

/// Index of the geometry with this identifier in the entity catalog.
#[must_use]
pub fn find_geometry_index(assets: &RuntimeEntityAssets, identifier: &str) -> Option<u32> {
    assets
        .geometries()
        .iter()
        .position(|geometry| geometry.identifier.as_ref() == identifier)
        .and_then(|index| u32::try_from(index).ok())
}

pub(super) fn bone_bind_pivot(bone: &EntityGeometryBone) -> [f32; 3] {
    // Pivots share the vertices' rig frame, where authored X is mirrored.
    bone.pivot.map_or([0.0; 3], |pivot| {
        [
            -pivot[0].get() / 16.0,
            pivot[1].get() / 16.0,
            pivot[2].get() / 16.0,
        ]
    })
}

pub(super) fn resolve_geometry_bones(
    assets: &RuntimeEntityAssets,
    geometry_index: usize,
) -> Result<Vec<EntityGeometryBone>, ActorRigGeometryError> {
    let parents = assets.geometry_parents();
    let mut chain = Vec::new();
    let mut current = geometry_index;
    for _ in 0..=parents.len() {
        chain.push(current);
        let Some(parent) = parents.get(current).copied().flatten() else {
            break;
        };
        current = parent;
    }
    if chain
        .last()
        .and_then(|index| parents.get(*index))
        .copied()
        .flatten()
        .is_some()
    {
        return Err(ActorRigGeometryError::InvalidAssetGeometry);
    }
    chain.reverse();
    let mut merged: Vec<EntityGeometryBone> = Vec::new();
    let mut cube_count = 0;
    for index in chain {
        let children = &assets
            .geometries()
            .get(index)
            .ok_or(ActorRigGeometryError::InvalidAssetGeometry)?
            .bones;
        append_geometry_bones(&mut merged, &mut cube_count, children)?;
    }
    Ok(merged)
}

fn append_geometry_bones(
    merged: &mut Vec<EntityGeometryBone>,
    cube_count: &mut usize,
    children: &[EntityGeometryBone],
) -> Result<(), ActorRigGeometryError> {
    for child in children {
        if let Some(existing) = merged
            .iter_mut()
            .find(|bone| bone.name.eq_ignore_ascii_case(&child.name))
        {
            let other_cubes = *cube_count - existing.cubes.len();
            let new_count =
                overlay_geometry_bone(existing, child, MAX_ENTITY_GEOMETRY_CUBES - other_cubes)?;
            *cube_count = other_cubes + new_count;
        } else {
            let new_count = cube_count
                .checked_add(child.cubes.len())
                .filter(|count| *count <= MAX_ENTITY_GEOMETRY_CUBES)
                .ok_or(ActorRigGeometryError::CatalogCapacity)?;
            if merged.len() >= MAX_RENDER_BONES_PER_ACTOR {
                return Err(ActorRigGeometryError::BoneCount);
            }
            // Check the cumulative model budget before cloning any authored cubes.
            merged.push(child.clone());
            *cube_count = new_count;
        }
    }
    Ok(())
}

fn overlay_geometry_bone(
    base: &mut EntityGeometryBone,
    child: &EntityGeometryBone,
    maximum_cubes: usize,
) -> Result<usize, ActorRigGeometryError> {
    let cube_count = base
        .append_inherited_cubes(child, maximum_cubes)
        .ok_or(ActorRigGeometryError::CatalogCapacity)?;
    if child.binding.is_some() {
        base.binding.clone_from(&child.binding);
    }
    if child.parent.is_some() {
        base.parent.clone_from(&child.parent);
    }
    if child.pivot.is_some() {
        base.pivot = child.pivot;
    }
    if child.rotation.is_some() {
        base.rotation = child.rotation;
    }
    if child.bind_pose_rotation.is_some() {
        base.bind_pose_rotation = child.bind_pose_rotation;
    }
    if child.mirror.is_some() {
        base.mirror = child.mirror;
    }
    if child.inflate.is_some() {
        base.inflate = child.inflate;
    }
    if child.never_render.is_some() {
        base.never_render = child.never_render;
    }
    if child.reset.is_some() {
        base.reset = child.reset;
    }
    // `reset` drops the cubes a bone inherited; it is how derived geometries hide a bone.
    if child.reset == Some(true) {
        base.texture_meshes = Box::default();
    }
    if !child.texture_meshes.is_empty() {
        base.texture_meshes = base
            .texture_meshes
            .iter()
            .chain(child.texture_meshes.iter())
            .cloned()
            .collect();
    }
    Ok(cube_count)
}

#[cfg(test)]
mod tests {
    use assets::{EntityGeometryBone, EntityGeometryCube, EntityGeometryScalar, EntityGeometryUv};

    use super::{MAX_ENTITY_GEOMETRY_CUBES, overlay_geometry_bone};

    #[test]
    fn catalog_planes_preserve_untextured_back_faces() {
        let temporary = tempfile::tempdir().unwrap();
        for family in [
            "entity",
            "models/entity",
            "animations",
            "animation_controllers",
            "render_controllers",
            "textures/entity",
        ] {
            std::fs::create_dir_all(temporary.path().join(family)).unwrap();
        }
        std::fs::write(temporary.path().join("models/entity/projectile.geo.json"),
            br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.projectile","texture_width":32,"texture_height":32},"bones":[{"name":"body","cubes":[{"origin":[0,-2.5,-3],"size":[0,5,16],"uv":{"east":{"uv":[0,0]}}}]}]}]}"#).unwrap();
        let compiled = pack_compiler::compile_entity_assets(
            temporary.path(),
            include_bytes!("../../../../assets/vanilla-source.json"),
        )
        .unwrap();
        let runtime =
            assets::RuntimeEntityAssets::decode(&assets::encode_entity_blob(&compiled).unwrap())
                .unwrap();
        let geometry = super::entity_geometry(&runtime, 0, super::EntityRigId(0)).unwrap();
        assert_eq!(geometry.vertices.len(), 6);
        assert!(
            geometry
                .vertices
                .iter()
                .all(|vertex| vertex.back_uv == vertex.uv)
        );
    }

    fn bone(reset: Option<bool>, cubes: usize) -> EntityGeometryBone {
        let zero = EntityGeometryScalar::ZERO;
        let cube = EntityGeometryCube {
            origin: [zero; 3],
            size: [zero; 3],
            pivot: [zero; 3],
            rotation: [zero; 3],
            uv: EntityGeometryUv::Box([zero; 2]),
            inflate: zero,
            mirror: false,
        };
        EntityGeometryBone {
            name: "body".into(),
            binding: None,
            texture_meshes: Box::new([]),
            parent: None,
            pivot: None,
            rotation: None,
            bind_pose_rotation: None,
            mirror: None,
            inflate: None,
            never_render: None,
            reset,
            cubes: vec![cube; cubes].into(),
        }
    }

    #[test]
    fn reset_drops_inherited_cubes_but_a_plain_overlay_keeps_them() {
        let mut kept = bone(None, 2);
        overlay_geometry_bone(&mut kept, &bone(None, 0), MAX_ENTITY_GEOMETRY_CUBES).unwrap();
        assert_eq!(kept.cubes.len(), 2);
        overlay_geometry_bone(&mut kept, &bone(None, 1), MAX_ENTITY_GEOMETRY_CUBES).unwrap();
        assert_eq!(
            kept.cubes.len(),
            3,
            "ordinary child cubes append, not replace"
        );

        let mut hidden = bone(None, 2);
        overlay_geometry_bone(&mut hidden, &bone(Some(true), 0), MAX_ENTITY_GEOMETRY_CUBES)
            .unwrap();
        assert!(hidden.cubes.is_empty());

        let mut replaced = bone(None, 2);
        overlay_geometry_bone(
            &mut replaced,
            &bone(Some(true), 1),
            MAX_ENTITY_GEOMETRY_CUBES,
        )
        .unwrap();
        assert_eq!(replaced.cubes.len(), 1);
    }
}
