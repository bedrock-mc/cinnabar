use std::{fs, path::Path};

use assets::{RuntimeActorCatalog, encode_actor_catalog, encode_entity_blob};
use image::{Rgba, RgbaImage};
use pack_compiler::{compile_actor_assets, compile_entity_assets};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const MANIFEST: &[u8] = include_bytes!("../../../../assets/vanilla-source.json");
/// Byte offset of the alpha of [`pack`]'s probe texel, which its plane samples.
const PROBE_ALPHA: usize = (7 * 16) * 4 + 3;

#[path = "actor/charged_overlay.rs"]
mod charged_overlay;
#[path = "actor/color_mask.rs"]
mod color_mask;
#[path = "actor/crystal.rs"]
mod crystal;
#[path = "actor/dragon.rs"]
mod dragon;
#[path = "actor/fish.rs"]
mod fish;
#[path = "actor/light_multiplier.rs"]
mod light_multiplier;
#[path = "actor/material_states.rs"]
mod material_states;
#[path = "actor/multitexture.rs"]
mod multitexture;
#[path = "actor/server_pack_budgets.rs"]
mod server_pack_budgets;
#[path = "actor/wind_charge.rs"]
mod wind_charge;
#[path = "actor/wolf.rs"]
mod wolf;

fn write(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn pack(alpha: u8, material: &str, conditional: bool) -> TempDir {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    write(root, "entity/example.entity.json", format!(r#"{{"format_version":"1.8.0","minecraft:client_entity":{{"description":{{"identifier":"minecraft:example","geometry":{{"default":"geometry.example"}},"materials":{{"default":"{material}"}},"textures":{{"default":"textures/entity/example"}},"render_controllers":["controller.render.example"]}}}}}}"#).as_bytes());
    write(root, "models/entity/example.geo.json", br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.example","texture_width":16,"texture_height":16},"bones":[{"name":"root","cubes":[{"origin":[0,0,0],"size":[0,2,7],"uv":[0,0]}]}]}]}"#);
    write(
        root,
        "animations/empty.json",
        br#"{"format_version":"1.8.0","animations":{}}"#,
    );
    write(
        root,
        "animation_controllers/empty.json",
        br#"{"format_version":"1.10.0","animation_controllers":{}}"#,
    );
    let geometry = if conditional {
        "query.is_alive ? Geometry.default : Geometry.default"
    } else {
        "Geometry.default"
    };
    write(root, "render_controllers/example.json", format!(r#"{{"format_version":"1.8.0","render_controllers":{{"controller.render.example":{{"geometry":"{geometry}","materials":[{{"*":"Material.default"}}],"textures":["Texture.default"]}}}}}}"#).as_bytes());
    let mut image = RgbaImage::from_pixel(16, 16, Rgba([17, 31, 47, 255]));
    image.put_pixel(0, 7, Rgba([0, 0, 0, alpha]));
    fs::create_dir_all(root.join("textures/entity")).unwrap();
    image
        .save(root.join("textures/entity/example.png"))
        .unwrap();
    temporary
}

#[test]
fn generic_actor_carrier_resolves_unconditional_route_and_exact_entity_identity() {
    let pack = pack(0, "entity_alphatest", false);
    let first = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    let second = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(first.bytes, second.bytes);
    let entities =
        encode_entity_blob(&compile_entity_assets(pack.path(), MANIFEST).unwrap()).unwrap();
    let runtime = RuntimeActorCatalog::decode(
        &first.bytes,
        &assets::RuntimeEntityAssets::decode(&entities).unwrap(),
    )
    .unwrap();
    assert_eq!(runtime.bindings().len(), 1);
    assert_eq!(runtime.textures().len(), 1);
    assert_eq!(
        (runtime.textures()[0].width, runtime.textures()[0].height),
        (16, 16)
    );
    assert_eq!(runtime.textures()[0].rgba8[PROBE_ALPHA], 0);
    let mut stale = entities.to_vec();
    stale[24] ^= 1;
    assert!(
        !assets::RuntimeEntityAssets::decode(&stale)
            .is_ok_and(|stale| RuntimeActorCatalog::decode(&first.bytes, &stale).is_ok())
    );
}

#[test]
fn variant_textures_compile_into_render_layers_and_the_carrier() {
    let pack = pack(0, "entity_alphatest", false);
    let root = pack.path();
    let path = root.join("entity/example.entity.json");
    let mut entity: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    entity["minecraft:client_entity"]["description"]["textures"]["other"] =
        serde_json::json!("textures/entity/other");
    fs::write(path, serde_json::to_vec(&entity).unwrap()).unwrap();
    write(root, "render_controllers/example.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.example":{"arrays":{"textures":{"Array.skins":["Texture.default","Texture.other"]}},"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Array.skins[query.variant]"],"part_visibility":[{"root":"query.is_baby"}]}}}"#);
    RgbaImage::from_pixel(16, 16, Rgba([90, 20, 10, 255]))
        .save(root.join("textures/entity/other.png"))
        .unwrap();
    let entities = compile_entity_assets(root, MANIFEST).unwrap();
    assert_eq!(entities.render.layers.len(), 1);
    assert_eq!(entities.render.slots.len(), 1);
    assert_eq!(entities.render.candidates.len(), 2);
    assert_eq!(entities.render.visibility.len(), 1);
    assert!(
        entities
            .render
            .candidates
            .iter()
            .all(|c| c.condition.is_some())
    );
    let compiled = compile_actor_assets(root, MANIFEST).unwrap();
    assert_eq!(compiled.report.bindings, 1);
    assert_eq!(compiled.report.textures, 2);
}

#[test]
fn handled_never_render_is_retained_instead_of_rejected_as_unknown_state() {
    let pack = pack(0, "entity_alphatest", false);
    let path = pack.path().join("models/entity/example.geo.json");
    let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    json["minecraft:geometry"][0]["bones"][0]["neverRender"] = serde_json::json!(true);
    fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.geometries[0].bones[0].never_render, Some(true));
    assert_eq!(
        compile_actor_assets(pack.path(), MANIFEST)
            .unwrap()
            .report
            .bindings,
        1
    );
}

#[test]
fn ordinary_cube_mirror_and_default_bone_flags_remain_admissible() {
    let pack = pack(0, "entity_alphatest", false);
    let path = pack.path().join("models/entity/example.geo.json");
    let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let bone = &mut json["minecraft:geometry"][0]["bones"][0];
    bone["inflate"] = serde_json::json!(0);
    bone["mirror"] = serde_json::json!(false);
    bone["cubes"][0]["mirror"] = serde_json::json!(true);
    fs::write(path, serde_json::to_vec(&json).unwrap()).unwrap();
    let entities =
        encode_entity_blob(&compile_entity_assets(pack.path(), MANIFEST).unwrap()).unwrap();
    let result = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        RuntimeActorCatalog::decode(
            &result.bytes,
            &assets::RuntimeEntityAssets::decode(&entities).unwrap()
        )
        .unwrap()
        .bindings()
        .len(),
        1
    );
}

#[test]
fn fractional_alpha_actor_pixels_are_counted_not_quantized() {
    let reason = "missing_or_ambiguous_texture";
    let pack = pack(128, "entity_alphatest", false);
    let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.report.bindings, 0);
    assert!(
        compiled
            .report
            .fallbacks
            .iter()
            .any(|entry| entry.reason.as_ref() == reason)
    );
}

#[test]
fn actor_pixels_are_not_cropped_to_geometry_dimensions() {
    let pack = pack(0, "entity_alphatest", false);
    RgbaImage::from_pixel(32, 16, Rgba([1, 2, 3, 255]))
        .save(pack.path().join("textures/entity/example.png"))
        .unwrap();
    let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.report.bindings, 1);
    let entities =
        encode_entity_blob(&compile_entity_assets(pack.path(), MANIFEST).unwrap()).unwrap();
    let catalog = RuntimeActorCatalog::decode(
        &compiled.bytes,
        &assets::RuntimeEntityAssets::decode(&entities).unwrap(),
    )
    .unwrap();
    let texture = &catalog.textures()[0];
    assert_eq!((texture.width, texture.height), (32, 16));
    assert_eq!(texture.rgba8.len(), 32 * 16 * 4);
    assert_eq!(&texture.rgba8[texture.rgba8.len() - 4..], &[1, 2, 3, 255]);
}

#[test]
fn runtime_rejects_rehashed_untrusted_pixels_and_binding_substitutions() {
    let pack = pack(0, "entity_alphatest", false);
    let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    let entities =
        encode_entity_blob(&compile_entity_assets(pack.path(), MANIFEST).unwrap()).unwrap();
    let catalog = RuntimeActorCatalog::decode(
        &compiled.bytes,
        &assets::RuntimeEntityAssets::decode(&entities).unwrap(),
    )
    .unwrap();
    let mut textures = catalog.textures().to_vec();
    let mut bindings = catalog.bindings().to_vec();
    let good_binding = bindings[0].clone();
    for field in 0..7 {
        bindings[0] = good_binding.clone();
        match field {
            0 => bindings[0].rig = u32::MAX,
            1 => bindings[0].entity_symbol = u32::MAX,
            2 => bindings[0].geometry = u32::MAX,
            3 => bindings[0].render_controller = u32::MAX,
            4 => bindings[0].texture = u32::MAX,
            5 => bindings[0].geometry_candidate = u32::MAX,
            _ => bindings[0].material = "".into(),
        }
        assert!(encode_actor_catalog(&entities, &textures, &bindings).is_err());
    }
    bindings[0] = good_binding;
    bindings.push(bindings[0].clone());
    assert!(encode_actor_catalog(&entities, &textures, &bindings).is_err());
    bindings.pop();
    textures[0].source = u32::MAX;
    assert!(encode_actor_catalog(&entities, &textures, &bindings).is_err());
    // Valid outer digest and pixel digest cannot authorize fractional alpha.
    let mut malicious = compiled.bytes;
    malicious[128 + 40 + 3] = 128;
    let pixel_end = 128 + 40 + 16 * 16 * 4;
    let pixel_hash = Sha256::digest(&malicious[168..pixel_end]);
    malicious[136..168].copy_from_slice(&pixel_hash);
    let end = malicious.len() - 32;
    let outer_hash = Sha256::digest(&malicious[..end]);
    malicious[end..].copy_from_slice(&outer_hash);
    assert!(
        RuntimeActorCatalog::decode(
            &malicious,
            &assets::RuntimeEntityAssets::decode(&entities).unwrap()
        )
        .is_err()
    );
}

#[test]
fn sibling_compiler_rejects_linked_texture_directory_outside_pack() {
    let pack = pack(0, "entity_alphatest", false);
    let outside = tempfile::tempdir().unwrap();
    let link = pack.path().join("textures/entity");
    let target = outside.path().join("entity");
    fs::rename(&link, &target).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let link_arg = link.to_str().unwrap().replace('/', "\\");
        let target_arg = target.to_str().unwrap().replace('/', "\\");
        assert!(
            std::process::Command::new("cmd")
                .args(["/d", "/c"])
                .raw_arg(format!("mklink /J \"{link_arg}\" \"{target_arg}\""))
                .status()
                .unwrap()
                .success()
        );
    }
    assert_eq!(
        fs::canonicalize(&link).unwrap(),
        fs::canonicalize(&target).unwrap()
    );
    assert!(compile_actor_assets(pack.path(), MANIFEST).is_err());
}

#[test]
fn explicit_negative_uv_sizes_admit_only_both_in_bounds_endpoints() {
    for (origin, size, expected) in [(16, -16, 1), (0, 16, 1), (17, -16, 0), (0, -1, 0)] {
        let pack = pack(0, "entity_alphatest", false);
        write(pack.path(), "models/entity/example.geo.json", format!(r#"{{"format_version":"1.12.0","minecraft:geometry":[{{"description":{{"identifier":"geometry.example","texture_width":16,"texture_height":16}},"bones":[{{"name":"root","cubes":[{{"origin":[0,0,0],"size":[0,2,7],"uv":{{"west":{{"uv":[{origin},0],"uv_size":[{size},2]}},"east":{{"uv":[0,0],"uv_size":[16,2]}}}}}}]}}]}}]}}"#).as_bytes());
        let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
        assert_eq!(compiled.report.bindings, expected);
        if expected == 0 {
            assert!(
                compiled
                    .report
                    .fallbacks
                    .iter()
                    .any(|fallback| fallback.reason.as_ref() == "uv_extents_or_inheritance")
            );
        }
    }
}

#[test]
fn sibling_compiler_preserves_duplicate_json_and_source_size_protections() {
    let duplicate = pack(0, "entity_alphatest", false);
    write(
        duplicate.path(),
        "render_controllers/example.json",
        br#"{"render_controllers":{},"render_controllers":{}}"#,
    );
    assert!(compile_actor_assets(duplicate.path(), MANIFEST).is_err());
    let oversized = pack(0, "entity_alphatest", false);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(oversized.path().join("textures/entity/example.png"))
        .unwrap();
    file.set_len(assets::MAX_ENTITY_SOURCE_BYTES as u64 + 1)
        .unwrap();
    assert!(compile_actor_assets(oversized.path(), MANIFEST).is_err());
    let escaped = pack(0, "entity_alphatest", false);
    write(escaped.path(), "entity/example.entity.json", br#"{"format_version":"1.8.0","minecraft:client_entity":{"description":{"identifier":"minecraft:example","textures":{"default":"../outside"}}}}"#);
    if let Ok(compiled) = compile_actor_assets(escaped.path(), MANIFEST) {
        assert_eq!(compiled.report.bindings, 0);
    }
}

/// Builds a sprite whose authored texture size can differ from its source raster.
fn item_sprite_pack(path: &str, declared: u16, raster: u32) -> TempDir {
    let pack = pack(0, "entity_alphatest", false);
    let root = pack.path();
    let entity_path = root.join("entity/example.entity.json");
    let mut entity: serde_json::Value =
        serde_json::from_slice(&fs::read(&entity_path).unwrap()).unwrap();
    entity["minecraft:client_entity"]["description"]["textures"]["default"] =
        serde_json::json!(path);
    fs::write(entity_path, serde_json::to_vec(&entity).unwrap()).unwrap();
    write(root, "models/entity/example.geo.json", format!(r#"{{"format_version":"1.12.0","minecraft:geometry":[{{"description":{{"identifier":"geometry.example","texture_width":{declared},"texture_height":{declared}}},"bones":[{{"name":"body","cubes":[{{"origin":[-4,-2,0],"size":[8,8,0],"uv":{{"north":{{"uv":[0,0],"uv_size":[8,8]}}}}}}]}}]}}]}}"#).as_bytes());
    let image_path = root.join(format!("{path}.png"));
    fs::create_dir_all(image_path.parent().unwrap()).unwrap();
    RgbaImage::from_pixel(raster, raster, Rgba([30, 180, 120, 255]))
        .save(image_path)
        .unwrap();
    pack
}

#[test]
fn thrown_item_artwork_accepts_item_icon_sources() {
    let pack = item_sprite_pack("textures/items/example", 16, 16);
    let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        compiled.report.bindings, 1,
        "{:?}",
        compiled.report.fallbacks
    );
    let entity =
        encode_entity_blob(&compile_entity_assets(pack.path(), MANIFEST).unwrap()).unwrap();
    let catalog = RuntimeActorCatalog::decode(
        &compiled.bytes,
        &assets::RuntimeEntityAssets::decode(&entity).unwrap(),
    )
    .unwrap();
    assert_eq!(catalog.textures().len(), 1);
}

#[test]
fn sprite_uvs_use_declared_dimensions_independently_of_raster_resolution() {
    let pack = item_sprite_pack("textures/entity/example", 8, 16);
    let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        compiled.report.bindings, 1,
        "{:?}",
        compiled.report.fallbacks
    );
    let entity =
        encode_entity_blob(&compile_entity_assets(pack.path(), MANIFEST).unwrap()).unwrap();
    let catalog = RuntimeActorCatalog::decode(
        &compiled.bytes,
        &assets::RuntimeEntityAssets::decode(&entity).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (catalog.textures()[0].width, catalog.textures()[0].height),
        (16, 16)
    );
}
