//! Rig geometry built from the runtime entity catalog: bone merging, names, and pivots.
use std::sync::Arc;

use assets::{
    EntityGeometryBone, EntityGeometryCube, EntityGeometryScalar, EntityGeometryUv,
    RuntimeEntityAssets,
};

use crate::{BlockEntityAtlas, SkullKind};

use super::{
    ActorRigGeometry, ActorRigGeometryError, EntityRigId, MAX_ACTOR_RIG_VERTICES,
    MAX_RENDER_BONES_PER_ACTOR,
};

#[path = "material.rs"]
mod material;
#[cfg(test)]
#[path = "skin_geometry_tests.rs"]
mod skin_geometry_tests;

pub(super) fn geometry_from_runtime_assets(
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
pub(super) fn pack_geometries(assets: &RuntimeEntityAssets) -> Vec<ActorRigGeometry> {
    let mut geometries: Vec<_> = (0..assets.rig_geometries().len())
        .filter_map(|binding| {
            let id = super::pack_rig_id(u32::try_from(binding).ok()?);
            let geometry = assets.rig_geometries().get(binding)?.geometry as usize;
            geometry_from_geometry_index(assets, geometry, id).ok()
        })
        .collect();
    geometries.extend(super::rig::layer_geometries(assets, super::pack_rig_id(0)));
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

pub(super) fn geometry_from_geometry_index(
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
            super::geometry::append_entity_cube_vertices(
                &mut vertices,
                cube,
                bone_index as u32,
                assets
                    .geometries()
                    .get(geometry_index)
                    .map(|geometry| (geometry.texture_width, geometry.texture_height))
                    .ok_or(ActorRigGeometryError::InvalidAssetGeometry)?,
                bone.mirror.unwrap_or(false),
                bone.inflate.map_or(0.0, |inflate| inflate.get()),
            )?;
            if vertices.len() > MAX_ACTOR_RIG_VERTICES {
                return Err(ActorRigGeometryError::CatalogCapacity);
            }
        }
    }
    material::apply_native_arrow_material(assets, geometry_index, &mut vertices);
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
            super::geometry::append_entity_cube_vertices(
                &mut vertices,
                cube,
                bone_index as u32,
                texture_size,
                bone.mirror.unwrap_or(false),
                bone.inflate.map_or(0.0, |inflate| inflate.get()),
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

/// A worn skull: the player head cube (plus the hat layer for humanoid heads) on one bone, with
/// its UVs mapped into the block-entity atlas's static region. `None` when the kind has no
/// packed texture.
#[must_use]
pub fn skull_geometry(
    id: EntityRigId,
    atlas: &BlockEntityAtlas,
    kind: SkullKind,
) -> Option<ActorRigGeometry> {
    let texture = kind.texture(atlas)?;
    let [atlas_width, _] = atlas.size();
    let static_height = atlas.static_height();
    let rect = texture.rect_uv([0.0, 0.0, texture.logical[0], texture.logical[1]]);
    let region = [
        rect[0] / atlas_width as f32,
        rect[1] / static_height as f32,
        rect[2] / atlas_width as f32,
        rect[3] / static_height as f32,
    ];
    let logical = (texture.logical[0] as u16, texture.logical[1] as u16);
    let scalar =
        |value: f32| EntityGeometryScalar::new(value).unwrap_or(EntityGeometryScalar::ZERO);
    let cube = |uv: [f32; 2], inflate: f32| EntityGeometryCube {
        origin: [-4.0, 24.0, -4.0].map(scalar),
        size: [8.0; 3].map(scalar),
        pivot: [EntityGeometryScalar::ZERO; 3],
        rotation: [EntityGeometryScalar::ZERO; 3],
        uv: EntityGeometryUv::Box(uv.map(scalar)),
        inflate: scalar(inflate),
        mirror: false,
    };
    let mut vertices = Vec::new();
    let mut layers = vec![cube([0.0, 0.0], 0.0)];
    if kind.has_hat_layer() {
        layers.push(cube([32.0, 0.0], 0.25));
    }
    for layer in &layers {
        super::geometry::append_entity_cube_vertices(&mut vertices, layer, 0, logical, false, 0.0)
            .ok()?;
    }
    for vertex in &mut vertices {
        for uv in [&mut vertex.uv, &mut vertex.back_uv] {
            uv[0] = region[0] + uv[0] * (region[2] - region[0]);
            uv[1] = region[1] + uv[1] * (region[3] - region[1]);
        }
    }
    ActorRigGeometry::new(id, Arc::from(vertices), Arc::from(vec![[0.0, 1.5, 0.0]])).ok()
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
    for index in chain {
        for child in assets
            .geometries()
            .get(index)
            .ok_or(ActorRigGeometryError::InvalidAssetGeometry)?
            .bones
            .iter()
        {
            if let Some(existing) = merged
                .iter_mut()
                .find(|bone| bone.name.eq_ignore_ascii_case(&child.name))
            {
                overlay_geometry_bone(existing, child);
            } else {
                merged.push(child.clone());
            }
        }
    }
    Ok(merged)
}

fn overlay_geometry_bone(base: &mut EntityGeometryBone, child: &EntityGeometryBone) {
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
        base.cubes = Box::default();
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
    if !child.cubes.is_empty() {
        base.cubes.clone_from(&child.cubes);
    }
}

#[cfg(test)]
mod tests {
    use assets::{EntityGeometryBone, EntityGeometryCube, EntityGeometryScalar, EntityGeometryUv};

    use super::overlay_geometry_bone;

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
        let compiled = asset_compiler::compile_entity_assets(
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
                .all(|vertex| vertex.back_uv == super::super::geometry::ONE_SIDED_BACK_UV)
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
        overlay_geometry_bone(&mut kept, &bone(None, 0));
        assert_eq!(kept.cubes.len(), 2);

        let mut hidden = bone(None, 2);
        overlay_geometry_bone(&mut hidden, &bone(Some(true), 0));
        assert!(hidden.cubes.is_empty());

        let mut replaced = bone(None, 2);
        overlay_geometry_bone(&mut replaced, &bone(Some(true), 1));
        assert_eq!(replaced.cubes.len(), 1);
    }
}
