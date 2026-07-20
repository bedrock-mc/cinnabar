use assets::BiomeVisualProfile;

pub(super) fn find_biome_profile<'a>(
    profiles: &'a [BiomeVisualProfile],
    identifier: &str,
) -> Option<&'a BiomeVisualProfile> {
    profiles
        .binary_search_by(|profile| profile.biome_identifier.as_ref().cmp(identifier))
        .ok()
        .map(|index| &profiles[index])
}

pub(super) const fn dimension_fallback_biome(dimension: i32) -> Option<&'static str> {
    match dimension {
        0 => Some("minecraft:plains"),
        1 => Some("minecraft:hell"),
        2 => Some("minecraft:the_end"),
        _ => None,
    }
}
