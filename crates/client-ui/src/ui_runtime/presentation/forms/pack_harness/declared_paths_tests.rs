//! Unpacked fixture packs follow the same indexed paths as admitted server packs.

use super::{dir_pack, scratch_dir};
use crate::ui_runtime::presentation::forms::server_pack::ServerAtlas;

// Unix permits colons in names, reproducing the drive-letter split without Windows.
#[cfg(unix)]
#[test]
fn dir_pack_keeps_colons_in_directory_names() {
    let root = scratch_dir("colon-pack-harness");
    let lower = root.join("drive:lower");
    let upper = root.join("drive:upper");
    for (dir, contents) in [(&lower, b"lower"), (&upper, b"upper")] {
        std::fs::create_dir_all(dir.join("ui")).unwrap();
        std::fs::write(dir.join("ui/test.json"), contents).unwrap();
    }

    let pack = dir_pack([lower, upper]);
    std::fs::remove_dir_all(root).unwrap();
    assert_eq!(pack.ui_layers.len(), 2);
    assert_eq!(
        pack.ui_layers[0],
        [("ui/test.json".into(), b"lower".to_vec())]
    );
    assert_eq!(
        pack.ui_layers[1],
        [("ui/test.json".into(), b"upper".to_vec())]
    );
}

#[test]
fn dir_pack_loads_declared_paths_and_inherits_them_across_layers() {
    let root = scratch_dir("indexed-pack-harness");
    let lower = root.join("lower");
    let upper = root.join("upper");
    std::fs::create_dir_all(lower.join("ui")).unwrap();
    std::fs::create_dir_all(lower.join("custom")).unwrap();
    std::fs::create_dir_all(upper.join("custom")).unwrap();
    std::fs::write(
        lower.join("ui/_ui_defs.json"),
        br#"{"ui_defs":["custom/inventory.uidx","ui/legacy.json"]}"#,
    )
    .unwrap();
    std::fs::write(
        lower.join("custom/inventory.uidx"),
        br#"{"namespace":"custom","inventory":{"type":"panel","size":[10,10]}}"#,
    )
    .unwrap();
    std::fs::write(
        upper.join("custom/inventory.uidx"),
        br#"{"inventory":{"size":[20,20]}}"#,
    )
    .unwrap();
    std::fs::write(
        lower.join("ui/legacy.json"),
        br#"{"namespace":"legacy","screen":{"type":"panel"}}"#,
    )
    .unwrap();
    std::fs::write(lower.join("custom/unlisted.uidx"), b"unlisted").unwrap();

    let pack = dir_pack([lower, upper]);
    std::fs::remove_dir_all(root).unwrap();
    assert_eq!(pack.ui_layers.len(), 2);
    for layer in &pack.ui_layers {
        assert!(
            layer
                .iter()
                .any(|(path, _)| path == "custom/inventory.uidx")
        );
        assert!(layer.iter().all(|(path, _)| path != "custom/unlisted.uidx"));
    }
    let mut catalog = json_ui::Catalog::default();
    for layer in &pack.ui_layers {
        catalog.apply_pack(
            layer
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        );
    }
    assert_eq!(
        catalog.lookup("custom", "inventory").unwrap().props["size"],
        serde_json::json!([20, 20])
    );
    assert!(catalog.lookup("legacy", "screen").is_some());
    assert!(catalog.diagnostics().is_empty());
}

#[test]
fn dir_pack_retains_custom_art_and_independent_sidecar_overrides() {
    let root = scratch_dir("custom-art-pack-harness");
    let lower = root.join("lower");
    let upper = root.join("upper");
    for layer in [&lower, &upper] {
        std::fs::create_dir_all(layer.join("assets/gui")).unwrap();
    }
    image::RgbaImage::from_pixel(4, 8, image::Rgba([24, 48, 72, 255]))
        .save(lower.join("assets/gui/inventory.png"))
        .unwrap();
    std::fs::write(
        lower.join("assets/gui/inventory.json"),
        br#"{"base_size":[4,8],"nineslice_size":1}"#,
    )
    .unwrap();
    std::fs::write(
        upper.join("assets/gui/inventory.json"),
        br#"{"base_size":[12,16],"nineslice_size":2}"#,
    )
    .unwrap();

    let pack = dir_pack([lower, upper]);
    std::fs::remove_dir_all(root).unwrap();
    let atlas = ServerAtlas::new(&pack.textures, None, 1);
    assert!(atlas.has_image("assets/gui/inventory"));
    assert_eq!(atlas.image_size("assets/gui/inventory"), Some([4.0, 8.0]));
    assert_eq!(
        atlas.sidecar("assets/gui/inventory").unwrap().base_size,
        [12.0, 16.0]
    );
}
