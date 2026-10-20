use bedrock_packs::{ResourcePackArchive, ResourcePackHandoff};
use std::io::{Cursor, Write};

use uuid::Uuid;
use zip::{ZipWriter, write::SimpleFileOptions};

use super::{
    AdmissionError, LayeredPackView, PackRejection, validate_handoff, validate_handoff_for_device,
};

const PACK_ID: Uuid = Uuid::from_u128(0x11111111_2222_3333_4444_555555555555);

fn pack_zip(id: Uuid, entries: &[(&str, &[u8])]) -> Vec<u8> {
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"name":"test","description":"test","uuid":"{id}","version":[1,2,3]}},"modules":[{{"type":"resources","uuid":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","version":[1,2,3]}}]}}"#
    );
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("manifest.json", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(manifest.as_bytes()).unwrap();
    for (path, bytes) in entries {
        writer
            .start_file(*path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn archive(id: Uuid, bytes: Vec<u8>) -> ResourcePackArchive {
    ResourcePackArchive::unencrypted(id, "1.2.3".into(), String::new(), bytes)
}

fn handoff(archives: Vec<ResourcePackArchive>) -> ResourcePackHandoff {
    ResourcePackHandoff::from_archives(archives)
}

fn tiered_archive(requested: &str) -> ResourcePackArchive {
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{PACK_ID}","version":[1,2,3]}},
        "modules":[{{"type":"resources"}}],"subpacks":[
        {{"folder_name":"lite","memory_tier":0}},
        {{"folder_name":"full","memory_tier":12}}]}}"#
    );
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in [
        ("manifest.json", manifest.as_bytes()),
        ("font/glyph_E1.png", b"root"),
        ("subpacks/lite/font/glyph_E1.png", b"lite"),
        ("subpacks/full/font/glyph_E1.png", b"full"),
    ] {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    ResourcePackArchive::unencrypted(
        PACK_ID,
        "1.2.3".into(),
        requested.into(),
        writer.finish().unwrap().into_inner(),
    )
}

#[test]
fn server_device_selection_applies_automatic_and_supported_explicit_subpacks() {
    for (requested, memory, expected) in [
        ("", 24 << 30, "full"),
        ("", 4 << 30, "lite"),
        ("", 0, "lite"),
        ("lite", 24 << 30, "lite"),
        ("full", 4 << 30, "lite"),
        ("unavailable label", 24 << 30, "full"),
    ] {
        let stack = validate_handoff_for_device(handoff(vec![tiered_archive(requested)]), memory);
        assert!(stack.rejections().is_empty());
        assert_eq!(stack.packs()[0].sub_pack_name(), expected);
        let view = LayeredPackView::new(stack);
        assert_eq!(
            view.read("font/glyph_E1.png").unwrap().as_ref(),
            expected.as_bytes()
        );
        assert!(view.list("subpacks/").is_empty());
    }
    let exact_root = LayeredPackView::new(validate_handoff(handoff(vec![tiered_archive("")])));
    assert_eq!(
        exact_root.read("font/glyph_E1.png").unwrap().as_ref(),
        b"root"
    );
}

#[test]
fn actual_server_font_subpack_uses_full_resolution_glyph_page() {
    let Some(path) = std::env::var_os("CINNABAR_SUBPACK_FIXTURE") else {
        eprintln!(
            "skipping actual_server_font_subpack_uses_full_resolution_glyph_page: missing CINNABAR_SUBPACK_FIXTURE cached font pack"
        );
        return;
    };
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping actual_server_font_subpack_uses_full_resolution_glyph_page: missing CINNABAR_SUBPACK_FIXTURE cached font pack"
            );
            return;
        }
        Err(error) => panic!("read cached font fixture: {error}"),
    };
    let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_reader(zip.by_name("manifest.json").unwrap()).unwrap();
    let id = manifest["header"]["uuid"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let version = crate::manifest::Version::from_value(&manifest["header"]["version"])
        .unwrap()
        .0
        .map(|part| part.to_string())
        .join(".");
    let handoff = ResourcePackHandoff::from_archives(vec![ResourcePackArchive::unencrypted(
        id,
        version,
        String::new(),
        bytes.clone(),
    )]);
    let stack = validate_handoff_for_device(handoff, 24 << 30);
    assert!(stack.rejections().is_empty());
    assert_eq!(stack.packs()[0].sub_pack_name(), "f");
    let view = LayeredPackView::new(stack);
    let selected = view.read("font/glyph_E1.png").unwrap();
    use std::io::Read;
    let mut authored = Vec::new();
    zip.by_name("subpacks/f/font/glyph_E1.png")
        .unwrap()
        .read_to_end(&mut authored)
        .unwrap();
    assert_eq!(selected.as_ref(), authored);
    assert_eq!(
        u32::from_be_bytes(selected[16..20].try_into().unwrap()),
        4096
    );
    assert_eq!(
        u32::from_be_bytes(selected[20..24].try_into().unwrap()),
        4096
    );
}

#[test]
fn empty_production_handoff_is_validated_without_archives() {
    let stack = validate_handoff(ResourcePackHandoff::default());
    assert!(stack.packs().is_empty() && stack.rejections().is_empty());
}

#[test]
fn nonempty_production_handoff_preserves_selected_metadata() {
    let stack = validate_handoff(handoff(vec![archive(PACK_ID, pack_zip(PACK_ID, &[]))]));
    assert_eq!(stack.packs().len(), 1);
    assert_eq!(stack.packs()[0].pack_id(), PACK_ID);
    assert_eq!(stack.packs()[0].version(), "1.2.3");
    assert_eq!(stack.packs()[0].sub_pack_name(), "");
}

#[test]
fn required_plaintext_stack_with_advertised_keys_applies_every_layer() {
    let top = Uuid::from_u128(2);
    let archives =
        [(top, b"upper".as_slice()), (PACK_ID, b"lower".as_slice())].map(|(id, text)| {
            ResourcePackArchive::with_content_key(
                id,
                "1.2.3".into(),
                String::new(),
                pack_zip(id, &[("texts/en_US.lang", text)]),
                b"0123456789abcdefghijklmnopqrstuv".to_vec(),
            )
        });
    let handoff = handoff(archives.into()).with_required(true);
    assert!(handoff.required());
    let stack = validate_handoff(handoff);
    assert!(stack.rejections().is_empty(), "{:?}", stack.rejections());
    assert_eq!(stack.packs().len(), 2);
    let view = LayeredPackView::new(stack);
    assert_eq!(
        view.read_layers("texts/en_US.lang")
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
        [b"lower".as_slice(), b"upper".as_slice()]
    );
    assert_eq!(
        view.read("texts/en_US.lang").as_deref(),
        Some(b"upper".as_slice())
    );
}

#[test]
fn plaintext_key_advertisement_still_rejects_invalid_key_lengths() {
    let stack = validate_handoff(
        handoff(vec![ResourcePackArchive::with_content_key(
            PACK_ID,
            "1.2.3".into(),
            String::new(),
            pack_zip(PACK_ID, &[]),
            b"short".to_vec(),
        )])
        .with_required(true),
    );
    assert!(stack.packs().is_empty());
    assert_eq!(
        stack.rejections(),
        [PackRejection {
            stack_index: 0,
            reason: AdmissionError::InvalidContentKey
        }]
    );
}

#[test]
fn required_stack_with_undeclared_subpack_labels_keeps_root_layers() {
    let upper = Uuid::from_u128(2);
    let archives = [
        (upper, "Server UI", b"upper".as_slice()),
        (PACK_ID, "Common assets", b"lower".as_slice()),
    ]
    .map(|(id, selected, text)| {
        ResourcePackArchive::unencrypted(
            id,
            "1.2.3".into(),
            selected.into(),
            pack_zip(id, &[("texts/en_US.lang", text)]),
        )
    });
    let stack = validate_handoff(handoff(archives.into()).with_required(true));
    assert!(stack.rejections().is_empty());
    assert_eq!(stack.packs().len(), 2);
    assert_eq!(stack.packs()[0].sub_pack_name(), "Common assets");
    assert_eq!(stack.packs()[1].sub_pack_name(), "Server UI");
    let view = LayeredPackView::new(stack);
    assert_eq!(
        view.read("texts/en_US.lang").as_deref(),
        Some(b"upper".as_slice())
    );
}

#[test]
fn a_bad_pack_is_dropped_with_its_reason_and_the_rest_still_apply() {
    let good = Uuid::from_u128(2);
    let stack = validate_handoff(handoff(vec![
        archive(PACK_ID, vec![0; 32]),
        archive(good, pack_zip(good, &[])),
        archive(good, pack_zip(good, &[])),
    ]));
    assert_eq!(stack.packs().len(), 1);
    assert_eq!(stack.packs()[0].pack_id(), good);
    assert_eq!(
        stack.rejections(),
        [
            PackRejection {
                stack_index: 0,
                reason: AdmissionError::InvalidZipFooter
            },
            PackRejection {
                stack_index: 2,
                reason: AdmissionError::DuplicatePack
            },
        ]
    );
}

/// Every distinct selected archive survives a stack larger than the former admission count.
#[test]
fn server_stack_retains_every_selected_archive() {
    let archives = (1..=40)
        .map(|index| {
            let id = Uuid::from_u128(index);
            archive(id, pack_zip(id, &[]))
        })
        .collect();
    let stack = validate_handoff(handoff(archives));
    assert_eq!(stack.packs().len(), 40);
    assert!(stack.rejections().is_empty());
}

// The first server entry wins an overlapping path; merges apply it last.
#[test]
fn first_server_stack_entry_has_the_highest_precedence() {
    let (top, bottom) = (Uuid::from_u128(3), Uuid::from_u128(4));
    let stack = validate_handoff(handoff(vec![
        archive(top, pack_zip(top, &[("texts/en_US.lang", b"k=top")])),
        archive(
            bottom,
            pack_zip(
                bottom,
                &[("texts/en_US.lang", b"k=bottom"), ("only.txt", b"x")],
            ),
        ),
    ]));
    let view = LayeredPackView::new(stack);
    assert_eq!(
        view.read("texts/en_US.lang").as_deref(),
        Some(b"k=top".as_slice())
    );
    assert_eq!(
        view.read("TEXTS/EN_US.LANG").as_deref(),
        Some(b"k=top".as_slice())
    );
    let layers = view.read_layers("texts/en_US.lang");
    assert_eq!(
        layers.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
        [b"k=bottom".as_slice(), b"k=top".as_slice()]
    );
    assert_eq!(view.read("only.txt").as_deref(), Some(b"x".as_slice()));
    assert_eq!(view.list("texts/"), ["texts/en_US.lang"]);
}

/// Rejected entries keep their wire indexes when admitted layers are reordered.
#[test]
fn rejection_indexes_and_first_pack_priority_survive_normalization() {
    let high = Uuid::from_u128(5);
    let low = Uuid::from_u128(6);
    let rejected = Uuid::from_u128(7);
    let stack = validate_handoff(handoff(vec![
        archive(high, pack_zip(high, &[("shared.txt", b"high")])),
        archive(rejected, Vec::new()),
        archive(
            low,
            pack_zip(
                low,
                &[("shared.txt", b"low"), ("fallback.txt", b"fallback")],
            ),
        ),
    ]));
    assert_eq!(
        stack.rejections(),
        [PackRejection {
            stack_index: 1,
            reason: AdmissionError::InvalidZipFooter,
        }]
    );
    let view = LayeredPackView::new(stack);
    assert_eq!(view.read("shared.txt").as_deref(), Some(b"high".as_slice()));
    assert_eq!(
        view.read("fallback.txt").as_deref(),
        Some(b"fallback".as_slice())
    );
}
