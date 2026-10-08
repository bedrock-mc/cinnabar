#![cfg(feature = "handoff")]

use resource_pack::{LayeredPackView, PackDependency, validate_handoff};
use std::{
    collections::BTreeSet,
    io::{Cursor, Write},
};

/// Builds a tracked pack view containing the requested file names.
fn view(paths: &[&str]) -> LayeredPackView {
    let id = "00000000-0000-0000-0000-000000000021";
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(manifest.as_bytes()).unwrap();
    for path in paths {
        zip.start_file(*path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"fixture").unwrap();
    }
    let archive = protocol::ResourcePackArchive::unencrypted(
        id.parse().unwrap(),
        "1.0.0".into(),
        String::new(),
        zip.finish().unwrap().into_inner(),
    );
    LayeredPackView::tracked(validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![archive]),
    ))
}

#[test]
fn filtered_listing_ignores_language_additions_and_observes_new_images() {
    let base = view(&["assets/gui/inventory.png"]);
    let language = view(&["assets/gui/inventory.png", "texts/new.lang"]);
    let image = view(&["assets/gui/inventory.png", "custom/button.jpg"]);
    let suffixes = [".png", ".jpg", ".png"];
    let original = base.list_with_suffixes("", &suffixes);
    assert_eq!(original, language.list_with_suffixes("", &suffixes));
    assert_ne!(original, image.list_with_suffixes("", &suffixes));
    let expected = BTreeSet::from([PackDependency::DirectoryWithSuffixes {
        prefix: String::new(),
        suffixes: vec![".jpg".into(), ".png".into()],
    }]);
    assert_eq!(base.dependencies().unwrap().snapshot(), expected);
    let _ = base.list_with_suffixes("", &[".jpg", ".png"]);
    assert_eq!(base.dependencies().unwrap().snapshot(), expected);
    assert_eq!(
        image.list_with_suffixes("assets/", &suffixes),
        ["assets/gui/inventory.png"]
    );
}
