//! Held models require every texture used by their closed, resting geometry.

use super::*;
use std::collections::BTreeSet;

/// Names the textures used by each held model, sharing the placed renderer's paths.
fn texture_names(kind: HeldBlockEntity) -> Vec<String> {
    use assets::block_entity_geometry as geometry;
    match kind {
        HeldBlockEntity::Chest(key) => vec![key.variant().texture(false)],
        HeldBlockEntity::Shulker(color) => vec![geometry::shulker_texture(color)],
        HeldBlockEntity::DecoratedPot => vec![
            geometry::POT_BASE_TEXTURE.0.to_owned(),
            geometry::POT_SIDE_TEXTURE.0.to_owned(),
        ],
        HeldBlockEntity::Conduit => vec![geometry::CONDUIT_TEXTURE.0.to_owned()],
        HeldBlockEntity::Skull(kind) => vec![head_texture(kind).to_owned()],
    }
}

/// Resolves both box heads and entity-derived heads through the shared model authority.
fn head_texture(kind: SkullKind) -> &'static str {
    assets::block_entity_geometry::skull_texture(kind as u8)
        .map(|(name, _)| name)
        .or_else(|| {
            assets::block_entity_geometry::heads::skull_mob_head(kind as u8)
                .map(|head| head.texture)
        })
        .unwrap()
}

/// Supplies original non-cubic mob head geometry independently of installed carriers.
fn head_models() -> HeadModels {
    use assets::block_entity_geometry::{
        BoxSpec,
        heads::{HeadBox, HeadModel},
    };
    let head = HeadModel {
        boxes: vec![HeadBox {
            matrix: glam::Mat4::IDENTITY,
            spec: BoxSpec::new([-5.0, 0.0, -4.0], [10.0, 8.0, 8.0], [0.0; 2]),
        }],
        texture: [64.0; 2],
    };
    HeadModels {
        piglin: Some(head.clone()),
        dragon: Some(head),
        ..Default::default()
    }
}

/// Builds synthetic texture tiles, retaining a valid empty atlas when none are supplied.
fn atlas_with_textures(names: impl Iterator<Item = String>) -> BlockEntityAtlas {
    let side = 64;
    let names = names.collect::<BTreeSet<_>>();
    let placements = names
        .into_iter()
        .enumerate()
        .map(|(index, name)| assets::BlockEntityPlacement {
            name: name.into(),
            x: 0,
            y: index as u32 * side,
            width: side,
            height: side,
        })
        .collect::<Vec<_>>();
    let height = side * placements.len().max(1) as u32;
    let bytes = assets::encode_block_entity_catalog(
        b"{}",
        side,
        height,
        &vec![255; (side * height * 4) as usize],
        &placements,
    )
    .unwrap();
    BlockEntityAtlas::from_assets(&assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap())
}

#[test]
fn every_held_kind_requires_all_of_its_geometry_textures() {
    let kinds = HeldBlockEntity::all();
    let textures = kinds
        .iter()
        .copied()
        .flat_map(texture_names)
        .collect::<BTreeSet<_>>();
    let complete = atlas_with_textures(textures.iter().cloned());
    let heads = head_models();
    let prepared = PreparedHeldBlockModels::with_heads(&complete, &heads);
    for kind in kinds {
        assert!(
            model_with_heads(EntityRigId(1), &complete, kind, &heads).is_some(),
            "{kind:?}"
        );
        assert!(
            prepared
                .models()
                .iter()
                .any(|(candidate, _)| *candidate == kind),
            "{kind:?}"
        );
        for missing in texture_names(kind) {
            let partial = atlas_with_textures(
                textures
                    .iter()
                    .filter(|name| name.as_str() != missing.as_str())
                    .cloned(),
            );
            assert!(
                model_with_heads(EntityRigId(1), &partial, kind, &heads).is_none(),
                "{kind:?} must retain its icon without {missing}"
            );
            let prepared = PreparedHeldBlockModels::with_heads(&partial, &heads);
            assert!(
                !prepared
                    .models()
                    .iter()
                    .any(|(candidate, _)| *candidate == kind),
                "{kind:?} must not be published without {missing}"
            );
        }
    }
}

#[test]
fn incomplete_pot_emission_records_missing_artwork_without_hiding_placed_parts() {
    let kind = HeldBlockEntity::DecoratedPot;
    let textures = texture_names(kind);
    for missing in &textures {
        let atlas = atlas_with_textures(textures.iter().filter(|name| *name != missing).cloned());
        let mut builder = MeshBuilder::new(atlas.size());
        kind.emit(&mut builder, &atlas, &HeadModels::default());
        assert!(
            !builder.solid.is_empty(),
            "resident placed parts remain visible"
        );
        assert!(
            builder.missing_required_texture,
            "missing {missing} must be recorded"
        );
        assert!(held_block_entity_model(EntityRigId(1), &atlas, kind).is_none());
    }
}

#[test]
fn held_heads_have_volume_and_use_their_model_texture() {
    let heads = head_models();
    for kind in SkullKind::kinds() {
        let atlas = atlas_with_textures(std::iter::once(head_texture(kind).to_owned()));
        let model = model_with_heads(EntityRigId(1), &atlas, HeldBlockEntity::Skull(kind), &heads)
            .expect("head geometry must be available to held-item rendering");
        for axis in 0..3 {
            let (min, max) = model.geometry.vertices.iter().fold(
                (f32::INFINITY, f32::NEG_INFINITY),
                |(min, max), vertex| {
                    (
                        min.min(vertex.position[axis]),
                        max.max(vertex.position[axis]),
                    )
                },
            );
            assert!(
                max - min >= 0.5,
                "{kind:?} must have head-sized depth on axis {axis}"
            );
            assert!(
                (min + max).abs() < 1.0e-5,
                "held geometry is centered on its grip"
            );
        }
        assert!(
            model
                .geometry
                .vertices
                .iter()
                .all(|vertex| { vertex.uv.iter().all(|uv| (0.0..=1.0).contains(uv)) })
        );
    }
}

#[test]
fn held_head_geometry_changes_invalidate_prepared_identity() {
    let atlas = atlas_with_textures(std::iter::once(head_texture(SkullKind::Piglin).to_owned()));
    let mut heads = head_models();
    let first = PreparedHeldBlockModels::with_heads(&atlas, &heads);
    heads.piglin.as_mut().unwrap().boxes[0].spec.size[0] += 2.0;
    let second = PreparedHeldBlockModels::with_heads(&atlas, &heads);
    assert_ne!(first.identity(), second.identity());
    assert!(Arc::ptr_eq(
        &first.raster().unwrap().rgba8,
        &second.raster().unwrap().rgba8
    ));
}
