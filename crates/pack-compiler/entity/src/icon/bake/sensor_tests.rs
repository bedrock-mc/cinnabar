use super::*;

#[test]
fn sensor_inventory_keeps_body_tendrils_and_calibration_crystal() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("textures/blocks")).unwrap();
    std::fs::write(root.path().join("blocks.json"), r#"{"sculk_sensor":{"textures":"body","carried_textures":{"up":"tendril","down":"tendril","side":"tendril"}},"calibrated_sculk_sensor":{"textures":"body","carried_textures":{"up":"tendril","down":"tendril","north":"tendril","south":"crystal","east":"tendril","west":"tendril"}}}"#).unwrap();
    std::fs::write(root.path().join("textures/terrain_texture.json"), r#"{"texture_data":{"body":{"textures":"textures/blocks/body"},"tendril":{"textures":"textures/blocks/tendril"},"crystal":{"textures":"textures/blocks/crystal"}}}"#).unwrap();
    std::fs::write(root.path().join("textures/flipbook_textures.json"), "[]").unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([255, 0, 0, 255]))
        .save(root.path().join("textures/blocks/body.png"))
        .unwrap();
    let mut tendril = image::RgbaImage::new(16, 16);
    for x in [0, 1, 14, 15] {
        for y in 4..12 {
            tendril.put_pixel(x, y, image::Rgba([0, 0, 255, 255]));
        }
    }
    tendril
        .save(root.path().join("textures/blocks/tendril.png"))
        .unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([0, 255, 0, 255]))
        .save(root.path().join("textures/blocks/crystal.png"))
        .unwrap();
    let blocks = IconBlocks::read(root.path()).unwrap();
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    for name in [
        "minecraft:sculk_sensor",
        "minecraft:calibrated_sculk_sensor",
    ] {
        let visual = records
            .iter()
            .find(|r| r.name.as_ref() == name)
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
        .expect("sensor has an inventory preview");
        assert!(live.is_none());
        let pixels: Vec<_> = icon
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] != 0)
            .collect();
        assert!(
            pixels.iter().any(|p| p[0] > 0 && p[1] == 0 && p[2] == 0),
            "{name}: body artwork"
        );
        assert!(
            pixels.iter().any(|p| p[2] > 0 && p[0] == 0 && p[1] == 0),
            "{name}: tendril artwork"
        );
        assert!(
            pixels.iter().filter(|p| p[2] > 0).all(|p| p[2] == 140),
            "{name}: inactive tendril brightness"
        );
        assert_eq!(
            pixels.iter().any(|p| p[1] > 0 && p[0] == 0 && p[2] == 0),
            name == "minecraft:calibrated_sculk_sensor"
        );
    }
}
