use super::*;

#[test]
fn dried_ghast_inventory_keeps_separate_body_and_tentacle_artwork() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("textures/blocks")).unwrap();
    std::fs::write(
        root.path().join("blocks.json"),
        r#"{"dried_ghast":{"textures":"body","carried_textures":"feet"}}"#,
    )
    .unwrap();
    std::fs::write(root.path().join("textures/terrain_texture.json"), r#"{"texture_data":{"body":{"textures":"textures/blocks/body"},"feet":{"textures":"textures/blocks/feet"}}}"#).unwrap();
    std::fs::write(root.path().join("textures/flipbook_textures.json"), "[]").unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([255, 0, 0, 255]))
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
        .chunks_exact(4)
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
        pixels.len() < model::SIDE * model::SIDE / 2,
        "small body keeps its carried footprint"
    );
}
