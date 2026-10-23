use super::*;

#[test]
fn dripleaf_inventory_keeps_leaf_tops_edges_and_crossed_stems() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("textures/blocks")).unwrap();
    std::fs::write(root.path().join("blocks.json"), r#"{"big_dripleaf":{"textures":{"up":"edge","down":"edge","north":"stem","south":"leaf","east":"leaf","west":"leaf"}},"small_dripleaf_block":{"textures":{"up":"small_leaf","down":"small_edge","north":"stem","south":"stem","east":"stem","west":"stem"}}}"#).unwrap();
    std::fs::write(root.path().join("textures/terrain_texture.json"), r#"{"texture_data":{"leaf":{"textures":"textures/blocks/leaf"},"edge":{"textures":"textures/blocks/edge"},"stem":{"textures":"textures/blocks/stem"},"small_leaf":{"textures":"textures/blocks/small_leaf"},"small_edge":{"textures":"textures/blocks/small_edge"}}}"#).unwrap();
    std::fs::write(root.path().join("textures/flipbook_textures.json"), "[]").unwrap();
    for (name, color) in [
        ("leaf", [0, 255, 0, 255]),
        ("edge", [255, 0, 0, 255]),
        ("stem", [0, 0, 255, 255]),
    ] {
        image::RgbaImage::from_pixel(16, 16, image::Rgba(color))
            .save(root.path().join(format!("textures/blocks/{name}.png")))
            .unwrap();
    }
    for (name, color) in [
        ("small_leaf", [0, 255, 0, 255]),
        ("small_edge", [255, 0, 0, 255]),
    ] {
        let image = image::RgbaImage::from_fn(16, 16, |x, y| {
            image::Rgba(if x < 8 && y < 8 {
                color
            } else {
                [255, 255, 0, 255]
            })
        });
        image
            .save(root.path().join(format!("textures/blocks/{name}.png")))
            .unwrap();
    }
    let blocks = IconBlocks::read(root.path()).unwrap();
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    for name in ["minecraft:big_dripleaf", "minecraft:small_dripleaf_block"] {
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
        .expect("dripleaf has a carried preview");
        assert!(live.is_none());
        assert!(
            icon.rgba8
                .chunks_exact(4)
                .all(|p| p[3] == 0 || p[0] == 0 || p[1] == 0),
            "{name}: cropped small-leaf artwork excludes other atlas quarters"
        );
        let pixels: Vec<_> = icon.rgba8.chunks_exact(4).filter(|p| p[3] != 0).collect();
        for channel in 0..3 {
            assert!(
                pixels.iter().any(|p| p[channel] > 0
                    && p[(channel + 1) % 3] == 0
                    && p[(channel + 2) % 3] == 0),
                "{name}: material channel {channel}"
            );
        }
        assert!(
            pixels
                .iter()
                .filter(|p| p[1] > 0 || p[2] > 0)
                .all(|p| p[1] == 255 || p[2] == 255),
            "{name}: leaves and stems stay unshaded"
        );
    }
}
