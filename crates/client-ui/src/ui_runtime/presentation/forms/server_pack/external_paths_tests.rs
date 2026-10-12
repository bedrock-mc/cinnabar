use super::*;
use resource_pack::test_support::pack_view;
use ui_engine::atlas::ServerAtlas;

#[test]
fn pack_image_path_uses_supported_extensions_without_directory_restriction() {
    assert!(ServerUiPack::is_image_path("assets/gui/inventory.png"));
    assert!(ServerUiPack::is_image_path("custom/menu.tga"));
    assert!(!ServerUiPack::is_image_path("assets/gui/inventory.json"));
}

#[test]
fn animated_sidecars_survive_lazy_pack_reads_and_image_independence() {
    let files = vec![(
        "custom/gui/noise.json".into(),
        br#"{"frames":[{"frame":{"x":8,"y":0},"duration":50}]}"#.to_vec(),
    )];
    for atlas in [
        ServerAtlas::new(&files, None, 1),
        ServerAtlas::new(&[], Some(pack_view(&files)), 1),
    ] {
        let first = atlas.aseprite_frames("custom/gui/noise").unwrap();
        assert_eq!(first[0].x, 8);
        assert!(Arc::ptr_eq(
            &first,
            &atlas.aseprite_frames("custom/gui/noise").unwrap()
        ));
        assert!(!atlas.has_image("custom/gui/noise"));
    }
}
