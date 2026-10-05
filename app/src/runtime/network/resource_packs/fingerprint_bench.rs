use super::*;
use std::io::Cursor;

/// Admits a stored archive with one inert payload for cache-key fixtures.
fn fixture_stack(payload: &[u8]) -> Arc<resource_pack::ValidatedPackStack> {
    use std::io::Write;
    let id = "00000000-0000-0000-0000-0000000000f1";
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file("manifest.json", options).unwrap();
    archive.write_all(manifest.as_bytes()).unwrap();
    archive.start_file("unused.bin", options).unwrap();
    archive.write_all(payload).unwrap();
    let stack =
        resource_pack::validate_handoff(protocol::ResourcePackHandoff::from_archives(vec![
            protocol::ResourcePackArchive::unencrypted(
                id.parse().unwrap(),
                "1.0.0".into(),
                String::new(),
                archive.finish().unwrap().into_inner(),
            ),
        ]));
    assert_eq!(stack.packs().len(), 1);
    stack
}

/// Measures warm compile-cache lookup costs on a stored 32 MiB archive.
#[test]
fn shared_stack_fingerprint_timing() {
    use sha2::{Digest, Sha256};
    use std::{hint::black_box, time::Instant};
    if std::env::var_os("CINNABAR_JOIN_CACHE_BENCH").is_none() {
        eprintln!("missing fixture CINNABAR_JOIN_CACHE_BENCH; skipping pack cache timing");
        return;
    }
    let stack = fixture_stack(&vec![17; 32 * 1024 * 1024]);
    let view = LayeredPackView::new(Arc::clone(&stack));
    let blocks = protocol::CustomBlocks::default();
    let mut repeated_hash_samples = Vec::new();
    let mut samples = Vec::new();
    for _ in 0..21 {
        let started = Instant::now();
        for pack in stack.packs() {
            black_box(Sha256::digest(&*pack.archive_bytes()));
        }
        repeated_hash_samples.push(started.elapsed().as_secs_f64() * 1000.0);
        let started = Instant::now();
        let fingerprint = stack_fingerprint(&stack);
        black_box(cached_block_overlay(
            &fingerprint,
            &view,
            &blocks,
            false,
            || None,
        ));
        black_box(super::super::entity_pack::compile_session_entities(
            &fingerprint,
            &view,
        ));
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    repeated_hash_samples.remove(0);
    repeated_hash_samples.sort_by(f64::total_cmp);
    samples.remove(0);
    samples.sort_by(f64::total_cmp);
    println!(
        "pack_cache_fingerprint bytes={} repeated_sha256_p50_ms={:.3} cached_lookup_p50_ms={:.3} cached_lookup_p95_ms={:.3}",
        stack.packs()[0].archive_bytes().len(),
        repeated_hash_samples[10],
        samples[10],
        samples[19]
    );
}

#[test]
fn shared_fingerprint_keeps_content_in_the_cache_identity() {
    let first = stack_fingerprint(&fixture_stack(b"first archive"));
    let second = stack_fingerprint(&fixture_stack(b"second archive"));
    assert_eq!(
        (&first[0].0, &first[0].1, &first[0].2),
        (&second[0].0, &second[0].1, &second[0].2)
    );
    assert_ne!(first[0].3, second[0].3);
}
