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
    let prepared = PreparedHeldBlockModels::new(&complete);
    for kind in kinds {
        assert!(
            held_block_entity_model(EntityRigId(1), &complete, kind).is_some(),
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
                held_block_entity_model(EntityRigId(1), &partial, kind).is_none(),
                "{kind:?} must retain its icon without {missing}"
            );
            let prepared = PreparedHeldBlockModels::new(&partial);
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
        kind.emit(&mut builder, &atlas);
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
