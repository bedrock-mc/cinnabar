use assets::{HudCatalogError, HudTexture, HudTextureRole, RuntimeHudCatalog, encode_hud_catalog};
use sha2::{Digest, Sha256};

#[test]
fn exact_heart_catalog_round_trips_with_provenance() {
    let manifest = [0x5a; 32];
    let textures = textures();
    let bytes = encode_hud_catalog(manifest, &textures).unwrap();
    let runtime = RuntimeHudCatalog::decode(&bytes, manifest).unwrap();

    assert_eq!(runtime.identity().source_manifest_sha256, manifest);
    assert_eq!(runtime.textures(), textures);
    assert_eq!(
        runtime.texture(HudTextureRole::HeartHalf).source_path.as_ref(),
        "textures/ui/heart_half.png"
    );
}

#[test]
fn carrier_rejects_wrong_provenance_and_modified_payload() {
    let manifest = [0x5a; 32];
    let mut bytes = encode_hud_catalog(manifest, &textures()).unwrap().into_vec();
    assert!(matches!(
        RuntimeHudCatalog::decode(&bytes, [0x6b; 32]),
        Err(HudCatalogError::SourceManifestMismatch)
    ));
    bytes[48] ^= 1;
    assert!(matches!(
        RuntimeHudCatalog::decode(&bytes, manifest),
        Err(HudCatalogError::CarrierHashMismatch)
    ));
}

#[test]
fn encoder_rejects_stand_in_or_wrong_geometry() {
    let mut textures = textures();
    textures[0].width = 8;
    assert!(matches!(
        encode_hud_catalog([0x5a; 32], &textures),
        Err(HudCatalogError::InvalidCatalog { .. })
    ));
}

fn textures() -> Vec<HudTexture> {
    HudTextureRole::ALL
        .into_iter()
        .enumerate()
        .map(|(index, role)| {
            let rgba8 = vec![u8::try_from(index + 1).unwrap(); 9 * 9 * 4].into_boxed_slice();
            HudTexture {
                role,
                source_path: role.source_path().into(),
                source_bytes: 100 + u32::try_from(index).unwrap(),
                source_sha256: [u8::try_from(index + 1).unwrap(); 32],
                pixels_sha256: Sha256::digest(&rgba8).into(),
                width: 9,
                height: 9,
                rgba8,
            }
        })
        .collect()
}
