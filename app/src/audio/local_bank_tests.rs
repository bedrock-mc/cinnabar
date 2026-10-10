use assets::RuntimeAudioCatalog;
use client_presentation::audio::{SoundBank, sound_bank_path};
use std::{fs, path::Path, sync::Arc};
#[test]
fn local_pinned_bank_has_audible_grass_and_dirt_break_alternatives_when_present() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let world = root.join(crate::asset_startup::DEFAULT_ASSET_PATH);
    let catalog_path = crate::asset_startup::audio_asset_path(&world);
    let bank_path = sound_bank_path(&world);
    if !catalog_path.is_file() || !bank_path.is_file() {
        eprintln!(
            "skipping local_pinned_bank_has_audible_grass_and_dirt_break_alternatives_when_present: fixture unavailable; requires installed local carriers (make assets)"
        );
        return; // Optional developer carriers; the synthetic regression always runs.
    }
    let catalog = RuntimeAudioCatalog::decode(&fs::read(catalog_path).unwrap()).unwrap();
    let mut bank = SoundBank::open(&bank_path, Some(Arc::new(catalog)))
        .unwrap()
        .expect("local pinned bank");
    for identifier in ["minecraft:grass_block", "minecraft:dirt"] {
        let material = bank
            .tables()
            .material_of(identifier)
            .expect("block material");
        let route = bank.tables().block(material, "break").expect("break route");
        let alternatives = bank
            .definition(&route.sound)
            .expect("pack definition")
            .alternatives
            .clone();
        assert!(!alternatives.is_empty());
        for alternative in &alternatives {
            let pcm = bank
                .pcm(&alternative.name, false)
                .expect("decoded break sound");
            assert!(pcm.frames() > 0);
            assert!(pcm.samples.iter().any(|sample| *sample != 0));
        }
    }
}
