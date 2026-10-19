use std::io::{Cursor, Write};

use sha2::Digest;
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

#[test]
fn repeated_encrypted_reads_share_one_decode_across_pack_clones() {
    let pack = admit(encrypted_archive(), "", ContentKey::new(PACK_KEY));
    let other = pack.clone();
    for reader in [&pack, &other, &pack, &other] {
        assert_eq!(
            reader
                .read_file("texts/en_US.lang")
                .unwrap()
                .unwrap()
                .as_ref(),
            b"a=secret"
        );
        assert_eq!(
            reader
                .file_fingerprint_with_limit("texts/en_US.lang", MAX_FILE_BYTES)
                .unwrap()
                .unwrap(),
            <[u8; 32]>::from(sha2::Sha256::digest(b"a=secret"))
        );
    }
    assert_eq!(
        pack.entry("texts/en_US.lang")
            .unwrap()
            .cached
            .reads
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
}

/// Gives a fixture fresh entry identities within a chosen plaintext budget.
fn with_budget(mut pack: ValidatedPack, budget: usize) -> ValidatedPack {
    pack.reads = Arc::new(ReadCache::new(budget));
    for entry in pack.files.values_mut() {
        entry.cached = pack.reads.entry();
    }
    pack
}

#[test]
fn concurrent_subscribers_share_encrypted_reads_and_fingerprints() {
    let pack = admit(encrypted_archive(), "", ContentKey::new(PACK_KEY));
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let pack = pack.clone();
            scope.spawn(move || {
                for _ in 0..8 {
                    assert_eq!(
                        pack.read_file("texts/en_US.lang")
                            .unwrap()
                            .unwrap()
                            .as_ref(),
                        b"a=secret"
                    );
                    assert_eq!(
                        pack.file_fingerprint_with_limit("TEXTS/EN_US.LANG", MAX_FILE_BYTES)
                            .unwrap()
                            .unwrap(),
                        <[u8; 32]>::from(sha2::Sha256::digest(b"a=secret"))
                    );
                }
            });
        }
    });
    assert_eq!(
        pack.entry("texts/en_US.lang")
            .unwrap()
            .cached
            .reads
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
}

#[test]
fn eviction_preserves_bytes_and_keeps_fingerprints_without_reinflation() {
    let pack = with_budget(
        admit(
            zip_files(&[("a.txt", b"aaaa"), ("b.txt", b"bbbb"), ("c.txt", b"cccc")]),
            "",
            None,
        ),
        8,
    );
    for (path, expected) in [("a.txt", b"aaaa"), ("b.txt", b"bbbb"), ("c.txt", b"cccc")] {
        assert_eq!(pack.read_file(path).unwrap().unwrap().as_ref(), expected);
    }
    assert_eq!(
        pack.file_fingerprint_with_limit("a.txt", MAX_FILE_BYTES)
            .unwrap()
            .unwrap(),
        <[u8; 32]>::from(sha2::Sha256::digest(b"aaaa"))
    );
    let reads = &pack.entry("a.txt").unwrap().cached.reads;
    assert_eq!(reads.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert_eq!(pack.read_file("a.txt").unwrap().unwrap().as_ref(), b"aaaa");
    assert_eq!(reads.load(std::sync::atomic::Ordering::Relaxed), 2);
    assert_eq!(pack.reads.retained_bytes(), 8);
}

#[test]
fn cached_and_uncached_encrypted_fingerprints_and_limits_match() {
    let cached = admit(encrypted_archive(), "", ContentKey::new(PACK_KEY));
    let uncached = with_budget(admit(encrypted_archive(), "", ContentKey::new(PACK_KEY)), 0);
    for path in ["texts/en_US.lang", "TEXTS/EN_US.LANG", "missing.json"] {
        for limit in [MAX_FILE_BYTES, 0, 7, 8, MAX_FILE_BYTES] {
            let baseline = uncached
                .entry(path)
                .map(|entry| {
                    uncached
                        .read_uncached(path, entry, limit)
                        .map(|bytes| <[u8; 32]>::from(sha2::Sha256::digest(&bytes)))
                })
                .transpose();
            assert_eq!(cached.file_fingerprint_with_limit(path, limit), baseline);
            assert_eq!(
                cached.file_fingerprint_with_limit(path, limit),
                uncached.file_fingerprint_with_limit(path, limit)
            );
            assert_eq!(
                cached.read_file_with_limit(path, limit),
                uncached.read_file_with_limit(path, limit)
            );
        }
    }
    assert_eq!(uncached.reads.retained_bytes(), 0);
}

#[test]
fn files_larger_than_the_budget_keep_only_their_hashes() {
    let pack = with_budget(admit(encrypted_archive(), "", ContentKey::new(PACK_KEY)), 2);
    for _ in 0..4 {
        assert!(
            pack.file_fingerprint_with_limit("texts/en_US.lang", MAX_FILE_BYTES)
                .unwrap()
                .is_some()
        );
    }
    assert_eq!(
        pack.entry("texts/en_US.lang")
            .unwrap()
            .cached
            .reads
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    assert_eq!(
        pack.read_file("texts/en_US.lang")
            .unwrap()
            .unwrap()
            .as_ref(),
        b"a=secret"
    );
    assert_eq!(
        pack.entry("texts/en_US.lang")
            .unwrap()
            .cached
            .reads
            .load(std::sync::atomic::Ordering::Relaxed),
        2
    );
    assert_eq!(pack.reads.retained_bytes(), 0);
}

#[test]
fn admission_preserves_the_archive_allocation() {
    let archive = zip_files(&[("test.txt", b"fixture")]);
    let allocation = archive.as_ptr();
    let pack = admit(archive, "", None);
    assert_eq!(pack.archive_bytes().as_ptr(), allocation);
}

#[test]
#[cfg(feature = "handoff")]
fn stack_layers_and_composed_clones_share_the_admission_budget() {
    let files = [("a.txt".into(), b"fixture".to_vec())];
    let view = crate::test_support::layered_pack_view(&[&files, &files]);
    let layers = view.stack().packs();
    assert!(Arc::ptr_eq(&layers[0].reads, &layers[1].reads));
    let empty = stack(vec![], &[]);
    let composed = ValidatedPackStack::compose(view.stack(), &empty).unwrap();
    assert!(Arc::ptr_eq(&layers[0].reads, &composed.packs()[0].reads));
    for layer in composed.packs() {
        assert_eq!(
            layer.read_file("a.txt").unwrap().unwrap().as_ref(),
            b"fixture"
        );
    }
    for layer in layers {
        assert_eq!(
            layer.read_file("a.txt").unwrap().unwrap().as_ref(),
            b"fixture"
        );
        assert_eq!(
            layer
                .entry("a.txt")
                .unwrap()
                .cached
                .reads
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
    }
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
fn keyed_jsonc_reads_keep_the_previous_plaintext_selection() {
    for body in [
        br#"{"a":1,"a":2,"number":1.0000000000000002}"#.as_slice(),
        b"\xef\xbb\xbf{/* comment */\"text\":\"\xff\",\"a\":[1,],}",
        b"{\"ignored\":1e309}",
        b"{} {}",
        b"{,}",
    ] {
        let index = format!(r#"{{"content":[{{"path":"test.json","key":"{FILE_KEY}"}}]}}"#);
        let mut ciphertext = body.to_vec();
        crate::crypto::tests::encrypt(FILE_KEY.as_bytes(), &mut ciphertext);
        for raw in [body, ciphertext.as_slice()] {
            let mut decrypted = raw.to_vec();
            ContentKey::new(FILE_KEY.as_bytes())
                .unwrap()
                .decrypt(&mut decrypted);
            let expected = if pack_json::normalize_jsonc(&decrypted).is_none()
                && pack_json::normalize_jsonc(raw).is_some()
            {
                raw
            } else {
                &decrypted
            };
            let pack = admit(
                zip_files(&[("contents.json", index.as_bytes()), ("test.json", raw)]),
                "",
                ContentKey::new(PACK_KEY),
            );
            assert_eq!(
                pack.read_file("test.json").unwrap().unwrap().as_ref(),
                expected
            );
        }
    }
}
