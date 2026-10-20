//! Persistent subscriber results must survive a new cache and dependency recorder.
use super::*;

#[test]
fn fresh_block_and_icon_caches_skip_compilation_and_restore_reads() {
    for subscriber in ["blocks", "icons"] {
        let dir = tempfile::tempdir().unwrap();
        let stack = resource_pack::validate_handoff(protocol::ResourcePackHandoff::default());
        let key: [u8; 32] = cache_key(subscriber).finalize().into();
        let mut compiles = 0;
        let mut reads = None;
        for _ in 0..2 {
            let cache = CompileCache::new(dir.path().into(), 1 << 20);
            let view = resource_pack::LayeredPackView::tracked(stack.clone());
            let output = cache.get_or_compile(&key, &view, || {
                compiles += 1;
                let _ = view.read("textures/missing.png");
                vec![0u8, 3, 255]
            });
            assert_eq!(output, [0, 3, 255]);
            let next = view.dependencies().unwrap().snapshot();
            if let Some(reads) = &reads {
                assert_eq!(reads, &next);
            }
            reads = Some(next);
        }
        assert_eq!(
            compiles, 1,
            "{subscriber} recompiled across cache instances"
        );
    }
}

#[test]
fn compressible_pixel_planes_are_stored_compactly_and_roundtrip_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CompileCache::new(dir.path().into(), 1 << 20);
    let pixels: Vec<u8> = (0..262144).map(|i| (i % 4) as u8).collect();
    cache.store(&[3; 32], &pixels);
    assert_eq!(cache.load(&[3; 32]).unwrap(), pixels);
    assert!(fs::metadata(cache.entry(&[3; 32])).unwrap().len() < pixels.len() as u64 / 10);
}

#[test]
fn old_format_is_rejected_even_with_a_valid_checksum() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CompileCache::new(dir.path().into(), 1 << 20);
    cache.store(&[3; 32], b"payload");
    let path = cache.entry(&[3; 32]);
    let mut bytes = fs::read(&path).unwrap();
    bytes[..8].copy_from_slice(b"CNBRCMP1");
    let end = bytes.len() - HASH;
    let digest = Sha256::digest(&bytes[..end]);
    bytes[end..].copy_from_slice(&digest);
    fs::write(path, bytes).unwrap();
    assert!(cache.load(&[3; 32]).is_none());
}

#[test]
fn damaged_and_undecodable_subscriber_entries_recompile() {
    let dir = tempfile::tempdir().unwrap();
    let stack = resource_pack::validate_handoff(protocol::ResourcePackHandoff::default());
    let cache = CompileCache::new(dir.path().into(), 1 << 20);
    for subscriber in ["blocks", "icons"] {
        let key: [u8; 32] = cache_key(subscriber).finalize().into();
        let mut compiles = 0;
        for damage in 0..4 {
            let view = resource_pack::LayeredPackView::tracked(stack.clone());
            let _: Vec<u8> = cache.get_or_compile(&key, &view, || {
                compiles += 1;
                vec![3u8, 7, 255]
            });
            let path = cache.entry(&key);
            match damage {
                0 => {
                    let mut bytes = fs::read(&path).unwrap();
                    bytes[HEADER] ^= 1;
                    fs::write(path, bytes).unwrap();
                }
                1 => {
                    let mut bytes = fs::read(&path).unwrap();
                    bytes.truncate(bytes.len() / 2);
                    fs::write(path, bytes).unwrap();
                }
                2 => cache.store(&key, b"invalid subscriber output"),
                _ => {}
            }
        }
        assert_eq!(
            compiles, 4,
            "{subscriber} did not recompile invalid entries"
        );
    }
}

#[test]
fn invalid_compressed_lengths_and_streams_are_rejected_with_valid_checksums() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CompileCache::new(dir.path().into(), 1 << 20);
    for damage in 0..3 {
        cache.store(&[5; 32], &[7; 4096]);
        let path = cache.entry(&[5; 32]);
        let mut bytes = fs::read(&path).unwrap();
        match damage {
            0 => bytes[DECODED_LENGTH_OFFSET..HEADER].copy_from_slice(&4095u64.to_le_bytes()),
            1 => bytes[DECODED_LENGTH_OFFSET..HEADER].copy_from_slice(&4097u64.to_le_bytes()),
            _ => bytes[HEADER] = 0xff,
        }
        let end = bytes.len() - HASH;
        let digest = Sha256::digest(&bytes[..end]);
        bytes[end..].copy_from_slice(&digest);
        fs::write(path, bytes).unwrap();
        assert!(cache.load(&[5; 32]).is_none());
    }
}
