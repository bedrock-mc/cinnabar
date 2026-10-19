use std::io::{Cursor, Write};

use uuid::Uuid;
use zip::{ZipWriter, write::SimpleFileOptions};

use super::*;
use crate::{crypto::ContentKey, parser::validate_archive_parts};

const PACK_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_5a3e);
const PACK_KEY: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";
const FILE_KEY: &str = "abcdefghijklmnopqrstuvwxyz012345";

/// Creates an in-memory archive from the requested file entries.
fn zip_files(files: &[(&str, &[u8])]) -> Vec<u8> {
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{PACK_ID}","version":[1,0,0]}},"modules":[{{"type":"resources"}}],"subpacks":[{{"folder_name":"high","name":"High","memory_tier":1}}]}}"#
    );
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in
        std::iter::once(("manifest.json", manifest.as_bytes())).chain(files.iter().copied())
    {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// Validates an archive fixture with the chosen subpack and decryption key.
fn admit(archive: Vec<u8>, subpack: &str, key: Option<ContentKey>) -> ValidatedPack {
    validate_archive_parts(PACK_ID, "1.0.0", subpack, archive, key, None)
        .unwrap()
        .0
}

/// Creates a validated stack with the chosen layers and rejection metadata.
fn stack(packs: Vec<ValidatedPack>, rejections: &[PackRejection]) -> ValidatedPackStack {
    ValidatedPackStack {
        packs: packs.into_boxed_slice(),
        rejections: rejections.into(),
        fingerprint: Default::default(),
    }
}

/// An archive whose language file is encrypted under [`FILE_KEY`], listed by an encrypted index.
fn encrypted_archive() -> Vec<u8> {
    let index = format!(r#"{{"content":[{{"path":"texts/en_US.lang","key":"{FILE_KEY}"}}]}}"#);
    let mut contents = vec![0u8; 256];
    let mut body = index.into_bytes();
    crate::crypto::tests::encrypt(PACK_KEY, &mut body);
    contents.extend_from_slice(&body);
    let mut lang = b"a=secret".to_vec();
    crate::crypto::tests::encrypt(FILE_KEY.as_bytes(), &mut lang);
    zip_files(&[("contents.json", &contents), ("texts/en_US.lang", &lang)])
}

// Separately received copies of one archive read identically, so either may stand for the other.
#[test]
fn separately_admitted_copies_of_one_archive_have_the_same_contents() {
    let files: &[(&str, &[u8])] = &[("a.txt", b"root"), ("subpacks/high/a.txt", b"high")];
    let first = stack(vec![admit(zip_files(files), "high", None)], &[]);
    let second = stack(vec![admit(zip_files(files), "high", None)], &[]);
    assert!(first.same_contents(&second));
    let encrypted = || {
        stack(
            vec![admit(encrypted_archive(), "", ContentKey::new(PACK_KEY))],
            &[],
        )
    };
    assert!(encrypted().same_contents(&encrypted()));
}

// Any difference a read could observe makes the stacks differ.
#[test]
fn any_observable_difference_makes_stacks_differ() {
    let base: &[(&str, &[u8])] = &[("a.txt", b"root"), ("subpacks/high/a.txt", b"high")];
    let reference = stack(vec![admit(zip_files(base), "high", None)], &[]);
    let edited = stack(
        vec![admit(
            zip_files(&[("a.txt", b"ROOT"), ("subpacks/high/a.txt", b"high")]),
            "high",
            None,
        )],
        &[],
    );
    let other_subpack = stack(vec![admit(zip_files(base), "", None)], &[]);
    let rejected = stack(
        vec![admit(zip_files(base), "high", None)],
        &[PackRejection {
            stack_index: 1,
            reason: AdmissionError::InvalidZipFooter,
        }],
    );
    let doubled = stack(
        vec![
            admit(zip_files(base), "high", None),
            admit(zip_files(base), "high", None),
        ],
        &[],
    );
    for other in [&edited, &other_subpack, &rejected, &doubled] {
        assert!(!reference.same_contents(other));
        assert!(!other.same_contents(&reference));
    }
}

// The same archive bytes decrypted with a different per-file key read differently.
#[test]
fn a_different_file_key_makes_the_same_bytes_differ() {
    let reference = admit(encrypted_archive(), "", ContentKey::new(PACK_KEY));
    let mut rekeyed = admit(encrypted_archive(), "", ContentKey::new(PACK_KEY));
    rekeyed.keys = Arc::from([ContentKey::new(b"vutsrqponmlkjihgfedcba9876543210").unwrap()]);
    assert_ne!(
        reference.read_file("texts/en_US.lang").unwrap(),
        rekeyed.read_file("texts/en_US.lang").unwrap()
    );
    assert!(!stack(vec![reference], &[]).same_contents(&stack(vec![rekeyed], &[])));
}

#[test]
#[cfg(feature = "handoff")]
fn admission_hashes_each_archive_once_and_composition_reuses_the_digests() {
    use sha2::{Digest, Sha256};
    let layers = [
        vec![("a.txt".into(), b"first".to_vec())],
        vec![("b.txt".into(), b"second".to_vec())],
    ];
    let view = crate::test_support::layered_pack_view(&[&layers[0], &layers[1]]);
    let admitted = view.stack();
    for pack in admitted.packs() {
        assert_eq!(pack.content_hash.computations(), 1);
    }
    let expected: crate::StackFingerprint = admitted
        .packs()
        .iter()
        .map(|pack| {
            (
                pack.pack_id().to_string(),
                pack.version().to_owned(),
                pack.sub_pack_name().to_owned(),
                Sha256::digest(&*pack.archive_bytes()).into(),
            )
        })
        .collect();
    let empty = crate::validate_handoff(protocol::ResourcePackHandoff::default());
    for _ in 0..3 {
        let composed = ValidatedPackStack::compose(&empty, admitted).unwrap();
        assert!(admitted.same_contents(&composed));
        assert_eq!(composed.fingerprint(), &expected);
    }
    assert_eq!(admitted.fingerprint(), &expected);
    for pack in admitted.packs() {
        assert_eq!(pack.content_hash.computations(), 1);
    }
}
