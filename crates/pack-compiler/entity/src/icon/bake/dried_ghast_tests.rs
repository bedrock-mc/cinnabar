use super::*;

/// Writes independently colored body faces and a carried attachment texture.
fn material_fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("textures/blocks")).unwrap();
    std::fs::write(root.path().join("blocks.json"), r#"{"dried_ghast":{"textures":{"down":"bottom","up":"body","side":"body"},"carried_textures":"feet"}}"#).unwrap();
    std::fs::write(root.path().join("textures/terrain_texture.json"), r#"{"texture_data":{"body":{"textures":"textures/blocks/body"},"bottom":{"textures":"textures/blocks/bottom"},"feet":{"textures":"textures/blocks/feet"}}}"#).unwrap();
    std::fs::write(root.path().join("textures/flipbook_textures.json"), "[]").unwrap();
    for name in ["body", "bottom", "feet"] {
        image::RgbaImage::new(32, 32)
            .save(root.path().join(format!("textures/blocks/{name}.png")))
            .unwrap();
    }
    root
}

/// Renders the fixture through the same inventory material selection as compiled packs.
fn fixture_icon(root: &std::path::Path) -> IconSprite {
    let blocks = IconBlocks::read(root).unwrap();
    let visual = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .iter()
    .find(|r| r.name.as_ref() == "minecraft:dried_ghast")
    .unwrap()
    .sequential_id;
    model_raster(
        root,
        &RuntimeAssets::diagnostic(),
        Some(&blocks),
        &[],
        visual,
        0,
    )
    .unwrap()
    .expect("complete carried artwork remains drawable")
    .0
}

#[test]
fn dried_ghast_inventory_culls_opaque_rear_faces_behind_transparent_art() {
    let root = material_fixture();
    image::RgbaImage::from_pixel(32, 32, image::Rgba([255, 0, 0, 255]))
        .save(root.path().join("textures/blocks/bottom.png"))
        .unwrap();
    assert!(
        fixture_icon(root.path())
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 0)
    );
}

#[test]
fn dried_ghast_inventory_uses_the_first_tentacle_flipbook_frame() {
    for frames in [8, 64] {
        let root = material_fixture();
        std::fs::write(root.path().join("textures/flipbook_textures.json"), r#"[{"flipbook_texture":"textures/blocks/feet","atlas_tile":"feet","ticks_per_frame":1}]"#).unwrap();
        image::RgbaImage::from_fn(32, 32 * frames, |_, y| {
            image::Rgba(if y < 32 {
                [0, 0, 255, 255]
            } else {
                [0, 255, 0, 255]
            })
        })
        .save(root.path().join("textures/blocks/feet.png"))
        .unwrap();
        let icon = fixture_icon(root.path());
        assert!(
            icon.rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[2] > 0 && p[3] != 0)
        );
        assert!(icon.rgba8.as_chunks::<4>().0.iter().all(|p| p[1] == 0));
    }
}

#[test]
fn dried_ghast_inventory_applies_tentacle_overlay_color() {
    let root = material_fixture();
    image::RgbaImage::from_pixel(32, 32, image::Rgba([0, 0, 255, 255]))
        .save(root.path().join("textures/blocks/feet.png"))
        .unwrap();
    let catalog = root.path().join("textures/terrain_texture.json");
    let original = std::fs::read_to_string(&catalog).unwrap();
    let mut tinted: serde_json::Value = serde_json::from_str(&original).unwrap();
    tinted["texture_data"]["feet"]["textures"] =
        serde_json::json!({"path":"textures/blocks/feet","overlay_color":"#808080"});
    std::fs::write(&catalog, tinted.to_string()).unwrap();
    let actual = fixture_icon(root.path());
    std::fs::write(&catalog, original).unwrap();
    image::RgbaImage::from_pixel(32, 32, image::Rgba([0, 0, 128, 255]))
        .save(root.path().join("textures/blocks/feet.png"))
        .unwrap();
    assert_eq!(actual.rgba8, fixture_icon(root.path()).rgba8);
}

#[test]
fn dried_ghast_inventory_keeps_separate_body_and_tentacle_artwork() {
    for side in [16, 32, 64, 8] {
        assert_body_and_tentacle_artwork(side, false);
    }
}

#[test]
fn dried_ghast_inventory_uses_the_first_body_flipbook_frame() {
    for side in [16, 32, 128] {
        assert_body_and_tentacle_artwork(side, true);
    }
}

/// Checks carried materials at each resource pack body resolution.
fn assert_body_and_tentacle_artwork(side: u32, animated: bool) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("textures/blocks")).unwrap();
    std::fs::write(
        root.path().join("blocks.json"),
        r#"{"dried_ghast":{"textures":"body","carried_textures":"feet"}}"#,
    )
    .unwrap();
    std::fs::write(root.path().join("textures/terrain_texture.json"), r#"{"texture_data":{"body":{"textures":"textures/blocks/body"},"feet":{"textures":"textures/blocks/feet"}}}"#).unwrap();
    std::fs::write(root.path().join("textures/flipbook_textures.json"), if animated {
        r#"[{"flipbook_texture":"textures/blocks/body","atlas_tile":"body","ticks_per_frame":1}]"#
    } else { "[]" }).unwrap();
    image::RgbaImage::from_fn(side, side * if animated { 2 } else { 1 }, |_, y| {
        image::Rgba(if y < side {
            [255, 0, 0, 255]
        } else {
            [0, 255, 0, 255]
        })
    })
    .save(root.path().join("textures/blocks/body.png"))
    .unwrap();
    image::RgbaImage::from_pixel(32, 32, image::Rgba([0, 0, 255, 255]))
        .save(root.path().join("textures/blocks/feet.png"))
        .unwrap();
    let blocks = IconBlocks::read(root.path()).unwrap();
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let visual = records
        .iter()
        .find(|r| r.name.as_ref() == "minecraft:dried_ghast")
        .unwrap()
        .sequential_id;
    let (icon, live_model) = model_raster(
        root.path(),
        &RuntimeAssets::diagnostic(),
        Some(&blocks),
        &[],
        visual,
        0,
    )
    .unwrap()
    .expect("dried ghast has a shaped inventory preview");
    assert!(
        live_model.is_none(),
        "keep the carried preview instead of the placed fallback"
    );
    let pixels = icon
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[3] != 0)
        .collect::<Vec<_>>();
    assert!(
        pixels.iter().any(|p| p[0] > 0 && p[2] == 0),
        "body artwork stays on the body"
    );
    assert!(
        pixels.iter().any(|p| p[2] > 0 && p[0] == 0),
        "tentacle artwork stays on the feet"
    );
    assert!(
        pixels.iter().all(|p| p[1] == 0),
        "later body frames stay out of the preview"
    );
    assert!(
        pixels.len() < model::SIDE * model::SIDE / 2,
        "small body keeps its carried footprint"
    );
}
