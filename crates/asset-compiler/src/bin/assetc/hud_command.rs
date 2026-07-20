use serde::Serialize;

#[derive(Serialize)]
pub(super) struct HudAssetsReport {
    pub(super) schema: u32,
    pub(super) canonical_pack_path: Box<str>,
    pub(super) source_manifest_sha256: Box<str>,
    pub(super) carrier_sha256: Box<str>,
    pub(super) counts: HudAssetCounts,
}

#[derive(Serialize)]
pub(super) struct HudAssetCounts {
    pub(super) textures: usize,
    pub(super) source_bytes: usize,
    pub(super) decoded_bytes: usize,
}
