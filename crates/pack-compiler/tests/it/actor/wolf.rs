use super::*;
use std::collections::BTreeSet;

/// The downloaded fixture supplies art; missing local inputs leave a named diagnostic.
fn pinned_root() -> Option<std::path::PathBuf> {
    let Some(root) = std::env::var_os("CINNABAR_VANILLA_ACTOR_ROOT") else {
        eprintln!("missing fixture: CINNABAR_VANILLA_ACTOR_ROOT (pinned vanilla wolf pack)");
        return None;
    };
    let root = std::path::PathBuf::from(root);
    if !root.join("entity/wolf.entity.json").is_file() {
        eprintln!(
            "missing fixture: pinned vanilla wolf entity at {}",
            root.display()
        );
        return None;
    }
    Some(root)
}

#[test]
fn pinned_wolf_states_retain_the_selected_rasters_and_dye_mask_bytes() {
    let Some(root) = pinned_root() else { return };
    let entities = compile_entity_assets(&root, MANIFEST).unwrap();
    let entity_bytes = encode_entity_blob(&entities).unwrap();
    let runtime = assets::RuntimeEntityAssets::decode(&entity_bytes).unwrap();
    let compiled = compile_actor_assets(&root, MANIFEST).unwrap();
    let catalog = RuntimeActorCatalog::decode(&compiled.bytes, &runtime).unwrap();
    let mut selected = BTreeSet::new();
    for layer in &entities.render.layers {
        let rig = &entities.rig_bindings[layer.rig as usize];
        if entities.symbols[rig.entity_symbol as usize]
            .identifier
            .as_ref()
            != "minecraft:wolf"
        {
            continue;
        }
        let first = layer.first_slot as usize;
        for slot in &entities.render.slots[first..first + usize::from(layer.slot_count)] {
            let first = slot.first_candidate as usize;
            selected.extend(
                entities.render.candidates[first..first + usize::from(slot.candidate_count)]
                    .iter()
                    .map(|candidate| candidate.source),
            );
        }
    }
    assert!(
        !selected.is_empty(),
        "wolf controllers select their state textures"
    );
    let mut indices = Vec::new();
    for source in selected {
        let asset = &entities.sources[source as usize];
        let index = catalog
            .texture_of_source(source)
            .unwrap_or_else(|| panic!("selected wolf texture was rejected: {}", asset.path));
        let raster = image::open(root.join(asset.path.as_ref()))
            .unwrap()
            .into_rgba8();
        let texture = &catalog.textures()[index as usize];
        assert_eq!(
            texture.rgba8.as_ref(),
            raster.as_raw(),
            "{} preserves every alpha byte",
            asset.path
        );
        indices.push(index as usize);
    }
    for index in indices {
        assert!(
            catalog.texture_uses_color_mask(index),
            "selected wolf rasters use alpha as dye weight"
        );
    }
}

#[test]
fn custom_wolf_rasters_do_not_claim_the_stock_dye_mask_contract() {
    let fixture = pack(4, "wolf", false);
    let compiled = compile_actor_assets(fixture.path(), MANIFEST).unwrap();
    assert_eq!(compiled.report.textures, 0);
    assert_eq!(compiled.report.bindings, 0);
}
