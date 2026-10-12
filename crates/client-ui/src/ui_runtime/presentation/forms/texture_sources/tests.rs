use std::{io::Cursor, sync::Arc};

use ui_engine::atlas::{ServerAtlas, VANILLA_IN_PACKAGE};

use super::VanillaPack;

/// Encodes a deterministic PNG fixture.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba([1, 2, 3, 255]))
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

// Vanilla's `textures/ui/White` must not open `white.png`, even on a
// case-insensitive file system.
#[test]
fn vanilla_images_match_their_exact_spelling() {
    let root = std::env::temp_dir().join(format!(
        "cinnabar-exact-case-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let ui = root.join("textures/ui");
    std::fs::create_dir_all(&ui).unwrap();
    std::fs::write(ui.join("white.png"), png(2, 2)).unwrap();
    let atlas = ServerAtlas::new(&[], None, 1)
        .with_fallbacks(Some(Arc::new(VanillaPack::new(root.clone()))), None);
    assert_eq!(atlas.fallback_size("textures/ui/white"), Some([2.0, 2.0]));
    assert_eq!(atlas.fallback_size("textures/ui/White"), None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn vanilla_sidecars_inherit_without_an_image_and_keep_exact_case() {
    let root = std::env::temp_dir().join(format!(
        "cinnabar-sidecar-case-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let ui = root.join("textures/ui");
    std::fs::create_dir_all(&ui).unwrap();
    let path = ui.join("border.json");
    std::fs::write(&path, br#"{"base_size":[16,16],"nineslice_size":4}"#).unwrap();
    let atlas = ServerAtlas::new(&[], None, 1)
        .with_fallbacks(Some(Arc::new(VanillaPack::new(root.clone()))), None);
    assert!(atlas.fallback_size("textures/ui/border").is_none());
    assert!(atlas.sidecar("textures/ui/border").is_none());
    let meta = atlas.fallback_sidecar("textures/ui/border").unwrap();
    assert_eq!(meta.base_size, [16.0, 16.0]);
    assert_eq!(atlas.fallback_sidecar("textures/ui/Border"), None);
    assert_eq!(
        atlas.fallback_sidecar(&format!("{VANILLA_IN_PACKAGE}textures/ui/border")),
        Some(meta)
    );
    std::fs::remove_file(path).unwrap();
    assert_eq!(atlas.fallback_sidecar("textures/ui/border"), Some(meta));
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn vanilla_fallback_rejects_parent_paths_and_symlink_escapes() {
    let root = std::env::temp_dir().join(format!(
        "cinnabar-fallback-boundary-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let vanilla = root.join("vanilla");
    std::fs::create_dir_all(vanilla.join("textures")).unwrap();
    std::fs::write(root.join("private.png"), png(2, 2)).unwrap();
    std::fs::write(root.join("private.json"), br#"{"nineslice_size":4}"#).unwrap();
    let atlas = ServerAtlas::new(&[], None, 1)
        .with_fallbacks(Some(Arc::new(VanillaPack::new(vanilla.clone()))), None);
    for key in [
        format!("{VANILLA_IN_PACKAGE}../private"),
        "textures/../../private".into(),
    ] {
        assert!(atlas.fallback_size(&key).is_none(), "{key}");
        assert!(atlas.fallback_sidecar(&key).is_none(), "{key}");
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("private.png"), vanilla.join("textures/link.png"))
            .unwrap();
        std::os::unix::fs::symlink(
            root.join("private.json"),
            vanilla.join("textures/link.json"),
        )
        .unwrap();
        assert!(atlas.fallback_size("textures/link").is_none());
        assert!(atlas.fallback_sidecar("textures/link").is_none());
    }
    std::fs::remove_dir_all(root).unwrap();
}
