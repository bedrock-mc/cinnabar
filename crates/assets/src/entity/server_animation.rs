use super::{EntityAssetKind, RuntimeEntityAssets};

impl RuntimeEntityAssets {
    /// Resolves an entity alias before the pack's globally named server-selected clips.
    #[must_use]
    pub fn server_animation_clip(&self, binding: usize, identifier: &str) -> Option<u32> {
        let geometry = self.rig_geometries().get(binding)?;
        let first = geometry.first_animation as usize;
        let end = first.checked_add(geometry.animation_count as usize)?;
        if let Some(alias) = self
            .rig_animations()
            .get(first..end)?
            .iter()
            .find(|binding| {
                self.molang_symbols()
                    .get(binding.name as usize)
                    .is_some_and(|name| name.identifier.as_ref() == identifier)
            })
        {
            return Some(alias.clip);
        }
        self.symbol_candidates(EntityAssetKind::Animation, identifier)
            .iter()
            .find_map(|symbol| {
                let index = self
                    .symbols()
                    .iter()
                    .position(|candidate| std::ptr::eq(candidate, symbol))?;
                self.clip_for_geometry(index as u32, geometry.geometry)
            })
    }
}
