use super::*;
use std::io::Write;

/// Creates custom-path artwork with a sidecar and known pixels.
fn files() -> Vec<(String, Vec<u8>)> {
    let mut png = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(512, 512, image::Rgba([24, 48, 72, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    vec![
        ("assets/gui/inventory.png".into(), png.into_inner()),
        (
            "assets/gui/inventory.json".into(),
            br#"{"base_size":[176,166],"nineslice_size":2}"#.to_vec(),
        ),
    ]
}

/// Admits an archive containing the supplied artwork as a server pack.
pub(super) fn view(files: &[(String, Vec<u8>)]) -> resource_pack::LayeredPackView {
    let id = "00000000-0000-0000-0000-000000000011";
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in std::iter::once(("manifest.json", manifest.as_bytes())).chain(
        files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    ) {
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    let archive = protocol::ResourcePackArchive::unencrypted(
        id.parse().unwrap(),
        "1.0.0".into(),
        String::new(),
        zip.finish().unwrap().into_inner(),
    );
    resource_pack::LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![archive]),
    ))
}

#[test]
fn arbitrary_pack_texture_keeps_source_dimensions_sidecar_and_pixels() {
    let files = files();
    let pack = view(&files);
    for mut atlas in [
        ServerAtlas::new(&files, None, 1),
        ServerAtlas::new(&[], Some(pack), 1),
    ] {
        let key = "assets/gui/inventory";
        assert!(atlas.has_image(key));
        assert_eq!(atlas.image_size(key), Some([512.0, 512.0]));
        assert_eq!(atlas.sidecar(key).unwrap().base_size, [176.0, 166.0]);
        atlas.require([key]);
        let placement = atlas.placement(key).unwrap();
        assert_eq!(placement.rect[2..], [256, 256]);
        let [x, y, _, _] = placement.rect;
        let offset = ((u32::from(y) * PAGE_SIDE + u32::from(x)) * 4) as usize;
        assert_eq!(
            &atlas.pages[usize::from(placement.page)].pixels[offset..offset + 4],
            &[24, 48, 72, 255]
        );
    }
}

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
        ServerAtlas::new(&[], Some(view(&files)), 1),
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
