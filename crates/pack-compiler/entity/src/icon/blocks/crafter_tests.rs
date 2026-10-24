use super::*;

#[test]
fn crafter_inventory_faces_use_the_idle_south_up_pose() {
    let root = tempfile::tempdir().unwrap();
    super::tests::write_pack(
        root.path(),
        r#"{"crafter":{"textures":{"up":"up","down":"down","north":"north","south":"south","west":"west","east":"east"}}}"#,
        r#"{"texture_data":{"up":{"textures":"textures/blocks/up"},"down":{"textures":"textures/blocks/down"},"north":{"textures":"textures/blocks/north"},"south":{"textures":"textures/blocks/south"},"west":{"textures":"textures/blocks/west"},"east":{"textures":"textures/blocks/east"}}}"#,
        "[]",
    );
    for (i, name) in ["west", "east", "down", "up", "north", "south"]
        .iter()
        .enumerate()
    {
        let mut art = image::RgbaImage::from_pixel(16, 16, image::Rgba([i as u8, 10, 20, 255]));
        art.put_pixel(0, 0, image::Rgba([i as u8, 100, 200, 255]));
        art.save(root.path().join(format!("textures/blocks/{name}.png")))
            .unwrap();
    }
    let blocks = IconBlocks::read(root.path()).unwrap();
    let mut seen = 0;
    for record in blocks
        .records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:crafter")
    {
        let visual = BlockVisualId(record.sequential_id);
        let pose = &blocks.records[blocks.icon_state(visual).0 as usize];
        let state: serde_json::Value = serde_json::from_str(&pose.canonical_state).unwrap();
        assert_eq!(state["orientation"]["value"], "south_up");
        assert_eq!(state["crafting"]["value"], 0);
        assert_eq!(state["triggered_bit"]["value"], 0);
        let tiles = blocks
            .carried_cube_tiles(root.path(), visual)
            .unwrap()
            .unwrap();
        for (face, source) in [
            (BlockFace::West, BlockFace::East),
            (BlockFace::East, BlockFace::West),
            (BlockFace::Down, BlockFace::Down),
            (BlockFace::Up, BlockFace::Up),
            (BlockFace::North, BlockFace::South),
            (BlockFace::South, BlockFace::North),
        ] {
            let tile = &tiles[face as usize];
            assert_eq!(tile.rgba8[0], source as u8);
            assert_eq!(
                &tile.rgba8[1..4],
                if face == BlockFace::Down {
                    &[10, 20, 255]
                } else {
                    &[100, 200, 255]
                }
            );
            if face == BlockFace::Down {
                assert_eq!(&tile.rgba8[1021..1024], &[100, 200, 255]);
            }
        }
        seen += 1;
    }
    assert!(seen >= 48);
}
