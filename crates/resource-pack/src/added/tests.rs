use std::io::{Cursor, Write};

use uuid::Uuid;
use zip::{ZipWriter, write::SimpleFileOptions};

use super::*;
use crate::{ResourcePackHandoff, validate_handoff};

/// A one-file resource pack under `id`.
fn pack_zip(id: u128) -> Vec<u8> {
    let id = Uuid::from_u128(id);
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"name":"t","description":"t","uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources","uuid":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","version":[1,0,0]}}]}}"#
    );
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in [("manifest.json", manifest.as_bytes()), ("a.txt", b"a")] {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// A server stack of one valid pack and one rejected entry.
fn server(id: u128) -> ValidatedPackStack {
    let archives = vec![
        bedrock_packs::ResourcePackArchive::unencrypted(
            Uuid::from_u128(id),
            "1.0.0".into(),
            String::new(),
            pack_zip(id),
        ),
        bedrock_packs::ResourcePackArchive::unencrypted(
            Uuid::from_u128(99),
            "1.0.0".into(),
            String::new(),
            b"not a zip".to_vec(),
        ),
    ];
    let stack = validate_handoff(ResourcePackHandoff::from_archives(archives));
    std::sync::Arc::try_unwrap(stack).unwrap()
}

fn added(id: u128, above_server: bool) -> AddedPack {
    AddedPack {
        archive: pack_zip(id),
        above_server,
    }
}

#[test]
fn added_packs_layer_around_the_server_stack_and_keep_its_rejections() {
    let server = server(1);
    let packs = vec![added(2, true), added(3, false), added(4, false)];
    let (stack, refused) = compose_added_packs(&server, packs, u64::MAX);
    assert!(refused.is_empty());
    let order: Vec<_> = stack.packs().iter().map(|pack| pack.pack_id()).collect();
    let ids = [4, 3, 1, 2].map(Uuid::from_u128);
    assert_eq!(order, ids, "lowest precedence first: below, server, above");
    assert_eq!(stack.rejections().len(), server.rejections().len());
}

#[test]
fn bad_or_repeated_packs_are_left_out_without_touching_the_server_stack() {
    let server = server(1);
    let packs = vec![
        added(1, true),
        AddedPack {
            archive: b"garbage".to_vec(),
            above_server: false,
        },
        added(5, true),
        added(5, false),
    ];
    let (stack, refused) = compose_added_packs(&server, packs, u64::MAX);
    assert_eq!(refused.len(), 3);
    let order: Vec<_> = stack.packs().iter().map(|pack| pack.pack_id()).collect();
    assert_eq!(order, [1, 5].map(Uuid::from_u128));
}
