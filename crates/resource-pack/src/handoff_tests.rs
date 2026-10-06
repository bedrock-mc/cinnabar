use std::io::{Cursor, Write};

use protocol::{ResourcePackArchive, ResourcePackHandoff};
use uuid::Uuid;
use zip::{ZipWriter, write::SimpleFileOptions};

use super::{AdmissionError, LayeredPackView, MAX_PACKS, PackRejection, validate_handoff};

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
fn required_stack_with_undeclared_subpack_labels_keeps_root_layers() {
    let upper = Uuid::from_u128(2);
    let archives = [
        (PACK_ID, "Common assets", b"lower".as_slice()),
        (upper, "Server UI", b"upper".as_slice()),
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

#[test]
fn stack_bounds_drop_only_the_packs_beyond_them() {
    let archives = (0..=MAX_PACKS as u128)
        .map(|index| {
            let id = Uuid::from_u128(index + 10);
            archive(id, pack_zip(id, &[]))
        })
        .collect();
    let stack = validate_handoff(handoff(archives));
    assert_eq!(stack.packs().len(), MAX_PACKS);
    assert_eq!(
        stack.rejections(),
        [PackRejection {
            stack_index: MAX_PACKS,
            reason: AdmissionError::TooManyPacks
        }]
    );
}

// The last stack entry wins an overlapping path; merges see it last.
#[test]
fn last_stack_entry_has_the_highest_precedence() {
    let (top, bottom) = (Uuid::from_u128(3), Uuid::from_u128(4));
    let stack = validate_handoff(handoff(vec![
        archive(
            bottom,
            pack_zip(
                bottom,
                &[("texts/en_US.lang", b"k=bottom"), ("only.txt", b"x")],
            ),
        ),
        archive(top, pack_zip(top, &[("texts/en_US.lang", b"k=top")])),
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
