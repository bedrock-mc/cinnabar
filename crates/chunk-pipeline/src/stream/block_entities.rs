use super::*;

impl WorldStream {
    /// The decoded NBT of the block entity at `position` in the current dimension.
    #[must_use]
    pub fn block_entity_compound(&self, position: [i32; 3]) -> Option<Arc<world::NbtCompound>> {
        let key = BlockEntityKey::new(
            self.authority.current_dimension(),
            position[0],
            position[1],
            position[2],
        );
        self.authority.terrain().block_entity(key)?.parsed()
    }

    /// The `CustomName` of the block entity at `position`, if it has one.
    #[must_use]
    pub fn block_entity_custom_name(&self, position: [i32; 3]) -> Option<String> {
        let nbt = self.block_entity_compound(position)?;
        nbt.string("CustomName")
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
    }

    pub(super) fn refresh_block_entity_visual(&mut self, key: BlockEntityKey) {
        let Some(source) = self.authority.terrain().block_entity(key) else {
            self.block_entity_visuals.remove(key);
            return;
        };
        let Some(backing) = self.backing_block_identity(key) else {
            self.block_entity_visuals.remove(key);
            return;
        };
        self.block_entity_visuals.upsert(
            key,
            adjudicate_block_entity_visual(source.as_ref(), backing),
        );
    }
    pub(super) fn refresh_block_entity_visuals_for_sub_chunk(&mut self, key: SubChunkKey) {
        self.block_entity_visuals.remove_sub_chunk(key);
        let entities = self
            .authority
            .terrain()
            .chunk(key.chunk())
            .into_iter()
            .flat_map(|chunk| chunk.block_entities())
            .filter_map(|(entity, _)| (entity.sub_chunk() == key).then_some(entity))
            .collect::<Vec<_>>();
        for entity in entities {
            self.refresh_block_entity_visual(entity);
        }
    }
    pub(super) fn refresh_block_entity_visuals_for_chunk(&mut self, key: ChunkKey) {
        self.block_entity_visuals.remove_chunk(key);
        let entities = self
            .authority
            .terrain()
            .chunk(key)
            .into_iter()
            .flat_map(|chunk| chunk.block_entities())
            .map(|(entity, _)| entity)
            .collect::<Vec<_>>();
        for entity in entities {
            self.refresh_block_entity_visual(entity);
        }
    }
    pub(super) fn backing_block_identity(
        &self,
        key: BlockEntityKey,
    ) -> Option<BackingBlockIdentity> {
        let sub_chunk = self.authority.terrain().sub_chunk(key.sub_chunk())?;
        let runtime_id = sub_chunk.runtime_id(
            0,
            key.x.rem_euclid(16) as u8,
            key.y.rem_euclid(16) as u8,
            key.z.rem_euclid(16) as u8,
        )?;
        Some(BackingBlockIdentity::from_runtime(
            runtime_id,
            self.authority.network_id_mode(),
            self.authority.runtime_assets(),
        ))
    }
}
