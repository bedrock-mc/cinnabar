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
    let pixels: Vec<_> = icon
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[3] != 0)
        .collect();
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

#[test]
fn shrieker_cap_cutouts_leave_the_inner_floor_visible() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("textures/blocks")).unwrap();
    std::fs::write(root.path().join("blocks.json"), r#"{"sculk_shrieker":{"textures":{"up":"cap","down":"clear","side":"clear"},"carried_textures":"inner"}}"#).unwrap();
    std::fs::write(root.path().join("textures/terrain_texture.json"), r#"{"texture_data":{"cap":{"textures":"textures/blocks/cap"},"clear":{"textures":"textures/blocks/clear"},"inner":{"textures":"textures/blocks/inner"}}}"#).unwrap();
    std::fs::write(root.path().join("textures/flipbook_textures.json"), "[]").unwrap();
    image::RgbaImage::new(16, 16)
        .save(root.path().join("textures/blocks/clear.png"))
        .unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([0, 255, 0, 255]))
        .save(root.path().join("textures/blocks/inner.png"))
        .unwrap();
    image::RgbaImage::from_fn(16, 16, |_, y| {
        image::Rgba(if y < 8 { [255, 0, 0, 255] } else { [0; 4] })
    })
    .save(root.path().join("textures/blocks/cap.png"))
    .unwrap();
    let blocks = IconBlocks::read(root.path()).unwrap();
    let visual = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .iter()
    .find(|r| r.name.as_ref() == "minecraft:sculk_shrieker")
    .unwrap()
    .sequential_id;
    let icon = model_raster(
        root.path(),
        &RuntimeAssets::diagnostic(),
        Some(&blocks),
        &[],
        visual,
        0,
    )
    .unwrap()
    .unwrap()
    .0;
    // This ray crosses the transparent half of the cap and meets the green inner floor.
    let projected = assets::gui_item::project_cube([0.25, 0.9375, 0.75]);
    let [x, y] = [projected[0], projected[1]]
        .map(|v| (v * model::SIDE as f32 / assets::gui_item::GUI_ITEM_SIDE).floor() as usize);
    let pixel = &icon.rgba8[(y * model::SIDE + x) * 4..][..4];
    assert!(
        pixel[0] == 0 && pixel[1] > 0 && pixel[3] != 0,
        "cutout ray sees the inner floor: {pixel:?}"
    );
}
