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

#[derive(Serialize)]
pub(super) struct LangAssetsReport {
    pub(super) schema: u32,
    pub(super) canonical_pack_path: Box<str>,
    pub(super) source_manifest_sha256: Box<str>,
    pub(super) carrier_sha256: Box<str>,
    pub(super) counts: LangAssetCounts,
}

#[derive(Serialize)]
pub(super) struct LangAssetCounts {
    pub(super) entries: usize,
    pub(super) duplicate_keys: usize,
    pub(super) skipped_oversized: usize,
    pub(super) source_bytes: usize,
}
