/// Builds a synthetic world blob whose embedded provenance replaces exactly
/// one pinned identity slot, leaving the other three bound correctly.
fn synthetic_blob_with_world_provenance(mutify: impl FnOnce(&mut BlobProvenance)) -> Box<[u8]> {
    let mut compiled_provenance = *pinned_world_provenance();
    mutify(&mut compiled_provenance);
    let mips = [16_u32, 8, 4, 2, 1]
        .into_iter()
        .map(|size| TextureMip {
            size,
            rgba8: vec![0x11; (size * size * 4) as usize].into_boxed_slice(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    encode_blob(&CompiledAssets {
        visuals: vec![BlockVisual::diagnostic(
            BlockFlags::empty(),
            ::assets::ContributorRole::Primary,
        )]
        .into_boxed_slice(),
        light_properties: vec![::assets::LightProperties::default()].into_boxed_slice(),
        hashed: Box::new([]),
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
        }]
        .into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray { layers: 1, mips })].into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: compiled_provenance,
    })
    .unwrap()
}

#[test]
fn stale_source_manifest_identity_fails_startup_closed_with_rebuild_command() {
    let directory = temporary_directory("stale-world-manifest-provenance");
    let path = directory.join("custom-world.mcbea");
    fs::write(
        &path,
        synthetic_blob_with_world_provenance(|provenance| {
            provenance.source_manifest_sha256 = [0x42; 32];
        }),
    )
    .unwrap();

    let error = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap_err();
    let message = error.to_string();
    assert!(message.contains(&path.display().to_string()), "{message}");
    assert!(message.contains("stale provenance"), "{message}");
    assert!(message.contains("source manifest"), "{message}");
    assert!(
        message.contains(&format!("{:02x}", 0x42)),
        "the foreign identity must be named: {message}"
    );
    assert!(message.contains(COMPILE_COMMAND), "{message}");
    assert!(matches!(
        error,
        bedrock_client::asset_startup::AssetStartupError::WorldAssetsProvenance { .. }
    ));

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn foreign_registry_identity_fails_startup_closed_naming_the_component() {
    type Mutator = fn(&mut BlobProvenance);
    for (component, mutify) in [
        (
            "block registry",
            (|provenance: &mut BlobProvenance| {
                provenance.block_registry_sha256 = [0x43; 32];
            }) as Mutator,
        ),
        (
            "light registry",
            (|provenance: &mut BlobProvenance| {
                provenance.light_registry_sha256 = [0x44; 32];
            }) as Mutator,
        ),
        (
            "biome registry",
            (|provenance: &mut BlobProvenance| {
                provenance.biome_registry_sha256 = [0x45; 32];
            }) as Mutator,
        ),
    ] {
        let directory = temporary_directory("foreign-registry-provenance");
        let path = directory.join("custom-world.mcbea");
        fs::write(&path, synthetic_blob_with_world_provenance(mutify)).unwrap();

        let error = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap_err();
        let message = error.to_string();
        assert!(message.contains(&path.display().to_string()), "{message}");
        assert!(message.contains("stale provenance"), "{message}");
        assert!(message.contains(component), "{message}");
        assert!(message.contains(COMPILE_COMMAND), "{message}");

        fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn matching_pinned_world_identity_reaches_gameplay_startup() {
    let directory = temporary_directory("matching-world-provenance");
    let path = directory.join("custom-world.mcbea");
    fs::write(&path, synthetic_blob()).unwrap();
    fs::write(
        atmosphere_asset_path(&path),
        synthetic_atmosphere_blob(0x71),
    )
    .unwrap();
    fs::write(entity_asset_path(&path), synthetic_entity_blob(0x73)).unwrap();

    let loaded = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap();
    assert_eq!(loaded.kind, LoadedAssetKind::CompiledBlob);
    assert_eq!(loaded.runtime.provenance(), pinned_world_provenance());

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn mismatched_atmosphere_carrier_provenance_fails_closed_with_rebuild_command() {
    let directory = temporary_directory("mismatched-atmosphere-provenance");
    let path = directory.join("custom-world.mcbea");
    fs::write(&path, synthetic_blob()).unwrap();
    let atmosphere_path = atmosphere_asset_path(&path);
    fs::write(
        &atmosphere_path,
        synthetic_atmosphere_blob_with_manifest(0x72, [0x88; 32]),
    )
    .unwrap();

    let error = load_runtime_assets(select_asset_path(Some(&path), None)).unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains(&atmosphere_path.display().to_string()),
        "{message}"
    );
    assert!(message.contains("stale provenance"), "{message}");
    assert!(message.contains(ATMOSPHERE_COMPILE_COMMAND), "{message}");
    assert!(matches!(
        error,
        bedrock_client::asset_startup::AssetStartupError::AtmosphereAssetsProvenance { .. }
    ));

    fs::remove_dir_all(directory).unwrap();
}
