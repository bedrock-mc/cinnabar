use crate::valentine::GAME_VERSION;
use crate::valentine::types::{BlockCoordinates, Experiments, GameMode, Vec2F, Vec3F};
use crate::valentine::{
    AvailableEntityIdentifiersPacket, BiomeDefinitionListPacket, CreativeContentPacket,
    EducationSharedResourceUri, ItemRegistryPacket, PermissionLevel, StartGamePacket,
    StartGamePacketChatRestrictionLevel, StartGamePacketDimension, StartGamePacketEditorWorldType,
};
use std::sync::Arc;
use uuid::Uuid;
use valentine::bedrock::codec::Nbt;

#[derive(Clone, Debug)]
pub struct WorldTemplate {
    pub start_game_template: StartGamePacket,
    pub item_registry: Arc<ItemRegistryPacket>,
    pub biome_definitions: Arc<BiomeDefinitionListPacket>,
    pub available_entities: Arc<AvailableEntityIdentifiersPacket>,
    pub creative_content: Arc<CreativeContentPacket>,
}

#[derive(Debug)]
pub struct WorldJoinParams {
    pub start_game: StartGamePacket,
    pub item_registry: Arc<ItemRegistryPacket>,
    pub biome_definitions: Arc<BiomeDefinitionListPacket>,
    pub available_entities: Arc<AvailableEntityIdentifiersPacket>,
    pub creative_content: Arc<CreativeContentPacket>,
}

impl WorldTemplate {
    pub fn to_join_params(&self, entity_id: i64) -> WorldJoinParams {
        let mut start = self.start_game_template.clone();
        start.entity_id = entity_id;
        start.runtime_entity_id = entity_id;

        WorldJoinParams {
            start_game: start,
            item_registry: self.item_registry.clone(),
            biome_definitions: self.biome_definitions.clone(),
            available_entities: self.available_entities.clone(),
            creative_content: self.creative_content.clone(),
        }
    }
}

impl Default for WorldTemplate {
    fn default() -> Self {
        let start_game_template = StartGamePacket {
            entity_id: 0,
            runtime_entity_id: 0,
            player_gamemode: GameMode::Survival,
            player_position: Vec3F {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: Vec2F { x: 0.0, z: 0.0 },
            seed: 0,
            dimension: StartGamePacketDimension::Overworld,
            generator: 1,
            world_gamemode: GameMode::Survival,
            difficulty: 0,
            spawn_position: BlockCoordinates { x: 0, y: 0, z: 0 },
            game_version: GAME_VERSION.into(),
            level_id: "".into(),
            world_name: "Jolyne Server".into(),
            world_identifier: "".into(),
            server_authoritative_inventory: false,
            server_authoritative_block_breaking: false,
            block_network_ids_are_hashes: false,
            block_pallette_checksum: 0,
            biome_type: 0,
            biome_name: "minecraft:plains".into(),
            hardcore: false,
            achievements_disabled: true,
            editor_world_type: StartGamePacketEditorWorldType::NotEditor,
            created_in_editor: false,
            exported_from_editor: false,
            day_cycle_stop_time: 0,
            edu_offer: 0,
            edu_features_enabled: false,
            edu_product_uuid: "".into(),
            rain_level: 0.0,
            lightning_level: 0.0,
            has_confirmed_platform_locked_content: false,
            is_multiplayer: true,
            broadcast_to_lan: false,
            xbox_live_broadcast_mode: 0,
            platform_broadcast_mode: 0,
            enable_commands: true,
            is_texturepacks_required: false,
            gamerules: vec![],
            experiments: Experiments::new(),
            experiments_previously_used: false,
            bonus_chest: false,
            map_enabled: false,
            permission_level: PermissionLevel::Member,
            server_chunk_tick_range: 4,
            has_locked_behavior_pack: false,
            has_locked_resource_pack: false,
            is_from_locked_world_template: false,
            msa_gamertags_only: false,
            is_from_world_template: false,
            is_world_template_option_locked: false,
            only_spawn_v_1_villagers: false,
            persona_disabled: false,
            custom_skins_disabled: false,
            emote_chat_muted: false,
            limited_world_width: 0,
            limited_world_length: 0,
            is_new_nether: true,
            edu_resource_uri: EducationSharedResourceUri {
                button_name: "".into(),
                link_uri: "".into(),
            },
            experimental_gameplay_override: false,
            chat_restriction_level: StartGamePacketChatRestrictionLevel::None,
            disable_player_interactions: false,
            server_editor_connection_policy: 0,
            allow_anonymous_block_drops_in_editor_worlds: false,
            server_identifier: "".into(),
            scenario_identifier: "".into(),
            owner_identifier: "".into(),
            premium_world_template_id: "".into(),
            is_trial: false,
            rewind_history_size: 0,
            current_tick: 0,
            enchantment_seed: 0,
            block_properties: vec![],
            multiplayer_correlation_id: Uuid::new_v4().to_string(),
            engine: GAME_VERSION.into(),
            property_data: Nbt::default(),
            world_template_id: Uuid::nil(),
            client_side_generation: false,
            server_controlled_sound: false,
            is_chat_logging: false,
            server_join_info: None,
        };

        Self {
            start_game_template,
            item_registry: Arc::new(ItemRegistryPacket { itemstates: vec![] }),
            biome_definitions: Arc::new(BiomeDefinitionListPacket {
                biome_definitions: vec![],
                string_list: vec![],
            }),
            available_entities: Arc::new(AvailableEntityIdentifiersPacket {
                nbt: Nbt::default(),
            }),
            creative_content: Arc::new(CreativeContentPacket {
                groups: vec![],
                items: vec![],
            }),
        }
    }
}
