// Immutable carrier tables share one binding so terrain fits eight vertex storage slots.
// Offsets name words after this header; model length excludes both animation tables.
struct ChunkAssetTables {
    animation_count: u32,
    frame_offset: u32,
    model_offset: u32,
    model_word_count: u32,
    words: array<u32>,
}

@group(0) @binding(9) var<storage, read> chunk_asset_tables: ChunkAssetTables;

fn chunk_animation(index: u32) -> AnimationGpu {
    let record_words = min(chunk_asset_tables.frame_offset, arrayLength(&chunk_asset_tables.words));
    if (index >= chunk_asset_tables.animation_count || index >= record_words / 4u) {
        return AnimationGpu(0u, 1u, 1u, 0u);
    }
    let base = index * 4u;
    return AnimationGpu(chunk_asset_tables.words[base], chunk_asset_tables.words[base + 1u],
        chunk_asset_tables.words[base + 2u], chunk_asset_tables.words[base + 3u]);
}

fn chunk_animation_frame(index: u32) -> u32 {
    let start = chunk_asset_tables.frame_offset;
    let end = min(chunk_asset_tables.model_offset, arrayLength(&chunk_asset_tables.words));
    if (start >= end || index >= end - start) { return 0u; }
    return chunk_asset_tables.words[start + index];
}

fn model_template_word_count() -> u32 {
    let total = arrayLength(&chunk_asset_tables.words);
    if (chunk_asset_tables.model_offset > total) { return 0u; }
    return min(chunk_asset_tables.model_word_count, total - chunk_asset_tables.model_offset);
}

fn model_template_word(index: u32) -> u32 {
    if (index >= model_template_word_count()) { return 0u; }
    return chunk_asset_tables.words[chunk_asset_tables.model_offset + index];
}
