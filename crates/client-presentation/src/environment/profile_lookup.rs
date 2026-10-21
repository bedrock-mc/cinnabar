use assets::BiomeVisualProfile;

/// Finds a biome by identifier in the sorted runtime profile registry.
pub(super) fn find_biome_profile<'a>(
    profiles: &'a [BiomeVisualProfile],
    identifier: &str,
) -> Option<&'a BiomeVisualProfile> {
    profiles
        .binary_search_by(|profile| profile.biome_identifier.as_ref().cmp(identifier))
        .ok()
        .map(|index| &profiles[index])
}
