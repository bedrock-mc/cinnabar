use super::PhysicsCollisionRegistries;

impl PhysicsCollisionRegistries {
    /// Keeps carrier identities stable while constructing the session's admitted wire palette.
    pub(super) fn admit_server_definitions(
        &self,
        definitions: &protocol::CustomBlocks,
    ) -> assets::SequentialIdRemap {
        let Some(server_defined) = assets::server_defined_blocks_for_registry(self.breg_sha256)
        else {
            return assets::SequentialIdRemap::default();
        };
        let mut admitted = vec![true; self.sequential_count];
        let mut omitted = false;
        for definition in server_defined {
            if definitions
                .vanilla_blocks
                .iter()
                .any(|name| name.as_ref() == definition.name)
            {
                continue;
            }
            let start = definition.first_internal_id as usize;
            let end = start + definition.state_count as usize;
            admitted[start..end].fill(false);
            omitted = true;
        }
        if !omitted {
            return assets::SequentialIdRemap::default();
        }
        let palette = (0..self.sequential_count as u32)
            .filter(|&internal| admitted.get(internal as usize).copied().unwrap_or(true))
            .collect();
        assets::SequentialIdRemap::from_palette(palette, self.sequential_count as u32)
    }
}
