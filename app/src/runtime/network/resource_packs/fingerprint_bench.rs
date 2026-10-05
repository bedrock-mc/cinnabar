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
#[ignore = "offline cache timing fixture"]
fn shared_stack_fingerprint_timing() {
    use std::{hint::black_box, time::Instant};
    let stack = fixture_stack(&vec![17; 32 * 1024 * 1024]);
    let view = LayeredPackView::new(Arc::clone(&stack));
    let blocks = protocol::CustomBlocks::default();
    let mut samples = Vec::new();
    for _ in 0..21 {
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
    samples.remove(0);
    samples.sort_by(f64::total_cmp);
    println!(
        "pack_cache_fingerprint bytes={} median_ms={:.3} p95_ms={:.3}",
        stack.packs()[0].archive_bytes().len(),
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

/// Admits each archive named in `CINNABAR_JOIN_PACKS` (colon-separated, top of the stack first).
fn join_stack(paths: &std::ffi::OsStr) -> Arc<resource_pack::ValidatedPackStack> {
    let archives = std::env::split_paths(paths)
        .map(|path| {
            let bytes = std::fs::read(&path).unwrap();
            let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
            let mut manifest = String::new();
            std::io::Read::read_to_string(
                &mut zip.by_name("manifest.json").unwrap(),
                &mut manifest,
            )
            .unwrap();
            let manifest: serde_json::Value =
                serde_json::from_str(manifest.trim_start_matches('\u{feff}')).unwrap();
            let header = &manifest["header"];
            let version = header["version"]
                .as_array()
                .unwrap()
                .iter()
                .map(|part| part.as_u64().unwrap().to_string())
                .collect::<Vec<_>>()
                .join(".");
            protocol::ResourcePackArchive::unencrypted(
                header["uuid"].as_str().unwrap().parse().unwrap(),
                version,
                String::new(),
                bytes,
            )
        })
        .collect();
    let stack =
        resource_pack::validate_handoff(protocol::ResourcePackHandoff::from_archives(archives));
    assert!(stack.rejections().is_empty(), "{:?}", stack.rejections());
    stack
}

/// Icon keys a server registry would send: every item texture the stack names.
fn join_icon_keys(view: &LayeredPackView) -> Vec<(Arc<str>, Arc<str>)> {
    let root = view
        .read("textures/item_texture.json")
        .and_then(|bytes| parse_pack_json(&bytes));
    root.as_ref()
        .and_then(|root| root.get("texture_data")?.as_object().cloned())
        .into_iter()
        .flatten()
        .map(|(key, _)| (Arc::from(format!("bench:{key}")), Arc::from(key)))
        .collect()
}

/// Times a join's preparation, then a rejoin, on a local server stack. `CINNABAR_JOIN_SERIAL`
/// compiles on one thread; `CINNABAR_JOIN_CACHE` names a disk cache that a second run reads.
#[test]
#[ignore = "offline join timing; CINNABAR_JOIN_PACKS names cached unencrypted archives"]
fn join_preparation_timing() {
    use std::time::Instant;
    let paths = std::env::var_os("CINNABAR_JOIN_PACKS").expect("pack archives");
    if let Some(compiled) = std::env::var_os("CINNABAR_JOIN_CARRIERS") {
        let path = std::path::Path::new(&compiled).join(
            std::path::Path::new(crate::asset_startup::DEFAULT_ASSET_PATH)
                .file_name()
                .unwrap(),
        );
        let loaded =
            crate::asset_startup::load_runtime_assets(crate::asset_startup::AssetSelection {
                path,
                source: crate::asset_startup::AssetPathSource::CommandLine,
            })
            .unwrap();
        let artwork =
            crate::asset_startup::require_actor_artwork(&loaded.selected_path, &loaded.entities)
                .unwrap();
        super::super::set_base_actor_artwork(artwork, Arc::clone(loaded.entities.runtime()));
    }
    if let Some(dir) = std::env::var_os("CINNABAR_JOIN_CACHE") {
        set_compile_cache_dir(dir.into());
    }
    let stack = join_stack(&paths);
    let view = LayeredPackView::new(Arc::clone(&stack));
    let inputs = Arc::new(super::super::pack_reload::PackInputs {
        icons: join_icon_keys(&view),
        ..Default::default()
    });
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(if std::env::var_os("CINNABAR_JOIN_SERIAL").is_some() {
            1
        } else {
            0
        })
        .build()
        .unwrap();
    for round in ["join", "rejoin"] {
        let started = Instant::now();
        let application =
            pool.install(|| prepare_validated_application(Arc::clone(&stack), Arc::clone(&inputs)));
        println!(
            "JOIN_PREPARE {round} ms={:.1} icons={} glyphs={} entities={} ui={} sounds={}",
            started.elapsed().as_secs_f64() * 1e3,
            application.item_icons.is_some(),
            application.glyph_sheets.is_some(),
            application.entities.is_some(),
            application.server_ui.is_some(),
            application.server_sounds.is_some(),
        );
    }
}
