use std::{fs, path::Path};

use ::image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};

use super::CompileRuleResult;
use super::inspect_animation_inventory;
use assets::TILE_SIZE;

fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture parent");
    }
    fs::write(path, contents).expect("write fixture");
}

fn write_png(path: impl AsRef<Path>, width: u32, height: u32, rgba8: &[u8]) {
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(rgba8, width, height, ExtendedColorType::Rgba8)
        .expect("encode synthetic PNG");
    write(path, png);
}

#[test]
fn animation_inventory_inspects_a_bounded_pack_without_installing_it() {
    let directory = tempfile::tempdir().expect("create inventory fixture");
    write(directory.path().join("blocks.json"), "{}");
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{
                "still":{"textures":"textures/blocks/still"},
                "animated":{"textures":"textures/blocks/animated"}
            }}"#,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        r#"[{"flipbook_texture":"textures/blocks/animated","atlas_tile":"animated"}]"#,
    );
    write_png(
        directory.path().join("textures/blocks/still.png"),
        TILE_SIZE,
        TILE_SIZE,
        &vec![7; (TILE_SIZE * TILE_SIZE * 4) as usize],
    );
    let mut strip = vec![0; (TILE_SIZE * TILE_SIZE * 2 * 4) as usize];
    for pixel in strip
        .chunks_exact_mut(4)
        .take((TILE_SIZE * TILE_SIZE) as usize)
    {
        pixel.copy_from_slice(&[10, 20, 30, 255]);
    }
    for pixel in strip
        .chunks_exact_mut(4)
        .skip((TILE_SIZE * TILE_SIZE) as usize)
    {
        pixel.copy_from_slice(&[40, 50, 60, 255]);
    }
    write_png(
        directory.path().join("textures/blocks/animated.png"),
        TILE_SIZE,
        TILE_SIZE * 2,
        &strip,
    );

    let inventory = inspect_animation_inventory(directory.path(), 3, 2)
        .expect("inspect synthetic animation inventory");

    assert_eq!(inventory.static_sources, 1);
    assert_eq!(inventory.reachable_animations, 1);
    assert_eq!(inventory.physical_animation_frames, 2);
    assert_eq!(inventory.deduplicated_layers, 4);
    assert_eq!(inventory.page_layers.as_ref(), [3, 1]);
}

#[test]
fn animation_inventory_counts_catalog_only_missing_static_aliases() {
    let directory = tempfile::tempdir().expect("create missing-static fixture");
    write(directory.path().join("blocks.json"), "{}");
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{
                "virtual":{"textures":"textures/blocks/not_a_physical_file"}
            }}"#,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );

    let inventory = inspect_animation_inventory(directory.path(), 8, 2)
        .expect("catalog-only static aliases are measurable, not animation failures");

    assert_eq!(inventory.catalog_static_sources, 1);
    assert_eq!(inventory.static_sources, 0);
    assert_eq!(inventory.missing_static_sources, 1);
    assert_eq!(inventory.deduplicated_layers, 1, "diagnostic only");
}

#[test]
fn animation_inventory_counts_non_tile_static_uv_sheets_without_paging_them() {
    let directory = tempfile::tempdir().expect("create non-tile fixture");
    write(directory.path().join("blocks.json"), "{}");
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{
                "model_uv":{"textures":"textures/blocks/model_uv"}
            }}"#,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    write_png(
        directory.path().join("textures/blocks/model_uv.png"),
        24,
        12,
        &vec![255; 24 * 12 * 4],
    );

    let inventory = inspect_animation_inventory(directory.path(), 8, 2)
        .expect("non-tile model sheets remain outside texture pages");

    assert_eq!(inventory.catalog_static_sources, 1);
    assert_eq!(inventory.static_sources, 0);
    assert_eq!(inventory.missing_static_sources, 0);
    assert_eq!(inventory.non_tile_static_sources, 1);
    assert_eq!(inventory.deduplicated_layers, 1, "diagnostic only");
}

#[test]
fn animation_inventory_rejects_a_missing_flipbook_strip() {
    let directory = tempfile::tempdir().expect("create missing-animation fixture");
    write(directory.path().join("blocks.json"), "{}");
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{
                "animated":{"textures":"textures/blocks/missing_strip"}
            }}"#,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        r#"[{
                "flipbook_texture":"textures/blocks/missing_strip",
                "atlas_tile":"animated"
            }]"#,
    );

    let error = inspect_animation_inventory(directory.path(), 8, 2)
        .expect_err("a missing physical animation strip must fail closed");
    assert!(matches!(
        error,
        assets::AssetError::MissingAnimationTexture { ref source_path }
            if source_path.as_ref() == "textures/blocks/missing_strip"
    ));
}

#[test]
fn visual_family_dispatch_uses_explicit_ordered_outcomes() {
    assert!(matches!(
        CompileRuleResult::NoMatch,
        CompileRuleResult::NoMatch
    ));
    assert!(matches!(
        CompileRuleResult::Reject,
        CompileRuleResult::Reject
    ));
    let visual = assets::BlockVisual::diagnostic(
        assets::BlockFlags::empty(),
        assets::ContributorRole::Primary,
    );
    assert!(matches!(
        CompileRuleResult::Compiled(visual),
        CompileRuleResult::Compiled(observed) if observed == visual
    ));
}
