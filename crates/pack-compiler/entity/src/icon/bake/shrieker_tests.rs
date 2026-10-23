use super::*;

#[test]
fn shrieker_inventory_keeps_open_walls_and_first_inner_frame() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("textures/blocks")).unwrap();
    std::fs::write(root.path().join("blocks.json"), r#"{"sculk_shrieker":{"textures":{"up":"cap","down":"bottom","side":"wall"},"carried_textures":"inner"}}"#).unwrap();
    std::fs::write(root.path().join("textures/terrain_texture.json"), r#"{"texture_data":{"cap":{"textures":"textures/blocks/cap"},"bottom":{"textures":"textures/blocks/bottom"},"wall":{"textures":"textures/blocks/wall"},"inner":{"textures":"textures/blocks/inner"}}}"#).unwrap();
    std::fs::write(root.path().join("textures/flipbook_textures.json"), r#"[{"flipbook_texture":"textures/blocks/inner","atlas_tile":"inner","ticks_per_frame":1}]"#).unwrap();
    for name in ["bottom", "wall"] {
        let mut image = image::RgbaImage::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                if y >= 8 || x < 3 || x >= 13 {
                    image.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
                }
            }
        }
        image
            .save(root.path().join(format!("textures/blocks/{name}.png")))
            .unwrap();
    }
    image::RgbaImage::new(16, 16)
        .save(root.path().join("textures/blocks/cap.png"))
        .unwrap();
    let mut inner = image::RgbaImage::from_pixel(16, 32, image::Rgba([0, 0, 255, 255]));
    for y in 0..16 {
        for x in 0..16 {
            inner.put_pixel(x, y, image::Rgba([0, 255, 0, 255]));
        }
    }
    inner
        .save(root.path().join("textures/blocks/inner.png"))
        .unwrap();
    let blocks = IconBlocks::read(root.path()).unwrap();
    let visual = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .into_iter()
    .find(|r| r.name.as_ref() == "minecraft:sculk_shrieker")
    .unwrap()
    .sequential_id;
    let (icon, live) = model_raster(
        root.path(),
        &RuntimeAssets::diagnostic(),
        Some(&blocks),
        &[],
        visual,
        0,
    )
    .unwrap()
    .expect("shrieker has an inventory preview");
    assert!(live.is_none());
    let pixels: Vec<_> = icon.rgba8.chunks_exact(4).filter(|p| p[3] != 0).collect();
    assert!(pixels.iter().any(|p| p[0] > 0 && p[1] == 0), "outer walls");
    assert!(
        pixels.iter().any(|p| p[1] > 0 && p[0] == 0),
        "inner floor through open top"
    );
    assert!(
        pixels.iter().all(|p| p[2] == 0),
        "later animation frames stay out of the default preview"
    );
}
