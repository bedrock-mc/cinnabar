use std::sync::Arc;

/// Keeps custom biome names intact while qualifying known retail names.
pub(super) fn canonical_biome_name(name: &str) -> Arc<str> {
    if name.contains(':') {
        return Arc::from(name);
    }
    const RETAIL_BIOMES: &str = include_str!("../../data/retail_biomes_1_26_50.txt");
    let known_retail = RETAIL_BIOMES
        .lines()
        .any(|identifier| identifier.strip_prefix("minecraft:") == Some(name));
    if known_retail {
        Arc::from(format!("minecraft:{name}"))
    } else {
        Arc::from(name)
    }
}
