//! Immutable registry identities captured before background decoding.

use assets::{NetworkIdMode, ResolvedBiomeTints, RuntimeAssets};
use protocol::DimensionRange;
use std::sync::Arc;
use world::{BiomeIds, BlockIds, DimensionSlots};

/// Session registries that decode workers resolve raw ids against, as the
/// vanilla palettes do: unknown blocks become air and unknown biomes the
/// dimension's fallback biome.
#[derive(Clone)]
pub struct DecodeIds {
    pub assets: Arc<RuntimeAssets>,
    pub custom_blocks: std::ops::Range<u32>,
    pub(crate) custom_identities: Arc<std::collections::HashMap<u32, u32>>,
    pub remap: Arc<assets::SequentialIdRemap>,
    pub(crate) diagnostics: Arc<super::DecodeDiagnostics>,
    pub(crate) session_id: u64,
    pub mode: NetworkIdMode,
    pub air: u32,
    pub biome_tints: Arc<ResolvedBiomeTints>,
    pub default_biome: u32,
}

impl std::fmt::Debug for DecodeIds {
    /// Summarizes registry identity without retaining bulky asset debug output.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DecodeIds")
            .field("mode", &self.mode)
            .field("air", &self.air)
            .field("default_biome", &self.default_biome)
            .finish_non_exhaustive()
    }
}

impl BlockIds for DecodeIds {
    /// Returns the active registry's air value.
    fn air(&self) -> u32 {
        self.air
    }

    /// Maps unknown wire ids to the same fallback used by vanilla decoding.
    fn resolve(&self, network_id: u32) -> u32 {
        let wire_id = network_id;
        let network_id = if self.mode == NetworkIdMode::Sequential {
            self.remap.to_internal(network_id)
        } else {
            network_id
        };
        let known = self.assets.is_known(self.mode, network_id)
            || self.custom_blocks.contains(&network_id)
            || (self.mode == NetworkIdMode::Hashed
                && self.custom_identities.contains_key(&network_id));
        self.diagnostics.observe(wire_id, network_id, self, known);
        if known { network_id } else { self.air }
    }

    fn resolve_persistent(&self, entry: &world::NbtCompound) -> u32 {
        let Some(hash) = persistent_hash(entry) else {
            self.diagnostics.observe_invalid_persistent(self.session_id);
            return self.air;
        };
        let internal_id = |id| match self.mode {
            NetworkIdMode::Sequential => id,
            NetworkIdMode::Hashed => hash,
        };
        let internal = self
            .assets
            .sequential_id_for_hash(hash)
            .map(internal_id)
            .or_else(|| self.custom_identities.get(&hash).copied())
            .or_else(|| {
                self.assets
                    .is_diagnostic()
                    .then(|| assets::pinned_block_sequential_id(hash).map(internal_id))
                    .flatten()
            });
        self.diagnostics
            .observe(hash, internal.unwrap_or(self.air), self, internal.is_some());
        internal.unwrap_or(self.air)
    }
}

fn persistent_hash(entry: &world::NbtCompound) -> Option<u32> {
    use protocol::CustomStateValue;
    use world::NbtValue;

    let name = entry.string("name")?;
    let name = if name.contains(':') {
        std::borrow::Cow::Borrowed(name)
    } else {
        std::borrow::Cow::Owned(format!("minecraft:{name}"))
    };
    let states = entry.compound("states");
    let values: Vec<_> = states
        .into_iter()
        .flat_map(|states| states.iter())
        .map(|(key, value)| {
            let value = match value {
                NbtValue::Byte(0) => CustomStateValue::Bool(false),
                NbtValue::Byte(1) => CustomStateValue::Bool(true),
                NbtValue::Int(value) => CustomStateValue::Int(i64::from(*value)),
                NbtValue::String(value) => CustomStateValue::String(Arc::from(value.as_ref())),
                _ => return None,
            };
            Some((key, value))
        })
        .collect::<Option<_>>()?;
    Some(protocol::block_state_network_hash(
        &name,
        values.iter().map(|(key, value)| (*key, value)),
    ))
}

impl BiomeIds for DecodeIds {
    /// Returns this dimension's fallback biome.
    fn default_biome(&self) -> u32 {
        self.default_biome
    }

    /// Maps unknown wire ids to the same fallback used by vanilla decoding.
    fn resolve(&self, biome_id: u16) -> u32 {
        let biome_id = u32::from(biome_id);
        if !self.assets.is_diagnostic()
            && self.biome_tints.dense_index(biome_id) == assets::MISSING_BIOME_DENSE_INDEX
        {
            self.default_biome
        } else {
            biome_id
        }
    }
}

/// Converts the admitted dimension range into packed palette slots.
pub fn dimension_slots(range: DimensionRange) -> DimensionSlots {
    DimensionSlots {
        base_sub_chunk_y: range.base_sub_chunk_y,
        count: range.sub_chunk_count,
    }
}

/// Vanilla's fallback biome ids: ocean, hell, and the_end.
pub fn default_biome_id(dimension: i32) -> u32 {
    match dimension {
        1 => 8,
        2 => 9,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use world::{NbtCompound, NbtValue};

    #[test]
    fn persistent_hash_converts_supported_nbt_types_and_normalizes_names() {
        use protocol::CustomStateValue;

        let mut states = NbtCompound::default();
        states.insert("powered_bit", NbtValue::Byte(1));
        states.insert("open_bit", NbtValue::Byte(0));
        states.insert("direction", NbtValue::Int(-2));
        states.insert("variant", NbtValue::String("example".into()));
        let mut entry = NbtCompound::default();
        entry.insert("name", NbtValue::String("stone".into()));
        entry.insert("states", NbtValue::Compound(states.clone()));
        let expected_states = [
            ("variant", CustomStateValue::String("example".into())),
            ("direction", CustomStateValue::Int(-2)),
            ("open_bit", CustomStateValue::Bool(false)),
            ("powered_bit", CustomStateValue::Bool(true)),
        ];
        let expected = protocol::block_state_network_hash(
            "minecraft:stone",
            expected_states.iter().map(|(key, value)| (*key, value)),
        );
        assert_eq!(persistent_hash(&entry), Some(expected));
        entry.insert("name", NbtValue::String("minecraft:stone".into()));
        assert_eq!(persistent_hash(&entry), Some(expected));
        for value in [NbtValue::Byte(-1), NbtValue::Byte(2), NbtValue::Short(1)] {
            let mut invalid_states = states.clone();
            invalid_states.insert("powered_bit", value);
            entry.insert("states", NbtValue::Compound(invalid_states));
            assert_eq!(persistent_hash(&entry), None);
        }
    }

    #[test]
    fn persistent_unknown_and_wrongly_typed_names_use_air() {
        let assets = Arc::new(RuntimeAssets::diagnostic());
        let ids = DecodeIds {
            biome_tints: Arc::new(assets.biome_assets().resolve_live(&[]).unwrap()),
            assets,
            custom_blocks: 0..0,
            custom_identities: Arc::default(),
            remap: Arc::default(),
            diagnostics: Arc::default(),
            session_id: 0,
            mode: NetworkIdMode::Sequential,
            air: 0,
            default_biome: default_biome_id(0),
        };
        let mut entry = NbtCompound::default();
        entry.insert("name", NbtValue::String("example:unknown".into()));
        assert_eq!(ids.resolve_persistent(&entry), ids.air);
        entry.insert("name", NbtValue::Int(3));
        assert_eq!(ids.resolve_persistent(&entry), ids.air);
        assert_eq!(ids.diagnostics.invalid_persistent_count(), 1);
        entry.insert("name", NbtValue::String("stone".into()));
        let stone = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap()
        .into_vec()
        .into_iter()
        .find(|record| record.name.as_ref() == "minecraft:stone")
        .unwrap();
        assert_eq!(ids.resolve_persistent(&entry), stone.sequential_id);
    }

    #[test]
    fn persistent_palettes_use_internal_identity_in_both_network_modes() {
        use assets::{
            BlockFlags, BlockOverlay, BlockVisual, ContributorRole, LightProperties, Material,
            NO_ANIMATION, NO_MODEL_TEMPLATE, TextureRef, VisualKind, VisualSupport,
        };
        let mut entry = NbtCompound::default();
        entry.insert("name", NbtValue::String("example:solid".into()));
        let mut states = NbtCompound::default();
        states.insert("powered_bit", NbtValue::Byte(1));
        states.insert("direction", NbtValue::Int(-2));
        states.insert("variant", NbtValue::String("example".into()));
        entry.insert("states", NbtValue::Compound(states));
        let hash = persistent_hash(&entry).unwrap();
        let base = RuntimeAssets::diagnostic();
        let overlay = BlockOverlay {
            visuals: vec![BlockVisual {
                faces: [0; 6],
                flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                kind: VisualKind::Cube,
                support: VisualSupport::Exact,
                contributor_role: ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            }],
            light_properties: vec![LightProperties::OPAQUE_DARK],
            materials: vec![Material {
                texture: TextureRef::new(1, 0).unwrap(),
                flags: 0,
                animation: NO_ANIMATION,
                ..Material::unvaried()
            }],
            texture: Some(base.texture_pages()[0].texture.clone()),
            hashes: vec![Some(hash)],
            ..BlockOverlay::default()
        };
        let assets = Arc::new(base.with_block_overlay(1, &overlay).unwrap());
        for (mode, expected) in [
            (NetworkIdMode::Sequential, 1),
            (NetworkIdMode::Hashed, hash),
        ] {
            let ids = DecodeIds {
                biome_tints: Arc::new(assets.biome_assets().resolve_live(&[]).unwrap()),
                assets: Arc::clone(&assets),
                custom_blocks: 1..2,
                custom_identities: Arc::new(std::collections::HashMap::from([(hash, 99)])),
                remap: Arc::new(assets::SequentialIdRemap::new([(0, 1, 2)])),
                diagnostics: Arc::default(),
                session_id: 0,
                mode,
                air: 0,
                default_biome: default_biome_id(0),
            };
            let mut payload = vec![8, 1, 0];
            payload.extend(entry.encode_root().unwrap());
            let decoded = world::SubChunk::decode(&payload, &ids);
            assert_eq!(decoded.runtime_id(0, 0, 0, 0), Some(expected));
            let mut unknown = entry.clone();
            unknown.insert("name", NbtValue::String("example:absent".into()));
            assert_eq!(ids.resolve_persistent(&unknown), ids.air);
        }
    }
}
