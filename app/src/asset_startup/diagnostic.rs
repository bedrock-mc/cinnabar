use super::*;

pub(super) fn diagnostic_assets(
    selection: AssetSelection,
    source: VanillaSource,
    atmosphere: LoadedAtmosphereAssets,
    entities: LoadedEntityAssets,
    fonts: LoadedFontAssets,
) -> LoadedAssets {
    let runtime = Arc::new(RuntimeAssets::diagnostic());
    let metrics = runtime_metrics(&runtime, source, "diagnostic".to_owned());
    let notice = format!(
        "compiled vanilla assets were not found at {}; using the programmatic diagnostic texture\n\
         Fetch and compile the local vanilla pack explicitly (the app never downloads it):\n  {FETCH_COMMAND}\n  {COMPILE_COMMAND}",
        selection.path.display()
    );
    LoadedAssets {
        runtime,
        atmosphere,
        entities,
        fonts,
        metrics,
        selected_path: selection.path,
        kind: LoadedAssetKind::Diagnostic,
        notice: Some(notice),
    }
}

pub(super) fn runtime_metrics(
    runtime: &RuntimeAssets,
    source: VanillaSource,
    blob_sha256: String,
) -> AssetMetrics {
    let pages = runtime.texture_pages();
    AssetMetrics {
        source_tag: source.tag,
        source_sha256: source.sha256,
        blob_sha256,
        texture_layers: pages.iter().map(|page| page.texture.layers).sum(),
        texture_pages: u32::try_from(pages.len()).unwrap_or(u32::MAX),
        texture_bytes_including_mips: pages
            .iter()
            .flat_map(|page| page.texture.mips.iter())
            .map(|mip| mip.rgba8.len() as u64)
            .sum(),
        material_count: u32::try_from(runtime.materials().len()).unwrap_or(u32::MAX),
        model_template_count: u32::try_from(runtime.model_templates().len()).unwrap_or(u32::MAX),
        model_quad_count: u32::try_from(runtime.model_quads().len()).unwrap_or(u32::MAX),
        animation_count: u32::try_from(runtime.animations().len()).unwrap_or(u32::MAX),
        animation_frame_count: u32::try_from(runtime.animation_frames().len()).unwrap_or(u32::MAX),
        missing_mapping_count: runtime.missing_count(),
        diagnostic_quad_count: 0,
        diagnostic_attribution: Default::default(),
    }
}
