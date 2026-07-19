use assets::{
    HUD_SOURCE_MANIFEST_SHA256, HudTexture, HudTextureRole, RuntimeHudCatalog, encode_hud_catalog,
};
use sha2::{Digest, Sha256};

#[test]
fn hud_carrier_round_trips_all_required_survival_roles_with_provenance() {
    let manifest = HUD_SOURCE_MANIFEST_SHA256;
    let textures = HudTextureRole::ALL
        .into_iter()
        .map(|role| fixture_texture(role, role as u8))
        .collect::<Vec<_>>();

    let bytes = encode_hud_catalog(manifest, &textures).unwrap();
    let catalog = RuntimeHudCatalog::decode(&bytes).unwrap();

    assert_eq!(catalog.source_manifest_sha256(), manifest);
    assert_eq!(catalog.textures().len(), HudTextureRole::ALL.len());
    assert!(
        catalog
            .texture(HudTextureRole::SelectedHotbarSlot)
            .rgba8
            .iter()
            .all(|value| *value == HudTextureRole::SelectedHotbarSlot as u8)
    );
}

#[test]
fn hud_carrier_rejects_corruption_missing_roles_and_wrong_dimensions() {
    let manifest = HUD_SOURCE_MANIFEST_SHA256;
    let textures = HudTextureRole::ALL
        .into_iter()
        .map(|role| fixture_texture(role, role as u8))
        .collect::<Vec<_>>();
    let mut bytes = encode_hud_catalog(manifest, &textures).unwrap();
    let payload_index = bytes.len() - 33;
    bytes[payload_index] ^= 0xff;
    assert!(RuntimeHudCatalog::decode(&bytes).is_err());

    assert!(encode_hud_catalog(manifest, &textures[..textures.len() - 1]).is_err());

    let mut wrong_size = textures;
    wrong_size[HudTextureRole::HeartFull as usize].width = 8;
    assert!(encode_hud_catalog(manifest, &wrong_size).is_err());
}

#[test]
fn hud_carrier_rejects_unreviewed_source_identity() {
    let textures = HudTextureRole::ALL
        .into_iter()
        .map(|role| fixture_texture(role, role as u8))
        .collect::<Vec<_>>();
    assert!(encode_hud_catalog([0x42; 32], &textures).is_err());
}

fn fixture_texture(role: HudTextureRole, value: u8) -> HudTexture {
    let [width, height] = role.expected_size();
    let rgba8 = vec![value; width as usize * height as usize * 4].into_boxed_slice();
    HudTexture {
        role,
        source_bytes: rgba8.len() as u32,
        source_sha256: Sha256::digest(&rgba8).into(),
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width,
        height,
        rgba8,
    }
}
