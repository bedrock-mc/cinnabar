// Code generated from canonical protocol manifest v2. DO NOT EDIT.

use crate::enums::*;
use crate::types::*;
use crate::wire;
use crate::wire::{Direction, Peer};

/// Login is sent when the client initially tries to join the server. It is the first packet sent and contains
/// information specific to the player.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Login {
    /// ClientProtocol is the protocol version of the player. The player is disconnected if the protocol is
    /// incompatible with the protocol of the server. It has been superseded by the protocol version sent in the
    /// RequestNetworkSettings packet, so this should no longer be used by the server.
    pub client_network_version: wire::I32BE,
    /// `connection_request` is a string containing information about the player and JWTs that may be used to
    /// verify if the player is connected to XBOX Live. The connection request also contains the necessary client
    /// public key to initiate encryption.
    pub connection_request: bytes::Bytes,
}

impl Login {
    pub const ID: u32 = 1;
}
impl wire::Encode for Login {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.client_network_version.encode(writer)?;
        self.connection_request.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for Login {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let client_network_version = <wire::I32BE as wire::Decode>::decode(reader)?;
        let connection_request =
            reader.read_shared_bytes(crate::fields::LOGIN_CONNECTION_REQUEST)?;
        Ok(Self {
            client_network_version,
            connection_request,
        })
    }
}

/// PlayStatus is sent by the server to update a player on the play status. This includes failed statuses due
/// to a mismatched version, but also success statuses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayStatus {
    /// `status` is the status of the packet. It is one of the constants found above.
    pub status: PlayStatusType,
}

impl PlayStatus {
    pub const ID: u32 = 2;
}
impl wire::Encode for PlayStatus {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.status.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayStatus {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let status = <PlayStatusType as wire::Decode>::decode(reader)?;
        Ok(Self { status })
    }
}

/// ServerToClientHandshake is sent by the server to the client to complete the key exchange in order to
/// initialise encryption on client and server side. It is followed up by a ClientToServerHandshake packet
/// from the client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerToClientHandshake {
    pub handshake_web_token: wire::Str,
}

impl ServerToClientHandshake {
    pub const ID: u32 = 3;
}
impl wire::Encode for ServerToClientHandshake {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.handshake_web_token.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerToClientHandshake {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let handshake_web_token =
            reader.read_str(crate::fields::SERVER_TO_CLIENT_HANDSHAKE_HANDSHAKE_WEB_TOKEN)?;
        Ok(Self {
            handshake_web_token,
        })
    }
}

/// ClientToServerHandshake is sent by the client in response to a ServerToClientHandshake packet sent by the
/// server. It is the first encrypted packet in the login handshake and serves as a confirmation that
/// encryption is correctly initialised client side.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientToServerHandshake {}

impl ClientToServerHandshake {
    pub const ID: u32 = 4;
}
impl wire::Encode for ClientToServerHandshake {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        let _ = writer;
        Ok(())
    }
}

impl wire::Decode for ClientToServerHandshake {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let _ = reader;
        Ok(Self {})
    }
}

/// Disconnect may be sent by the server to disconnect the client using an optional message to send as the
/// disconnect screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Disconnect {
    /// `reason` is the reason for the disconnection. This affects the error code displayed on the Ore UI
    /// disconnection screen and is one of the constants above.
    pub reason: ConnectionDisconnectFailReason,
    pub messages: DisconnectMessages,
}

impl Disconnect {
    pub const ID: u32 = 5;
}
impl wire::Encode for Disconnect {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.reason.encode(writer)?;
        self.messages.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for Disconnect {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let reason = <ConnectionDisconnectFailReason as wire::Decode>::decode(reader)?;
        let messages = <DisconnectMessages as wire::Decode>::decode(reader)?;
        Ok(Self { reason, messages })
    }
}

/// ResourcePacksInfo is sent by the server to inform the client on what resource packs the server has. It
/// sends a list of the resource packs it has and basic information on them like the version and description.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourcePacksInfo {
    pub resource_pack_required: bool,
    pub has_addon_packs: bool,
    /// `has_scripts` specifies if any of the resource packs contain scripts in them. If set to true, only clients
    /// that support scripts will be able to download them.
    pub has_scripts: bool,
    /// `force_disable_vibrant_visuals` specifies if the vibrant visuals feature should be forcibly disabled on
    /// the server. If set to true, the server will ensure that vibrant visuals are not enabled, regardless of the
    /// client's settings.
    pub force_disable_vibrant_visuals: bool,
    pub world_template_id_and_version: PackIdVersion,
    pub resource_packs: Vec<PackInfoData>,
}

impl ResourcePacksInfo {
    pub const ID: u32 = 6;
}
impl wire::Encode for ResourcePacksInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.resource_pack_required.encode(writer)?;
        self.has_addon_packs.encode(writer)?;
        self.has_scripts.encode(writer)?;
        self.force_disable_vibrant_visuals.encode(writer)?;
        self.world_template_id_and_version.encode(writer)?;
        wire::encode_collection_limits(writer, self.resource_packs.as_slice(), 0, 65535)?;
        Ok(())
    }
}

impl wire::Decode for ResourcePacksInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let resource_pack_required = <bool as wire::Decode>::decode(reader)?;
        let has_addon_packs = <bool as wire::Decode>::decode(reader)?;
        let has_scripts = <bool as wire::Decode>::decode(reader)?;
        let force_disable_vibrant_visuals = <bool as wire::Decode>::decode(reader)?;
        let world_template_id_and_version = <PackIdVersion as wire::Decode>::decode(reader)?;
        let resource_packs = wire::decode_collection_limits::<PackInfoData>(
            reader,
            crate::fields::RESOURCE_PACKS_INFO_RESOURCE_PACKS,
            32,
            0,
            65535,
        )?;
        Ok(Self {
            resource_pack_required,
            has_addon_packs,
            has_scripts,
            force_disable_vibrant_visuals,
            world_template_id_and_version,
            resource_packs,
        })
    }
}

/// ResourcePackStack is sent by the server to send the order in which resource packs and behaviour packs
/// should be applied (and downloaded) by the client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourcePackStack {
    /// `texture_pack_required` specifies if the client must accept the texture packs the server has in order to
    /// join the server. If set to true, the client gets the option to either download the resource packs and
    /// join, or quit entirely. Behaviour packs never have to be downloaded.
    pub texture_pack_required: bool,
    /// TexturePacks is a list of texture packs that the client needs to download before joining the server. The
    /// order of these texture packs specifies the order that they are applied in on the client side. The first in
    /// the list will be applied first.
    pub texture_pack_list: Vec<PackInstanceId>,
    /// `base_game_version` is the vanilla version that the client should set its resource pack stack to.
    pub base_game_version: wire::Str,
    /// `experiments` holds a list of experiments that are either enabled or disabled in the world that the player
    /// spawns in. It is not clear why experiments are sent both here and in the StartGame packet.
    pub experiments: Experiments,
    /// `include_editor_packs` specifies if vanilla editor packs should be included in the resource pack stack
    /// when connecting to an editor world.
    pub include_editor_packs: bool,
}

impl ResourcePackStack {
    pub const ID: u32 = 7;
}
impl wire::Encode for ResourcePackStack {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.texture_pack_required.encode(writer)?;
        wire::encode_collection_limits(writer, self.texture_pack_list.as_slice(), 0, 65535)?;
        self.base_game_version.encode(writer)?;
        self.experiments.encode(writer)?;
        self.include_editor_packs.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ResourcePackStack {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let texture_pack_required = <bool as wire::Decode>::decode(reader)?;
        let texture_pack_list = wire::decode_collection_limits::<PackInstanceId>(
            reader,
            crate::fields::RESOURCE_PACK_STACK_TEXTURE_PACK_LIST,
            3,
            0,
            65535,
        )?;
        let base_game_version =
            reader.read_str(crate::fields::RESOURCE_PACK_STACK_BASE_GAME_VERSION)?;
        let experiments = <Experiments as wire::Decode>::decode(reader)?;
        let include_editor_packs = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            texture_pack_required,
            texture_pack_list,
            base_game_version,
            experiments,
            include_editor_packs,
        })
    }
}

/// ResourcePackClientResponse is sent by the client in response to resource packets sent by the server. It is
/// used to let the server know what action needs to be taken for the client to have all resource packs ready
/// and set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourcePackClientResponse {
    /// `response` is the response type of the response. It is one of the constants found above.
    pub response: ResourcePackClientResponseData,
}

impl ResourcePackClientResponse {
    pub const ID: u32 = 8;
}
impl wire::Encode for ResourcePackClientResponse {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.response.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ResourcePackClientResponse {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let response = <ResourcePackClientResponseData as wire::Decode>::decode(reader)?;
        Ok(Self { response })
    }
}

/// Text is sent by the client to the server to send chat messages, and by the server to the client to forward
/// or send messages, which may be chat, popups, tips etc.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Text {
    pub localize: bool,
    pub body: TextData,
    /// XUID is the XBOX Live user ID of the player that sent the message. It is only set for packets of
    /// TextTypeChat. When sent to a player, the player will only be shown the chat message if a player with this
    /// XUID is present in the player list and not muted, or if the XUID is empty.
    pub sender_xuid: wire::Str,
    pub platform_id: wire::Str,
    /// `filtered_message` is a filtered version of Message with all the profanity removed. The client will use
    /// this over Message if this field is not empty and they have the "Filter Profanity" setting enabled.
    /// Wire presence: optional value is preceded by a presence marker.
    pub filtered_message: Option<wire::Str>,
}

impl Text {
    pub const ID: u32 = 9;
}
impl wire::Encode for Text {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.localize.encode(writer)?;
        self.body.encode(writer)?;
        wire::encode_string_limits(writer, &self.sender_xuid, 0, 64)?;
        wire::encode_string_limits(writer, &self.platform_id, 0, 256)?;
        match &self.filtered_message {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_string_limits(writer, value, 0, 65536)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for Text {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let localize = <bool as wire::Decode>::decode(reader)?;
        let body = <TextData as wire::Decode>::decode(reader)?;
        let sender_xuid =
            wire::decode_string_limits(reader, crate::fields::TEXT_SENDER_XUID, 0, 64)?;
        let platform_id =
            wire::decode_string_limits(reader, crate::fields::TEXT_PLATFORM_ID, 0, 256)?;
        let filtered_message = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(wire::decode_string_limits(
                    reader,
                    crate::fields::TEXT_FILTERED_MESSAGE,
                    0,
                    65536,
                )?)
            }
        };
        Ok(Self {
            localize,
            body,
            sender_xuid,
            platform_id,
            filtered_message,
        })
    }
}

/// SetTime is sent by the server to update the current time client-side. The client actually advances time
/// client-side by itself, so this packet does not need to be sent each tick. It is merely a means of
/// synchronising time between server and client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetTime {
    /// `time` is the current time. The time is not limited to 24000 (time of day), but continues progressing
    /// after that.
    pub time: wire::ZigZag32,
}

impl SetTime {
    pub const ID: u32 = 10;
}
impl wire::Encode for SetTime {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.time.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetTime {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let time = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self { time })
    }
}

/// StartGame is sent by the server to send information about the world the player will be spawned in. It
/// contains information about the position the player spawns in, and information about the world in general
/// such as its game rules.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StartGame {
    /// EntityUniqueID is the unique ID of the player. The unique ID is a value that remains consistent across
    /// different sessions of the same world, but most servers simply fill the runtime ID of the entity out for
    /// this field.
    pub entity_id: ActorUniqueID,
    /// EntityRuntimeID is the runtime ID of the player. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub runtime_id: ActorRuntimeID,
    pub game_type: GameType,
    /// PlayerPosition is the spawn position of the player in the world. In servers this is often the same as the
    /// world's spawn position found below.
    pub position: glam::Vec3,
    pub rotation: glam::Vec2,
    pub settings: LevelSettings,
    /// `level_id` is a base64 encoded world ID that is used to identify the world.
    pub level_id: wire::Str,
    pub level_name: wire::Str,
    /// `template_content_identity` is a UUID specific to the premium world template that might have been used to
    /// generate the world. Servers should always fill out an empty string for this.
    pub template_content_identity: wire::Str,
    /// Trial specifies if the world was a trial world, meaning features are limited and there is a time limit on
    /// the world.
    pub is_trial: bool,
    pub movement_settings: SyncedPlayerMovementSettings,
    /// Time is the total time that has elapsed since the start of the world.
    pub level_current_time: wire::U64LE,
    /// `enchantment_seed` is the seed used to seed the random used to produce enchantments in the enchantment
    /// table. Note that the exact correct random implementation must be used to produce the correct results both
    /// client- and server-side.
    pub enchantment_seed: wire::ZigZag32,
    pub block_properties: Vec<ServerBlockProperty>,
    /// `multiplayer_correlation_id` is a unique ID specifying the multi-player session of the player. A random
    /// UUID should be filled out for this field.
    pub multiplayer_correlation_id: wire::Str,
    pub enable_item_stack_net_manager: bool,
    pub server_version: wire::Str,
    pub player_property_data: wire::NetworkNbt,
    pub server_block_type_registry_checksum: wire::U64LE,
    /// `world_template_id` is a UUID that identifies the template that was used to generate the world. Servers
    /// that do not use a world based off of a template can set this to an empty UUID.
    pub world_template_id: uuid::Uuid,
    /// ClientSideGeneration is true if the client should use the features registered in the FeatureRegistry
    /// packet to generate terrain client-side to save on bandwidth.
    pub server_enabled_client_side_generation: bool,
    pub block_network_ids_are_hashes: bool,
    pub network_permissions: NetworkPermissions,
    /// Wire presence: optional value is preceded by a presence marker.
    pub server_configuration_join_info: Option<ServerConfigurationServerConfigurationJoinInfo>,
    pub server_telemetry_data: SocialEventsServerTelemetryData,
}

impl StartGame {
    pub const ID: u32 = 11;
}
impl wire::Encode for StartGame {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.entity_id.encode(writer)?;
        self.runtime_id.encode(writer)?;
        self.game_type.encode(writer)?;
        self.position.encode(writer)?;
        self.rotation.encode(writer)?;
        self.settings.encode(writer)?;
        self.level_id.encode(writer)?;
        self.level_name.encode(writer)?;
        self.template_content_identity.encode(writer)?;
        self.is_trial.encode(writer)?;
        self.movement_settings.encode(writer)?;
        self.level_current_time.encode(writer)?;
        wire::check_number_limits(self.level_current_time.0, Some(0), None)?;
        self.enchantment_seed.encode(writer)?;
        wire::encode_collection(writer, self.block_properties.as_slice())?;
        self.multiplayer_correlation_id.encode(writer)?;
        self.enable_item_stack_net_manager.encode(writer)?;
        self.server_version.encode(writer)?;
        self.player_property_data.encode(writer)?;
        self.server_block_type_registry_checksum.encode(writer)?;
        wire::check_number_limits(self.server_block_type_registry_checksum.0, Some(0), None)?;
        self.world_template_id.encode(writer)?;
        self.server_enabled_client_side_generation.encode(writer)?;
        self.block_network_ids_are_hashes.encode(writer)?;
        self.network_permissions.encode(writer)?;
        match &self.server_configuration_join_info {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.server_telemetry_data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StartGame {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let entity_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let game_type = <GameType as wire::Decode>::decode(reader)?;
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let rotation = <glam::Vec2 as wire::Decode>::decode(reader)?;
        let settings = <LevelSettings as wire::Decode>::decode(reader)?;
        let level_id = reader.read_str(crate::fields::START_GAME_LEVEL_ID)?;
        let level_name = reader.read_str(crate::fields::START_GAME_LEVEL_NAME)?;
        let template_content_identity =
            reader.read_str(crate::fields::START_GAME_TEMPLATE_CONTENT_IDENTITY)?;
        let is_trial = <bool as wire::Decode>::decode(reader)?;
        let movement_settings = <SyncedPlayerMovementSettings as wire::Decode>::decode(reader)?;
        let level_current_time = {
            let value = <wire::U64LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let enchantment_seed = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let block_properties = wire::decode_collection::<ServerBlockProperty>(
            reader,
            crate::fields::START_GAME_BLOCK_PROPERTIES,
            2,
        )?;
        let multiplayer_correlation_id =
            reader.read_str(crate::fields::START_GAME_MULTIPLAYER_CORRELATION_ID)?;
        let enable_item_stack_net_manager = <bool as wire::Decode>::decode(reader)?;
        let server_version = reader.read_str(crate::fields::START_GAME_SERVER_VERSION)?;
        let player_property_data = <wire::NetworkNbt as wire::Decode>::decode(reader)?;
        let server_block_type_registry_checksum = {
            let value = <wire::U64LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let world_template_id = <uuid::Uuid as wire::Decode>::decode(reader)?;
        let server_enabled_client_side_generation = <bool as wire::Decode>::decode(reader)?;
        let block_network_ids_are_hashes = <bool as wire::Decode>::decode(reader)?;
        let network_permissions = <NetworkPermissions as wire::Decode>::decode(reader)?;
        let server_configuration_join_info = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(
                    <ServerConfigurationServerConfigurationJoinInfo as wire::Decode>::decode(
                        reader,
                    )?,
                )
            }
        };
        let server_telemetry_data =
            <SocialEventsServerTelemetryData as wire::Decode>::decode(reader)?;
        Ok(Self {
            entity_id,
            runtime_id,
            game_type,
            position,
            rotation,
            settings,
            level_id,
            level_name,
            template_content_identity,
            is_trial,
            movement_settings,
            level_current_time,
            enchantment_seed,
            block_properties,
            multiplayer_correlation_id,
            enable_item_stack_net_manager,
            server_version,
            player_property_data,
            server_block_type_registry_checksum,
            world_template_id,
            server_enabled_client_side_generation,
            block_network_ids_are_hashes,
            network_permissions,
            server_configuration_join_info,
            server_telemetry_data,
        })
    }
}

/// AddPlayer is sent by the server to the client to make a player entity show up client-side. It is one of
/// the few entities that cannot be sent using the AddActor packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AddPlayer {
    /// `uuid` is the UUID of the player. It is the same UUID that the client sent in the Login packet at the
    /// start of the session. A player with this UUID must exist in the player list (built up using the PlayerList
    /// packet), for it to show up in-game.
    pub uuid: uuid::Uuid,
    pub player_name: wire::Str,
    /// EntityRuntimeID is the runtime ID of the player. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub target_runtime_id: ActorRuntimeID,
    /// `platform_chat_id` is an identifier only set for particular platforms when chatting (presumably only for
    /// Nintendo Switch). It is otherwise an empty string, and is used to decide which players are able to chat
    /// with each other.
    pub platform_chat_id: wire::Str,
    /// `position` is the position to spawn the player on. If the player is on a distance that the viewer cannot
    /// see it, the player will still show up if the viewer moves closer.
    pub position: glam::Vec3,
    /// `velocity` is the initial velocity the player spawns with. This velocity will initiate client side
    /// movement of the player.
    pub velocity: glam::Vec3,
    pub rotation: glam::Vec2,
    pub y_head_rotation: wire::F32LE,
    pub carried_item: NetworkItemStackDescriptorSerializedData,
    /// GameType is the game type of the player. If set to GameTypeSpectator, the player will not be shown to
    /// viewers.
    pub player_game_type: GameType,
    pub entity_data: SynchedActorDataCopyableDataList,
    pub synched_properties: PropertySyncData,
    pub abilities_data: SerializedAbilitiesData,
    /// EntityLinks is a list of entity links that are currently active on the player. These links alter the way
    /// the player shows up when first spawned in terms of it shown as riding an entity. Setting these links is
    /// important for new viewers to see the player is riding another entity.
    pub actor_links: Vec<EntityLink>,
    /// `device_id` is the device ID set in one of the files found in the storage of the device of the player. It
    /// may be changed freely, so it should not be relied on for anything.
    pub device_id: wire::Str,
    /// `build_platform` is the build platform/device OS of the player that is about to be added, as it sent in
    /// the Login packet when joining.
    pub build_platform: BuildPlatform,
}

impl AddPlayer {
    pub const ID: u32 = 12;
}
impl wire::Encode for AddPlayer {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.uuid.encode(writer)?;
        self.player_name.encode(writer)?;
        self.target_runtime_id.encode(writer)?;
        self.platform_chat_id.encode(writer)?;
        self.position.encode(writer)?;
        self.velocity.encode(writer)?;
        self.rotation.encode(writer)?;
        self.y_head_rotation.encode(writer)?;
        self.carried_item.encode(writer)?;
        self.player_game_type.encode(writer)?;
        self.entity_data.encode(writer)?;
        self.synched_properties.encode(writer)?;
        self.abilities_data.encode(writer)?;
        wire::encode_collection(writer, self.actor_links.as_slice())?;
        self.device_id.encode(writer)?;
        self.build_platform.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AddPlayer {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let uuid = <uuid::Uuid as wire::Decode>::decode(reader)?;
        let player_name = reader.read_str(crate::fields::ADD_PLAYER_PLAYER_NAME)?;
        let target_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let platform_chat_id = reader.read_str(crate::fields::ADD_PLAYER_PLATFORM_CHAT_ID)?;
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let velocity = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let rotation = <glam::Vec2 as wire::Decode>::decode(reader)?;
        let y_head_rotation = <wire::F32LE as wire::Decode>::decode(reader)?;
        let carried_item =
            <NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?;
        let player_game_type = <GameType as wire::Decode>::decode(reader)?;
        let entity_data = <SynchedActorDataCopyableDataList as wire::Decode>::decode(reader)?;
        let synched_properties = <PropertySyncData as wire::Decode>::decode(reader)?;
        let abilities_data = <SerializedAbilitiesData as wire::Decode>::decode(reader)?;
        let actor_links = wire::decode_collection::<EntityLink>(
            reader,
            crate::fields::ADD_PLAYER_ACTOR_LINKS,
            9,
        )?;
        let device_id = reader.read_str(crate::fields::ADD_PLAYER_DEVICE_ID)?;
        let build_platform = <BuildPlatform as wire::Decode>::decode(reader)?;
        Ok(Self {
            uuid,
            player_name,
            target_runtime_id,
            platform_chat_id,
            position,
            velocity,
            rotation,
            y_head_rotation,
            carried_item,
            player_game_type,
            entity_data,
            synched_properties,
            abilities_data,
            actor_links,
            device_id,
            build_platform,
        })
    }
}

/// AddActor is sent by the server to the client to spawn an entity to the player. It is used for every entity
/// except other players, for which the AddPlayer packet is used.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AddActor {
    /// EntityUniqueID is the unique ID of the entity. The unique ID is a value that remains consistent across
    /// different sessions of the same world, but most servers simply fill the runtime ID of the entity out for
    /// this field.
    pub target_actor_id: ActorUniqueID,
    /// EntityRuntimeID is the runtime ID of the entity. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub target_runtime_id: ActorRuntimeID,
    /// EntityType is the string entity type of the entity, for example 'minecraft:skeleton'. A list of these
    /// entities may be found online.
    pub actor_type: wire::Str,
    /// `position` is the position to spawn the entity on. If the entity is on a distance that the player cannot
    /// see it, the entity will still show up if the player moves closer.
    pub position: glam::Vec3,
    /// `velocity` is the initial velocity the entity spawns with. This velocity will initiate client side
    /// movement of the entity.
    pub velocity: glam::Vec3,
    pub rotation: glam::Vec2,
    pub y_head_rotation: wire::F32LE,
    pub y_body_rotation: wire::F32LE,
    /// Attributes is a slice of attributes that the entity has. It includes attributes such as its health,
    /// movement speed, etc.
    pub attributes_list: Vec<SyncedAttribute>,
    pub actor_data: SynchedActorDataCopyableDataList,
    pub synched_properties: PropertySyncData,
    /// EntityLinks is a list of entity links that are currently active on the entity. These links alter the way
    /// the entity shows up when first spawned in terms of it shown as riding an entity. Setting these links is
    /// important for new viewers to see the entity is riding another entity.
    pub actor_links: Vec<EntityLink>,
}

impl AddActor {
    pub const ID: u32 = 13;
}
impl wire::Encode for AddActor {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_actor_id.encode(writer)?;
        self.target_runtime_id.encode(writer)?;
        self.actor_type.encode(writer)?;
        self.position.encode(writer)?;
        self.velocity.encode(writer)?;
        self.rotation.encode(writer)?;
        self.y_head_rotation.encode(writer)?;
        self.y_body_rotation.encode(writer)?;
        wire::encode_collection(writer, self.attributes_list.as_slice())?;
        self.actor_data.encode(writer)?;
        self.synched_properties.encode(writer)?;
        wire::encode_collection(writer, self.actor_links.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for AddActor {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_actor_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let target_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let actor_type = reader.read_str(crate::fields::ADD_ACTOR_ACTOR_TYPE)?;
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let velocity = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let rotation = <glam::Vec2 as wire::Decode>::decode(reader)?;
        let y_head_rotation = <wire::F32LE as wire::Decode>::decode(reader)?;
        let y_body_rotation = <wire::F32LE as wire::Decode>::decode(reader)?;
        let attributes_list = wire::decode_collection::<SyncedAttribute>(
            reader,
            crate::fields::ADD_ACTOR_ATTRIBUTES_LIST,
            13,
        )?;
        let actor_data = <SynchedActorDataCopyableDataList as wire::Decode>::decode(reader)?;
        let synched_properties = <PropertySyncData as wire::Decode>::decode(reader)?;
        let actor_links =
            wire::decode_collection::<EntityLink>(reader, crate::fields::ADD_ACTOR_ACTOR_LINKS, 9)?;
        Ok(Self {
            target_actor_id,
            target_runtime_id,
            actor_type,
            position,
            velocity,
            rotation,
            y_head_rotation,
            y_body_rotation,
            attributes_list,
            actor_data,
            synched_properties,
            actor_links,
        })
    }
}

/// RemoveActor is sent by the server to remove an entity that currently exists in the world from the client-
/// side. Sending this packet if the client cannot already see this entity will have no effect.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RemoveActor {
    /// EntityUniqueID is the unique ID of the entity to be removed. The unique ID is a value that remains
    /// consistent across different sessions of the same world, but most servers simply fill the runtime ID of the
    /// entity out for this field.
    pub target_actor_id: ActorUniqueID,
}

impl RemoveActor {
    pub const ID: u32 = 14;
}
impl wire::Encode for RemoveActor {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_actor_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RemoveActor {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_actor_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        Ok(Self { target_actor_id })
    }
}

/// AddItemActor is sent by the server to the client to make an item entity show up. It is one of the few
/// entities that cannot be sent using the AddActor packet
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AddItemActor {
    /// EntityUniqueID is the unique ID of the entity. The unique ID is a value that remains consistent across
    /// different sessions of the same world, but most servers simply fill the runtime ID of the entity out for
    /// this field.
    pub target_actor_id: ActorUniqueID,
    /// EntityRuntimeID is the runtime ID of the entity. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub target_runtime_id: ActorRuntimeID,
    /// `item` is the item that is spawned. It must have a valid ID for it to show up client-side. If it is not a
    /// valid item, the client will crash when coming near.
    pub item: NetworkItemStackDescriptorSerializedData,
    /// `position` is the position to spawn the entity on. If the entity is on a distance that the player cannot
    /// see it, the entity will still show up if the player moves closer.
    pub position: glam::Vec3,
    /// `velocity` is the initial velocity the entity spawns with. This velocity will initiate client side
    /// movement of the entity.
    pub velocity: glam::Vec3,
    /// EntityMetadata is a map of entity metadata, which includes flags and data properties that alter in
    /// particular the way the entity looks. Flags include ones such as 'on fire' and 'sprinting'. The metadata
    /// values are indexed by their property key.
    pub entity_data: SynchedActorDataCopyableDataList,
    /// FromFishing specifies if the item was obtained by fishing it up using a fishing rod. It is not clear why
    /// the client needs to know this.
    pub is_from_fishing: bool,
}

impl AddItemActor {
    pub const ID: u32 = 15;
}
impl wire::Encode for AddItemActor {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_actor_id.encode(writer)?;
        self.target_runtime_id.encode(writer)?;
        self.item.encode(writer)?;
        self.position.encode(writer)?;
        self.velocity.encode(writer)?;
        self.entity_data.encode(writer)?;
        self.is_from_fishing.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AddItemActor {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_actor_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let target_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let item = <NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?;
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let velocity = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let entity_data = <SynchedActorDataCopyableDataList as wire::Decode>::decode(reader)?;
        let is_from_fishing = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            target_actor_id,
            target_runtime_id,
            item,
            position,
            velocity,
            entity_data,
            is_from_fishing,
        })
    }
}

/// ServerPlayerPostMovePosition is sent by the server with the player's position after movement processing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerPlayerPostMovePosition {
    /// Position is the player's position after the server has processed movement.
    pub pos: glam::Vec3,
}

impl ServerPlayerPostMovePosition {
    pub const ID: u32 = 16;
}
impl wire::Encode for ServerPlayerPostMovePosition {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.pos.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerPlayerPostMovePosition {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let pos = <glam::Vec3 as wire::Decode>::decode(reader)?;
        Ok(Self { pos })
    }
}

/// TakeItemActor is sent by the server when a player picks up an item entity. It makes the item entity
/// disappear to viewers and shows the pick-up animation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TakeItemActor {
    /// ItemEntityRuntimeID is the entity runtime ID of the item that is being taken by another entity. It will
    /// disappear to viewers after showing the pick-up animation.
    pub item_runtime_id: ActorRuntimeID,
    /// TakerEntityRuntimeID is the runtime ID of the entity that took the item, which is usually a player, but
    /// could be another entity like a zombie too.
    pub actor_runtime_id: ActorRuntimeID,
}

impl TakeItemActor {
    pub const ID: u32 = 17;
}
impl wire::Encode for TakeItemActor {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.item_runtime_id.encode(writer)?;
        self.actor_runtime_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for TakeItemActor {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let item_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let actor_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        Ok(Self {
            item_runtime_id,
            actor_runtime_id,
        })
    }
}

/// MoveActorAbsolute is sent by the server to move an entity to an absolute position. It is typically used
/// for movements where high accuracy isn't needed, such as for long range teleporting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MoveActorAbsolute {
    pub move_data: MoveActorAbsoluteData,
}

impl MoveActorAbsolute {
    pub const ID: u32 = 18;
}
impl wire::Encode for MoveActorAbsolute {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.move_data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MoveActorAbsolute {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let move_data = <MoveActorAbsoluteData as wire::Decode>::decode(reader)?;
        Ok(Self { move_data })
    }
}

/// MovePlayer is sent by players to send their movement to the server, and by the server to update the
/// movement of player entities to other players.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MovePlayer {
    /// EntityRuntimeID is the runtime ID of the player. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub player_runtime_id: ActorRuntimeID,
    /// `position` is the position to spawn the player on. If the player is on a distance that the viewer cannot
    /// see it, the player will still show up if the viewer moves closer.
    pub position: glam::Vec3,
    pub rotation: glam::Vec2,
    pub y_head_rotation: wire::F32LE,
    /// Mode is the mode of the movement. It specifies the way the player's movement should be shown to other
    /// players. It is one of the constants above.
    pub position_mode: PlayerPositionModeComponentPositionMode,
    /// `on_ground` specifies if the player is considered on the ground. Note that proxies or hacked clients could
    /// fake this to always be true, so it should not be taken for granted.
    pub on_ground: bool,
    /// RiddenEntityRuntimeID is the runtime ID of the entity that the player might currently be riding. If not
    /// riding, this should be left 0.
    pub riding_runtime_id: ActorRuntimeID,
    /// Wire presence: optional value is preceded by a presence marker.
    pub teleport_data: Option<MovePlayerTeleportData>,
    /// `tick` is the server tick at which the packet was sent. It is used in relation to
    /// CorrectPlayerMovePrediction.
    pub tick: PlayerInputTick,
}

impl MovePlayer {
    pub const ID: u32 = 19;
}
impl wire::Encode for MovePlayer {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.player_runtime_id.encode(writer)?;
        self.position.encode(writer)?;
        self.rotation.encode(writer)?;
        self.y_head_rotation.encode(writer)?;
        self.position_mode.encode(writer)?;
        self.on_ground.encode(writer)?;
        self.riding_runtime_id.encode(writer)?;
        match &self.teleport_data {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.tick.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MovePlayer {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let player_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let rotation = <glam::Vec2 as wire::Decode>::decode(reader)?;
        let y_head_rotation = <wire::F32LE as wire::Decode>::decode(reader)?;
        let position_mode =
            <PlayerPositionModeComponentPositionMode as wire::Decode>::decode(reader)?;
        let on_ground = <bool as wire::Decode>::decode(reader)?;
        let riding_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let teleport_data = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<MovePlayerTeleportData as wire::Decode>::decode(reader)?)
            }
        };
        let tick = <PlayerInputTick as wire::Decode>::decode(reader)?;
        Ok(Self {
            player_runtime_id,
            position,
            rotation,
            y_head_rotation,
            position_mode,
            on_ground,
            riding_runtime_id,
            teleport_data,
            tick,
        })
    }
}

/// UpdateBlock is sent by the server to update a block client-side, without resending the entire chunk that
/// the block is located in. It is particularly useful for small modifications like block breaking/placing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateBlock {
    /// `block_position` is the block position at which a block is updated.
    pub block_position: BlockPos,
    /// `block_runtime_id` is the runtime ID of the block that is placed at Position after sending the packet to
    /// the client.
    pub block_runtime_id: wire::VarUInt,
    /// `flags` is a combination of flags that specify the way the block is updated client-side. It is a
    /// combination of the flags above, but typically sending only the BlockUpdateNetwork flag is sufficient.
    pub flags: wire::VarUInt,
    /// `layer` is the world layer on which the block is updated. For most blocks, this is the first layer, as
    /// that layer is the default layer to place blocks on, but for blocks inside of each other, this differs.
    pub layer: wire::VarUInt,
}

impl UpdateBlock {
    pub const ID: u32 = 21;
}
impl wire::Encode for UpdateBlock {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.block_position.encode(writer)?;
        self.block_runtime_id.encode(writer)?;
        wire::check_number_limits(self.block_runtime_id.0, Some(0), None)?;
        self.flags.encode(writer)?;
        wire::check_number_limits(self.flags.0, Some(0), None)?;
        self.layer.encode(writer)?;
        wire::check_number_limits(self.layer.0, Some(0), None)?;
        Ok(())
    }
}

impl wire::Decode for UpdateBlock {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let block_position = <BlockPos as wire::Decode>::decode(reader)?;
        let block_runtime_id = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let flags = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let layer = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        Ok(Self {
            block_position,
            block_runtime_id,
            flags,
            layer,
        })
    }
}

/// AddPainting is sent by the server to the client to make a painting entity show up. It is one of the few
/// entities that cannot be sent using the AddActor packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AddPainting {
    /// EntityUniqueID is the unique ID of the entity. The unique ID is a value that remains consistent across
    /// different sessions of the same world, but most servers simply fill the runtime ID of the entity out for
    /// this field.
    pub target_actor_id: ActorUniqueID,
    /// EntityRuntimeID is the runtime ID of the entity. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub target_runtime_id: ActorRuntimeID,
    /// `position` is the position to spawn the entity on. If the entity is on a distance that the player cannot
    /// see it, the entity will still show up if the player moves closer.
    pub position: glam::Vec3,
    /// `direction` is the facing direction of the painting.
    pub direction: wire::ZigZag32,
    /// Title is the title of the painting. It specifies the motive of the painting. The title of the painting
    /// must be valid.
    pub motif: wire::Str,
}

impl AddPainting {
    pub const ID: u32 = 22;
}
impl wire::Encode for AddPainting {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_actor_id.encode(writer)?;
        self.target_runtime_id.encode(writer)?;
        self.position.encode(writer)?;
        self.direction.encode(writer)?;
        self.motif.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AddPainting {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_actor_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let target_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let direction = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let motif = reader.read_str(crate::fields::ADD_PAINTING_MOTIF)?;
        Ok(Self {
            target_actor_id,
            target_runtime_id,
            position,
            direction,
            motif,
        })
    }
}

/// LevelEvent is sent by the server to make a certain event in the level occur. It ranges from particles, to
/// sounds, and other events such as starting rain and block breaking.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelEvent {
    /// `event_id` is the ID of the event that is being 'called'. It is one of the events found in the constants
    /// above.
    pub event_id: wire::ZigZag32,
    /// `position` is the position of the level event. Practically every event requires this Vec3 set for it, as
    /// particles, sounds and block editing relies on it.
    pub position: glam::Vec3,
    /// `data` is an integer holding additional data of the event. The type of data held depends on the EventType.
    pub data: wire::ZigZag32,
}

impl LevelEvent {
    pub const ID: u32 = 25;
}
impl wire::Encode for LevelEvent {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.event_id.encode(writer)?;
        self.position.encode(writer)?;
        self.data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LevelEvent {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let event_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let data = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self {
            event_id,
            position,
            data,
        })
    }
}

/// BlockEvent is sent by the server to initiate a certain event that has something to do with blocks in
/// specific, for example opening a chest.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockEvent {
    /// `block_position` is the position of the block that an event occurred at.
    pub block_position: BlockPos,
    /// `event_type` is the type of the block event. The event type decides the way the event data that follows is
    /// used. It is one of the constants found above.
    pub event_type: wire::ZigZag32,
    /// `event_value` holds event type specific data. For chests for example, opening the chest means the data
    /// must hold 1, whereas closing it should hold 0.
    pub event_value: wire::ZigZag32,
}

impl BlockEvent {
    pub const ID: u32 = 26;
}
impl wire::Encode for BlockEvent {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.block_position.encode(writer)?;
        self.event_type.encode(writer)?;
        self.event_value.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BlockEvent {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let block_position = <BlockPos as wire::Decode>::decode(reader)?;
        let event_type = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let event_value = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self {
            block_position,
            event_type,
            event_value,
        })
    }
}

/// ActorEvent is sent by the server when a particular event happens that has to do with an entity. Some of
/// these events are entity-specific, for example a wolf shaking itself dry, but others are used for each
/// entity, such as dying.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActorEvent {
    /// EntityRuntimeID is the runtime ID of the entity. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub target_runtime_id: ActorRuntimeID,
    /// EventType is the ID of the event to be called. It is one of the constants that can be found above.
    pub event_id: ActorEventType,
    /// EventData is optional data associated with a particular event. The data has a different function for
    /// different events, however most events don't use this field at all.
    pub data: wire::ZigZag32,
    /// `fire_at_position` is the position in the same world at which the event should fire. If this is not
    /// present, the position entity will be used instead.
    /// Wire presence: optional value is preceded by a presence marker.
    pub fire_at_position: Option<glam::Vec3>,
}

impl ActorEvent {
    pub const ID: u32 = 27;
}
impl wire::Encode for ActorEvent {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_runtime_id.encode(writer)?;
        self.event_id.encode(writer)?;
        self.data.encode(writer)?;
        match &self.fire_at_position {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ActorEvent {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let event_id = <ActorEventType as wire::Decode>::decode(reader)?;
        let data = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let fire_at_position = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec3 as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            target_runtime_id,
            event_id,
            data,
            fire_at_position,
        })
    }
}

/// MobEffect is sent by the server to apply an effect to the player, for example an effect like poison. It
/// may also be used to modify existing effects, or removing them completely.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MobEffect {
    /// EntityRuntimeID is the runtime ID of the entity. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub target_runtime_id: ActorRuntimeID,
    /// Operation is the operation of the packet. It is either MobEffectAdd, MobEffectModify or MobEffectRemove
    /// and specifies the result of the packet client-side.
    pub event_id: MobEffectEvent,
    /// EffectType is the ID of the effect to be added, removed or modified. It is one of the constants that may
    /// be found above.
    pub effect_id: wire::ZigZag32,
    /// Amplifier is the amplifier of the effect. Take note that the amplifier is not the same as the effect's
    /// level. The level is usually one higher than the amplifier, and the amplifier can actually be negative to
    /// reverse the behaviour effect.
    pub effect_amplifier: wire::ZigZag32,
    /// Particles specifies if viewers of the entity that gets the effect shows particles around it. If set to
    /// false, no particles are emitted around the entity.
    pub show_particles: bool,
    /// Duration is the duration of the effect in ticks (20 per second). After the duration has elapsed, the
    /// effect will be removed automatically client-side. A negative duration means the effect never expires.
    pub effect_duration_ticks: wire::ZigZag32,
    /// `tick` is the server tick at which the packet was sent. It is used in relation to
    /// CorrectPlayerMovePrediction.
    pub tick: PlayerInputTick,
    /// `ambient` specifies if the effect is ambient. If set to false, it will not get treated as an ambient
    /// effect.
    pub ambient: bool,
}

impl MobEffect {
    pub const ID: u32 = 28;
}
impl wire::Encode for MobEffect {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_runtime_id.encode(writer)?;
        self.event_id.encode(writer)?;
        self.effect_id.encode(writer)?;
        self.effect_amplifier.encode(writer)?;
        self.show_particles.encode(writer)?;
        self.effect_duration_ticks.encode(writer)?;
        self.tick.encode(writer)?;
        self.ambient.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MobEffect {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let event_id = <MobEffectEvent as wire::Decode>::decode(reader)?;
        let effect_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let effect_amplifier = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let show_particles = <bool as wire::Decode>::decode(reader)?;
        let effect_duration_ticks = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let tick = <PlayerInputTick as wire::Decode>::decode(reader)?;
        let ambient = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            target_runtime_id,
            event_id,
            effect_id,
            effect_amplifier,
            show_particles,
            effect_duration_ticks,
            tick,
            ambient,
        })
    }
}

/// UpdateAttributes is sent by the server to update an amount of attributes of any entity in the world. These
/// attributes include ones such as the health or the movement speed of the entity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateAttributes {
    /// EntityRuntimeID is the runtime ID of the entity. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub target_runtime_id: ActorRuntimeID,
    /// Attributes is a slice of new attributes that the entity gets. It includes attributes such as its health,
    /// movement speed, etc. Note that only changed attributes have to be sent in this packet. It is not required
    /// to send attributes that did not have their values changed.
    pub attribute_list: Vec<AttributeData>,
    /// `tick` is the server tick at which the packet was sent. It is used in relation to
    /// CorrectPlayerMovePrediction.
    pub tick: PlayerInputTick,
}

impl UpdateAttributes {
    pub const ID: u32 = 29;
}
impl wire::Encode for UpdateAttributes {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_runtime_id.encode(writer)?;
        wire::encode_collection(writer, self.attribute_list.as_slice())?;
        self.tick.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for UpdateAttributes {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let attribute_list = wire::decode_collection::<AttributeData>(
            reader,
            crate::fields::UPDATE_ATTRIBUTES_ATTRIBUTE_LIST,
            26,
        )?;
        let tick = <PlayerInputTick as wire::Decode>::decode(reader)?;
        Ok(Self {
            target_runtime_id,
            attribute_list,
            tick,
        })
    }
}

/// InventoryTransaction is a packet sent by the client. It essentially exists out of multiple sub-packets,
/// each of which have something to do with the inventory in one way or another. Some of these sub-packets
/// directly relate to the inventory, others relate to interaction with the world, that could potentially
/// result in a change in the inventory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InventoryTransaction {
    /// `legacy_request_id` is an ID that is only non-zero at times when sent by the client. The server should
    /// always send 0 for this. When this field is not 0, the LegacySetItemSlots slice below will have values in
    /// it. LegacyRequestID ties in with the ItemStackResponse packet. If this field is non-0, the server should
    /// respond with an ItemStackResponse packet. Some inventory actions such as dropping an item out of the
    /// hotbar are still one using this packet, and the ItemStackResponse packet needs to tie in with it.
    pub legacy_request_id: ItemStackLegacyRequestID,
    /// `legacy_set_item_slots` are only present if the LegacyRequestID is non-zero. These item slots inform the
    /// server of the slots that were changed during the inventory transaction, and the server should send back an
    /// ItemStackResponse packet with these slots present in it. (Or false with no slots, if rejected.)
    /// Wire presence: optional value is preceded by a presence marker.
    pub legacy_set_item_slots: Option<Vec<LegacySetSlot>>,
    /// TransactionData is a data object that holds data specific to the type of transaction that the
    /// TransactionPacket held. Its concrete type must be one of NormalTransactionData, MismatchTransactionData
    /// UseItemTransactionData, UseItemOnEntityTransactionData or ReleaseItemTransactionData. If nil is set, the
    /// transaction will be assumed to of type InventoryTransactionTypeNormal.
    pub transaction: InventoryTransactionPacketData,
}

impl InventoryTransaction {
    pub const ID: u32 = 30;
}
impl wire::Encode for InventoryTransaction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.legacy_request_id.encode(writer)?;
        match &self.legacy_set_item_slots {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_collection(writer, value.as_slice())?;
            }
            None => writer.write_u8(0)?,
        }
        self.transaction.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InventoryTransaction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let legacy_request_id = <ItemStackLegacyRequestID as wire::Decode>::decode(reader)?;
        let legacy_set_item_slots = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(wire::decode_collection::<LegacySetSlot>(
                    reader,
                    crate::fields::INVENTORY_TRANSACTION_LEGACY_SET_ITEM_SLOTS,
                    2,
                )?)
            }
        };
        let transaction = <InventoryTransactionPacketData as wire::Decode>::decode(reader)?;
        Ok(Self {
            legacy_request_id,
            legacy_set_item_slots,
            transaction,
        })
    }
}

/// MobEquipment is sent by the client to the server and the server to the client to make the other side aware
/// of the new item that an entity is holding. It is used to show the item in the hand of entities such as
/// zombies too.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MobEquipment {
    /// EntityRuntimeID is the runtime ID of the entity. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub target_runtime_id: ActorRuntimeID,
    /// NewItem is the new item held after sending the MobEquipment packet. The entity will be shown holding that
    /// item to the player it was sent to.
    pub item: NetworkItemStackDescriptorSerializedData,
    /// InventorySlot is the slot in the inventory that was held. This is the same as HotBarSlot, and only remains
    /// for backwards compatibility.
    pub slot: wire::U8,
    /// HotBarSlot is the slot in the hot bar that was held. It is the same as InventorySlot, which is only there
    /// for backwards compatibility purposes.
    pub selected_slot: wire::U8,
    /// WindowID is the window ID of the window that had its equipped item changed. This is usually the window ID
    /// of the normal inventory, but may also be something else, for example with the off hand.
    pub container_id: wire::U8,
}

impl MobEquipment {
    pub const ID: u32 = 31;
}
impl wire::Encode for MobEquipment {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_runtime_id.encode(writer)?;
        self.item.encode(writer)?;
        self.slot.encode(writer)?;
        wire::check_number_limits(self.slot.0, Some(0), Some(255))?;
        self.selected_slot.encode(writer)?;
        wire::check_number_limits(self.selected_slot.0, Some(0), Some(255))?;
        self.container_id.encode(writer)?;
        wire::check_number_limits(self.container_id.0, Some(0), Some(255))?;
        Ok(())
    }
}

impl wire::Decode for MobEquipment {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let item = <NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?;
        let slot = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let selected_slot = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let container_id = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        Ok(Self {
            target_runtime_id,
            item,
            slot,
            selected_slot,
            container_id,
        })
    }
}

/// MobArmourEquipment is sent by the server to the client to update the armour an entity is wearing. It is
/// sent for both players and other entities, such as zombies.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MobArmorEquipment {
    /// EntityRuntimeID is the runtime ID of the entity. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub target_runtime_id: ActorRuntimeID,
    /// Helmet is the equipped helmet of the entity. Items that are not wearable on the head will not be rendered
    /// by the client. Unlike in Java Edition, blocks cannot be worn.
    pub head: NetworkItemStackDescriptorSerializedData,
    /// Chestplate is the chestplate of the entity. Items that are not wearable as chestplate will not be
    /// rendered.
    pub torso: NetworkItemStackDescriptorSerializedData,
    /// Leggings is the item worn as leggings by the entity. Items not wearable as leggings will not be rendered
    /// client-side.
    pub legs: NetworkItemStackDescriptorSerializedData,
    /// Boots is the item worn as boots by the entity. Items not wearable as boots will not be rendered.
    pub feet: NetworkItemStackDescriptorSerializedData,
    /// `body` is the item worn on the body of the entity. Items not wearable on the body will not be rendered.
    pub body: NetworkItemStackDescriptorSerializedData,
}

impl MobArmorEquipment {
    pub const ID: u32 = 32;
}
impl wire::Encode for MobArmorEquipment {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_runtime_id.encode(writer)?;
        self.head.encode(writer)?;
        self.torso.encode(writer)?;
        self.legs.encode(writer)?;
        self.feet.encode(writer)?;
        self.body.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MobArmorEquipment {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let head = <NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?;
        let torso = <NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?;
        let legs = <NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?;
        let feet = <NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?;
        let body = <NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?;
        Ok(Self {
            target_runtime_id,
            head,
            torso,
            legs,
            feet,
            body,
        })
    }
}

/// Interact is sent by the client when it interacts with another entity in some way. It used to be used for
/// normal entity and block interaction, but this is no longer the case now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Interact {
    /// `action` type is the ID of the action that was executed by the player. It is one of the constants that may
    /// be found above.
    pub action: InteractAction,
    /// TargetEntityRuntimeID is the runtime ID of the entity that the player interacted with. This is empty for
    /// the InteractActionOpenInventory action type.
    pub target_runtime_id: ActorRuntimeID,
    /// `position` associated with the ActionType above. For the InteractActionMouseOverEntity, this is the
    /// position relative to the entity moused over over which the player hovered with its mouse/touch. For the
    /// InteractActionLeaveVehicle, this is the position that the player spawns at after leaving the vehicle.
    /// Wire presence: optional value is preceded by a presence marker.
    pub position: Option<glam::Vec3>,
}

impl Interact {
    pub const ID: u32 = 33;
}
impl wire::Encode for Interact {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.action.encode(writer)?;
        self.target_runtime_id.encode(writer)?;
        match &self.position {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for Interact {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let action = <InteractAction as wire::Decode>::decode(reader)?;
        let target_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let position = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec3 as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            action,
            target_runtime_id,
            position,
        })
    }
}

/// BlockPickRequest is sent by the client when it requests to pick a block in the world and place its item in
/// their inventory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockPickRequest {
    /// `position` is the position at which the client requested to pick the block. The block at that position
    /// should have its item put in HotBarSlot if it is empty.
    pub position: BlockPos,
    /// AddBlockNBT specifies if the item should get all NBT tags from the block, meaning the item places a block
    /// practically always equal to the one picked.
    pub with_data: bool,
    /// HotBarSlot is the slot that was held at the time of picking a block.
    pub max_slots: wire::U8,
}

impl BlockPickRequest {
    pub const ID: u32 = 34;
}
impl wire::Encode for BlockPickRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.position.encode(writer)?;
        self.with_data.encode(writer)?;
        self.max_slots.encode(writer)?;
        wire::check_number_limits(self.max_slots.0, Some(0), Some(255))?;
        Ok(())
    }
}

impl wire::Decode for BlockPickRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let position = <BlockPos as wire::Decode>::decode(reader)?;
        let with_data = <bool as wire::Decode>::decode(reader)?;
        let max_slots = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        Ok(Self {
            position,
            with_data,
            max_slots,
        })
    }
}

/// ActorPickRequest is sent by the client when it tries to pick an entity, so that it gets a spawn egg which
/// can spawn that entity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActorPickRequest {
    /// EntityUniqueID is the unique ID of the entity that was attempted to be picked. The server must find the
    /// type of that entity and provide the correct spawn egg to the player.
    pub actor_id: wire::I64LE,
    /// HotBarSlot is the held hot bar slot of the player at the time of trying to pick the entity. If empty, the
    /// resulting spawn egg should be put into this slot.
    pub max_slots: wire::U8,
    /// `with_data` is true if the pick request requests the entity metadata.
    pub with_data: bool,
}

impl ActorPickRequest {
    pub const ID: u32 = 35;
}
impl wire::Encode for ActorPickRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.actor_id.encode(writer)?;
        self.max_slots.encode(writer)?;
        wire::check_number_limits(self.max_slots.0, Some(0), Some(255))?;
        self.with_data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ActorPickRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let actor_id = <wire::I64LE as wire::Decode>::decode(reader)?;
        let max_slots = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let with_data = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            actor_id,
            max_slots,
            with_data,
        })
    }
}

/// PlayerAction is sent by the client when it executes any action, for example starting to sprint, swim,
/// starting the breaking of a block, dropping an item, etc.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerAction {
    /// EntityRuntimeID is the runtime ID of the player. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub player_runtime_id: ActorRuntimeID,
    /// ActionType is the ID of the action that was executed by the player. It is one of the constants that may be
    /// found in protocol/player.go.
    pub action: PlayerActionType,
    /// `block_position` is the position of the target block, if the action with the ActionType set concerned a
    /// block. If that is not the case, the block position will be zero.
    pub block_position: BlockPos,
    /// ResultPosition is the position of the action's result. When a UseItemOn action is sent, this is the
    /// position of the block clicked, but when a block is placed, this is the position at which the block will be
    /// placed.
    pub result_pos: BlockPos,
    /// BlockFace is the face of the target block that was touched. If the action with the ActionType set
    /// concerned a block. If not, the face is always 0.
    pub face: wire::ZigZag32,
}

impl PlayerAction {
    pub const ID: u32 = 36;
}
impl wire::Encode for PlayerAction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.player_runtime_id.encode(writer)?;
        self.action.encode(writer)?;
        self.block_position.encode(writer)?;
        self.result_pos.encode(writer)?;
        self.face.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerAction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let player_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let action = <PlayerActionType as wire::Decode>::decode(reader)?;
        let block_position = <BlockPos as wire::Decode>::decode(reader)?;
        let result_pos = <BlockPos as wire::Decode>::decode(reader)?;
        let face = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self {
            player_runtime_id,
            action,
            block_position,
            result_pos,
            face,
        })
    }
}

/// HurtArmour is sent by the server to damage the player's armour after being hit. The packet should never be
/// used by servers as it hands the responsibility over to the player completely, while the server can easily
/// reliably update the armour damage of players itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HurtArmor {
    /// `cause` is the cause of the damage dealt to the armour.
    pub cause: wire::ZigZag32,
    /// `damage` is the amount of damage points that was dealt to the player. The damage to the armour will be
    /// calculated by the client based upon this damage, and will also be based upon any enchantments like thorns
    /// that the armour may have.
    pub damage: wire::ZigZag32,
    /// ArmourSlots is a bitset of all armour slots affected.
    pub armor_slots: wire::VarULong,
}

impl HurtArmor {
    pub const ID: u32 = 38;
}
impl wire::Encode for HurtArmor {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.cause.encode(writer)?;
        self.damage.encode(writer)?;
        self.armor_slots.encode(writer)?;
        wire::check_number_limits(self.armor_slots.0, Some(0), None)?;
        Ok(())
    }
}

impl wire::Decode for HurtArmor {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let cause = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let damage = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let armor_slots = {
            let value = <wire::VarULong as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        Ok(Self {
            cause,
            damage,
            armor_slots,
        })
    }
}

/// SetActorData is sent by the server to update the entity metadata of an entity. It includes flags such as
/// if the entity is on fire, but also properties such as the air it has left until it starts drowning.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetActorData {
    /// EntityRuntimeID is the runtime ID of the entity. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub target_runtime_id: ActorRuntimeID,
    /// EntityMetadata is a map of entity metadata, which includes flags and data properties that alter in
    /// particular the way the entity looks. Flags include ones such as 'on fire' and 'sprinting'. The metadata
    /// values are indexed by their property key.
    pub actor_data: SynchedActorDataCopyableDataList,
    /// EntityProperties is a list of properties that the entity inhibits. These properties define and alter
    /// specific attributes of the entity.
    pub synched_properties: PropertySyncData,
    /// `tick` is the server tick at which the packet was sent. It is used in relation to
    /// CorrectPlayerMovePrediction.
    pub tick: PlayerInputTick,
}

impl SetActorData {
    pub const ID: u32 = 39;
}
impl wire::Encode for SetActorData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_runtime_id.encode(writer)?;
        self.actor_data.encode(writer)?;
        self.synched_properties.encode(writer)?;
        self.tick.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetActorData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let actor_data = <SynchedActorDataCopyableDataList as wire::Decode>::decode(reader)?;
        let synched_properties = <PropertySyncData as wire::Decode>::decode(reader)?;
        let tick = <PlayerInputTick as wire::Decode>::decode(reader)?;
        Ok(Self {
            target_runtime_id,
            actor_data,
            synched_properties,
            tick,
        })
    }
}

/// SetActorMotion is sent by the server to change the client-side velocity of an entity. It is usually used
/// in combination with server-side movement calculation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetActorMotion {
    /// EntityRuntimeID is the runtime ID of the entity. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub target_runtime_id: ActorRuntimeID,
    /// Velocity is the new velocity the entity gets. This velocity will initiate the client-side movement of the
    /// entity.
    pub motion: glam::Vec3,
    /// `tick` is the server tick at which the packet was sent. It is used in relation to
    /// CorrectPlayerMovePrediction.
    pub tick: PlayerInputTick,
}

impl SetActorMotion {
    pub const ID: u32 = 40;
}
impl wire::Encode for SetActorMotion {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_runtime_id.encode(writer)?;
        self.motion.encode(writer)?;
        self.tick.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetActorMotion {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let motion = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let tick = <PlayerInputTick as wire::Decode>::decode(reader)?;
        Ok(Self {
            target_runtime_id,
            motion,
            tick,
        })
    }
}

/// SetActorLink is sent by the server to initiate an entity link client-side, meaning one entity will start
/// riding another.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetActorLink {
    /// `link` is the link to be set client-side. It links two entities together, so that one entity rides
    /// another. Note that players that see those entities later will not see the link, unless it is also sent in
    /// the AddActor and AddPlayer packets.
    pub link: EntityLink,
}

impl SetActorLink {
    pub const ID: u32 = 41;
}
impl wire::Encode for SetActorLink {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.link.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetActorLink {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let link = <EntityLink as wire::Decode>::decode(reader)?;
        Ok(Self { link })
    }
}

/// SetHealth is sent by the server. It sets the health of the player it is sent to. The SetHealth packet
/// should no longer be used. Instead, the health attribute should be used so that the health and maximum
/// health may be changed directly.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetHealth {
    /// `health` is the new health of the player.
    pub health: wire::ZigZag32,
}

impl SetHealth {
    pub const ID: u32 = 42;
}
impl wire::Encode for SetHealth {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.health.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetHealth {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let health = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self { health })
    }
}

/// SetSpawnPosition is sent by the server to update the spawn position of a player, for example when sleeping
/// in a bed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetSpawnPosition {
    pub spawn_position_type: SpawnPositionType,
    /// Position is the new position of the spawn that was set. If SpawnType is SpawnTypeWorld, compasses will
    /// point to this position. As of 1.16, Position is always the position of the player.
    pub block_position: BlockPos,
    pub dimension_type: DimensionType,
    pub spawn_block_pos: BlockPos,
}

impl SetSpawnPosition {
    pub const ID: u32 = 43;
}
impl wire::Encode for SetSpawnPosition {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.spawn_position_type.encode(writer)?;
        self.block_position.encode(writer)?;
        self.dimension_type.encode(writer)?;
        self.spawn_block_pos.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetSpawnPosition {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let spawn_position_type = <SpawnPositionType as wire::Decode>::decode(reader)?;
        let block_position = <BlockPos as wire::Decode>::decode(reader)?;
        let dimension_type = <DimensionType as wire::Decode>::decode(reader)?;
        let spawn_block_pos = <BlockPos as wire::Decode>::decode(reader)?;
        Ok(Self {
            spawn_position_type,
            block_position,
            dimension_type,
            spawn_block_pos,
        })
    }
}

/// Animate is sent by the server to send a player animation from one player to all viewers of that player. It
/// is used for a couple of actions, such as arm swimming and critical hits.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Animate {
    /// ActionType is the ID of the animation action to execute. It is one of the action type constants that may
    /// be found above.
    pub action: AnimateAction,
    /// EntityRuntimeID is the runtime ID of the player that the animation should be played upon. The runtime ID
    /// is unique for each world session, and entities are generally identified in packets using this runtime ID.
    pub target_actor_runtime_id: ActorRuntimeID,
    /// `data` ...
    pub data: wire::F32LE,
    /// `swing_source` is the source for swing actions. It is one of the action type constants that may be found
    /// above.
    /// Wire presence: optional value is preceded by a presence marker.
    pub swing_source: Option<wire::Str>,
}

impl Animate {
    pub const ID: u32 = 44;
}
impl wire::Encode for Animate {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.action.encode(writer)?;
        self.target_actor_runtime_id.encode(writer)?;
        self.data.encode(writer)?;
        match &self.swing_source {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for Animate {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let action = <AnimateAction as wire::Decode>::decode(reader)?;
        let target_actor_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let data = <wire::F32LE as wire::Decode>::decode(reader)?;
        let swing_source = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(reader.read_str(crate::fields::ANIMATE_SWING_SOURCE)?)
            }
        };
        Ok(Self {
            action,
            target_actor_runtime_id,
            data,
            swing_source,
        })
    }
}

/// Respawn is sent by the server to make a player respawn client-side. It is sent in response to a
/// PlayerAction packet with ActionType PlayerActionRespawn. As of 1.13, the server sends two of these packets
/// with different states, and the client sends one of these back in order to complete the respawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Respawn {
    /// `position` is the position on which the player should be respawned. The position might be in a different
    /// dimension, in which case the client should first be sent a ChangeDimension packet.
    pub position: glam::Vec3,
    /// `state` is the 'state' of the respawn. It is one of the constants that may be found above, and the value
    /// the packet contains depends on whether the server or client sends it.
    pub state: PlayerRespawnState,
    /// EntityRuntimeID is the entity runtime ID of the player that the respawn packet concerns. This is
    /// apparently for the server to recognise which player sends this packet.
    pub player_runtime_id: ActorRuntimeID,
}

impl Respawn {
    pub const ID: u32 = 45;
}
impl wire::Encode for Respawn {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.position.encode(writer)?;
        self.state.encode(writer)?;
        self.player_runtime_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for Respawn {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let state = <PlayerRespawnState as wire::Decode>::decode(reader)?;
        let player_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        Ok(Self {
            position,
            state,
            player_runtime_id,
        })
    }
}

/// ContainerOpen is sent by the server to open a container client-side. This container must be physically
/// present in the world, for the packet to have any effect. Unlike Java Edition, Bedrock Edition requires
/// that chests for example must be present and in range to open its inventory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContainerOpen {
    /// WindowID is the ID representing the window that is being opened. It may be used later to close the
    /// container using a ContainerClose packet.
    pub container_id: wire::U8,
    /// `container_type` is the type ID of the container that is being opened when opening the container at the
    /// position of the packet. It depends on the block/entity, and could, for example, be the window type of a
    /// chest or a hopper, but also a horse inventory.
    pub container_type: wire::U8,
    /// ContainerPosition is the position of the container opened. The position must point to a block entity that
    /// actually has a container. If that is not the case, the window will not be opened and the packet will be
    /// ignored, if a valid ContainerEntityUniqueID has not also been provided.
    pub position: BlockPos,
    /// ContainerEntityUniqueID is the unique ID of the entity container that was opened. It is only used if the
    /// ContainerType is one that points to an entity, for example a horse.
    pub target_actor_id: ActorUniqueID,
}

impl ContainerOpen {
    pub const ID: u32 = 46;
}
impl wire::Encode for ContainerOpen {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.container_id.encode(writer)?;
        wire::check_number_limits(self.container_id.0, Some(0), Some(255))?;
        self.container_type.encode(writer)?;
        wire::check_number_limits(self.container_type.0, Some(0), Some(255))?;
        self.position.encode(writer)?;
        self.target_actor_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ContainerOpen {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let container_id = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let container_type = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let position = <BlockPos as wire::Decode>::decode(reader)?;
        let target_actor_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        Ok(Self {
            container_id,
            container_type,
            position,
            target_actor_id,
        })
    }
}

/// ContainerClose is sent by the server to close a container the player currently has opened, which was
/// opened using the ContainerOpen packet, or by the client to tell the server it closed a particular
/// container, such as the crafting grid.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContainerClose {
    /// WindowID is the ID representing the window of the container that should be closed. It must be equal to the
    /// one sent in the ContainerOpen packet to close the designated window.
    pub container_id: wire::U8,
    /// `container_type` is the type of container that the server is trying to close. This is used to validate on
    /// the client side whether or not the server's close request is valid.
    pub container_type: wire::U8,
    /// ServerSide determines whether or not the container was force-closed by the server. If this value is not
    /// set correctly, the client may ignore the packet and respond with a PacketViolationWarning.
    pub server_initiated_close: bool,
}

impl ContainerClose {
    pub const ID: u32 = 47;
}
impl wire::Encode for ContainerClose {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.container_id.encode(writer)?;
        wire::check_number_limits(self.container_id.0, Some(0), Some(255))?;
        self.container_type.encode(writer)?;
        wire::check_number_limits(self.container_type.0, Some(0), Some(255))?;
        self.server_initiated_close.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ContainerClose {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let container_id = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let container_type = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let server_initiated_close = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            container_id,
            container_type,
            server_initiated_close,
        })
    }
}

/// PlayerHotBar is sent by the server to the client. It used to be used to link hot bar slots of the player
/// to actual slots in the inventory, but as of 1.2, this was changed and hot bar slots are no longer a free
/// floating part of the inventory. Since 1.2, the packet has been re-purposed, but its new functionality is
/// not clear.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerHotbar {
    /// SelectedHotBarSlot ...
    pub selected_slot: wire::VarUInt,
    /// WindowID ...
    pub container_id: wire::U8,
    /// SelectHotBarSlot ...
    pub should_select_slot: bool,
}

impl PlayerHotbar {
    pub const ID: u32 = 48;
}
impl wire::Encode for PlayerHotbar {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.selected_slot.encode(writer)?;
        wire::check_number_limits(self.selected_slot.0, Some(0), None)?;
        self.container_id.encode(writer)?;
        wire::check_number_limits(self.container_id.0, Some(0), Some(255))?;
        self.should_select_slot.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerHotbar {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let selected_slot = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let container_id = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let should_select_slot = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            selected_slot,
            container_id,
            should_select_slot,
        })
    }
}

/// InventoryContent is sent by the server to update the full content of a particular inventory. It is usually
/// sent for the main inventory of the player, but also works for other inventories that are currently opened
/// by the player.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InventoryContent {
    /// `container_id` is the ID that identifies one of the windows that the client currently has opened, or one
    /// of the consistent windows such as the main inventory.
    pub container_id: wire::VarUInt,
    /// `slots` is the new content of the inventory. The length of this slice must be equal to the full size of
    /// the inventory window updated.
    pub slots: Vec<NetworkItemStackDescriptorSerializedData>,
    /// `full_container_name` is the protocol.FullContainerName that describes the container that the content is
    /// for.
    pub full_container_name: FullContainerName,
    /// `storage_item` is the item that is acting as the storage container for the inventory. If the inventory is
    /// not a dynamic container then this field should be left empty. When set, only the item type is used by the
    /// client and none of the other stack info.
    pub storage_item: NetworkItemStackDescriptorSerializedData,
}

impl InventoryContent {
    pub const ID: u32 = 49;
}
impl wire::Encode for InventoryContent {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.container_id.encode(writer)?;
        wire::check_number_limits(self.container_id.0, Some(0), None)?;
        wire::encode_collection(writer, self.slots.as_slice())?;
        self.full_container_name.encode(writer)?;
        self.storage_item.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InventoryContent {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let container_id = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let slots = wire::decode_collection::<NetworkItemStackDescriptorSerializedData>(
            reader,
            crate::fields::INVENTORY_CONTENT_SLOTS,
            8,
        )?;
        let full_container_name = <FullContainerName as wire::Decode>::decode(reader)?;
        let storage_item =
            <NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?;
        Ok(Self {
            container_id,
            slots,
            full_container_name,
            storage_item,
        })
    }
}

/// InventorySlot is sent by the server to update a single slot in one of the inventory windows that the
/// client currently has opened. Usually this is the main inventory, but it may also be the off hand or, for
/// example, a chest inventory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InventorySlot {
    /// `container_id` is the ID of the window that the packet modifies. It must point to one of the windows that
    /// the client currently has opened.
    pub container_id: wire::U8,
    /// `slot` is the index of the slot that the packet modifies. The new item will be set to the slot at this
    /// index.
    pub slot: wire::VarUInt,
    /// `full_container_name` is the protocol.FullContainerName that describes the container that the content is
    /// for.
    /// Wire presence: optional value is preceded by a presence marker.
    pub full_container_name: Option<FullContainerName>,
    /// `storage_item` is the item that is acting as the storage container for the inventory. If the inventory is
    /// not a dynamic container then this field should be left empty. When set, only the item type is used by the
    /// client and none of the other stack info.
    /// Wire presence: optional value is preceded by a presence marker.
    pub storage_item: Option<NetworkItemStackDescriptorSerializedData>,
    /// `item` is the item to be put in the slot at Slot. It will overwrite any item that may currently be present
    /// in that slot.
    pub item: NetworkItemStackDescriptorSerializedData,
}

impl InventorySlot {
    pub const ID: u32 = 50;
}
impl wire::Encode for InventorySlot {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.container_id.encode(writer)?;
        wire::check_number_limits(self.container_id.0, Some(0), Some(255))?;
        self.slot.encode(writer)?;
        wire::check_number_limits(self.slot.0, Some(0), None)?;
        match &self.full_container_name {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.storage_item {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.item.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for InventorySlot {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let container_id = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let slot = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let full_container_name = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<FullContainerName as wire::Decode>::decode(reader)?)
            }
        };
        let storage_item = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?)
            }
        };
        let item = <NetworkItemStackDescriptorSerializedData as wire::Decode>::decode(reader)?;
        Ok(Self {
            container_id,
            slot,
            full_container_name,
            storage_item,
            item,
        })
    }
}

/// ContainerSetData is sent by the server to update specific data of a single container, meaning a block such
/// as a furnace or a brewing stand. This data is usually used by the client to display certain features
/// client-side.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContainerSetData {
    /// WindowID is the ID of the window that should have its data set. The player must have a window open with
    /// the window ID passed, or nothing will happen.
    pub container_id: wire::U8,
    /// Key is the key of the property. It is one of the constants that can be found above. Multiple properties
    /// share the same key, but the functionality depends on the type of the container that the data is set to.
    pub id: wire::ZigZag32,
    /// `value` is the value of the property. Its use differs per property.
    pub value: wire::ZigZag32,
}

impl ContainerSetData {
    pub const ID: u32 = 51;
}
impl wire::Encode for ContainerSetData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.container_id.encode(writer)?;
        wire::check_number_limits(self.container_id.0, Some(0), Some(255))?;
        self.id.encode(writer)?;
        self.value.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ContainerSetData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let container_id = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let value = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self {
            container_id,
            id,
            value,
        })
    }
}

/// CraftingData is sent by the server to let the client know all crafting data that the server maintains.
/// This includes shapeless crafting, crafting table recipes, furnace recipes etc. Each crafting station's
/// recipes are included in it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CraftingData {
    /// `shaped_recipes` through SmithingTrimRecipes are the typed recipe vectors used by protocol 2168.
    pub shaped_recipes: Vec<ShapedRecipe>,
    pub shapeless_recipes: Vec<ShapelessRecipe>,
    pub multi_recipes: Vec<MultiRecipe>,
    pub user_data_shapeless_recipes: Vec<ShapelessRecipe>,
    pub shapeless_chemistry_recipes: Vec<ShapelessRecipe>,
    pub shaped_chemistry_recipes: Vec<ShapedRecipe>,
    pub smithing_transform_recipes: Vec<SmithingTransformRecipe>,
    pub smithing_trim_recipes: Vec<SmithingTrimRecipe>,
    /// `potion_mixes` is a list of all potion mixing recipes which may be used in the brewing stand.
    pub potion_mixes: Vec<PotionMixDataEntry>,
    /// `container_mixes` is a list of all recipes to convert a potion from one type to another, such as from a
    /// drinkable potion to a splash potion, or from a splash potion to a lingering potion.
    pub container_mixes: Vec<ContainerMixDataEntry>,
    /// `material_reducers` is a list of all material reducers which is used in education edition chemistry.
    pub material_reducers: Vec<MaterialReducerDataEntry>,
    /// `clear_recipes` indicates if all recipes currently active on the client should be cleaned. Doing this
    /// means that the client will have no recipes active by itself: Any CraftingData packets previously sent will
    /// also be discarded, and only the recipes in this CraftingData packet will be used.
    pub clear_recipes: bool,
}

impl CraftingData {
    pub const ID: u32 = 52;
}
impl wire::Encode for CraftingData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.shaped_recipes.as_slice())?;
        wire::encode_collection(writer, self.shapeless_recipes.as_slice())?;
        wire::encode_collection(writer, self.multi_recipes.as_slice())?;
        wire::encode_collection(writer, self.user_data_shapeless_recipes.as_slice())?;
        wire::encode_collection(writer, self.shapeless_chemistry_recipes.as_slice())?;
        wire::encode_collection(writer, self.shaped_chemistry_recipes.as_slice())?;
        wire::encode_collection(writer, self.smithing_transform_recipes.as_slice())?;
        wire::encode_collection(writer, self.smithing_trim_recipes.as_slice())?;
        wire::encode_collection(writer, self.potion_mixes.as_slice())?;
        wire::encode_collection(writer, self.container_mixes.as_slice())?;
        wire::encode_collection(writer, self.material_reducers.as_slice())?;
        self.clear_recipes.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CraftingData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let shaped_recipes = wire::decode_collection::<ShapedRecipe>(
            reader,
            crate::fields::CRAFTING_DATA_SHAPED_RECIPES,
            26,
        )?;
        let shapeless_recipes = wire::decode_collection::<ShapelessRecipe>(
            reader,
            crate::fields::CRAFTING_DATA_SHAPELESS_RECIPES,
            23,
        )?;
        let multi_recipes = wire::decode_collection::<MultiRecipe>(
            reader,
            crate::fields::CRAFTING_DATA_MULTI_RECIPES,
            17,
        )?;
        let user_data_shapeless_recipes = wire::decode_collection::<ShapelessRecipe>(
            reader,
            crate::fields::CRAFTING_DATA_USER_DATA_SHAPELESS_RECIPES,
            23,
        )?;
        let shapeless_chemistry_recipes = wire::decode_collection::<ShapelessRecipe>(
            reader,
            crate::fields::CRAFTING_DATA_SHAPELESS_CHEMISTRY_RECIPES,
            23,
        )?;
        let shaped_chemistry_recipes = wire::decode_collection::<ShapedRecipe>(
            reader,
            crate::fields::CRAFTING_DATA_SHAPED_CHEMISTRY_RECIPES,
            26,
        )?;
        let smithing_transform_recipes = wire::decode_collection::<SmithingTransformRecipe>(
            reader,
            crate::fields::CRAFTING_DATA_SMITHING_TRANSFORM_RECIPES,
            18,
        )?;
        let smithing_trim_recipes = wire::decode_collection::<SmithingTrimRecipe>(
            reader,
            crate::fields::CRAFTING_DATA_SMITHING_TRIM_RECIPES,
            12,
        )?;
        let potion_mixes = wire::decode_collection::<PotionMixDataEntry>(
            reader,
            crate::fields::CRAFTING_DATA_POTION_MIXES,
            6,
        )?;
        let container_mixes = wire::decode_collection::<ContainerMixDataEntry>(
            reader,
            crate::fields::CRAFTING_DATA_CONTAINER_MIXES,
            3,
        )?;
        let material_reducers = wire::decode_collection::<MaterialReducerDataEntry>(
            reader,
            crate::fields::CRAFTING_DATA_MATERIAL_REDUCERS,
            2,
        )?;
        let clear_recipes = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            shaped_recipes,
            shapeless_recipes,
            multi_recipes,
            user_data_shapeless_recipes,
            shapeless_chemistry_recipes,
            shaped_chemistry_recipes,
            smithing_transform_recipes,
            smithing_trim_recipes,
            potion_mixes,
            container_mixes,
            material_reducers,
            clear_recipes,
        })
    }
}

/// GUIDataPickItem is sent by the server to make the client 'select' a hot bar slot. It currently appears to
/// be broken however, and does not actually set the selected slot to the hot bar slot set in the packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuiDataPickItem {
    /// `item_name` is the name of the item that shows up in the top part of the popup that shows up when
    /// selecting an item. It is shown as if an item was selected by the player itself.
    pub item_name: wire::Str,
    /// ItemEffects is the line under the ItemName, where the effects of the item are usually situated.
    pub item_effect_name: wire::Str,
    /// HotBarSlot is the hot bar slot to be selected/picked. This does not currently work, so it does not matter
    /// what number this is.
    pub slot: wire::I32LE,
}

impl GuiDataPickItem {
    pub const ID: u32 = 54;
}
impl wire::Encode for GuiDataPickItem {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.item_name.encode(writer)?;
        self.item_effect_name.encode(writer)?;
        self.slot.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for GuiDataPickItem {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let item_name = reader.read_str(crate::fields::GUI_DATA_PICK_ITEM_ITEM_NAME)?;
        let item_effect_name =
            reader.read_str(crate::fields::GUI_DATA_PICK_ITEM_ITEM_EFFECT_NAME)?;
        let slot = <wire::I32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            item_name,
            item_effect_name,
            slot,
        })
    }
}

/// BlockActorData is sent by the server to update data of a block entity client-side, for example the data of
/// a chest.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockActorData {
    /// Position is the position of the block that holds the block entity. If no block entity is at this position,
    /// the packet is ignored by the client.
    pub block_position: BlockPos,
    pub actor_data_tags: wire::NetworkNbt,
}

impl BlockActorData {
    pub const ID: u32 = 56;
}
impl wire::Encode for BlockActorData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.block_position.encode(writer)?;
        self.actor_data_tags.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BlockActorData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let block_position = <BlockPos as wire::Decode>::decode(reader)?;
        let actor_data_tags = <wire::NetworkNbt as wire::Decode>::decode(reader)?;
        Ok(Self {
            block_position,
            actor_data_tags,
        })
    }
}

/// LevelChunk is sent by the server to provide the client with a chunk of a world data (16xYx16 blocks).
/// Typically, a certain amount of chunks is sent to the client before sending it the spawn PlayStatus packet,
/// so that the client spawns in a loaded world.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelChunk {
    /// Position contains the X and Z coordinates of the chunk sent. You can convert a block coordinate to a chunk
    /// coordinate by right-shifting it four bits.
    pub chunk_position: ChunkPos,
    pub dimension_id: DimensionType,
    pub sub_chunks_count: wire::VarUInt,
    /// SubChunkLimit is the maximum amount of sub-chunks a client will request when in request mode. A value of
    /// -1 means there is no limit.
    /// Wire presence: optional value is preceded by a presence marker.
    pub client_request_sub_chunk_limit: Option<wire::ZigZag32>,
    /// `cache_enabled` specifies if the client blob cache should be enabled. This system is based on hashes of
    /// blobs which are consistent and saved by the client in combination with that blob, so that the server does
    /// not have the same chunk multiple times. If the client does not yet have a blob with the hash sent, it will
    /// send a ClientCacheBlobStatus packet containing the hashes is does not have the data of.
    pub cache_enabled: bool,
    pub cache_metadata: Vec<SubChunkMetadata>,
    pub serialized_chunk_data: bytes::Bytes,
}

impl LevelChunk {
    pub const ID: u32 = 58;
}
impl wire::Encode for LevelChunk {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.chunk_position.encode(writer)?;
        self.dimension_id.encode(writer)?;
        self.sub_chunks_count.encode(writer)?;
        wire::check_number_limits(self.sub_chunks_count.0, Some(0), Some(64))?;
        match &self.client_request_sub_chunk_limit {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
                wire::check_number_limits(value.0, Some(-1), Some(64))?;
            }
            None => writer.write_u8(0)?,
        }
        self.cache_enabled.encode(writer)?;
        wire::encode_collection_limits(writer, self.cache_metadata.as_slice(), 0, 65)?;
        self.serialized_chunk_data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LevelChunk {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let chunk_position = <ChunkPos as wire::Decode>::decode(reader)?;
        let dimension_id = <DimensionType as wire::Decode>::decode(reader)?;
        let sub_chunks_count = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(64))?;
            value
        };
        let client_request_sub_chunk_limit = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some({
                    let value = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
                    wire::validate_number_limits(value.0, Some(-1), Some(64))?;
                    value
                })
            }
        };
        let cache_enabled = <bool as wire::Decode>::decode(reader)?;
        let cache_metadata = wire::decode_collection_limits::<SubChunkMetadata>(
            reader,
            crate::fields::LEVEL_CHUNK_CACHE_METADATA,
            8,
            0,
            65,
        )?;
        let serialized_chunk_data =
            reader.read_shared_bytes(crate::fields::LEVEL_CHUNK_SERIALIZED_CHUNK_DATA)?;
        Ok(Self {
            chunk_position,
            dimension_id,
            sub_chunks_count,
            client_request_sub_chunk_limit,
            cache_enabled,
            cache_metadata,
            serialized_chunk_data,
        })
    }
}

/// SetCommandsEnabled is sent by the server to enable or disable the ability to execute commands for the
/// client. If disabled, the client itself will stop the execution of commands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetCommandsEnabled {
    /// `commands_enabled` defines if the commands should be enabled, or if false, disabled.
    pub commands_enabled: bool,
}

impl SetCommandsEnabled {
    pub const ID: u32 = 59;
}
impl wire::Encode for SetCommandsEnabled {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.commands_enabled.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetCommandsEnabled {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let commands_enabled = <bool as wire::Decode>::decode(reader)?;
        Ok(Self { commands_enabled })
    }
}

/// SetDifficulty is sent by the server to update the client-side difficulty of the client. The actual effect
/// of this packet on the client isn't very significant, as the difficulty is handled server-side.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetDifficulty {
    /// `difficulty` is the new difficulty that the world has.
    pub difficulty: wire::VarUInt,
}

impl SetDifficulty {
    pub const ID: u32 = 60;
}
impl wire::Encode for SetDifficulty {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.difficulty.encode(writer)?;
        wire::check_number_limits(self.difficulty.0, Some(0), None)?;
        Ok(())
    }
}

impl wire::Decode for SetDifficulty {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let difficulty = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        Ok(Self { difficulty })
    }
}

/// ChangeDimension is sent by the server to the client to send a dimension change screen client-side. Once
/// the screen is cleared client-side, the client will send a PlayerAction packet with
/// PlayerActionDimensionChangeDone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChangeDimension {
    pub dimension_id: DimensionType,
    /// `position` is the position in the new dimension that the player is spawned in.
    pub position: glam::Vec3,
    /// `respawn` specifies if the dimension change was respawn based, meaning that the player died in one
    /// dimension and got respawned into another. The client will send a PlayerAction packet with
    /// PlayerActionDimensionChangeRequest if it dies in another dimension, indicating that it needs a
    /// DimensionChange packet with Respawn set to true.
    pub respawn: bool,
    /// `loading_screen_id` is a unique ID for the loading screen that the player is currently in. The client will
    /// update the server on its state through the ServerBoundLoadingScreen packet, and it can be used to not send
    /// specific packets to the client if it is changing dimensions. This field should be unique for every
    /// ChangeDimension packet sent.
    /// Wire presence: optional value is preceded by a presence marker.
    pub loading_screen_id: Option<wire::U32LE>,
}

impl ChangeDimension {
    pub const ID: u32 = 61;
}
impl wire::Encode for ChangeDimension {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.dimension_id.encode(writer)?;
        self.position.encode(writer)?;
        self.respawn.encode(writer)?;
        match &self.loading_screen_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ChangeDimension {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let dimension_id = <DimensionType as wire::Decode>::decode(reader)?;
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let respawn = <bool as wire::Decode>::decode(reader)?;
        let loading_screen_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::U32LE as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            dimension_id,
            position,
            respawn,
            loading_screen_id,
        })
    }
}

/// SetPlayerGameType is sent by the server to update the game type, which is otherwise known as the game
/// mode, of a player.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetPlayerGameType {
    /// GameType is the new game type of the player. It is one of the constants that can be found above. Some of
    /// these game types require additional flags to be set in an AdventureSettings packet for the game mode to
    /// obtain its full functionality.
    pub player_game_type: GameType,
}

impl SetPlayerGameType {
    pub const ID: u32 = 62;
}
impl wire::Encode for SetPlayerGameType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.player_game_type.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetPlayerGameType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let player_game_type = <GameType as wire::Decode>::decode(reader)?;
        Ok(Self { player_game_type })
    }
}

/// PlayerList is sent by the server to update the client-side player list in the in-game menu screen. It
/// shows the icon of each player if the correct XUID is written in the packet. Sending the PlayerList packet
/// is obligatory when sending an AddPlayer packet. The added player will not show up to a client if it has
/// not been added to the player list, because several properties of the player are obtained from the player
/// list, such as the skin.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerList {
    /// `entries` is a list of all player list entries that should be added/removed from the player list,
    /// depending on the ActionType set.
    pub entries: Vec<PlayerListData>,
}

impl PlayerList {
    pub const ID: u32 = 63;
}
impl wire::Encode for PlayerList {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection_limits(writer, self.entries.as_slice(), 0, 1000)?;
        Ok(())
    }
}

impl wire::Decode for PlayerList {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let entries = wire::decode_collection_limits::<PlayerListData>(
            reader,
            crate::fields::PLAYER_LIST_ENTRIES,
            18,
            0,
            1000,
        )?;
        Ok(Self { entries })
    }
}

/// SimpleEvent is used for enabling or disabling commands and for unlocking world template settings (both
/// unlocking UI buttons on client and the actual setting on the server). This is fired from the client to the
/// server and a SetCommandsEnabled is sent back when enabling commands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SimpleEvent {
    /// EventType is the type of the event to be called. It is one of the constants that may be found above.
    pub type_: Subtype,
}

impl SimpleEvent {
    pub const ID: u32 = 64;
}
impl wire::Encode for SimpleEvent {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.type_.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SimpleEvent {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let type_ = <Subtype as wire::Decode>::decode(reader)?;
        Ok(Self { type_ })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LegacyTelemetryEvent {
    pub target_actor_id: ActorUniqueID,
    pub event_type: LegacyTelemetryType,
    pub use_player_id: bool,
    pub event_data: EventData,
}

impl LegacyTelemetryEvent {
    pub const ID: u32 = 65;
}
impl wire::Encode for LegacyTelemetryEvent {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_actor_id.encode(writer)?;
        self.event_type.encode(writer)?;
        self.use_player_id.encode(writer)?;
        self.event_data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LegacyTelemetryEvent {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_actor_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let event_type = <LegacyTelemetryType as wire::Decode>::decode(reader)?;
        let use_player_id = <bool as wire::Decode>::decode(reader)?;
        let event_data = <EventData as wire::Decode>::decode(reader)?;
        Ok(Self {
            target_actor_id,
            event_type,
            use_player_id,
            event_data,
        })
    }
}

/// SpawnExperienceOrb is sent by the server to spawn an experience orb entity client-side. Much like the
/// AddPainting packet, it is one of the few packets that spawn an entity without using the AddActor packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpawnExperienceOrb {
    /// `position` is the position to spawn the experience orb on. If the entity is on a distance that the player
    /// cannot see it, the entity will still show up if the player moves closer.
    pub position: glam::Vec3,
    /// `xp_value` is the amount of experience in experience points that the orb carries. The client-side size of
    /// the orb depends on the amount of experience in the orb: There are 11 possible sizes for the orb, for
    /// 1–2, 3–6, 7–16, 17–36, 37–72, 73–148, 149–306, 307–616, 617–1236, 1237–2476, and 2477
    /// and up.
    pub xp_value: wire::ZigZag32,
}

impl SpawnExperienceOrb {
    pub const ID: u32 = 66;
}
impl wire::Encode for SpawnExperienceOrb {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.position.encode(writer)?;
        self.xp_value.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SpawnExperienceOrb {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let xp_value = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self { position, xp_value })
    }
}

/// ClientBoundMapItemData is sent by the server to the client to update the data of a map shown to the
/// client. It is sent with a combination of flags that specify what data is updated. The
/// ClientBoundMapItemData packet may be used to update specific parts of the map only. It is not required to
/// send the entire map each time when updating one part.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientboundMapItemData {
    /// `map_id` is the unique identifier that represents the map that is updated over network. It remains
    /// consistent across sessions.
    pub map_id: ActorUniqueID,
    /// `dimension` is the dimension of the map that should be updated, for example the overworld (0), the nether
    /// (1) or the end (2).
    pub dimension: wire::U8,
    /// LockedMap specifies if the map that was updated was a locked map, which may be done using a cartography
    /// table.
    pub is_locked: bool,
    /// Origin is the center position of the map being updated.
    pub map_origin: BlockPos,
    /// MapsIncludedIn holds an array of map IDs that the map updated is included in. This has to do with the
    /// scale of the map: Each map holds its own map ID and all map IDs of maps that include this map and have a
    /// bigger scale. This means that a scale 0 map will have 5 map IDs in this slice, whereas a scale 4 map will
    /// have only 1 (its own). The actual use of this field remains unknown.
    /// Wire presence: optional value is preceded by a presence marker.
    pub creation_map_ids: Option<Vec<ActorUniqueID>>,
    /// `scale` is the scale of the map as it is shown in-game.
    /// Wire presence: optional value is preceded by a presence marker.
    pub scale: Option<wire::I8>,
    /// TrackedObjects is a list of tracked objects on the map, which may either be entities or blocks. The client
    /// makes sure these tracked objects are actually tracked. (position updated etc.)
    /// Wire presence: optional value is preceded by a presence marker.
    pub tracked_actor_ids: Option<Vec<MapItemTrackedActorUniqueId>>,
    /// `decorations` is a list of fixed decorations located on the map. The decorations will not change
    /// client-side, unless the server updates them.
    /// Wire presence: optional value is preceded by a presence marker.
    pub decorations: Option<Vec<MapDecoration>>,
    /// `width` is the width of the texture area that was updated. The width may be a subset of the total width of
    /// the map.
    /// Wire presence: optional value is preceded by a presence marker.
    pub width: Option<wire::ZigZag32>,
    /// `height` is the height of the texture area that was updated. The height may be a subset of the total
    /// height of the map.
    /// Wire presence: optional value is preceded by a presence marker.
    pub height: Option<wire::ZigZag32>,
    /// XOffset is the X offset in pixels at which the updated texture area starts. From this X, the updated
    /// texture will extend exactly Width pixels to the right.
    /// Wire presence: optional value is preceded by a presence marker.
    pub start_x: Option<wire::ZigZag32>,
    /// YOffset is the Y offset in pixels at which the updated texture area starts. From this Y, the updated
    /// texture will extend exactly Height pixels up.
    /// Wire presence: optional value is preceded by a presence marker.
    pub start_y: Option<wire::ZigZag32>,
    /// `pixels` is a list of pixel colours for the new texture of the map. It is indexed as Pixels[y*height + x].
    /// Wire presence: optional value is preceded by a presence marker.
    pub pixels: Option<Vec<wire::U32LE>>,
}

impl ClientboundMapItemData {
    pub const ID: u32 = 67;
}
impl wire::Encode for ClientboundMapItemData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.map_id.encode(writer)?;
        self.dimension.encode(writer)?;
        wire::check_number_limits(self.dimension.0, Some(0), Some(255))?;
        self.is_locked.encode(writer)?;
        self.map_origin.encode(writer)?;
        match &self.creation_map_ids {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_collection_limits(writer, value.as_slice(), 0, 65535)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.scale {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.tracked_actor_ids {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_collection_limits(writer, value.as_slice(), 0, 65535)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.decorations {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_collection_limits(writer, value.as_slice(), 0, 65535)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.width {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.height {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.start_x {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.start_y {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.pixels {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_collection_limits(writer, value.as_slice(), 0, 16384)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ClientboundMapItemData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let map_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let dimension = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let is_locked = <bool as wire::Decode>::decode(reader)?;
        let map_origin = <BlockPos as wire::Decode>::decode(reader)?;
        let creation_map_ids = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(wire::decode_collection_limits::<ActorUniqueID>(
                    reader,
                    crate::fields::CLIENTBOUND_MAP_ITEM_DATA_CREATION_MAP_IDS,
                    1,
                    0,
                    65535,
                )?)
            }
        };
        let scale = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::I8 as wire::Decode>::decode(reader)?)
            }
        };
        let tracked_actor_ids = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(
                    wire::decode_collection_limits::<MapItemTrackedActorUniqueId>(
                        reader,
                        crate::fields::CLIENTBOUND_MAP_ITEM_DATA_TRACKED_ACTOR_IDS,
                        6,
                        0,
                        65535,
                    )?,
                )
            }
        };
        let decorations = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(wire::decode_collection_limits::<MapDecoration>(
                    reader,
                    crate::fields::CLIENTBOUND_MAP_ITEM_DATA_DECORATIONS,
                    9,
                    0,
                    65535,
                )?)
            }
        };
        let width = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::ZigZag32 as wire::Decode>::decode(reader)?)
            }
        };
        let height = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::ZigZag32 as wire::Decode>::decode(reader)?)
            }
        };
        let start_x = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::ZigZag32 as wire::Decode>::decode(reader)?)
            }
        };
        let start_y = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::ZigZag32 as wire::Decode>::decode(reader)?)
            }
        };
        let pixels = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(wire::decode_collection_limits::<wire::U32LE>(
                    reader,
                    crate::fields::CLIENTBOUND_MAP_ITEM_DATA_PIXELS,
                    4,
                    0,
                    16384,
                )?)
            }
        };
        Ok(Self {
            map_id,
            dimension,
            is_locked,
            map_origin,
            creation_map_ids,
            scale,
            tracked_actor_ids,
            decorations,
            width,
            height,
            start_x,
            start_y,
            pixels,
        })
    }
}

/// MapInfoRequest is sent by the client to request the server to deliver information of a certain map in the
/// inventory of the player. The server should respond with a ClientBoundMapItemData packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapInfoRequest {
    /// MapID is the unique identifier that represents the map that is requested over network. It remains
    /// consistent across sessions.
    pub map_unique_id: ActorUniqueID,
    /// ClientPixels is a slice of pixels sent from the client to notify the server about the pixels that it isn't
    /// aware of.
    pub client_pixels_list: Vec<PixelRequest>,
}

impl MapInfoRequest {
    pub const ID: u32 = 68;
}
impl wire::Encode for MapInfoRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.map_unique_id.encode(writer)?;
        wire::check_length(self.client_pixels_list.len(), 0, 16384)?;
        wire::encode_collection_u32le(writer, self.client_pixels_list.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for MapInfoRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let map_unique_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let client_pixels_list = wire::decode_collection_u32le::<PixelRequest>(
            reader,
            crate::fields::MAP_INFO_REQUEST_CLIENT_PIXELS_LIST,
            6,
        )?;
        Ok(Self {
            map_unique_id,
            client_pixels_list,
        })
    }
}

/// RequestChunkRadius is sent by the client to the server to update the server on the chunk view radius that
/// it has set in the settings. The server may respond with a ChunkRadiusUpdated packet with either the chunk
/// radius requested, or a different chunk radius if the server chooses so.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RequestChunkRadius {
    /// `chunk_radius` is the requested chunk radius. This value is always the value set in the settings of the
    /// player.
    pub chunk_radius: wire::ZigZag32,
    /// `max_chunk_radius` is the maximum chunk radius that the player wants to receive. The reason for the client
    /// sending this is currently unknown.
    pub max_chunk_radius: wire::U8,
}

impl RequestChunkRadius {
    pub const ID: u32 = 69;
}
impl wire::Encode for RequestChunkRadius {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.chunk_radius.encode(writer)?;
        self.max_chunk_radius.encode(writer)?;
        wire::check_number_limits(self.max_chunk_radius.0, Some(0), Some(255))?;
        Ok(())
    }
}

impl wire::Decode for RequestChunkRadius {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let chunk_radius = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let max_chunk_radius = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        Ok(Self {
            chunk_radius,
            max_chunk_radius,
        })
    }
}

/// ChunkRadiusUpdated is sent by the server in response to a RequestChunkRadius packet. It defines the chunk
/// radius that the server allows the client to have. This may be lower than the chunk radius requested by the
/// client in the RequestChunkRadius packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChunkRadiusUpdated {
    /// `chunk_radius` is the final chunk radius that the client will adapt when it receives the packet. It does
    /// not have to be the same as the requested chunk radius.
    pub chunk_radius: wire::ZigZag32,
}

impl ChunkRadiusUpdated {
    pub const ID: u32 = 70;
}
impl wire::Encode for ChunkRadiusUpdated {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.chunk_radius.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ChunkRadiusUpdated {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let chunk_radius = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self { chunk_radius })
    }
}

/// GameRulesChanged is sent by the server to the client to update client-side game rules, such as game rules
/// like the 'showCoordinates' game rule.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GameRulesChanged {
    pub rule_data: GameRulesChangedData,
}

impl GameRulesChanged {
    pub const ID: u32 = 72;
}
impl wire::Encode for GameRulesChanged {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.rule_data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for GameRulesChanged {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let rule_data = <GameRulesChangedData as wire::Decode>::decode(reader)?;
        Ok(Self { rule_data })
    }
}

/// Camera is sent by the server to use an Education Edition camera on a player. It produces an image
/// client-side.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Camera {
    /// CameraEntityUniqueID is the unique ID of the camera entity from which the picture was taken.
    pub camera_id: ActorUniqueID,
    /// TargetPlayerUniqueID is the unique ID of the target player. The unique ID is a value that remains
    /// consistent across different sessions of the same world, but most servers simply fill the runtime ID of the
    /// player out for this field.
    pub target_player_id: ActorUniqueID,
}

impl Camera {
    pub const ID: u32 = 73;
}
impl wire::Encode for Camera {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.camera_id.encode(writer)?;
        self.target_player_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for Camera {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let camera_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let target_player_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        Ok(Self {
            camera_id,
            target_player_id,
        })
    }
}

/// BossEvent is sent by the server to make a specific 'boss event' occur in the world. It includes features
/// such as showing a boss bar to the player and turning the sky dark.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BossEvent {
    /// BossEntityUniqueID is the unique ID of the boss entity that the boss event sent involves. By default, the
    /// health percentage and title of the boss bar depend on the health and name tag of this entity. If
    /// BossEntityUniqueID is the same as the client's entity unique ID, its HealthPercentage and BossBarTitle can
    /// be freely altered.
    pub target_actor_id: ActorUniqueID,
    /// `event_type` is the type of the event. It is one of the BossEvent constants above.
    pub event_type: BossEventUpdateType,
    /// BossBarTitle is the title shown above the boss bar. It may be set to set a different title if the
    /// BossEntityUniqueID matches the client's entity unique ID.
    pub name: wire::Str,
    /// FilteredBossBarTitle is a filtered version of BossBarTitle with all the profanity removed. The client will
    /// use this over BossBarTitle if this field is not empty and they have the "Filter Profanity" setting
    /// enabled.
    pub filtered_name: wire::Str,
    /// HealthPercentage is the percentage of health that is shown in the boss bar (0.0-1.0). The HealthPercentage
    /// may be set to a specific value if the BossEntityUniqueID matches the client's entity unique ID.
    pub health_percent: wire::F32LE,
    /// Colour is the colour of the boss bar that is shown when a player is subscribed. It is one of the
    /// BossEventColour constants listed above.
    pub color: BossBarColor,
    /// `overlay` is the overlay of the boss bar that is shown on top of the boss bar when a player is subscribed.
    /// It is one of the BossEventOverlay constants listed above.
    pub overlay: BossBarOverlay,
}

impl BossEvent {
    pub const ID: u32 = 74;
}
impl wire::Encode for BossEvent {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_actor_id.encode(writer)?;
        self.event_type.encode(writer)?;
        wire::encode_string_limits(writer, &self.name, 0, 256)?;
        wire::encode_string_limits(writer, &self.filtered_name, 0, 256)?;
        self.health_percent.encode(writer)?;
        self.color.encode(writer)?;
        self.overlay.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BossEvent {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_actor_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let event_type = <BossEventUpdateType as wire::Decode>::decode(reader)?;
        let name = wire::decode_string_limits(reader, crate::fields::BOSS_EVENT_NAME, 0, 256)?;
        let filtered_name =
            wire::decode_string_limits(reader, crate::fields::BOSS_EVENT_FILTERED_NAME, 0, 256)?;
        let health_percent = <wire::F32LE as wire::Decode>::decode(reader)?;
        let color = <BossBarColor as wire::Decode>::decode(reader)?;
        let overlay = <BossBarOverlay as wire::Decode>::decode(reader)?;
        Ok(Self {
            target_actor_id,
            event_type,
            name,
            filtered_name,
            health_percent,
            color,
            overlay,
        })
    }
}

/// ShowCredits is sent by the server to show the Minecraft credits screen to the client. It is typically sent
/// when the player beats the ender dragon and leaves the End.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShowCredits {
    /// `player_runtime_id` is the entity runtime ID of the player to show the credits to. It's not clear why this
    /// field is actually here in the first place.
    pub player_runtime_id: ActorRuntimeID,
    /// StatusType is the status type of the credits. It is one of the constants above, and either starts or stops
    /// the credits.
    pub credits_state: wire::ZigZag32,
}

impl ShowCredits {
    pub const ID: u32 = 75;
}
impl wire::Encode for ShowCredits {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.player_runtime_id.encode(writer)?;
        self.credits_state.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ShowCredits {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let player_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let credits_state = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self {
            player_runtime_id,
            credits_state,
        })
    }
}

/// AvailableCommands is sent by the server to send a list of all commands that the player is able to use on
/// the server. This packet holds all the arguments of each commands as well, making it possible for the
/// client to provide auto-completion and command usages. AvailableCommands packets can be resent, but the
/// packet is often very big, so doing this very often should be avoided.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AvailableCommands {
    /// `enum_values` is a slice of all enum values of any enum in the AvailableCommands packet. EnumValues
    /// generally should contain each possible value only once. Enums are built by pointing to entries in this
    /// slice.
    pub enum_values: Vec<wire::Str>,
    /// `chained_subcommand_values` is a slice of all chained subcommand names. ChainedSubcommandValues generally
    /// should contain each possible value only once. ChainedSubcommands are built by pointing to entries in this
    /// slice.
    pub chained_subcommand_values: Vec<wire::Str>,
    /// `post_fixes`, like EnumValues, is a slice of all suffix values of any command parameter in the
    /// AvailableCommands packet.
    pub post_fixes: Vec<wire::Str>,
    /// `enum_data` is a slice of all (fixed) command enums present in any of the commands.
    pub enum_data: Vec<CommandEnum>,
    /// `chained_subcommand_data` is a slice of all subcommands that are followed by a chained command. An example
    /// usage of this is /execute which allows you to run another command as another entity or at a different
    /// position etc.
    pub chained_subcommand_data: Vec<ChainedSubcommand>,
    /// `commands` is a list of all commands that the client should show client-side. The AvailableCommands packet
    /// replaces any commands sent before. It does not only add the commands that are sent in it.
    pub commands: Vec<Command>,
    /// `soft_enums` is a slice of dynamic command enums. These command enums can be changed during runtime
    /// without having to resend an AvailableCommands packet.
    pub soft_enums: Vec<DynamicEnum>,
    /// `constraints` is a list of constraints that should be applied to certain options of enums in the commands
    /// above.
    pub constraints: Vec<CommandEnumConstraint>,
}

impl AvailableCommands {
    pub const ID: u32 = 76;
}
impl wire::Encode for AvailableCommands {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        writer.write_len_var_u32(self.enum_values.len())?;
        for item in self.enum_values.iter() {
            item.encode(writer)?;
        }
        writer.write_len_var_u32(self.chained_subcommand_values.len())?;
        for item in self.chained_subcommand_values.iter() {
            item.encode(writer)?;
        }
        writer.write_len_var_u32(self.post_fixes.len())?;
        for item in self.post_fixes.iter() {
            item.encode(writer)?;
        }
        wire::encode_collection(writer, self.enum_data.as_slice())?;
        wire::encode_collection_limits(writer, self.chained_subcommand_data.as_slice(), 0, 16)?;
        wire::encode_collection(writer, self.commands.as_slice())?;
        wire::encode_collection(writer, self.soft_enums.as_slice())?;
        wire::encode_collection(writer, self.constraints.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for AvailableCommands {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let enum_values = {
            let declared = u64::from(reader.read_var_u32()?);
            let count =
                reader.checked_count(crate::fields::AVAILABLE_COMMANDS_ENUM_VALUES, declared, 1)?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    reader.read_str(crate::fields::AVAILABLE_COMMANDS_ENUM_VALUES_ITEM)?,
                )?;
            }
            out
        };
        let chained_subcommand_values = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::AVAILABLE_COMMANDS_CHAINED_SUBCOMMAND_VALUES,
                declared,
                1,
            )?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    reader.read_str(
                        crate::fields::AVAILABLE_COMMANDS_CHAINED_SUBCOMMAND_VALUES_ITEM,
                    )?,
                )?;
            }
            out
        };
        let post_fixes = {
            let declared = u64::from(reader.read_var_u32()?);
            let count =
                reader.checked_count(crate::fields::AVAILABLE_COMMANDS_POST_FIXES, declared, 1)?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    reader.read_str(crate::fields::AVAILABLE_COMMANDS_POST_FIXES_ITEM)?,
                )?;
            }
            out
        };
        let enum_data = wire::decode_collection::<CommandEnum>(
            reader,
            crate::fields::AVAILABLE_COMMANDS_ENUM_DATA,
            2,
        )?;
        let chained_subcommand_data = wire::decode_collection_limits::<ChainedSubcommand>(
            reader,
            crate::fields::AVAILABLE_COMMANDS_CHAINED_SUBCOMMAND_DATA,
            2,
            0,
            16,
        )?;
        let commands = wire::decode_collection::<Command>(
            reader,
            crate::fields::AVAILABLE_COMMANDS_COMMANDS,
            11,
        )?;
        let soft_enums = wire::decode_collection::<DynamicEnum>(
            reader,
            crate::fields::AVAILABLE_COMMANDS_SOFT_ENUMS,
            2,
        )?;
        let constraints = wire::decode_collection::<CommandEnumConstraint>(
            reader,
            crate::fields::AVAILABLE_COMMANDS_CONSTRAINTS,
            9,
        )?;
        Ok(Self {
            enum_values,
            chained_subcommand_values,
            post_fixes,
            enum_data,
            chained_subcommand_data,
            commands,
            soft_enums,
            constraints,
        })
    }
}

/// CommandRequest is sent by the client to request the execution of a server-side command. Although some
/// servers support sending commands using the Text packet, this packet is guaranteed to have the correct
/// result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandRequest {
    /// `command` is the raw entered command line. The client does no parsing of the command line by itself
    /// (unlike it did in the early stages), but lets the server do that.
    pub command: wire::Str,
    /// `origin` is the data specifying the origin of the command. In other words, the source that the command was
    /// from, such as the player itself or a websocket server.
    pub origin: CommandOriginData,
    /// `is_internal` specifies if the command request internal. Setting it to false seems to work and the usage
    /// of this field is not known.
    pub is_internal: bool,
    /// `version` is the version of the command that is being executed. This field currently has no purpose or
    /// functionality.
    pub version: wire::Str,
}

impl CommandRequest {
    pub const ID: u32 = 77;
}
impl wire::Encode for CommandRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_string_limits(writer, &self.command, 0, 1000)?;
        self.origin.encode(writer)?;
        self.is_internal.encode(writer)?;
        self.version.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CommandRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let command =
            wire::decode_string_limits(reader, crate::fields::COMMAND_REQUEST_COMMAND, 0, 1000)?;
        let origin = <CommandOriginData as wire::Decode>::decode(reader)?;
        let is_internal = <bool as wire::Decode>::decode(reader)?;
        let version = reader.read_str(crate::fields::COMMAND_REQUEST_VERSION)?;
        Ok(Self {
            command,
            origin,
            is_internal,
            version,
        })
    }
}

/// CommandBlockUpdate is sent by the client to update a command block at a specific position. The command
/// block may be either a physical block or an entity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandBlockUpdate {
    pub target: CommandBlockUpdateData,
    /// `command` is the command currently entered in the command block. This is the command that is executed when
    /// the command block is activated.
    pub command: wire::Str,
    /// `last_output` is the output of the last command executed by the command block. It may be left empty to
    /// show simply no output at all, in combination with setting ShouldTrackOutput to false.
    pub last_output: wire::Str,
    /// `name` is the name of the command block updated. If not empty, it will show this name hovering above the
    /// command block when hovering over the block with the cursor.
    pub name: wire::Str,
    /// `filtered_name` is a filtered version of Name with all the profanity removed. The client will use this
    /// over Name if this field is not empty and they have the "Filter Profanity" setting enabled.
    pub filtered_name: wire::Str,
    /// ShouldTrackOutput specifies if the command block tracks output. If set to false, the output box won't be
    /// shown within the command block.
    pub track_output: bool,
    /// `tick_delay` is the delay in ticks between executions of a command block, if it is a repeating command
    /// block.
    pub tick_delay: wire::I32LE,
    /// `execute_on_first_tick` specifies if the command block should execute on the first tick, AKA as soon as
    /// the command block is enabled.
    pub execute_on_first_tick: bool,
}

impl CommandBlockUpdate {
    pub const ID: u32 = 78;
}
impl wire::Encode for CommandBlockUpdate {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target.encode(writer)?;
        self.command.encode(writer)?;
        self.last_output.encode(writer)?;
        self.name.encode(writer)?;
        self.filtered_name.encode(writer)?;
        self.track_output.encode(writer)?;
        self.tick_delay.encode(writer)?;
        self.execute_on_first_tick.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CommandBlockUpdate {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target = <CommandBlockUpdateData as wire::Decode>::decode(reader)?;
        let command = reader.read_str(crate::fields::COMMAND_BLOCK_UPDATE_COMMAND)?;
        let last_output = reader.read_str(crate::fields::COMMAND_BLOCK_UPDATE_LAST_OUTPUT)?;
        let name = reader.read_str(crate::fields::COMMAND_BLOCK_UPDATE_NAME)?;
        let filtered_name = reader.read_str(crate::fields::COMMAND_BLOCK_UPDATE_FILTERED_NAME)?;
        let track_output = <bool as wire::Decode>::decode(reader)?;
        let tick_delay = <wire::I32LE as wire::Decode>::decode(reader)?;
        let execute_on_first_tick = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            target,
            command,
            last_output,
            name,
            filtered_name,
            track_output,
            tick_delay,
            execute_on_first_tick,
        })
    }
}

/// CommandOutput is sent by the server to the client to send text as output of a command. Most servers do not
/// use this packet and instead simply send Text packets, but there is reason to send it. If the origin of a
/// CommandRequest packet is not the player itself, but, for example, a websocket server, sending a Text
/// packet will not do what is expected: The message should go to the websocket server, not to the client's
/// chat. The CommandOutput packet will make sure the messages are relayed to the correct origin of the
/// command request.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandOutput {
    pub origin_data: CommandOriginData,
    pub output: CommandOutputData,
}

impl CommandOutput {
    pub const ID: u32 = 79;
}
impl wire::Encode for CommandOutput {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.origin_data.encode(writer)?;
        self.output.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CommandOutput {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let origin_data = <CommandOriginData as wire::Decode>::decode(reader)?;
        let output = <CommandOutputData as wire::Decode>::decode(reader)?;
        Ok(Self {
            origin_data,
            output,
        })
    }
}

/// UpdateTrade is sent by the server to update the trades offered by a villager to a player. It is sent at
/// the moment that a player interacts with a villager.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateTrade {
    pub container_id: wire::U8,
    /// WindowType is an identifier specifying the type of the window opened. In vanilla, it appears this is
    /// always filled out with 15.
    pub type_: wire::U8,
    /// `size` is the amount of trading options that the villager has.
    pub size: wire::ZigZag32,
    pub trader_tier: wire::ZigZag32,
    /// `entity_unique_id` is the unique ID of the entity (usually a player) for which the trades are updated. The
    /// updated trades may apply only to this entity.
    pub entity_unique_id: ActorUniqueID,
    /// EntityUniqueID is the unique ID of the entity (usually a player) for which the trades are updated. The
    /// updated trades may apply only to this entity.
    pub last_trading_player: ActorUniqueID,
    /// `display_name` is the name displayed at the top of the trading UI. It is usually used to represent the
    /// profession of the villager in the UI.
    pub display_name: wire::Str,
    pub use_new_trade_screen: bool,
    pub using_economy_trade: bool,
    pub data: wire::NetworkNbt,
}

impl UpdateTrade {
    pub const ID: u32 = 80;
}
impl wire::Encode for UpdateTrade {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.container_id.encode(writer)?;
        wire::check_number_limits(self.container_id.0, Some(0), Some(255))?;
        self.type_.encode(writer)?;
        wire::check_number_limits(self.type_.0, Some(0), Some(255))?;
        self.size.encode(writer)?;
        self.trader_tier.encode(writer)?;
        self.entity_unique_id.encode(writer)?;
        self.last_trading_player.encode(writer)?;
        self.display_name.encode(writer)?;
        self.use_new_trade_screen.encode(writer)?;
        self.using_economy_trade.encode(writer)?;
        self.data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for UpdateTrade {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let container_id = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let type_ = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let size = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let trader_tier = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let entity_unique_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let last_trading_player = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let display_name = reader.read_str(crate::fields::UPDATE_TRADE_DISPLAY_NAME)?;
        let use_new_trade_screen = <bool as wire::Decode>::decode(reader)?;
        let using_economy_trade = <bool as wire::Decode>::decode(reader)?;
        let data = <wire::NetworkNbt as wire::Decode>::decode(reader)?;
        Ok(Self {
            container_id,
            type_,
            size,
            trader_tier,
            entity_unique_id,
            last_trading_player,
            display_name,
            use_new_trade_screen,
            using_economy_trade,
            data,
        })
    }
}

/// UpdateEquip is sent by the server to the client upon opening a horse inventory. It is used to set the
/// content of the inventory and specify additional properties, such as the items that are allowed to be put
/// in slots of the inventory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateEquip {
    pub container_id: wire::U8,
    /// WindowType is the type of the window that was opened. Generally, this is the type of a horse inventory, as
    /// the packet is specifically made for that.
    pub type_: wire::U8,
    /// `size` is the size of the horse inventory that should be opened. A bigger size does, in fact, change the
    /// amount of slots displayed.
    pub size: wire::ZigZag32,
    /// `entity_unique_id` is the unique ID of the entity whose equipment was 'updated' to the player. It is
    /// typically the horse entity that had its inventory opened.
    pub entity_unique_id: ActorUniqueID,
    pub data: wire::NetworkNbt,
}

impl UpdateEquip {
    pub const ID: u32 = 81;
}
impl wire::Encode for UpdateEquip {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.container_id.encode(writer)?;
        wire::check_number_limits(self.container_id.0, Some(0), Some(255))?;
        self.type_.encode(writer)?;
        wire::check_number_limits(self.type_.0, Some(0), Some(255))?;
        self.size.encode(writer)?;
        self.entity_unique_id.encode(writer)?;
        self.data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for UpdateEquip {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let container_id = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let type_ = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let size = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let entity_unique_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let data = <wire::NetworkNbt as wire::Decode>::decode(reader)?;
        Ok(Self {
            container_id,
            type_,
            size,
            entity_unique_id,
            data,
        })
    }
}

/// ResourcePackDataInfo is sent by the server to the client to inform the client about the data contained in
/// one of the resource packs that are about to be sent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourcePackDataInfo {
    /// UUID is the unique ID of the resource pack that the info concerns.
    pub resource_name: wire::Str,
    /// DataChunkSize is the maximum size in bytes of the chunks in which the total size of the resource pack to
    /// be sent will be divided. A size of 1MB (1024*1024) means that a resource pack of 15.5MB will be split into
    /// 16 data chunks.
    pub chunk_size: wire::U32LE,
    /// ChunkCount is the total amount of data chunks that the sent resource pack will exist out of. It is the
    /// total size of the resource pack divided by the DataChunkSize field. The client doesn't actually seem to
    /// use this field. Rather, it divides the size by the chunk size to calculate it itself.
    pub number_of_chunks: wire::U32LE,
    /// Size is the total size in bytes that the resource pack occupies. This is the size of the compressed
    /// archive (zip) of the resource pack.
    pub file_size: wire::U64LE,
    /// Hash is a SHA256 hash of the content of the resource pack.
    pub file_hash: bytes::Bytes,
    /// Premium specifies if the resource pack was a premium resource pack, meaning it was bought from the
    /// Minecraft store.
    pub is_premium_pack: bool,
    /// `pack_type` is the type of the resource pack. It is one of the resource pack types that may be found in
    /// the constants above.
    pub pack_type: wire::U8,
}

impl ResourcePackDataInfo {
    pub const ID: u32 = 82;
}
impl wire::Encode for ResourcePackDataInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.resource_name.encode(writer)?;
        self.chunk_size.encode(writer)?;
        wire::check_number_limits(self.chunk_size.0, Some(0), None)?;
        self.number_of_chunks.encode(writer)?;
        wire::check_number_limits(self.number_of_chunks.0, Some(0), None)?;
        self.file_size.encode(writer)?;
        wire::check_number_limits(self.file_size.0, Some(0), None)?;
        self.file_hash.encode(writer)?;
        self.is_premium_pack.encode(writer)?;
        self.pack_type.encode(writer)?;
        wire::check_number_limits(self.pack_type.0, Some(0), Some(255))?;
        Ok(())
    }
}

impl wire::Decode for ResourcePackDataInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let resource_name =
            reader.read_str(crate::fields::RESOURCE_PACK_DATA_INFO_RESOURCE_NAME)?;
        let chunk_size = {
            let value = <wire::U32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let number_of_chunks = {
            let value = <wire::U32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let file_size = {
            let value = <wire::U64LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let file_hash =
            reader.read_shared_bytes(crate::fields::RESOURCE_PACK_DATA_INFO_FILE_HASH)?;
        let is_premium_pack = <bool as wire::Decode>::decode(reader)?;
        let pack_type = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        Ok(Self {
            resource_name,
            chunk_size,
            number_of_chunks,
            file_size,
            file_hash,
            is_premium_pack,
            pack_type,
        })
    }
}

/// ResourcePackChunkData is sent to the client so that the client can download the resource pack. Each packet
/// holds a chunk of the compressed resource pack, of which the size is defined in the ResourcePackDataInfo
/// packet sent before.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourcePackChunkData {
    /// `resource_name` is the unique ID of the resource pack that the chunk of data is taken out of.
    pub resource_name: wire::Str,
    /// `chunk_id` is the current chunk index of the chunk. It is a number that starts at 0 and is incremented for
    /// each resource pack data chunk sent to the client.
    pub chunk_id: wire::U32LE,
    /// `byte_offset` is the current progress in bytes or offset in the data that the resource pack data chunk is
    /// taken from.
    pub byte_offset: wire::U64LE,
    /// RawPayload is a byte slice containing a chunk of data from the resource pack. It must be of the same size
    /// or less than the DataChunkSize set in the ResourcePackDataInfo packet.
    pub chunk_data: bytes::Bytes,
}

impl ResourcePackChunkData {
    pub const ID: u32 = 83;
}
impl wire::Encode for ResourcePackChunkData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.resource_name.encode(writer)?;
        self.chunk_id.encode(writer)?;
        wire::check_number_limits(self.chunk_id.0, Some(0), None)?;
        self.byte_offset.encode(writer)?;
        wire::check_number_limits(self.byte_offset.0, Some(0), None)?;
        self.chunk_data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ResourcePackChunkData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let resource_name =
            reader.read_str(crate::fields::RESOURCE_PACK_CHUNK_DATA_RESOURCE_NAME)?;
        let chunk_id = {
            let value = <wire::U32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let byte_offset = {
            let value = <wire::U64LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let chunk_data =
            reader.read_shared_bytes(crate::fields::RESOURCE_PACK_CHUNK_DATA_CHUNK_DATA)?;
        Ok(Self {
            resource_name,
            chunk_id,
            byte_offset,
            chunk_data,
        })
    }
}

/// ResourcePackChunkRequest is sent by the client to request a chunk of data from a particular resource pack,
/// that it has obtained information about in a ResourcePackDataInfo packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourcePackChunkRequest {
    /// `resource_name` is the unique ID of the resource pack that the chunk of data is requested from.
    pub resource_name: wire::Str,
    /// `chunk` is the requested chunk index of the chunk. It is a number that starts at 0 and is incremented for
    /// each resource pack data chunk requested.
    pub chunk: wire::I32LE,
}

impl ResourcePackChunkRequest {
    pub const ID: u32 = 84;
}
impl wire::Encode for ResourcePackChunkRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.resource_name.encode(writer)?;
        self.chunk.encode(writer)?;
        wire::check_number_limits(self.chunk.0, Some(0), None)?;
        Ok(())
    }
}

impl wire::Decode for ResourcePackChunkRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let resource_name =
            reader.read_str(crate::fields::RESOURCE_PACK_CHUNK_REQUEST_RESOURCE_NAME)?;
        let chunk = {
            let value = <wire::I32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        Ok(Self {
            resource_name,
            chunk,
        })
    }
}

/// Transfer is sent by the server to transfer a player from the current server to another. Doing so will
/// fully disconnect the client, bring it back to the main menu and make it connect to the next server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transfer {
    /// `server_address` is the address of the new server, which might be either a hostname or an actual IP
    /// address.
    pub server_address: wire::Str,
    /// `server_port` is the UDP port of the new server.
    pub server_port: wire::U16LE,
    /// `reload_world` currently has an unknown usage.
    pub reload_world: bool,
    /// `gatherings_configuration` optionally identifies the gathering being joined on the target server.
    /// Wire presence: optional value is preceded by a presence marker.
    pub gatherings_configuration: Option<ServerConfigurationGatheringsConfigurationJoinInfo>,
}

impl Transfer {
    pub const ID: u32 = 85;
}
impl wire::Encode for Transfer {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.server_address.encode(writer)?;
        self.server_port.encode(writer)?;
        wire::check_number_limits(self.server_port.0, Some(0), None)?;
        self.reload_world.encode(writer)?;
        match &self.gatherings_configuration {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for Transfer {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let server_address = reader.read_str(crate::fields::TRANSFER_SERVER_ADDRESS)?;
        let server_port = {
            let value = <wire::U16LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let reload_world = <bool as wire::Decode>::decode(reader)?;
        let gatherings_configuration = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(
                    <ServerConfigurationGatheringsConfigurationJoinInfo as wire::Decode>::decode(
                        reader,
                    )?,
                )
            }
        };
        Ok(Self {
            server_address,
            server_port,
            reload_world,
            gatherings_configuration,
        })
    }
}

/// PlaySound is sent by the server to play a sound to the client. Some of the sounds may only be started
/// using this packet and must be stopped using the StopSound packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaySound {
    /// `name` is the name of the sound to play.
    pub name: wire::Str,
    /// `position` is the position at which the sound was played. Some sounds do not depend on a position, which
    /// will then ignore it, but most of them will play with the direction based on the position compared to the
    /// player's position.
    pub position: BlockPos,
    /// `volume` is the relative volume of the sound to play. It will be less loud for the player if it is farther
    /// away from the position of the sound.
    pub volume: wire::F32LE,
    /// `pitch` is the pitch of the sound to play. Some sounds completely ignore this field, whereas others use it
    /// to specify the pitch as the field is intended.
    pub pitch: wire::F32LE,
    /// `loop_count` is the number of times to loop the sound before stopping. -1 means no looping at all.
    pub loop_count: wire::ZigZag32,
    /// `bypass_listener_range_check` specifies if the sound should be played regardless of how far away the
    /// player is from the position of the sound.
    pub bypass_listener_range_check: bool,
    /// `server_sound_handle` is an optional sound handle ID. It is currently unknown what this is for, and is not
    /// required to be set by servers.
    /// Wire presence: optional value is preceded by a presence marker.
    pub server_sound_handle: Option<ServerSoundHandle>,
    /// `playback_position_seconds` is an optional offset, in seconds, into the sound at which playback should
    /// start. If not set, the sound is played from the start.
    /// Wire presence: optional value is preceded by a presence marker.
    pub playback_position_seconds: Option<wire::F32LE>,
}

impl PlaySound {
    pub const ID: u32 = 86;
}
impl wire::Encode for PlaySound {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.name.encode(writer)?;
        self.position.encode(writer)?;
        self.volume.encode(writer)?;
        self.pitch.encode(writer)?;
        self.loop_count.encode(writer)?;
        self.bypass_listener_range_check.encode(writer)?;
        match &self.server_sound_handle {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.playback_position_seconds {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for PlaySound {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let name = reader.read_str(crate::fields::PLAY_SOUND_NAME)?;
        let position = <BlockPos as wire::Decode>::decode(reader)?;
        let volume = <wire::F32LE as wire::Decode>::decode(reader)?;
        let pitch = <wire::F32LE as wire::Decode>::decode(reader)?;
        let loop_count = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let bypass_listener_range_check = <bool as wire::Decode>::decode(reader)?;
        let server_sound_handle = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<ServerSoundHandle as wire::Decode>::decode(reader)?)
            }
        };
        let playback_position_seconds = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            name,
            position,
            volume,
            pitch,
            loop_count,
            bypass_listener_range_check,
            server_sound_handle,
            playback_position_seconds,
        })
    }
}

/// StopSound is sent by the server to stop a sound playing to the player, such as a playing music disk track
/// or other long-lasting sounds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StopSound {
    /// `sound_name` is the name of the sound that should be stopped from playing. If no sound with this name is
    /// currently active, the packet is ignored.
    pub sound_name: wire::Str,
    /// `stop_all_sounds` specifies if all sounds currently playing to the player should be stopped. If set to
    /// true, the SoundName field may be left empty.
    pub stop_all_sounds: bool,
    /// `stop_music_legacy` is currently unknown.
    pub stop_music_legacy: bool,
}

impl StopSound {
    pub const ID: u32 = 87;
}
impl wire::Encode for StopSound {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.sound_name.encode(writer)?;
        self.stop_all_sounds.encode(writer)?;
        self.stop_music_legacy.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StopSound {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let sound_name = reader.read_str(crate::fields::STOP_SOUND_SOUND_NAME)?;
        let stop_all_sounds = <bool as wire::Decode>::decode(reader)?;
        let stop_music_legacy = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            sound_name,
            stop_all_sounds,
            stop_music_legacy,
        })
    }
}

/// SetTitle is sent by the server to make a title, subtitle or action bar shown to a player. It has several
/// fields that allow setting the duration of the titles.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetTitle {
    /// ActionType is the type of the action that should be executed upon the title of a player. It is one of the
    /// constants above and specifies the response of the client to the packet.
    pub title_type: TitleType,
    /// Text is the text of the title, which has a different meaning depending on the ActionType that the packet
    /// has. The text is the text of a title, subtitle or action bar, depending on the type set.
    pub title_text: wire::Str,
    /// FadeInDuration is the duration that the title takes to fade in on the screen of the player. It is measured
    /// in 20ths of a second (AKA in ticks).
    pub fade_in_time: wire::ZigZag32,
    /// RemainDuration is the duration that the title remains on the screen of the player. It is measured in 20ths
    /// of a second (AKA in ticks).
    pub stay_time: wire::ZigZag32,
    /// FadeOutDuration is the duration that the title takes to fade out of the screen of the player. It is
    /// measured in 20ths of a second (AKA in ticks).
    pub fade_out_time: wire::ZigZag32,
    /// `xuid` is the XBOX Live user ID of the player, which will remain consistent as long as the player is
    /// logged in with the XBOX Live account. It is empty if the user is not logged into its XBL account.
    pub xuid: wire::Str,
    /// `platform_online_id` is either a uint64 or an empty string.
    pub platform_online_id: wire::Str,
    /// FilteredMessage is a filtered version of Message with all the profanity removed. The client will use this
    /// over Message if this field is not empty and they have the "Filter Profanity" setting enabled.
    pub filtered_title_message: wire::Str,
}

impl SetTitle {
    pub const ID: u32 = 88;
}
impl wire::Encode for SetTitle {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.title_type.encode(writer)?;
        self.title_text.encode(writer)?;
        self.fade_in_time.encode(writer)?;
        self.stay_time.encode(writer)?;
        self.fade_out_time.encode(writer)?;
        self.xuid.encode(writer)?;
        self.platform_online_id.encode(writer)?;
        self.filtered_title_message.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetTitle {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let title_type = <TitleType as wire::Decode>::decode(reader)?;
        let title_text = reader.read_str(crate::fields::SET_TITLE_TITLE_TEXT)?;
        let fade_in_time = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let stay_time = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let fade_out_time = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let xuid = reader.read_str(crate::fields::SET_TITLE_XUID)?;
        let platform_online_id = reader.read_str(crate::fields::SET_TITLE_PLATFORM_ONLINE_ID)?;
        let filtered_title_message =
            reader.read_str(crate::fields::SET_TITLE_FILTERED_TITLE_MESSAGE)?;
        Ok(Self {
            title_type,
            title_text,
            fade_in_time,
            stay_time,
            fade_out_time,
            xuid,
            platform_online_id,
            filtered_title_message,
        })
    }
}

/// AddBehaviourTree is sent by the server to the client. The packet is currently unused by both client and
/// server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AddBehaviorTree {
    /// BehaviourTree is an unused string.
    pub behavior_tree_structure_json: wire::Str,
}

impl AddBehaviorTree {
    pub const ID: u32 = 89;
}
impl wire::Encode for AddBehaviorTree {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.behavior_tree_structure_json.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AddBehaviorTree {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let behavior_tree_structure_json =
            reader.read_str(crate::fields::ADD_BEHAVIOR_TREE_BEHAVIOR_TREE_STRUCTURE_JSON)?;
        Ok(Self {
            behavior_tree_structure_json,
        })
    }
}

/// StructureBlockUpdate is sent by the client when it updates a structure block using the in-game UI. The
/// data it contains depends on the type of structure block that it is. In Minecraft Bedrock Edition v1.11,
/// there is only the Export structure block type, but in v1.13 the ones present in Java Edition will,
/// according to the wiki, be added too.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StructureBlockUpdate {
    /// Position is the position of the structure block that is updated.
    pub block_position: BlockPos,
    pub structure_data: StructureEditorData,
    /// ShouldTrigger specifies if the structure block should be triggered immediately after this packet reaches
    /// the server.
    pub trigger: bool,
    /// Waterlogged specifies if non-air blocks replace water or combine with water.
    pub is_waterlogged: bool,
}

impl StructureBlockUpdate {
    pub const ID: u32 = 90;
}
impl wire::Encode for StructureBlockUpdate {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.block_position.encode(writer)?;
        self.structure_data.encode(writer)?;
        self.trigger.encode(writer)?;
        self.is_waterlogged.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StructureBlockUpdate {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let block_position = <BlockPos as wire::Decode>::decode(reader)?;
        let structure_data = <StructureEditorData as wire::Decode>::decode(reader)?;
        let trigger = <bool as wire::Decode>::decode(reader)?;
        let is_waterlogged = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            block_position,
            structure_data,
            trigger,
            is_waterlogged,
        })
    }
}

/// ShowStoreOffer is sent by the server to show a Marketplace store offer to a player. It opens a window
/// client-side that displays the item. The ShowStoreOffer packet only works on the partnered servers: Servers
/// that are not partnered will not have a store buttons show up in the in-game pause menu and will, as a
/// result, not be able to open store offers on the client side. Sending the packet does therefore not work
/// when using a proxy that is not connected to with the domain of one of the partnered servers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShowStoreOffer {
    /// `offer_id` is a UUID that identifies the offer for which a window should be opened.
    pub offer_id: uuid::Uuid,
    /// Type is the type of the store offer that is being shown to the player. It is one of the constants that may
    /// be found above.
    pub redirect_type: ShowStoreOfferRedirectType,
}

impl ShowStoreOffer {
    pub const ID: u32 = 91;
}
impl wire::Encode for ShowStoreOffer {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.offer_id.encode(writer)?;
        self.redirect_type.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ShowStoreOffer {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let offer_id = <uuid::Uuid as wire::Decode>::decode(reader)?;
        let redirect_type = <ShowStoreOfferRedirectType as wire::Decode>::decode(reader)?;
        Ok(Self {
            offer_id,
            redirect_type,
        })
    }
}

/// PurchaseReceipt is sent by the client to the server to notify the server it purchased an item from the
/// Marketplace store that was offered by the server. The packet is only used for partnered servers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PurchaseReceipt {
    /// `purchase_receipts` is a list of receipts, or proofs of purchases, for the offers that have been purchased
    /// by the player.
    pub purchase_receipts: Vec<wire::Str>,
}

impl PurchaseReceipt {
    pub const ID: u32 = 92;
}
impl wire::Encode for PurchaseReceipt {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::check_length(self.purchase_receipts.len(), 0, 10000)?;
        writer.write_len_var_u32(self.purchase_receipts.len())?;
        for item in self.purchase_receipts.iter() {
            item.encode(writer)?;
        }
        Ok(())
    }
}

impl wire::Decode for PurchaseReceipt {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let purchase_receipts = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count_limits(
                crate::fields::PURCHASE_RECEIPT_PURCHASE_RECEIPTS,
                declared,
                1,
                0,
                10000,
            )?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    reader.read_str(crate::fields::PURCHASE_RECEIPT_PURCHASE_RECEIPTS_ITEM)?,
                )?;
            }
            out
        };
        Ok(Self { purchase_receipts })
    }
}

/// PlayerSkin is sent by the client to the server when it updates its own skin using the in-game skin picker.
/// It is relayed by the server, or sent if the server changes the skin of a player on its own accord. Note
/// that the packet can only be sent for players that are in the player list at the time of sending.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerSkin {
    /// `uuid` is the UUID of the player as sent in the Login packet when the client joined the server. It must
    /// match this UUID exactly for the skin to show up on the player.
    pub uuid: uuid::Uuid,
    /// Skin is the new skin to be applied on the player with the UUID in the field above. The skin, including its
    /// animations, will be shown after sending it.
    pub serialized_skin: SerializedSkinRef,
    /// NewSkinName no longer has a function: The field can be left empty at all times.
    pub localized_new_skin_name: wire::Str,
    /// OldSkinName no longer has a function: The field can be left empty at all times.
    pub localized_old_skin_name: wire::Str,
}

impl PlayerSkin {
    pub const ID: u32 = 93;
}
impl wire::Encode for PlayerSkin {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.uuid.encode(writer)?;
        self.serialized_skin.encode(writer)?;
        self.localized_new_skin_name.encode(writer)?;
        self.localized_old_skin_name.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerSkin {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let uuid = <uuid::Uuid as wire::Decode>::decode(reader)?;
        let serialized_skin = <SerializedSkinRef as wire::Decode>::decode(reader)?;
        let localized_new_skin_name =
            reader.read_str(crate::fields::PLAYER_SKIN_LOCALIZED_NEW_SKIN_NAME)?;
        let localized_old_skin_name =
            reader.read_str(crate::fields::PLAYER_SKIN_LOCALIZED_OLD_SKIN_NAME)?;
        Ok(Self {
            uuid,
            serialized_skin,
            localized_new_skin_name,
            localized_old_skin_name,
        })
    }
}

/// SubClientLogin is sent when a sub-client joins the server while another client is already connected to it.
/// The packet is sent as a result of split-screen game play, and allows up to four players to play using the
/// same network connection. After an initial Login packet from the 'main' client, each sub-client that
/// connects sends a SubClientLogin to request their own login.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubClientLogin {
    /// ConnectionRequest is a string containing information about the player and JWTs that may be used to verify
    /// if the player is connected to XBOX Live. The connection request also contains the necessary client public
    /// key to initiate encryption. The ConnectionRequest in this packet is identical to the one found in the
    /// Login packet.
    pub sub_client_connection_request: bytes::Bytes,
}

impl SubClientLogin {
    pub const ID: u32 = 94;
}
impl wire::Encode for SubClientLogin {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.sub_client_connection_request.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SubClientLogin {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let sub_client_connection_request = reader
            .read_shared_bytes(crate::fields::SUB_CLIENT_LOGIN_SUB_CLIENT_CONNECTION_REQUEST)?;
        Ok(Self {
            sub_client_connection_request,
        })
    }
}

/// AutomationClientConnect is used to make the client connect to a websocket server. This websocket server
/// has the ability to execute commands on the behalf of the client and it can listen for certain events fired
/// by the client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AutomationClientConnect {
    pub web_socket_data: WebSocketData,
}

impl AutomationClientConnect {
    pub const ID: u32 = 95;
}
impl wire::Encode for AutomationClientConnect {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.web_socket_data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AutomationClientConnect {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let web_socket_data = <WebSocketData as wire::Decode>::decode(reader)?;
        Ok(Self { web_socket_data })
    }
}

/// SetLastHurtBy is sent by the server to let the client know what entity type it was last hurt by. At this
/// moment, the packet is useless and should not be used. There is no behaviour that depends on if this packet
/// is sent or not.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetLastHurtBy {
    /// EntityType is the numerical type of the entity that the player was last hurt by.
    pub last_hurt_by: ActorType,
}

impl SetLastHurtBy {
    pub const ID: u32 = 96;
}
impl wire::Encode for SetLastHurtBy {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.last_hurt_by.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetLastHurtBy {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let last_hurt_by = <ActorType as wire::Decode>::decode(reader)?;
        Ok(Self { last_hurt_by })
    }
}

/// BookEdit is sent by the client when it edits a book. It is sent each time a modification was made and the
/// player stops its typing 'session', rather than simply after closing the book.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BookEdit {
    pub book_slot: wire::ZigZag32,
    pub operation: BookEditAction,
}

impl BookEdit {
    pub const ID: u32 = 97;
}
impl wire::Encode for BookEdit {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.book_slot.encode(writer)?;
        self.operation.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BookEdit {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let book_slot = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let operation = <BookEditAction as wire::Decode>::decode(reader)?;
        Ok(Self {
            book_slot,
            operation,
        })
    }
}

/// NPCRequest is sent by the client when it interacts with an NPC. The packet is specifically made for
/// Education Edition, where NPCs are available to use.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NpcRequest {
    /// EntityRuntimeID is the runtime ID of the NPC entity that the player interacted with. It is the same as
    /// sent by the server when spawning the entity.
    pub npc_runtime_id: ActorRuntimeID,
    /// `request_type` is the type of the request, which depends on the permission that the player has. It will be
    /// either a type that indicates that the NPC should show its dialog, or that it should open the editing
    /// window.
    pub request_type: RequestType,
    /// CommandString is the command string set in the NPC. It may consist of multiple commands, depending on what
    /// the player set in it.
    pub actions: wire::Str,
    /// ActionType is the type of the action to execute.
    pub action_index: wire::U8,
    /// `scene_name` is the name of the scene. This can be left empty to specify the last scene that the player
    /// was sent.
    pub scene_name: wire::Str,
}

impl NpcRequest {
    pub const ID: u32 = 98;
}
impl wire::Encode for NpcRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.npc_runtime_id.encode(writer)?;
        self.request_type.encode(writer)?;
        self.actions.encode(writer)?;
        self.action_index.encode(writer)?;
        wire::check_number_limits(self.action_index.0, Some(0), Some(255))?;
        self.scene_name.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for NpcRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let npc_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let request_type = <RequestType as wire::Decode>::decode(reader)?;
        let actions = reader.read_str(crate::fields::NPC_REQUEST_ACTIONS)?;
        let action_index = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let scene_name = reader.read_str(crate::fields::NPC_REQUEST_SCENE_NAME)?;
        Ok(Self {
            npc_runtime_id,
            request_type,
            actions,
            action_index,
            scene_name,
        })
    }
}

/// PhotoTransfer is sent by the server to transfer a photo (image) file to the client. It is typically used
/// to transfer photos so that the client can display it in a portfolio in Education Edition. While previously
/// usable in the default Bedrock Edition, the displaying of photos in books was disabled and the packet now
/// has little use anymore.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhotoTransfer {
    /// `photo_name` is the name of the photo to transfer. It is the exact file name that the client will download
    /// the photo as, including the extension of the file.
    pub photo_name: wire::Str,
    /// `photo_data` is the raw data of the photo image. The format of this data may vary: Formats such as JPEG or
    /// PNG work, as long as PhotoName has the correct extension.
    pub photo_data: bytes::Bytes,
    /// `book_id` is the ID of the book that the photo is associated with. If the PhotoName in a book with this ID
    /// is set to PhotoName, it will display the photo (provided Education Edition is used). The photo image is
    /// downloaded to a sub-folder with this book ID.
    pub book_id: wire::Str,
    /// `type_` is one of the three photo types above.
    pub type_: PhotoType,
    /// `source_type` is the source photo type. It is one of the three photo types above.
    pub source_type: PhotoType,
    /// `owner_id` is the entity unique ID of the photo's owner.
    pub owner_id: wire::I64LE,
    /// `new_photo_name` is the new name of the photo.
    pub new_photo_name: wire::Str,
}

impl PhotoTransfer {
    pub const ID: u32 = 99;
}
impl wire::Encode for PhotoTransfer {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.photo_name.encode(writer)?;
        wire::check_pattern(
            &self.photo_name,
            "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\\.jpeg$",
        )?;
        wire::encode_bytes_limits(writer, self.photo_data.as_ref(), 0, 20971520)?;
        self.book_id.encode(writer)?;
        self.type_.encode(writer)?;
        self.source_type.encode(writer)?;
        self.owner_id.encode(writer)?;
        self.new_photo_name.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PhotoTransfer {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let photo_name = {
            let value = reader.read_str(crate::fields::PHOTO_TRANSFER_PHOTO_NAME)?;
            wire::validate_pattern(
                &value,
                "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\\.jpeg$",
            )?;
            value
        };
        let photo_data = wire::decode_bytes_limits(
            reader,
            crate::fields::PHOTO_TRANSFER_PHOTO_DATA,
            0,
            20971520,
        )?;
        let book_id = reader.read_str(crate::fields::PHOTO_TRANSFER_BOOK_ID)?;
        let type_ = <PhotoType as wire::Decode>::decode(reader)?;
        let source_type = <PhotoType as wire::Decode>::decode(reader)?;
        let owner_id = <wire::I64LE as wire::Decode>::decode(reader)?;
        let new_photo_name = reader.read_str(crate::fields::PHOTO_TRANSFER_NEW_PHOTO_NAME)?;
        Ok(Self {
            photo_name,
            photo_data,
            book_id,
            type_,
            source_type,
            owner_id,
            new_photo_name,
        })
    }
}

/// ModalFormRequest is sent by the server to make the client open a form. This form may be either a modal
/// form which has two options, a menu form for a selection of options and a custom form for properties.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModalFormRequest {
    /// `form_id` is an ID used to identify the form. The ID is saved by the client and sent back when the player
    /// submits the form, so that the server can identify which form was submitted.
    pub form_id: wire::VarUInt,
    pub form_ui_json: wire::Str,
}

impl ModalFormRequest {
    pub const ID: u32 = 100;
}
impl wire::Encode for ModalFormRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.form_id.encode(writer)?;
        wire::check_number_limits(self.form_id.0, Some(0), None)?;
        self.form_ui_json.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ModalFormRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let form_id = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let form_ui_json = reader.read_str(crate::fields::MODAL_FORM_REQUEST_FORM_UI_JSON)?;
        Ok(Self {
            form_id,
            form_ui_json,
        })
    }
}

/// ModalFormResponse is sent by the client in response to a ModalFormRequest, after the player has submitted
/// the form sent. It contains the options/properties selected by the player, or a JSON encoded 'null' if the
/// form was closed by clicking the X at the top right corner of the form.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModalFormResponse {
    /// `form_id` is the form ID of the form the client has responded to. It is the same as the ID sent in the
    /// ModalFormRequest, and may be used to identify which form was submitted.
    pub form_id: wire::VarUInt,
    /// `json_response` is a JSON encoded value representing the response of the player. For a modal form, the
    /// response is either true or false, for a menu form, the response is an integer specifying the index of the
    /// button clicked, and for a custom form, the response is an array containing a value for each element.
    /// Wire presence: optional value is preceded by a presence marker.
    pub json_response: Option<wire::Str>,
    /// `form_cancel_reason` represents the reason why the form was cancelled. It is one of the constants above.
    /// Wire presence: optional value is preceded by a presence marker.
    pub form_cancel_reason: Option<ModalFormCancelReason>,
}

impl ModalFormResponse {
    pub const ID: u32 = 101;
}
impl wire::Encode for ModalFormResponse {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.form_id.encode(writer)?;
        wire::check_number_limits(self.form_id.0, Some(0), None)?;
        match &self.json_response {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.form_cancel_reason {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ModalFormResponse {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let form_id = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let json_response = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(reader.read_str(crate::fields::MODAL_FORM_RESPONSE_JSON_RESPONSE)?)
            }
        };
        let form_cancel_reason = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<ModalFormCancelReason as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            form_id,
            json_response,
            form_cancel_reason,
        })
    }
}

/// ServerSettingsRequest is sent by the client to request the settings specific to the server. These settings
/// are shown in a separate tab client-side, and have the same structure as a custom form.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerSettingsRequest {}

impl ServerSettingsRequest {
    pub const ID: u32 = 102;
}
impl wire::Encode for ServerSettingsRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        let _ = writer;
        Ok(())
    }
}

impl wire::Decode for ServerSettingsRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let _ = reader;
        Ok(Self {})
    }
}

/// ServerSettingsResponse is optionally sent by the server in response to a ServerSettingsRequest from the
/// client. It is structured the same as a ModalFormRequest packet, and if filled out correctly, will show a
/// specific tab for the server in the settings of the client. A ModalFormResponse packet is sent by the
/// client in response to a ServerSettingsResponse, when the client fills out the settings and closes the
/// settings again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerSettingsResponse {
    /// `form_id` is an ID used to identify the form. The ID is saved by the client and sent back when the player
    /// submits the form, so that the server can identify which form was submitted.
    pub form_id: wire::VarUInt,
    pub form_ui_json: wire::Str,
}

impl ServerSettingsResponse {
    pub const ID: u32 = 103;
}
impl wire::Encode for ServerSettingsResponse {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.form_id.encode(writer)?;
        wire::check_number_limits(self.form_id.0, Some(0), None)?;
        self.form_ui_json.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerSettingsResponse {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let form_id = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let form_ui_json = reader.read_str(crate::fields::SERVER_SETTINGS_RESPONSE_FORM_UI_JSON)?;
        Ok(Self {
            form_id,
            form_ui_json,
        })
    }
}

/// ShowProfile is sent by the server to show the XBOX Live profile of one player to another.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShowProfile {
    /// `player_xuid` is the XBOX Live User ID of the player whose profile should be shown to the player. If it is
    /// not a valid XUID, the client ignores the packet.
    pub player_xuid: wire::Str,
}

impl ShowProfile {
    pub const ID: u32 = 104;
}
impl wire::Encode for ShowProfile {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.player_xuid.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ShowProfile {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let player_xuid = reader.read_str(crate::fields::SHOW_PROFILE_PLAYER_XUID)?;
        Ok(Self { player_xuid })
    }
}

/// SetDefaultGameType is sent by the client when it toggles the default game type in the settings UI, and is
/// sent by the server when it actually changes the default game type, resulting in the toggle being changed
/// in the settings UI.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetDefaultGameType {
    /// GameType is the new game type that is set. When sent by the client, this is the requested new default game
    /// type.
    pub default_game_type: GameType,
}

impl SetDefaultGameType {
    pub const ID: u32 = 105;
}
impl wire::Encode for SetDefaultGameType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.default_game_type.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetDefaultGameType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let default_game_type = <GameType as wire::Decode>::decode(reader)?;
        Ok(Self { default_game_type })
    }
}

/// RemoveObjective is sent by the server to remove a scoreboard objective. It is used to stop showing a
/// scoreboard to a player.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RemoveObjective {
    /// `objective_name` is the name of the objective that the scoreboard currently active has. This name must be
    /// identical to the one sent in the SetDisplayObjective packet.
    pub objective_name: wire::Str,
}

impl RemoveObjective {
    pub const ID: u32 = 106;
}
impl wire::Encode for RemoveObjective {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.objective_name.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RemoveObjective {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let objective_name = reader.read_str(crate::fields::REMOVE_OBJECTIVE_OBJECTIVE_NAME)?;
        Ok(Self { objective_name })
    }
}

/// SetDisplayObjective is sent by the server to display an object as a scoreboard to the player. Once sent,
/// it should be followed up by a SetScore packet to set the lines of the packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetDisplayObjective {
    /// `display_slot_name` is the slot in which the scoreboard should be displayed. Available options can be
    /// found in the constants above.
    pub display_slot_name: wire::Str,
    /// `objective_name` is the name of the objective that the scoreboard displays. Filling out a random unique
    /// value for this field works: It is not displayed in the scoreboard.
    pub objective_name: wire::Str,
    /// `objective_display_name` is the name, or title, that is displayed at the top of the scoreboard.
    pub objective_display_name: wire::Str,
    /// `criteria_name` is the name of the criteria that need to be fulfilled in order for the score to be
    /// increased. This can be any kind of string and does not show up client-side.
    pub criteria_name: wire::Str,
    /// `sort_order` is the order in which entries on the scoreboard should be sorted. It is one of the constants
    /// that may be found above.
    pub sort_order: wire::ZigZag32,
}

impl SetDisplayObjective {
    pub const ID: u32 = 107;
}
impl wire::Encode for SetDisplayObjective {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.display_slot_name.encode(writer)?;
        self.objective_name.encode(writer)?;
        self.objective_display_name.encode(writer)?;
        self.criteria_name.encode(writer)?;
        self.sort_order.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetDisplayObjective {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let display_slot_name =
            reader.read_str(crate::fields::SET_DISPLAY_OBJECTIVE_DISPLAY_SLOT_NAME)?;
        let objective_name =
            reader.read_str(crate::fields::SET_DISPLAY_OBJECTIVE_OBJECTIVE_NAME)?;
        let objective_display_name =
            reader.read_str(crate::fields::SET_DISPLAY_OBJECTIVE_OBJECTIVE_DISPLAY_NAME)?;
        let criteria_name = reader.read_str(crate::fields::SET_DISPLAY_OBJECTIVE_CRITERIA_NAME)?;
        let sort_order = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self {
            display_slot_name,
            objective_name,
            objective_display_name,
            criteria_name,
            sort_order,
        })
    }
}

/// SetScore is sent by the server to send the contents of a scoreboard to the player. It may be used to
/// either add, remove or edit entries on the scoreboard.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetScore {
    /// `score_info` is a list of all entries that the client should operate on. Each entry's IdentityType
    /// specifies whether it is added, modified or removed.
    pub score_info: Vec<SetScoreInfoItem>,
}

impl SetScore {
    pub const ID: u32 = 108;
}
impl wire::Encode for SetScore {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.score_info.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for SetScore {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let score_info = wire::decode_collection::<SetScoreInfoItem>(
            reader,
            crate::fields::SET_SCORE_SCORE_INFO,
            4,
        )?;
        Ok(Self { score_info })
    }
}

/// LabTable is sent by the client to let the server know it started a chemical reaction in Education Edition,
/// and is sent by the server to other clients to show the effects. The packet is only functional if Education
/// features are enabled.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LabTable {
    /// `type_` is the type of the action that was executed. It is one of the constants above. Typically, only
    /// LabTableActionCombine is sent by the client, whereas LabTableActionReact is sent by the server.
    pub type_: LabTableType,
    /// `position` is the position at which the lab table used was located.
    pub position: BlockPos,
    /// `reaction` is the type of the reaction that took place as a result of the items put into the lab table.
    /// The reaction type can be either that of an item or a particle, depending on whatever the result was of the
    /// reaction.
    pub reaction: LabTableReactionType,
}

impl LabTable {
    pub const ID: u32 = 109;
}
impl wire::Encode for LabTable {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.type_.encode(writer)?;
        self.position.encode(writer)?;
        self.reaction.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LabTable {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let type_ = <LabTableType as wire::Decode>::decode(reader)?;
        let position = <BlockPos as wire::Decode>::decode(reader)?;
        let reaction = <LabTableReactionType as wire::Decode>::decode(reader)?;
        Ok(Self {
            type_,
            position,
            reaction,
        })
    }
}

/// UpdateBlockSynced is sent by the server to synchronise the falling of a falling block entity with the
/// transitioning back and forth from and to a solid block. It is used to prevent the entity from flickering,
/// and is used in places such as the pushing of blocks with pistons.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateBlockSynced {
    /// `block_position` is the block position at which a block is updated.
    pub block_position: BlockPos,
    /// `block_runtime_id` is the runtime ID of the block that is placed at Position after sending the packet to
    /// the client.
    pub block_runtime_id: wire::VarUInt,
    /// `flags` is a combination of flags that specify the way the block is updated client-side. It is a
    /// combination of the flags above, but typically sending only the BlockUpdateNetwork flag is sufficient.
    pub flags: wire::VarUInt,
    /// `layer` is the world layer on which the block is updated. For most blocks, this is the first layer, as
    /// that layer is the default layer to place blocks on, but for blocks inside of each other, this differs.
    pub layer: wire::VarUInt,
    /// `unique_actor_id` is the unique ID of the falling block entity that the block transitions to or that the
    /// entity transitions from. Note that for both possible values for TransitionType, the EntityUniqueID should
    /// point to the falling block entity involved.
    pub unique_actor_id: wire::VarULong,
    /// `actor_sync_message` is the type of the transition that happened. It is either BlockToEntityTransition,
    /// when a block placed becomes a falling entity, or EntityToBlockTransition, when a falling entity hits the
    /// ground and becomes a solid block again.
    pub actor_sync_message: wire::VarULong,
}

impl UpdateBlockSynced {
    pub const ID: u32 = 110;
}
impl wire::Encode for UpdateBlockSynced {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.block_position.encode(writer)?;
        self.block_runtime_id.encode(writer)?;
        wire::check_number_limits(self.block_runtime_id.0, Some(0), None)?;
        self.flags.encode(writer)?;
        wire::check_number_limits(self.flags.0, Some(0), None)?;
        self.layer.encode(writer)?;
        wire::check_number_limits(self.layer.0, Some(0), None)?;
        self.unique_actor_id.encode(writer)?;
        wire::check_number_limits(self.unique_actor_id.0, Some(0), None)?;
        self.actor_sync_message.encode(writer)?;
        wire::check_number_limits(self.actor_sync_message.0, Some(0), None)?;
        Ok(())
    }
}

impl wire::Decode for UpdateBlockSynced {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let block_position = <BlockPos as wire::Decode>::decode(reader)?;
        let block_runtime_id = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let flags = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let layer = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let unique_actor_id = {
            let value = <wire::VarULong as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let actor_sync_message = {
            let value = <wire::VarULong as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        Ok(Self {
            block_position,
            block_runtime_id,
            flags,
            layer,
            unique_actor_id,
            actor_sync_message,
        })
    }
}

/// MoveActorDelta is sent by the server to move an entity. The packet is specifically optimised to save as
/// much space as possible, by only writing non-zero fields. As of 1.16.100, this packet no longer actually
/// contains any deltas.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MoveActorDelta {
    pub move_data: MoveActorDeltaData,
}

impl MoveActorDelta {
    pub const ID: u32 = 111;
}
impl wire::Encode for MoveActorDelta {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.move_data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MoveActorDelta {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let move_data = <MoveActorDeltaData as wire::Decode>::decode(reader)?;
        Ok(Self { move_data })
    }
}

/// SetScoreboardIdentity is sent by the server to change the identity type of one of the entries on a
/// scoreboard. This is used to change, for example, an entry pointing to a player, to a fake player when it
/// leaves the server, and to change it back to a real player when it joins again. In non-vanilla situations,
/// the packet is quite useless.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetScoreboardIdentity {
    /// `scoreboard_identity_packet_type` is the type of the action to execute. The action is either
    /// ScoreboardIdentityActionRegister to associate an identity with the entry, or ScoreboardIdentityActionClear
    /// to remove associations with an entity.
    pub scoreboard_identity_packet_type: ScoreboardIdentityPacketType,
    /// `scoreboard_identity_info` is a list of all entries in the packet. Each of these entries points to one of
    /// the entries on a scoreboard. Depending on ActionType, their identity will either be registered or cleared.
    pub scoreboard_identity_info: Vec<ScoreboardIdentityPacketInfo>,
}

impl SetScoreboardIdentity {
    pub const ID: u32 = 112;
}
impl wire::Encode for SetScoreboardIdentity {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.scoreboard_identity_packet_type.encode(writer)?;
        wire::encode_collection(writer, self.scoreboard_identity_info.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for SetScoreboardIdentity {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let scoreboard_identity_packet_type =
            <ScoreboardIdentityPacketType as wire::Decode>::decode(reader)?;
        let scoreboard_identity_info = wire::decode_collection::<ScoreboardIdentityPacketInfo>(
            reader,
            crate::fields::SET_SCOREBOARD_IDENTITY_SCOREBOARD_IDENTITY_INFO,
            2,
        )?;
        Ok(Self {
            scoreboard_identity_packet_type,
            scoreboard_identity_info,
        })
    }
}

/// SetLocalPlayerAsInitialised is sent by the client in response to a PlayStatus packet with the status set
/// to 3. The packet marks the moment at which the client is fully initialised and can receive any packet
/// without discarding it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetLocalPlayerAsInitialized {
    /// EntityRuntimeID is the entity runtime ID the player was assigned earlier in the login sequence in the
    /// StartGame packet.
    pub player_id: ActorRuntimeID,
}

impl SetLocalPlayerAsInitialized {
    pub const ID: u32 = 113;
}
impl wire::Encode for SetLocalPlayerAsInitialized {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.player_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetLocalPlayerAsInitialized {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let player_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        Ok(Self { player_id })
    }
}

/// UpdateSoftEnum is sent by the server to update a soft enum, also known as a dynamic enum, previously sent
/// in the AvailableCommands packet. It is sent whenever the enum should get new options or when some of its
/// options should be removed. The UpdateSoftEnum packet will apply for enums that have been set in the
/// AvailableCommands packet with the 'Dynamic' field of the CommandEnum set to true.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateSoftEnum {
    /// `enum_name` is the type of the enum. This type must be identical to the one set in the AvailableCommands
    /// packet, because the client uses this to recognise which enum to update.
    pub enum_name: wire::Str,
    /// `values` is a list of options that should be updated. Depending on the ActionType field, either these
    /// options will be added to the enum, the enum options will be set to these options or all of these options
    /// will be removed from the enum.
    pub values: Vec<wire::Str>,
    /// `update_type` is the type of the action to execute on the enum. The Options field has a different result,
    /// depending on what ActionType is used.
    pub update_type: SoftEnumUpdateType,
}

impl UpdateSoftEnum {
    pub const ID: u32 = 114;
}
impl wire::Encode for UpdateSoftEnum {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.enum_name.encode(writer)?;
        writer.write_len_var_u32(self.values.len())?;
        for item in self.values.iter() {
            item.encode(writer)?;
        }
        self.update_type.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for UpdateSoftEnum {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let enum_name = reader.read_str(crate::fields::UPDATE_SOFT_ENUM_ENUM_NAME)?;
        let values = {
            let declared = u64::from(reader.read_var_u32()?);
            let count =
                reader.checked_count(crate::fields::UPDATE_SOFT_ENUM_VALUES, declared, 1)?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    reader.read_str(crate::fields::UPDATE_SOFT_ENUM_VALUES_ITEM)?,
                )?;
            }
            out
        };
        let update_type = <SoftEnumUpdateType as wire::Decode>::decode(reader)?;
        Ok(Self {
            enum_name,
            values,
            update_type,
        })
    }
}

/// NetworkStackLatency is sent by the server (and the client, on development builds) to measure the latency
/// over the entire Minecraft stack, rather than the RakNet latency. It has other usages too, such as the
/// ability to be used as some kind of acknowledgement packet, to know when the client has received a certain
/// other packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetworkStackLatency {
    /// Timestamp is the timestamp of the network stack latency packet. The client will, if NeedsResponse is set
    /// to true, send a NetworkStackLatency packet with this same timestamp packet in response.
    pub creation_time: wire::U64LE,
    /// NeedsResponse specifies if the sending side of this packet wants a response to the packet, meaning that
    /// the other side should send a NetworkStackLatency packet back.
    pub is_from_server: bool,
}

impl NetworkStackLatency {
    pub const ID: u32 = 115;
}
impl wire::Encode for NetworkStackLatency {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.creation_time.encode(writer)?;
        wire::check_number_limits(self.creation_time.0, Some(0), None)?;
        self.is_from_server.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for NetworkStackLatency {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let creation_time = {
            let value = <wire::U64LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let is_from_server = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            creation_time,
            is_from_server,
        })
    }
}

/// SpawnParticleEffect is sent by the server to spawn a particle effect client-side. Unlike other packets
/// that result in the appearing of particles, this packet can show particles that are not hardcoded in the
/// client. They can be added and changed through behaviour packs to implement custom particles.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpawnParticleEffect {
    /// Dimension is the dimension that the particle is spawned in. Its exact usage is not clear, as the dimension
    /// has no direct effect on the particle.
    pub dimension_id: wire::U8,
    /// EntityUniqueID is the unique ID of the entity that the spawned particle may be attached to. If this ID is
    /// not -1, the Position below will be interpreted as relative to the position of the entity associated with
    /// this unique ID.
    pub actor_id: ActorUniqueID,
    /// `position` is the position that the particle should be spawned at. If the position is too far away from
    /// the player, it will not show up. If EntityUniqueID is not -1, the position will be relative to the
    /// position of the entity.
    pub position: glam::Vec3,
    /// ParticleName is the name of the particle that should be shown. This name may point to a particle effect
    /// that is built-in, or to one implemented by behaviour packs.
    pub effect_name: wire::Str,
    /// `molang_variables` is an encoded JSON map of MoLang variables that may be applicable to the particle
    /// spawn. This can just be left empty in most cases.
    /// Wire presence: optional value is preceded by a presence marker.
    pub molang_variables: Option<wire::Str>,
}

impl SpawnParticleEffect {
    pub const ID: u32 = 118;
}
impl wire::Encode for SpawnParticleEffect {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.dimension_id.encode(writer)?;
        wire::check_number_limits(self.dimension_id.0, Some(0), Some(255))?;
        self.actor_id.encode(writer)?;
        self.position.encode(writer)?;
        self.effect_name.encode(writer)?;
        match &self.molang_variables {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for SpawnParticleEffect {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let dimension_id = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let actor_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let effect_name = reader.read_str(crate::fields::SPAWN_PARTICLE_EFFECT_EFFECT_NAME)?;
        let molang_variables = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(reader.read_str(crate::fields::SPAWN_PARTICLE_EFFECT_MOLANG_VARIABLES)?)
            }
        };
        Ok(Self {
            dimension_id,
            actor_id,
            position,
            effect_name,
            molang_variables,
        })
    }
}

/// AvailableActorIdentifiers is sent by the server at the start of the game to let the client know all
/// entities that are available on the server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AvailableActorIdentifiers {
    pub identifier_list: wire::NetworkNbt,
}

impl AvailableActorIdentifiers {
    pub const ID: u32 = 119;
}
impl wire::Encode for AvailableActorIdentifiers {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.identifier_list.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AvailableActorIdentifiers {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let identifier_list = <wire::NetworkNbt as wire::Decode>::decode(reader)?;
        Ok(Self { identifier_list })
    }
}

/// NetworkChunkPublisherUpdate is sent by the server to change the point around which chunks are and remain
/// loaded. This is useful for mini-game servers, where only one area is ever loaded, in which case the
/// NetworkChunkPublisherUpdate packet can be sent in the middle of it, so that no chunks ever need to be
/// additionally sent during the course of the game. In reality, the packet is not extraordinarily useful, and
/// most servers just send it constantly at the position of the player. If the packet is not sent at all, no
/// chunks will be shown to the player, regardless of where they are sent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetworkChunkPublisherUpdate {
    /// `new_position_for_view` is the block position around which chunks loaded will remain shown to the client.
    /// Most servers set this position to the position of the player itself.
    pub new_position_for_view: BlockPos,
    /// `new_radius_for_view` is the radius in blocks around Position that chunks sent show up in and will remain
    /// loaded in. Unlike the RequestChunkRadius and ChunkRadiusUpdated packets, this radius is in blocks rather
    /// than chunks, so the chunk radius needs to be multiplied by 16. (Or shifted to the left by 4.)
    pub new_radius_for_view: wire::VarUInt,
    /// `server_built_chunks_list` ... TODO: Figure out what this field is used for.
    pub server_built_chunks_list: Vec<ChunkPos>,
}

impl NetworkChunkPublisherUpdate {
    pub const ID: u32 = 121;
}
impl wire::Encode for NetworkChunkPublisherUpdate {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.new_position_for_view.encode(writer)?;
        self.new_radius_for_view.encode(writer)?;
        wire::check_number_limits(self.new_radius_for_view.0, Some(0), None)?;
        wire::check_length(self.server_built_chunks_list.len(), 0, 9216)?;
        wire::encode_collection_u32le(writer, self.server_built_chunks_list.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for NetworkChunkPublisherUpdate {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let new_position_for_view = <BlockPos as wire::Decode>::decode(reader)?;
        let new_radius_for_view = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let server_built_chunks_list = wire::decode_collection_u32le::<ChunkPos>(
            reader,
            crate::fields::NETWORK_CHUNK_PUBLISHER_UPDATE_SERVER_BUILT_CHUNKS_LIST,
            2,
        )?;
        Ok(Self {
            new_position_for_view,
            new_radius_for_view,
            server_built_chunks_list,
        })
    }
}

/// BiomeDefinitionList is sent by the server to let the client know all biomes that are available and
/// implemented on the server side. When enabled, it also includes information for the client to accurately
/// recreate the server-side generation in vanilla worlds/servers for increased performance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BiomeDefinitionList {
    pub map_of_biome_names_to_data: Vec<(wire::U16LE, BiomeDefinitionData)>,
    /// `string_list` is a makeshift dictionary implementation Mojang created to try and reduce the size of the
    /// overall packet. It is a list of common strings that are used in the biome definitions, such as biome
    /// names, float values or query expressions.
    pub string_list: BiomeStringList,
}

impl BiomeDefinitionList {
    pub const ID: u32 = 122;
}
impl wire::Encode for BiomeDefinitionList {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_map(writer, self.map_of_biome_names_to_data.as_slice())?;
        self.string_list.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for BiomeDefinitionList {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let map_of_biome_names_to_data = wire::decode_map::<wire::U16LE, BiomeDefinitionData>(
            reader,
            crate::fields::BIOME_DEFINITION_LIST_MAP_OF_BIOME_NAMES_TO_DATA,
            31,
        )?;
        let string_list = <BiomeStringList as wire::Decode>::decode(reader)?;
        Ok(Self {
            map_of_biome_names_to_data,
            string_list,
        })
    }
}

/// LevelSoundEvent is sent by the server to make any kind of built-in sound heard to a player. It is sent to,
/// for example, play a stepping sound or a shear sound. The packet is also sent by the client, in which case
/// it could be forwarded by the server to the other players online. If possible, the packets from the client
/// should be ignored however, and the server should play them on its own accord.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelSoundEvent {
    /// `sound_event` is the type of the sound to play. It is one of the constants above. Some of the sound types
    /// require additional data, which is set in the ExtraData field.
    pub sound_event: wire::Str,
    /// `position` is the position of the sound event. The player will be able to hear the direction of the sound
    /// based on what position is sent here.
    pub position: glam::Vec3,
    /// `data` is a packed integer that some sound types use to provide extra data. An example of this is the note
    /// sound, which is composed of a pitch and an instrument type.
    pub data: wire::ZigZag32,
    /// `actor_identifier` is the string entity type of the entity that emitted the sound, for example
    /// 'minecraft:skeleton'. Some sound types use this entity type for additional data.
    pub actor_identifier: wire::Str,
    /// `is_baby` specifies if the sound should be that of a baby mob. It is most notably used for parrot
    /// imitations, which will change based on if this field is set to true or not.
    pub is_baby: bool,
    /// `is_global` specifies if the sound should be played relatively or not. If set to true, the sound will have
    /// full volume, regardless of where the Position is, whereas if set to false, the sound's volume will be
    /// based on the distance to Position.
    pub is_global: bool,
    /// `actor_unique_id` is the unique ID of a source entity. The unique ID is a value that remains consistent
    /// across different sessions of the same world, but most servers simply fill the runtime ID of the entity out
    /// for this field.
    pub actor_unique_id: wire::I64LE,
    /// `fire_at_position` is the position in the same world at which the event should fire. If this is not
    /// present, the position entity will be used instead.
    /// Wire presence: optional value is preceded by a presence marker.
    pub fire_at_position: Option<glam::Vec3>,
}

impl LevelSoundEvent {
    pub const ID: u32 = 123;
}
impl wire::Encode for LevelSoundEvent {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.sound_event.encode(writer)?;
        self.position.encode(writer)?;
        self.data.encode(writer)?;
        self.actor_identifier.encode(writer)?;
        self.is_baby.encode(writer)?;
        self.is_global.encode(writer)?;
        self.actor_unique_id.encode(writer)?;
        match &self.fire_at_position {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for LevelSoundEvent {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let sound_event = reader.read_str(crate::fields::LEVEL_SOUND_EVENT_SOUND_EVENT)?;
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let data = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let actor_identifier =
            reader.read_str(crate::fields::LEVEL_SOUND_EVENT_ACTOR_IDENTIFIER)?;
        let is_baby = <bool as wire::Decode>::decode(reader)?;
        let is_global = <bool as wire::Decode>::decode(reader)?;
        let actor_unique_id = <wire::I64LE as wire::Decode>::decode(reader)?;
        let fire_at_position = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec3 as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            sound_event,
            position,
            data,
            actor_identifier,
            is_baby,
            is_global,
            actor_unique_id,
            fire_at_position,
        })
    }
}

/// LevelEventGeneric is sent by the server to send a 'generic' level event to the client. This packet sends
/// an NBT serialised object and may for that reason be used for any event holding additional data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelEventGeneric {
    /// `event_id` is a unique identifier that identifies the event called. The data that follows has fields in
    /// the NBT depending on what event it is.
    pub event_id: wire::ZigZag32,
    pub ctd: wire::NetworkNbt,
}

impl LevelEventGeneric {
    pub const ID: u32 = 124;
}
impl wire::Encode for LevelEventGeneric {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.event_id.encode(writer)?;
        self.ctd.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LevelEventGeneric {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let event_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let ctd = <wire::NetworkNbt as wire::Decode>::decode(reader)?;
        Ok(Self { event_id, ctd })
    }
}

/// LecternUpdate is sent by the client to update the server on which page was opened in a book on a lectern,
/// or if the book should be removed from it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LecternUpdate {
    /// Page is the page number in the book that was opened by the player on the lectern.
    pub new_page_to_show: wire::U8,
    /// PageCount is the number of pages that the book opened in the lectern has.
    pub total_pages: wire::U8,
    /// Position is the position of the lectern that was updated. If no lectern is at the block position, the
    /// packet should be ignored.
    pub position_of_lectern_to_update: BlockPos,
}

impl LecternUpdate {
    pub const ID: u32 = 125;
}
impl wire::Encode for LecternUpdate {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.new_page_to_show.encode(writer)?;
        wire::check_number_limits(self.new_page_to_show.0, Some(0), Some(255))?;
        self.total_pages.encode(writer)?;
        wire::check_number_limits(self.total_pages.0, Some(0), Some(255))?;
        self.position_of_lectern_to_update.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LecternUpdate {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let new_page_to_show = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let total_pages = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let position_of_lectern_to_update = <BlockPos as wire::Decode>::decode(reader)?;
        Ok(Self {
            new_page_to_show,
            total_pages,
            position_of_lectern_to_update,
        })
    }
}

/// ClientCacheStatus is sent by the client to the server at the start of the game. It is sent to let the
/// server know if it supports the client-side blob cache. Clients such as Nintendo Switch do not support the
/// cache, and attempting to use it anyway will fail.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientCacheStatus {
    /// `is_cache_supported` specifies if the blob cache is enabled. If false, the server should not attempt to
    /// use the blob cache. If true, it may do so, but it may also choose not to use it.
    pub is_cache_supported: bool,
}

impl ClientCacheStatus {
    pub const ID: u32 = 129;
}
impl wire::Encode for ClientCacheStatus {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.is_cache_supported.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ClientCacheStatus {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let is_cache_supported = <bool as wire::Decode>::decode(reader)?;
        Ok(Self { is_cache_supported })
    }
}

/// OnScreenTextureAnimation is sent by the server to show a certain animation on the screen of the player.
/// The packet is used, as an example, for when a raid is triggered and when a raid is defeated.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnScreenTextureAnimation {
    /// `effect_id` is the type of the animation to show. The packet provides no further extra data to allow
    /// modifying the duration or other properties of the animation.
    pub effect_id: wire::U32LE,
}

impl OnScreenTextureAnimation {
    pub const ID: u32 = 130;
}
impl wire::Encode for OnScreenTextureAnimation {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.effect_id.encode(writer)?;
        wire::check_number_limits(self.effect_id.0, Some(0), None)?;
        Ok(())
    }
}

impl wire::Decode for OnScreenTextureAnimation {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let effect_id = {
            let value = <wire::U32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        Ok(Self { effect_id })
    }
}

/// MapCreateLockedCopy is sent by the client to create a locked copy of one map into another map. In vanilla,
/// it is used in the cartography table to create a map that is locked and cannot be modified.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapCreateLockedCopy {
    /// `original_map_id` is the ID of the map that is being copied. The locked copy will obtain all content that
    /// is visible on this map, except the content will not change.
    pub original_map_id: ActorUniqueID,
    /// `new_map_id` is the ID of the map that holds the locked copy of the map that OriginalMapID points to. Its
    /// contents will be impossible to change.
    pub new_map_id: ActorUniqueID,
}

impl MapCreateLockedCopy {
    pub const ID: u32 = 131;
}
impl wire::Encode for MapCreateLockedCopy {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.original_map_id.encode(writer)?;
        self.new_map_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MapCreateLockedCopy {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let original_map_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let new_map_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        Ok(Self {
            original_map_id,
            new_map_id,
        })
    }
}

/// StructureTemplateDataRequest is sent by the client to request data of a structure.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StructureTemplateDataRequest {
    /// `structure_name` is the name of the structure that was set in the structure block's UI. This is the name
    /// used to export the structure to a file.
    pub structure_name: wire::Str,
    /// `structure_position` is the position of the structure block that has its template data requested.
    pub structure_position: BlockPos,
    /// `structure_settings` is a struct of settings that should be used for exporting the structure. These
    /// settings are identical to the last sent in the StructureBlockUpdate packet by the client.
    pub structure_settings: StructureSettings,
    /// `requested_operation` specifies the type of template data request that the player sent. It is one of the
    /// constants found above.
    pub requested_operation: StructureTemplateRequestOperation,
}

impl StructureTemplateDataRequest {
    pub const ID: u32 = 132;
}
impl wire::Encode for StructureTemplateDataRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_string_limits(writer, &self.structure_name, 0, 256)?;
        self.structure_position.encode(writer)?;
        self.structure_settings.encode(writer)?;
        self.requested_operation.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StructureTemplateDataRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let structure_name = wire::decode_string_limits(
            reader,
            crate::fields::STRUCTURE_TEMPLATE_DATA_REQUEST_STRUCTURE_NAME,
            0,
            256,
        )?;
        let structure_position = <BlockPos as wire::Decode>::decode(reader)?;
        let structure_settings = <StructureSettings as wire::Decode>::decode(reader)?;
        let requested_operation =
            <StructureTemplateRequestOperation as wire::Decode>::decode(reader)?;
        Ok(Self {
            structure_name,
            structure_position,
            structure_settings,
            requested_operation,
        })
    }
}

/// StructureTemplateDataResponse is sent by the server to send data of a structure to the client in response
/// to a StructureTemplateDataRequest packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StructureTemplateDataResponse {
    /// `structure_name` is the name of the structure that was requested. This is the name used to export the
    /// structure to a file.
    pub structure_name: wire::Str,
    pub structure_nbt: wire::NetworkNbt,
    /// `response_type` specifies the response type of the packet. This depends on the RequestType field sent in
    /// the StructureTemplateDataRequest packet and is one of the constants above.
    pub response_type: StructureTemplateResponseType,
}

impl StructureTemplateDataResponse {
    pub const ID: u32 = 133;
}
impl wire::Encode for StructureTemplateDataResponse {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.structure_name.encode(writer)?;
        self.structure_nbt.encode(writer)?;
        self.response_type.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for StructureTemplateDataResponse {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let structure_name =
            reader.read_str(crate::fields::STRUCTURE_TEMPLATE_DATA_RESPONSE_STRUCTURE_NAME)?;
        let structure_nbt = <wire::NetworkNbt as wire::Decode>::decode(reader)?;
        let response_type = <StructureTemplateResponseType as wire::Decode>::decode(reader)?;
        Ok(Self {
            structure_name,
            structure_nbt,
            response_type,
        })
    }
}

/// ClientCacheBlobStatus is part of the blob cache protocol. It is sent by the client to let the server know
/// what blobs it needs and which blobs it already has, in an ACK type system.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientCacheBlobStatus {
    /// `missing_ids` is a list of blob hashes that the client does not have a blob available for. The server
    /// should send the blobs matching these hashes as soon as possible.
    pub missing_ids: Vec<wire::U64LE>,
    /// `found_ids` is a list of blob hashes that the client has a blob available for. The blobs hashes here mean
    /// that the client already has them: The server does not need to send the blobs anymore.
    pub found_ids: Vec<wire::U64LE>,
}

impl ClientCacheBlobStatus {
    pub const ID: u32 = 135;
}
impl wire::Encode for ClientCacheBlobStatus {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection_limits(writer, self.missing_ids.as_slice(), 0, 4095)?;
        wire::encode_collection_limits(writer, self.found_ids.as_slice(), 0, 4095)?;
        Ok(())
    }
}

impl wire::Decode for ClientCacheBlobStatus {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let missing_ids = wire::decode_collection_limits::<wire::U64LE>(
            reader,
            crate::fields::CLIENT_CACHE_BLOB_STATUS_MISSING_IDS,
            8,
            0,
            4095,
        )?;
        let found_ids = wire::decode_collection_limits::<wire::U64LE>(
            reader,
            crate::fields::CLIENT_CACHE_BLOB_STATUS_FOUND_IDS,
            8,
            0,
            4095,
        )?;
        Ok(Self {
            missing_ids,
            found_ids,
        })
    }
}

/// ClientCacheMissResponse is part of the blob cache protocol. It is sent by the server in response to a
/// ClientCacheBlobStatus packet and contains the blob data of all blobs that the client acknowledged not to
/// have yet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientCacheMissResponse {
    /// `missing_blobs` is a list of all blobs that the client sent misses for in the ClientCacheBlobStatus. These
    /// blobs hold the data of the blobs with the hashes they are matched with.
    pub missing_blobs: Vec<MissingBlobData>,
}

impl ClientCacheMissResponse {
    pub const ID: u32 = 136;
}
impl wire::Encode for ClientCacheMissResponse {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection_limits(writer, self.missing_blobs.as_slice(), 0, 4095)?;
        Ok(())
    }
}

impl wire::Decode for ClientCacheMissResponse {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let missing_blobs = wire::decode_collection_limits::<MissingBlobData>(
            reader,
            crate::fields::CLIENT_CACHE_MISS_RESPONSE_MISSING_BLOBS,
            9,
            0,
            4095,
        )?;
        Ok(Self { missing_blobs })
    }
}

/// EducationSettings is a packet sent by the server to update Minecraft: Education Edition related settings.
/// It is unused by the normal base game.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EducationSettings {
    pub education_level_settings: EducationLevelSettings,
}

impl EducationSettings {
    pub const ID: u32 = 137;
}
impl wire::Encode for EducationSettings {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.education_level_settings.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EducationSettings {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let education_level_settings = <EducationLevelSettings as wire::Decode>::decode(reader)?;
        Ok(Self {
            education_level_settings,
        })
    }
}

/// Emote is sent by both the server and the client. When the client sends an emote, it sends this packet to
/// the server, after which the server will broadcast the packet to other players online.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Emote {
    /// EntityRuntimeID is the entity that sent the emote. When a player sends this packet, it has this field set
    /// as its own entity runtime ID.
    pub actor_runtime_id: ActorRuntimeID,
    /// `emote_id` is the ID of the emote to send.
    pub emote_id: wire::Str,
    /// EmoteLength is the number of ticks that the emote lasts for.
    pub emote_length_ticks: wire::VarUInt,
    /// `xuid` is the Xbox User ID of the player that sent the emote. It is only set when the emote is used by a
    /// player that is authenticated with Xbox Live.
    pub xuid: wire::Str,
    /// `platform_id` is an identifier only set for particular platforms when using an emote (presumably only for
    /// Nintendo Switch). It is otherwise an empty string, and is used to decide which players are able to emote
    /// with each other.
    pub platform_id: wire::Str,
    /// `flags` is a combination of flags that change the way the Emote packet operates. When the server sends
    /// this packet to other players, EmoteFlagServerSide must be present.
    pub flags: wire::U8,
}

impl Emote {
    pub const ID: u32 = 138;
}
impl wire::Encode for Emote {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.actor_runtime_id.encode(writer)?;
        self.emote_id.encode(writer)?;
        self.emote_length_ticks.encode(writer)?;
        wire::check_number_limits(self.emote_length_ticks.0, Some(0), None)?;
        self.xuid.encode(writer)?;
        self.platform_id.encode(writer)?;
        self.flags.encode(writer)?;
        wire::check_number_limits(self.flags.0, Some(0), Some(255))?;
        Ok(())
    }
}

impl wire::Decode for Emote {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let actor_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let emote_id = reader.read_str(crate::fields::EMOTE_EMOTE_ID)?;
        let emote_length_ticks = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let xuid = reader.read_str(crate::fields::EMOTE_XUID)?;
        let platform_id = reader.read_str(crate::fields::EMOTE_PLATFORM_ID)?;
        let flags = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        Ok(Self {
            actor_runtime_id,
            emote_id,
            emote_length_ticks,
            xuid,
            platform_id,
            flags,
        })
    }
}

/// MultiPlayerSettings is sent by the client to update multi-player related settings server-side and sent
/// back to online players by the server. The MultiPlayerSettings packet is a Minecraft: Education Edition
/// packet. It has no functionality for the base game.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MultiplayerSettings {
    /// ActionType is the action that should be done when this packet is sent. It is one of the constants that may
    /// be found above.
    pub packet_type: MultiplayerSettingsType,
}

impl MultiplayerSettings {
    pub const ID: u32 = 139;
}
impl wire::Encode for MultiplayerSettings {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.packet_type.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MultiplayerSettings {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let packet_type = <MultiplayerSettingsType as wire::Decode>::decode(reader)?;
        Ok(Self { packet_type })
    }
}

/// SettingsCommand is sent by the client when it changes a setting in the settings that results in the
/// issuing of a command to the server, such as when Show Coordinates is enabled.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsCommand {
    /// `command` is the full command line that was sent to the server as a result of the setting that the client
    /// changed.
    pub command: wire::Str,
    /// `suppress_output` specifies if the client requests the suppressing of the output of the command that was
    /// executed. Generally this is set to true, as the client won't need a message to confirm the output of the
    /// change.
    pub suppress_output: bool,
}

impl SettingsCommand {
    pub const ID: u32 = 140;
}
impl wire::Encode for SettingsCommand {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.command.encode(writer)?;
        self.suppress_output.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SettingsCommand {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let command = reader.read_str(crate::fields::SETTINGS_COMMAND_COMMAND)?;
        let suppress_output = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            command,
            suppress_output,
        })
    }
}

/// AnvilDamage is sent by the client to request the dealing damage to an anvil. This packet is completely
/// pointless and the server should never listen to it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnvilDamage {
    /// `block_position` is the position in the world that the anvil can be found at.
    pub block_position: BlockPos,
}

impl AnvilDamage {
    pub const ID: u32 = 141;
}
impl wire::Encode for AnvilDamage {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.block_position.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AnvilDamage {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let block_position = <BlockPos as wire::Decode>::decode(reader)?;
        Ok(Self { block_position })
    }
}

/// CompletedUsingItem is sent by the server to tell the client that it should be done using the item it is
/// currently using.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompletedUsingItem {
    /// `item_id` is the item ID of the item that the client completed using. This should typically be the ID of
    /// the item held in the hand.
    pub item_id: wire::I16LE,
    /// `item_use_method` is the method of the using of the item that was completed. It is one of the constants
    /// that may be found above.
    pub item_use_method: wire::I32LE,
}

impl CompletedUsingItem {
    pub const ID: u32 = 142;
}
impl wire::Encode for CompletedUsingItem {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.item_id.encode(writer)?;
        self.item_use_method.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CompletedUsingItem {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let item_id = <wire::I16LE as wire::Decode>::decode(reader)?;
        let item_use_method = <wire::I32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            item_id,
            item_use_method,
        })
    }
}

/// NetworkSettings is sent by the server to update a variety of network settings. These settings modify the
/// way packets are sent over the network stack.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetworkSettings {
    /// `compression_threshold` is the minimum size of a packet that is compressed when sent. If the size of a
    /// packet is under this value, it is not compressed. When set to 0, all packets will be left uncompressed.
    pub compression_threshold: wire::U16LE,
    /// `compression_algorithm` is the algorithm that is used to compress packets.
    pub compression_algorithm: PacketCompressionAlgorithm,
    /// ClientThrottle regulates whether the client should throttle players when exceeding of the threshold.
    /// Players outside threshold will not be ticked, improving performance on low-end devices.
    pub client_throttle_enabled: bool,
    /// `client_throttle_threshold` is the threshold for client throttling. If the number of players exceeds this
    /// value, the client will throttle players.
    pub client_throttle_threshold: wire::U8,
    /// `client_throttle_scalar` is the scalar for client throttling. The scalar is the amount of players that are
    /// ticked when throttling is enabled.
    pub client_throttle_scalar: wire::F32LE,
}

impl NetworkSettings {
    pub const ID: u32 = 143;
}
impl wire::Encode for NetworkSettings {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.compression_threshold.encode(writer)?;
        wire::check_number_limits(self.compression_threshold.0, Some(0), None)?;
        self.compression_algorithm.encode(writer)?;
        self.client_throttle_enabled.encode(writer)?;
        self.client_throttle_threshold.encode(writer)?;
        wire::check_number_limits(self.client_throttle_threshold.0, Some(0), Some(255))?;
        self.client_throttle_scalar.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for NetworkSettings {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let compression_threshold = {
            let value = <wire::U16LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let compression_algorithm = <PacketCompressionAlgorithm as wire::Decode>::decode(reader)?;
        let client_throttle_enabled = <bool as wire::Decode>::decode(reader)?;
        let client_throttle_threshold = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let client_throttle_scalar = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            compression_threshold,
            compression_algorithm,
            client_throttle_enabled,
            client_throttle_threshold,
            client_throttle_scalar,
        })
    }
}

/// PlayerAuthInput is sent by the client to allow for server authoritative movement. It is used to
/// synchronise the player input with the position server-side. The client sends this packet when the
/// ServerAuthoritativeMovementMode field in the StartGame packet is set to true, instead of the MovePlayer
/// packet. The client will send this packet once every tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerAuthInput {
    pub player_rotation: glam::Vec2,
    /// `position` holds the position that the player reports it has.
    pub position: glam::Vec3,
    /// `move_vector` is a Vec2 that specifies the direction in which the player moved, as a combination of X/Z
    /// values which are created using the WASD/controller stick state.
    pub move_vector: glam::Vec2,
    pub player_head_rotation: wire::F32LE,
    /// `input_data` is the set of input flags that together specify the way the player moved last tick. It holds
    /// the flags above.
    pub input_data: Vec<InputData>,
    /// `input_mode` specifies the way that the client inputs data to the screen. It is one of the constants that
    /// may be found above.
    pub input_mode: InputMode,
    /// `play_mode` specifies the way that the player is playing. The values it holds, which are rather random,
    /// may be found above.
    pub play_mode: ClientPlayMode,
    /// InteractionModel is a constant representing the interaction model the player is using. It is one of the
    /// constants that may be found above.
    pub new_interaction_model: NewInteractionModel,
    pub interact_rotation: glam::Vec2,
    /// Tick is the server tick at which the packet was sent. It is used in relation to
    /// CorrectPlayerMovePrediction.
    pub client_tick: PlayerInputTick,
    /// Delta was the delta between the old and the new position. There isn't any practical use for this field as
    /// it can be calculated by the server itself.
    pub pos_delta: glam::Vec3,
    /// Wire presence: optional value is preceded by a presence marker.
    pub item_use_transaction: Option<PackedItemUseLegacyInventoryTransaction>,
    /// `item_stack_request` is sent by the client to change an item in their inventory.
    /// Wire presence: optional value is preceded by a presence marker.
    pub item_stack_request: Option<ItemStackRequestData>,
    /// BlockActions is a slice of block actions that the client has interacted with.
    /// Wire presence: optional value is preceded by a presence marker.
    pub player_block_actions: Option<Vec<PlayerBlockActionData>>,
    /// `vehicle_rotation` is the rotation of the vehicle that the player is in, if any.
    /// Wire presence: optional value is preceded by a presence marker.
    pub vehicle_rotation: Option<glam::Vec2>,
    /// `client_predicted_vehicle` is the unique ID of the vehicle that the client predicts the player to be in.
    /// Wire presence: optional value is preceded by a presence marker.
    pub client_predicted_vehicle: Option<ActorUniqueID>,
    pub analog_move_vector: glam::Vec2,
    /// `camera_orientation` is the vector that represents the camera's forward direction which can be used to
    /// transform movement to be camera relative.
    pub camera_orientation: glam::Vec3,
    /// `raw_move_vector` is the value of MoveVector before it is affected by input permissions, sneaking/fly
    /// speeds and isn't normalised for analogue inputs.
    pub raw_move_vector: glam::Vec2,
}

impl PlayerAuthInput {
    pub const ID: u32 = 144;
}
impl wire::Encode for PlayerAuthInput {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.player_rotation.encode(writer)?;
        self.position.encode(writer)?;
        self.move_vector.encode(writer)?;
        self.player_head_rotation.encode(writer)?;
        wire::encode_collection(writer, self.input_data.as_slice())?;
        self.input_mode.encode(writer)?;
        self.play_mode.encode(writer)?;
        self.new_interaction_model.encode(writer)?;
        self.interact_rotation.encode(writer)?;
        self.client_tick.encode(writer)?;
        self.pos_delta.encode(writer)?;
        match &self.item_use_transaction {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.item_stack_request {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.player_block_actions {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_collection_limits(writer, value.as_slice(), 0, 100)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.vehicle_rotation {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.client_predicted_vehicle {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.analog_move_vector.encode(writer)?;
        self.camera_orientation.encode(writer)?;
        self.raw_move_vector.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerAuthInput {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let player_rotation = <glam::Vec2 as wire::Decode>::decode(reader)?;
        let position = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let move_vector = <glam::Vec2 as wire::Decode>::decode(reader)?;
        let player_head_rotation = <wire::F32LE as wire::Decode>::decode(reader)?;
        let input_data = wire::decode_collection::<InputData>(
            reader,
            crate::fields::PLAYER_AUTH_INPUT_INPUT_DATA,
            1,
        )?;
        let input_mode = <InputMode as wire::Decode>::decode(reader)?;
        let play_mode = <ClientPlayMode as wire::Decode>::decode(reader)?;
        let new_interaction_model = <NewInteractionModel as wire::Decode>::decode(reader)?;
        let interact_rotation = <glam::Vec2 as wire::Decode>::decode(reader)?;
        let client_tick = <PlayerInputTick as wire::Decode>::decode(reader)?;
        let pos_delta = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let item_use_transaction = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<PackedItemUseLegacyInventoryTransaction as wire::Decode>::decode(reader)?)
            }
        };
        let item_stack_request = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<ItemStackRequestData as wire::Decode>::decode(reader)?)
            }
        };
        let player_block_actions = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(wire::decode_collection_limits::<PlayerBlockActionData>(
                    reader,
                    crate::fields::PLAYER_AUTH_INPUT_PLAYER_BLOCK_ACTIONS,
                    5,
                    0,
                    100,
                )?)
            }
        };
        let vehicle_rotation = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec2 as wire::Decode>::decode(reader)?)
            }
        };
        let client_predicted_vehicle = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<ActorUniqueID as wire::Decode>::decode(reader)?)
            }
        };
        let analog_move_vector = <glam::Vec2 as wire::Decode>::decode(reader)?;
        let camera_orientation = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let raw_move_vector = <glam::Vec2 as wire::Decode>::decode(reader)?;
        Ok(Self {
            player_rotation,
            position,
            move_vector,
            player_head_rotation,
            input_data,
            input_mode,
            play_mode,
            new_interaction_model,
            interact_rotation,
            client_tick,
            pos_delta,
            item_use_transaction,
            item_stack_request,
            player_block_actions,
            vehicle_rotation,
            client_predicted_vehicle,
            analog_move_vector,
            camera_orientation,
            raw_move_vector,
        })
    }
}

/// CreativeContent is a packet sent by the server to set the creative inventory's content for a player.
/// Introduced in 1.16, this packet replaces the previous method - sending an InventoryContent packet with
/// creative inventory window ID. As of v1.21.60, this packet is no longer required to be sent as part of the
/// login sequence however the client will crash if they try to open their creative inventory before receiving
/// this packet. Every item must be part of a group, any items that are not part of a group will need to
/// reference an "anonymous group" which has an empty name OR no icon. The order of Groups and Items is how
/// the client will render items in the creative inventory compared to the previous, hard coded order.
/// Below is an example of defining 2 ungrouped items, 2 grouped items and then another 2 ungrouped items, all
/// in the nature category.
/// CreativeContent{ Groups: []protocol.CreativeGroup{ {Category: 1}, // No name or icon, this is the
/// "anonymous group" {Category: 1, Name: "itemGroup.name.planks", Icon: protocol.ItemStack{...}}, // A
/// "planks" group {Category: 1}, // Another "anonymous group" }, Items: []protocol.CreativeItem{
/// {CreativeItemNetworkID: 0, Item: protocol.ItemStack{...}, GroupIndex: 0}, // Ungrouped before "planks"
/// {CreativeItemNetworkID: 1, Item: protocol.ItemStack{...}, GroupIndex: 0}, // Ungrouped before "planks"
/// {CreativeItemNetworkID: 2, Item: protocol.ItemStack{...}, GroupIndex: 1}, // Grouped under the "planks"
/// group {CreativeItemNetworkID: 3, Item: protocol.ItemStack{...}, GroupIndex: 1}, // Grouped under the
/// "planks" group {CreativeItemNetworkID: 4, Item: protocol.ItemStack{...}, GroupIndex: 2}, // Ungrouped
/// after "planks" {CreativeItemNetworkID: 5, Item: protocol.ItemStack{...}, GroupIndex: 2}, // Ungrouped
/// after "planks" } }
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CreativeContent {
    /// `groups` is a list of the groups that should be added to the creative inventory.
    pub groups: Vec<CreativeGroupInfo>,
    /// `entries` is a list of the items that should be added to the creative inventory.
    pub entries: Vec<CreativeItemEntry>,
}

impl CreativeContent {
    pub const ID: u32 = 145;
}
impl wire::Encode for CreativeContent {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.groups.as_slice())?;
        wire::encode_collection(writer, self.entries.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for CreativeContent {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let groups = wire::decode_collection::<CreativeGroupInfo>(
            reader,
            crate::fields::CREATIVE_CONTENT_GROUPS,
            8,
        )?;
        let entries = wire::decode_collection::<CreativeItemEntry>(
            reader,
            crate::fields::CREATIVE_CONTENT_ENTRIES,
            8,
        )?;
        Ok(Self { groups, entries })
    }
}

/// PlayerEnchantOptions is sent by the server to update the enchantment options displayed when the user opens
/// the enchantment table and puts an item in. This packet was added in 1.16 and allows the server to decide
/// on the enchantments that can be selected by the player. The PlayerEnchantOptions packet should be sent
/// once for every slot update of the enchantment table. The vanilla server sends an empty
/// PlayerEnchantOptions packet when the player opens the enchantment table (air is present in the enchantment
/// table slot) and sends the packet with actual enchantments in it when items are put in that can have
/// enchantments.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerEnchantOptions {
    /// `options` is a list of possible enchantment options for the item that was put into the enchantment table.
    pub options: Vec<ItemEnchantOption>,
}

impl PlayerEnchantOptions {
    pub const ID: u32 = 146;
}
impl wire::Encode for PlayerEnchantOptions {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection_limits(writer, self.options.as_slice(), 0, 3)?;
        Ok(())
    }
}

impl wire::Decode for PlayerEnchantOptions {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let options = wire::decode_collection_limits::<ItemEnchantOption>(
            reader,
            crate::fields::PLAYER_ENCHANT_OPTIONS_OPTIONS,
            10,
            0,
            3,
        )?;
        Ok(Self { options })
    }
}

/// ItemStackRequest is sent by the client to change item stacks in an inventory. It is essentially a
/// replacement of the InventoryTransaction packet added in 1.16 for inventory specific actions, such as
/// moving items around or crafting. The InventoryTransaction packet is still used for actions such as placing
/// blocks and interacting with entities.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemStackRequest {
    /// `requests` holds a list of item stack requests. These requests are all separate, but the client buffers
    /// the requests, so you might find multiple unrelated requests in this packet.
    pub requests: Vec<ItemStackRequestPacketData>,
}

impl ItemStackRequest {
    pub const ID: u32 = 147;
}
impl wire::Encode for ItemStackRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection_limits(writer, self.requests.as_slice(), 0, 100)?;
        Ok(())
    }
}

impl wire::Decode for ItemStackRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let requests = wire::decode_collection_limits::<ItemStackRequestPacketData>(
            reader,
            crate::fields::ITEM_STACK_REQUEST_REQUESTS,
            7,
            0,
            100,
        )?;
        Ok(Self { requests })
    }
}

/// ItemStackResponse is sent by the server in response to an ItemStackRequest packet from the client. This
/// packet is used to either approve or reject ItemStackRequests from the client. If a request is approved,
/// the client will simply continue as normal. If rejected, the client will undo the actions so that the
/// inventory should be in sync with the server again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemStackResponse {
    /// `responses` is a list of responses to ItemStackRequests sent by the client before. Responses either
    /// approve or reject a request from the client. Vanilla limits the size of this slice to 4096.
    pub responses: Vec<ItemStackResponseInfo>,
}

impl ItemStackResponse {
    pub const ID: u32 = 148;
}
impl wire::Encode for ItemStackResponse {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.responses.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for ItemStackResponse {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let responses = wire::decode_collection::<ItemStackResponseInfo>(
            reader,
            crate::fields::ITEM_STACK_RESPONSE_RESPONSES,
            3,
        )?;
        Ok(Self { responses })
    }
}

/// PlayerArmourDamage is sent by the server to damage the armour of a player. It is a very efficient packet,
/// but generally it's much easier to just send a slot update for the damaged armour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerArmorDamage {
    /// List is a list of armour entries indicating which pieces of armour should receive damage.
    pub armor_slot_and_damage_pairs: Vec<ArmorSlotAndDamagePair>,
}

impl PlayerArmorDamage {
    pub const ID: u32 = 149;
}
impl wire::Encode for PlayerArmorDamage {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection_limits(writer, self.armor_slot_and_damage_pairs.as_slice(), 0, 5)?;
        Ok(())
    }
}

impl wire::Decode for PlayerArmorDamage {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let armor_slot_and_damage_pairs = wire::decode_collection_limits::<ArmorSlotAndDamagePair>(
            reader,
            crate::fields::PLAYER_ARMOR_DAMAGE_ARMOR_SLOT_AND_DAMAGE_PAIRS,
            3,
            0,
            5,
        )?;
        Ok(Self {
            armor_slot_and_damage_pairs,
        })
    }
}

/// CodeBuilder is an Education Edition packet sent by the server to the client to open the URL to a Code
/// Builder (websocket) server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CodeBuilder {
    /// `url` is the url to the Code Builder (websocket) server.
    pub url: wire::Str,
    /// `should_open_code_builder` specifies if the client should automatically open the Code Builder app. If set
    /// to true, the client will attempt to use the Code Builder app to connect to and interface with the server
    /// running at the URL above.
    pub should_open_code_builder: bool,
}

impl CodeBuilder {
    pub const ID: u32 = 150;
}
impl wire::Encode for CodeBuilder {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.url.encode(writer)?;
        self.should_open_code_builder.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CodeBuilder {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let url = reader.read_str(crate::fields::CODE_BUILDER_URL)?;
        let should_open_code_builder = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            url,
            should_open_code_builder,
        })
    }
}

/// UpdatePlayerGameType is sent by the server to change the game mode of a player. It is functionally
/// identical to the SetPlayerGameType packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdatePlayerGameType {
    /// GameType is the new game type of the player. It is one of the constants that can be found in
    /// set_player_game_type.go. Some of these game types require additional flags to be set in an UpdateAbilities
    /// packet for the game mode to obtain its full functionality.
    pub player_game_type: GameType,
    /// PlayerUniqueID is the entity unique ID of the player that should have its game mode updated. If this
    /// packet is sent to other clients with the player unique ID of another player, nothing happens.
    pub target_player: ActorUniqueID,
    /// `tick` is the server tick at which the packet was sent. It is used in relation to
    /// CorrectPlayerMovePrediction.
    pub tick: PlayerInputTick,
}

impl UpdatePlayerGameType {
    pub const ID: u32 = 151;
}
impl wire::Encode for UpdatePlayerGameType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.player_game_type.encode(writer)?;
        self.target_player.encode(writer)?;
        self.tick.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for UpdatePlayerGameType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let player_game_type = <GameType as wire::Decode>::decode(reader)?;
        let target_player = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let tick = <PlayerInputTick as wire::Decode>::decode(reader)?;
        Ok(Self {
            player_game_type,
            target_player,
            tick,
        })
    }
}

/// EmoteList is sent by the client every time it joins the server and when it equips new emotes. It may be
/// used by the server to find out which emotes the client has available. If the player has no emotes
/// equipped, this packet is not sent. Under certain circumstances, this packet is also sent from the server
/// to the client, but I was unable to find when this is done.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmoteList {
    /// PlayerRuntimeID is the runtime ID of the player that owns the emote pieces below. If sent by the client,
    /// this player runtime ID is always that of the player itself.
    pub runtime_id: ActorRuntimeID,
    /// EmotePieces is a list of emote pieces that the player with the runtime ID above has.
    pub emote_piece_ids: Vec<uuid::Uuid>,
}

impl EmoteList {
    pub const ID: u32 = 152;
}
impl wire::Encode for EmoteList {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.runtime_id.encode(writer)?;
        wire::encode_collection(writer, self.emote_piece_ids.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for EmoteList {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let emote_piece_ids = wire::decode_collection::<uuid::Uuid>(
            reader,
            crate::fields::EMOTE_LIST_EMOTE_PIECE_IDS,
            16,
        )?;
        Ok(Self {
            runtime_id,
            emote_piece_ids,
        })
    }
}

/// PositionTrackingDBServerBroadcast is sent by the server in response to the PositionTrackingDBClientRequest
/// packet. This packet is, as of 1.16, currently only used for lodestones. The server maintains a database
/// with tracking IDs and their position and dimension. The client will request these tracking IDs, (NBT tag
/// set on the lodestone compass with the tracking ID?) and the server will respond with the status of those
/// tracking IDs. What is actually done with the data sent depends on what the client chooses to do with it.
/// For the lodestone compass, it is used to make the compass point towards lodestones and to make it spin if
/// the lodestone at a position is no longer there.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PositionTrackingDBServerBroadcast {
    /// BroadcastAction specifies the status of the position tracking DB response. It is one of the constants
    /// above, specifying the result of the request with the ID below. The Update action is sent for setting the
    /// position of a lodestone compass, the Destroy and NotFound to indicate that there is not (no longer) a
    /// lodestone at that position.
    pub action: PositionTrackingDBServerBroadcastAction,
    pub id: PositionTrackingId,
    pub position_tracking_data: wire::NetworkNbt,
}

impl PositionTrackingDBServerBroadcast {
    pub const ID: u32 = 153;
}
impl wire::Encode for PositionTrackingDBServerBroadcast {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.action.encode(writer)?;
        self.id.encode(writer)?;
        self.position_tracking_data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PositionTrackingDBServerBroadcast {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let action = <PositionTrackingDBServerBroadcastAction as wire::Decode>::decode(reader)?;
        let id = <PositionTrackingId as wire::Decode>::decode(reader)?;
        let position_tracking_data = <wire::NetworkNbt as wire::Decode>::decode(reader)?;
        Ok(Self {
            action,
            id,
            position_tracking_data,
        })
    }
}

/// PositionTrackingDBClientRequest is a packet sent by the client to request the position and dimension of a
/// 'tracking ID'. These IDs are tracked in a database by the server. In 1.16, this is used for lodestones.
/// The client will send this request to find the position a lodestone compass needs to point to. If found, it
/// will point to the lodestone. If not, it will start spinning around. A PositionTrackingDBServerBroadcast
/// packet should be sent in response to this packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PositionTrackingDBClientRequest {
    /// RequestAction is the action that should be performed upon the receiving of the packet. It is one of the
    /// constants found above.
    pub action: PositionTrackingDBClientRequestAction,
    pub id: PositionTrackingId,
}

impl PositionTrackingDBClientRequest {
    pub const ID: u32 = 154;
}
impl wire::Encode for PositionTrackingDBClientRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.action.encode(writer)?;
        self.id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PositionTrackingDBClientRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let action = <PositionTrackingDBClientRequestAction as wire::Decode>::decode(reader)?;
        let id = <PositionTrackingId as wire::Decode>::decode(reader)?;
        Ok(Self { action, id })
    }
}

/// DebugInfo is a packet sent by the server to the client. It does not seem to do anything when sent to the
/// normal client in 1.16.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DebugInfo {
    /// PlayerUniqueID is the unique ID of the player that the packet is sent to.
    pub actor_id: ActorUniqueID,
    /// `data` is the debug data.
    pub data: bytes::Bytes,
}

impl DebugInfo {
    pub const ID: u32 = 155;
}
impl wire::Encode for DebugInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.actor_id.encode(writer)?;
        self.data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for DebugInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let actor_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let data = reader.read_shared_bytes(crate::fields::DEBUG_INFO_DATA)?;
        Ok(Self { actor_id, data })
    }
}

/// PacketViolationWarning is sent by the client when it receives an invalid packet from the server. It holds
/// some information on the error that occurred. noinspection GoNameStartsWithPackageName
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PacketViolationWarning {
    /// Type is the type of violation. It is one of the constants above.
    pub violation_type: PacketViolationType,
    /// Severity specifies the severity of the packet violation. The action the client takes after this violation
    /// depends on the severity sent.
    pub violation_severity: PacketViolationSeverity,
    /// PacketID is the ID of the invalid packet that was received.
    pub violation_packet_id: wire::ZigZag32,
    /// `violation_context` holds a description on the violation of the packet.
    pub violation_context: wire::Str,
}

impl PacketViolationWarning {
    pub const ID: u32 = 156;
}
impl wire::Encode for PacketViolationWarning {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.violation_type.encode(writer)?;
        self.violation_severity.encode(writer)?;
        self.violation_packet_id.encode(writer)?;
        self.violation_context.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PacketViolationWarning {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let violation_type = <PacketViolationType as wire::Decode>::decode(reader)?;
        let violation_severity = <PacketViolationSeverity as wire::Decode>::decode(reader)?;
        let violation_packet_id = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let violation_context =
            reader.read_str(crate::fields::PACKET_VIOLATION_WARNING_VIOLATION_CONTEXT)?;
        Ok(Self {
            violation_type,
            violation_severity,
            violation_packet_id,
            violation_context,
        })
    }
}

/// MotionPredictionHints is sent by the server to the client. There is a predictive movement component for
/// entities. This packet fills the "history" of that component and entity movement is computed based on the
/// points. Vanilla sends this packet instead of the SetActorMotion packet when 'spatial optimisations' are
/// enabled.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MotionPredictionHints {
    /// EntityRuntimeID is the runtime ID of the entity whose velocity is sent to the client.
    pub m_runtime_id: ActorRuntimeID,
    /// Velocity is the server-calculated velocity of the entity at the point of sending the packet.
    pub m_motion: glam::Vec3,
    /// OnGround specifies if the server currently thinks the entity is on the ground.
    pub m_on_ground: bool,
}

impl MotionPredictionHints {
    pub const ID: u32 = 157;
}
impl wire::Encode for MotionPredictionHints {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.m_runtime_id.encode(writer)?;
        self.m_motion.encode(writer)?;
        self.m_on_ground.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MotionPredictionHints {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let m_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let m_motion = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let m_on_ground = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            m_runtime_id,
            m_motion,
            m_on_ground,
        })
    }
}

/// AnimateEntity is sent by the server to animate an entity client-side. It may be used to play a single
/// animation, or to activate a controller which can start a sequence of animations based on different
/// conditions specified in an animation controller. Much of the documentation of this packet can be found at
/// https://learn.microsoft.com/en-us/minecraft/creator/reference/content/animationsreference
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimateEntity {
    /// `m_animation` is the name of a single animation to start playing.
    pub m_animation: wire::Str,
    /// `m_next_state` is the first state to start with. These states are declared in animation controllers
    /// (which, in themselves, are animations too). These states in turn may have animations and transitions to
    /// move to a next state.
    pub m_next_state: wire::Str,
    /// `m_stop_expression` is a MoLang expression that specifies when the animation should be stopped.
    pub m_stop_expression: wire::Str,
    /// `m_stop_expression_version` is the MoLang stop condition version.
    pub m_stop_expression_version: wire::I32LE,
    /// `m_controller` is the animation controller that is used to manage animations. These controllers decide
    /// when to play which animation.
    pub m_controller: wire::Str,
    /// `m_blend_out_time` does not currently seem to be used.
    pub m_blend_out_time: wire::F32LE,
    /// `m_runtime_ids` is list of runtime IDs of entities that the animation should be applied to.
    pub m_runtime_ids: Vec<ActorRuntimeID>,
}

impl AnimateEntity {
    pub const ID: u32 = 158;
}
impl wire::Encode for AnimateEntity {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.m_animation.encode(writer)?;
        self.m_next_state.encode(writer)?;
        self.m_stop_expression.encode(writer)?;
        self.m_stop_expression_version.encode(writer)?;
        self.m_controller.encode(writer)?;
        self.m_blend_out_time.encode(writer)?;
        wire::encode_collection(writer, self.m_runtime_ids.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for AnimateEntity {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let m_animation = reader.read_str(crate::fields::ANIMATE_ENTITY_M_ANIMATION)?;
        let m_next_state = reader.read_str(crate::fields::ANIMATE_ENTITY_M_NEXT_STATE)?;
        let m_stop_expression = reader.read_str(crate::fields::ANIMATE_ENTITY_M_STOP_EXPRESSION)?;
        let m_stop_expression_version = <wire::I32LE as wire::Decode>::decode(reader)?;
        let m_controller = reader.read_str(crate::fields::ANIMATE_ENTITY_M_CONTROLLER)?;
        let m_blend_out_time = <wire::F32LE as wire::Decode>::decode(reader)?;
        let m_runtime_ids = wire::decode_collection::<ActorRuntimeID>(
            reader,
            crate::fields::ANIMATE_ENTITY_M_RUNTIME_IDS,
            1,
        )?;
        Ok(Self {
            m_animation,
            m_next_state,
            m_stop_expression,
            m_stop_expression_version,
            m_controller,
            m_blend_out_time,
            m_runtime_ids,
        })
    }
}

/// CameraShake is sent by the server to make the camera shake client-side. This feature was added for map-
/// making partners.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraShake {
    /// `intensity` is the intensity of the shaking. The client limits this value to 4, so anything higher may not
    /// work.
    pub intensity: wire::F32LE,
    /// Duration is the number of seconds the camera will shake for.
    pub seconds: wire::F32LE,
    /// Type is the type of shake, and is one of the constants listed above. The different type affects how the
    /// shake looks in game.
    pub shake_type: CameraShakeType,
    /// Action is the action to be performed, and is one of the constants listed above. Currently the different
    /// actions will either add or stop shaking the client.
    pub shake_action: CameraShakeAction,
}

impl CameraShake {
    pub const ID: u32 = 159;
}
impl wire::Encode for CameraShake {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.intensity.encode(writer)?;
        self.seconds.encode(writer)?;
        self.shake_type.encode(writer)?;
        self.shake_action.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraShake {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let intensity = <wire::F32LE as wire::Decode>::decode(reader)?;
        let seconds = <wire::F32LE as wire::Decode>::decode(reader)?;
        let shake_type = <CameraShakeType as wire::Decode>::decode(reader)?;
        let shake_action = <CameraShakeAction as wire::Decode>::decode(reader)?;
        Ok(Self {
            intensity,
            seconds,
            shake_type,
            shake_action,
        })
    }
}

/// PlayerFog is sent by the server to render the different fogs in the Stack. The types of fog are controlled
/// by resource packs to change how they are rendered, and the ability to create custom fog.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerFog {
    /// `fog_stack` is a list of fog identifiers to be sent to the client. Examples of fog identifiers are
    /// "minecraft:fog_ocean" and "minecraft:fog_hell".
    pub fog_stack: Vec<wire::Str>,
}

impl PlayerFog {
    pub const ID: u32 = 160;
}
impl wire::Encode for PlayerFog {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        writer.write_len_var_u32(self.fog_stack.len())?;
        for item in self.fog_stack.iter() {
            item.encode(writer)?;
        }
        Ok(())
    }
}

impl wire::Decode for PlayerFog {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let fog_stack = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(crate::fields::PLAYER_FOG_FOG_STACK, declared, 1)?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    reader.read_str(crate::fields::PLAYER_FOG_FOG_STACK_ITEM)?,
                )?;
            }
            out
        };
        Ok(Self { fog_stack })
    }
}

/// CorrectPlayerMovePrediction is sent by the server if and only if StartGame.ServerAuthoritativeMovementMode
/// is set to AuthoritativeMovementModeServerWithRewind. The packet is used to correct movement at a specific
/// point in time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CorrectPlayerMovePrediction {
    /// `prediction_type` is the type of prediction that was corrected. It is one of the constants above.
    pub prediction_type: RewindType,
    /// Position is the position that the player is supposed to be at the tick written in the field below. The
    /// client will change its current position based on movement after that tick starting from the Position.
    pub pos: glam::Vec3,
    /// Delta is the change in position compared to what the client sent as its position at that specific tick.
    pub pos_delta: glam::Vec3,
    /// `rotation` is the rotation of the player at the tick written in the field below.
    pub rotation: glam::Vec2,
    /// `vehicle_angular_velocity` is the angular velocity of the vehicle that the rider is riding.
    /// Wire presence: optional value is preceded by a presence marker.
    pub vehicle_angular_velocity: Option<wire::F32LE>,
    /// `on_ground` specifies if the player was on the ground at the time of the tick below.
    pub on_ground: bool,
    /// `tick` is the tick of the movement which was corrected by this packet.
    pub tick: PlayerInputTick,
}

impl CorrectPlayerMovePrediction {
    pub const ID: u32 = 161;
}
impl wire::Encode for CorrectPlayerMovePrediction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.prediction_type.encode(writer)?;
        self.pos.encode(writer)?;
        self.pos_delta.encode(writer)?;
        self.rotation.encode(writer)?;
        match &self.vehicle_angular_velocity {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        self.on_ground.encode(writer)?;
        self.tick.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CorrectPlayerMovePrediction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let prediction_type = <RewindType as wire::Decode>::decode(reader)?;
        let pos = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let pos_delta = <glam::Vec3 as wire::Decode>::decode(reader)?;
        let rotation = <glam::Vec2 as wire::Decode>::decode(reader)?;
        let vehicle_angular_velocity = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let on_ground = <bool as wire::Decode>::decode(reader)?;
        let tick = <PlayerInputTick as wire::Decode>::decode(reader)?;
        Ok(Self {
            prediction_type,
            pos,
            pos_delta,
            rotation,
            vehicle_angular_velocity,
            on_ground,
            tick,
        })
    }
}

/// ItemRegistry is sent by the server to send the client a list of available items and attach client-side
/// components to a custom item. This packet was formerly known as the ItemComponent packet before 1.21.60,
/// which did not include item definitions but only the components.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemRegistry {
    /// `item_data` is a list of all items with their legacy IDs which are available in the game. Failing to send
    /// any of the items that are in the game will crash mobile clients. Any custom components are also attached
    /// to the items in this list.
    pub item_data: Vec<ItemData>,
}

impl ItemRegistry {
    pub const ID: u32 = 162;
}
impl wire::Encode for ItemRegistry {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.item_data.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for ItemRegistry {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let item_data =
            wire::decode_collection::<ItemData>(reader, crate::fields::ITEM_REGISTRY_ITEM_DATA, 6)?;
        Ok(Self { item_data })
    }
}

/// ClientBoundDebugRenderer is sent by the server to spawn an outlined cube on client-side.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientboundDebugRenderer {
    /// `type_` is the type of action. It is one of the constants above.
    pub type_: wire::Str,
    /// Wire presence: optional value is preceded by a presence marker.
    pub debug_marker_data: Option<DebugMarkerData>,
}

impl ClientboundDebugRenderer {
    pub const ID: u32 = 164;
}
impl wire::Encode for ClientboundDebugRenderer {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.type_.encode(writer)?;
        match &self.debug_marker_data {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ClientboundDebugRenderer {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let type_ = reader.read_str(crate::fields::CLIENTBOUND_DEBUG_RENDERER_TYPE)?;
        let debug_marker_data = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<DebugMarkerData as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            type_,
            debug_marker_data,
        })
    }
}

/// SyncActorProperty is an alternative to synced actor data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncActorProperty {
    /// `property_data` ...
    pub property_data: wire::NetworkNbt,
}

impl SyncActorProperty {
    pub const ID: u32 = 165;
}
impl wire::Encode for SyncActorProperty {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.property_data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SyncActorProperty {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let property_data = <wire::NetworkNbt as wire::Decode>::decode(reader)?;
        Ok(Self { property_data })
    }
}

/// AddVolumeEntity sends a volume entity's definition and metadata from server to client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AddVolumeEntity {
    pub entity_network_id: EntityNetId,
    pub components: wire::NetworkNbt,
    pub json_identifier: wire::Str,
    pub instance_name: wire::Str,
    pub min_bounds: BlockPos,
    pub max_bounds: BlockPos,
    pub dimension_type: DimensionType,
    /// `engine_version` is the engine version the entity is using, for example, '1.17.0'.
    pub engine_version: wire::Str,
}

impl AddVolumeEntity {
    pub const ID: u32 = 166;
}
impl wire::Encode for AddVolumeEntity {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.entity_network_id.encode(writer)?;
        self.components.encode(writer)?;
        wire::encode_string_limits(writer, &self.json_identifier, 1, 18446744073709551615)?;
        wire::encode_string_limits(writer, &self.instance_name, 1, 18446744073709551615)?;
        self.min_bounds.encode(writer)?;
        self.max_bounds.encode(writer)?;
        self.dimension_type.encode(writer)?;
        self.engine_version.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AddVolumeEntity {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let entity_network_id = <EntityNetId as wire::Decode>::decode(reader)?;
        let components = <wire::NetworkNbt as wire::Decode>::decode(reader)?;
        let json_identifier = wire::decode_string_limits(
            reader,
            crate::fields::ADD_VOLUME_ENTITY_JSON_IDENTIFIER,
            1,
            18446744073709551615,
        )?;
        let instance_name = wire::decode_string_limits(
            reader,
            crate::fields::ADD_VOLUME_ENTITY_INSTANCE_NAME,
            1,
            18446744073709551615,
        )?;
        let min_bounds = <BlockPos as wire::Decode>::decode(reader)?;
        let max_bounds = <BlockPos as wire::Decode>::decode(reader)?;
        let dimension_type = <DimensionType as wire::Decode>::decode(reader)?;
        let engine_version = reader.read_str(crate::fields::ADD_VOLUME_ENTITY_ENGINE_VERSION)?;
        Ok(Self {
            entity_network_id,
            components,
            json_identifier,
            instance_name,
            min_bounds,
            max_bounds,
            dimension_type,
            engine_version,
        })
    }
}

/// RemoveVolumeEntity indicates a volume entity to be removed from server to client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RemoveVolumeEntity {
    pub entity_network_id: EntityNetId,
    pub dimension_type: DimensionType,
}

impl RemoveVolumeEntity {
    pub const ID: u32 = 167;
}
impl wire::Encode for RemoveVolumeEntity {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.entity_network_id.encode(writer)?;
        self.dimension_type.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RemoveVolumeEntity {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let entity_network_id = <EntityNetId as wire::Decode>::decode(reader)?;
        let dimension_type = <DimensionType as wire::Decode>::decode(reader)?;
        Ok(Self {
            entity_network_id,
            dimension_type,
        })
    }
}

/// SimulationType is an in-progress packet. We currently do not know the use case.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SimulationType {
    /// `sim_type` is the simulation type selected.
    pub sim_type: SimulationTypeEnum,
}

impl SimulationType {
    pub const ID: u32 = 168;
}
impl wire::Encode for SimulationType {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.sim_type.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SimulationType {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let sim_type = <SimulationTypeEnum as wire::Decode>::decode(reader)?;
        Ok(Self { sim_type })
    }
}

/// NPCDialogue is a packet that allows the client to display dialog boxes for interacting with NPCs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NpcDialogue {
    /// EntityUniqueID is the unique ID of the NPC being requested.
    pub npc_id_raw_id: wire::U64LE,
    /// ActionType is the type of action for the packet.
    pub npc_dialogue_action_type: NpcDialogueActionType,
    /// `dialogue` is the text that the client should see.
    pub dialogue: wire::Str,
    /// `scene_name` is the identifier of the scene. If this is left empty, the client will use the last scene
    /// sent to it. https://docs.microsoft.com/en-us/minecraft/creator/documents/npcdialogue.
    pub scene_name: wire::Str,
    /// NPCName is the name of the NPC to be displayed to the client.
    pub npc_name: wire::Str,
    /// `action_json` is the JSON string of the buttons/actions the server can perform.
    pub action_json: wire::Str,
}

impl NpcDialogue {
    pub const ID: u32 = 169;
}
impl wire::Encode for NpcDialogue {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.npc_id_raw_id.encode(writer)?;
        wire::check_number_limits(self.npc_id_raw_id.0, Some(0), None)?;
        self.npc_dialogue_action_type.encode(writer)?;
        self.dialogue.encode(writer)?;
        self.scene_name.encode(writer)?;
        self.npc_name.encode(writer)?;
        self.action_json.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for NpcDialogue {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let npc_id_raw_id = {
            let value = <wire::U64LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let npc_dialogue_action_type = <NpcDialogueActionType as wire::Decode>::decode(reader)?;
        let dialogue = reader.read_str(crate::fields::NPC_DIALOGUE_DIALOGUE)?;
        let scene_name = reader.read_str(crate::fields::NPC_DIALOGUE_SCENE_NAME)?;
        let npc_name = reader.read_str(crate::fields::NPC_DIALOGUE_NPC_NAME)?;
        let action_json = reader.read_str(crate::fields::NPC_DIALOGUE_ACTION_JSON)?;
        Ok(Self {
            npc_id_raw_id,
            npc_dialogue_action_type,
            dialogue,
            scene_name,
            npc_name,
            action_json,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EduUriResource {
    pub edu_shared_uri_resource: EduSharedUriResource,
}

impl EduUriResource {
    pub const ID: u32 = 170;
}
impl wire::Encode for EduUriResource {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.edu_shared_uri_resource.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EduUriResource {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let edu_shared_uri_resource = <EduSharedUriResource as wire::Decode>::decode(reader)?;
        Ok(Self {
            edu_shared_uri_resource,
        })
    }
}

/// CreatePhoto is a packet that allows players to export photos from their portfolios into items in their
/// inventory. This packet only works on the Education Edition version of Minecraft.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CreatePhoto {
    /// EntityUniqueID is the unique ID of the entity.
    pub raw_id: wire::U64LE,
    /// `photo_name` is the name of the photo.
    pub photo_name: wire::Str,
    /// ItemName is the name of the photo as an item.
    pub photo_item_name: wire::Str,
}

impl CreatePhoto {
    pub const ID: u32 = 171;
}
impl wire::Encode for CreatePhoto {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.raw_id.encode(writer)?;
        wire::check_number_limits(self.raw_id.0, Some(0), None)?;
        self.photo_name.encode(writer)?;
        self.photo_item_name.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CreatePhoto {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let raw_id = {
            let value = <wire::U64LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let photo_name = reader.read_str(crate::fields::CREATE_PHOTO_PHOTO_NAME)?;
        let photo_item_name = reader.read_str(crate::fields::CREATE_PHOTO_PHOTO_ITEM_NAME)?;
        Ok(Self {
            raw_id,
            photo_name,
            photo_item_name,
        })
    }
}

/// UpdateSubChunkBlocks is essentially just UpdateBlock packet, however for a set of blocks in a sub-chunk.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateSubChunkBlocks {
    /// Position is the block position of the sub-chunk being referred to.
    pub sub_chunk_block_position: BlockPos,
    pub blocks_changed: UpdateSubChunkBlocksChangedInfo,
}

impl UpdateSubChunkBlocks {
    pub const ID: u32 = 172;
}
impl wire::Encode for UpdateSubChunkBlocks {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.sub_chunk_block_position.encode(writer)?;
        self.blocks_changed.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for UpdateSubChunkBlocks {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let sub_chunk_block_position = <BlockPos as wire::Decode>::decode(reader)?;
        let blocks_changed = <UpdateSubChunkBlocksChangedInfo as wire::Decode>::decode(reader)?;
        Ok(Self {
            sub_chunk_block_position,
            blocks_changed,
        })
    }
}

/// SubChunk sends data about multiple sub-chunks around a center point.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubChunk {
    /// `cache_enabled` is whether the sub-chunk caching is enabled or not.
    pub cache_enabled: bool,
    pub dimension_type: DimensionType,
    pub center_pos: SubChunkPos,
    pub sub_chunk_data: Vec<SubChunkData>,
}

impl SubChunk {
    pub const ID: u32 = 174;
}
impl wire::Encode for SubChunk {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.cache_enabled.encode(writer)?;
        self.dimension_type.encode(writer)?;
        self.center_pos.encode(writer)?;
        wire::encode_collection_limits(writer, self.sub_chunk_data.as_slice(), 0, 8192)?;
        Ok(())
    }
}

impl wire::Decode for SubChunk {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let cache_enabled = <bool as wire::Decode>::decode(reader)?;
        let dimension_type = <DimensionType as wire::Decode>::decode(reader)?;
        let center_pos = <SubChunkPos as wire::Decode>::decode(reader)?;
        let sub_chunk_data = wire::decode_collection_limits::<SubChunkData>(
            reader,
            crate::fields::SUB_CHUNK_SUB_CHUNK_DATA,
            10,
            0,
            8192,
        )?;
        Ok(Self {
            cache_enabled,
            dimension_type,
            center_pos,
            sub_chunk_data,
        })
    }
}

/// SubChunkRequest requests specific sub-chunks from the server using a center point.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubChunkRequest {
    pub dimension_type: DimensionType,
    pub sub_chunk_position_offset_list: Vec<SubChunkPosOffset>,
    pub center_pos: SubChunkPos,
}

impl SubChunkRequest {
    pub const ID: u32 = 175;
}
impl wire::Encode for SubChunkRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.dimension_type.encode(writer)?;
        wire::encode_collection_limits(
            writer,
            self.sub_chunk_position_offset_list.as_slice(),
            0,
            8192,
        )?;
        self.center_pos.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SubChunkRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let dimension_type = <DimensionType as wire::Decode>::decode(reader)?;
        let sub_chunk_position_offset_list = wire::decode_collection_limits::<SubChunkPosOffset>(
            reader,
            crate::fields::SUB_CHUNK_REQUEST_SUB_CHUNK_POSITION_OFFSET_LIST,
            3,
            0,
            8192,
        )?;
        let center_pos = <SubChunkPos as wire::Decode>::decode(reader)?;
        Ok(Self {
            dimension_type,
            sub_chunk_position_offset_list,
            center_pos,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerStartItemCooldown {
    pub item_category: wire::Str,
    pub duration_ticks: wire::ZigZag32,
}

impl PlayerStartItemCooldown {
    pub const ID: u32 = 176;
}
impl wire::Encode for PlayerStartItemCooldown {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.item_category.encode(writer)?;
        self.duration_ticks.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerStartItemCooldown {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let item_category =
            reader.read_str(crate::fields::PLAYER_START_ITEM_COOLDOWN_ITEM_CATEGORY)?;
        let duration_ticks = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        Ok(Self {
            item_category,
            duration_ticks,
        })
    }
}

/// ScriptMessage is used to communicate custom messages from the client to the server, or from the server to
/// the client. While the name may suggest this packet is used for the discontinued scripting API, it is
/// likely instead for the GameTest framework.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScriptMessage {
    /// Identifier is the identifier of the message, used by either party to identify the message data sent.
    pub message_id: wire::Str,
    /// Data contains the data of the message.
    pub message_value: bytes::Bytes,
}

impl ScriptMessage {
    pub const ID: u32 = 177;
}
impl wire::Encode for ScriptMessage {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.message_id.encode(writer)?;
        self.message_value.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ScriptMessage {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let message_id = reader.read_str(crate::fields::SCRIPT_MESSAGE_MESSAGE_ID)?;
        let message_value =
            reader.read_shared_bytes(crate::fields::SCRIPT_MESSAGE_MESSAGE_VALUE)?;
        Ok(Self {
            message_id,
            message_value,
        })
    }
}

/// CodeBuilderSource is an Education Edition packet sent by the client to the server to run an operation with
/// a code builder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CodeBuilderSource {
    /// `operation` is used to distinguish the operation performed. It is always one of the constants listed
    /// above.
    pub operation: CodeBuilderStorageQueryOptionsOperation,
    /// `category` is used to distinguish the category of the operation performed. It is always one of the
    /// constants listed above.
    pub category: CodeBuilderStorageQueryOptionsCategory,
    /// `code_status` is the status of the code builder. It is always one of the constants listed above.
    pub code_status: CodeBuilderExecutionStateCodeStatus,
}

impl CodeBuilderSource {
    pub const ID: u32 = 178;
}
impl wire::Encode for CodeBuilderSource {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.operation.encode(writer)?;
        self.category.encode(writer)?;
        self.code_status.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CodeBuilderSource {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let operation = <CodeBuilderStorageQueryOptionsOperation as wire::Decode>::decode(reader)?;
        let category = <CodeBuilderStorageQueryOptionsCategory as wire::Decode>::decode(reader)?;
        let code_status = <CodeBuilderExecutionStateCodeStatus as wire::Decode>::decode(reader)?;
        Ok(Self {
            operation,
            category,
            code_status,
        })
    }
}

/// TickingAreasLoadStatus is sent by the server to the client to notify the client of a ticking area's
/// loading status.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TickingAreasLoadStatus {
    /// `waiting_for_preload` is true if the server is waiting for the area's preload.
    pub waiting_for_preload: bool,
}

impl TickingAreasLoadStatus {
    pub const ID: u32 = 179;
}
impl wire::Encode for TickingAreasLoadStatus {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.waiting_for_preload.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for TickingAreasLoadStatus {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let waiting_for_preload = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            waiting_for_preload,
        })
    }
}

/// DimensionData is a packet sent from the server to the client containing information about data-driven
/// dimensions that the server may have registered. This packet does not seem to be sent by default, rather
/// only being sent when any data-driven dimensions are registered.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DimensionData {
    /// `definitions` contain a list of data-driven dimension definitions registered on the server.
    pub definitions: Vec<(wire::Str, DimensionDefinition)>,
}

impl DimensionData {
    pub const ID: u32 = 180;
}
impl wire::Encode for DimensionData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        writer.write_len_var_u32(self.definitions.len())?;
        for (key, value) in self.definitions.iter() {
            wire::encode_string_limits(writer, key, 0, 256)?;
            value.encode(writer)?;
        }
        Ok(())
    }
}

impl wire::Decode for DimensionData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let definitions = {
            let declared = u64::from(reader.read_var_u32()?);
            let count =
                reader.checked_count(crate::fields::DIMENSION_DATA_DEFINITIONS, declared, 22)?;
            let mut out: Vec<(wire::Str, DimensionDefinition)> = wire::alloc_vec(count)?;
            for _ in 0..count {
                let key = wire::decode_string_limits(
                    reader,
                    crate::fields::DIMENSION_DATA_DEFINITIONS_KEY,
                    0,
                    256,
                )?;
                let value = <DimensionDefinition as wire::Decode>::decode(reader)?;
                wire::push(&mut out, (key, value))?;
            }
            out
        };
        Ok(Self { definitions })
    }
}

/// AgentAction is an Education Edition packet sent from the server to the client to return a response to a
/// previously requested action.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentActionEvent {
    /// Identifier is a JSON identifier referenced in the initial action.
    pub request_id: wire::Str,
    /// `action` represents the action type that was requested. It is one of the constants defined above.
    pub action: AgentActionType,
    /// `response` is a JSON string containing the response to the action.
    pub response: wire::Str,
}

impl AgentActionEvent {
    pub const ID: u32 = 181;
}
impl wire::Encode for AgentActionEvent {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.request_id.encode(writer)?;
        self.action.encode(writer)?;
        self.response.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AgentActionEvent {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let request_id = reader.read_str(crate::fields::AGENT_ACTION_EVENT_REQUEST_ID)?;
        let action = <AgentActionType as wire::Decode>::decode(reader)?;
        let response = reader.read_str(crate::fields::AGENT_ACTION_EVENT_RESPONSE)?;
        Ok(Self {
            request_id,
            action,
            response,
        })
    }
}

/// ChangeMobProperty is a packet sent from the server to the client to change one of the properties of a mob
/// client-side.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChangeMobProperty {
    /// EntityUniqueID is the unique ID of the entity whose property is being changed.
    pub actor_id: ActorUniqueID,
    /// Property is the name of the property being updated.
    pub property_name: wire::Str,
    /// BoolValue is set if the property value is a bool type. If the type is not a bool, this field is ignored.
    pub bool_component_value: bool,
    /// StringValue is set if the property value is a string type. If the type is not a string, this field is
    /// ignored.
    pub string_component_value: wire::Str,
    /// IntValue is set if the property value is an int type. If the type is not an int, this field is ignored.
    pub int_component_value: wire::ZigZag32,
    /// FloatValue is set if the property value is a float type. If the type is not a float, this field is
    /// ignored.
    pub float_component_value: wire::F32LE,
}

impl ChangeMobProperty {
    pub const ID: u32 = 182;
}
impl wire::Encode for ChangeMobProperty {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.actor_id.encode(writer)?;
        self.property_name.encode(writer)?;
        self.bool_component_value.encode(writer)?;
        self.string_component_value.encode(writer)?;
        self.int_component_value.encode(writer)?;
        self.float_component_value.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ChangeMobProperty {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let actor_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let property_name = reader.read_str(crate::fields::CHANGE_MOB_PROPERTY_PROPERTY_NAME)?;
        let bool_component_value = <bool as wire::Decode>::decode(reader)?;
        let string_component_value =
            reader.read_str(crate::fields::CHANGE_MOB_PROPERTY_STRING_COMPONENT_VALUE)?;
        let int_component_value = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let float_component_value = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            actor_id,
            property_name,
            bool_component_value,
            string_component_value,
            int_component_value,
            float_component_value,
        })
    }
}

/// LessonProgress is a packet sent by the server to the client to inform the client of updated progress on a
/// lesson. This packet only functions on the Minecraft: Education Edition version of the game.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LessonProgress {
    /// Action is the action the client should perform to show progress. This is one of the constants defined
    /// above.
    pub lesson_action: wire::ZigZag32,
    /// `score` is the score the client should use when displaying the progress.
    pub score: wire::ZigZag32,
    /// Identifier is the identifier of the lesson that is being progressed.
    pub activity_id: wire::Str,
}

impl LessonProgress {
    pub const ID: u32 = 183;
}
impl wire::Encode for LessonProgress {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.lesson_action.encode(writer)?;
        self.score.encode(writer)?;
        self.activity_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for LessonProgress {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let lesson_action = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let score = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let activity_id = reader.read_str(crate::fields::LESSON_PROGRESS_ACTIVITY_ID)?;
        Ok(Self {
            lesson_action,
            score,
            activity_id,
        })
    }
}

/// RequestAbility is a packet sent by the client to the server to request permission for a specific ability
/// from the server. These abilities are defined above.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RequestAbility {
    /// `ability` is the ability that the client is requesting. This is one of the constants defined in the
    /// protocol/ability.go file.
    pub ability: wire::ZigZag32,
    pub value_type: RequestAbilityType,
    pub bool: bool,
    pub float: wire::F32LE,
}

impl RequestAbility {
    pub const ID: u32 = 184;
}
impl wire::Encode for RequestAbility {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.ability.encode(writer)?;
        wire::check_number_limits(self.ability.0, Some(0), Some(19))?;
        self.value_type.encode(writer)?;
        self.bool.encode(writer)?;
        self.float.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RequestAbility {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let ability = {
            let value = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(19))?;
            value
        };
        let value_type = <RequestAbilityType as wire::Decode>::decode(reader)?;
        let bool = <bool as wire::Decode>::decode(reader)?;
        let float = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            ability,
            value_type,
            bool,
            float,
        })
    }
}

/// RequestPermissions is a packet sent from the client to the server to request permissions that the client
/// does not currently have. It can only be sent by operators and host in vanilla Minecraft.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RequestPermissions {
    /// `target_player_id_raw_id` is the unique ID of the player. The unique ID is unique for the entire world and
    /// is often used in packets. Most servers send an EntityUniqueID equal to the EntityRuntimeID.
    pub target_player_id_raw_id: wire::I64LE,
    /// `player_permission_level` is the current permission level of the player. This is one of the constants that
    /// may be found in the AdventureSettings packet.
    pub player_permission_level: wire::ZigZag32,
    /// `custom_permission_flags` contains the requested permission flags.
    pub custom_permission_flags: wire::U16LE,
}

impl RequestPermissions {
    pub const ID: u32 = 185;
}
impl wire::Encode for RequestPermissions {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_player_id_raw_id.encode(writer)?;
        self.player_permission_level.encode(writer)?;
        self.custom_permission_flags.encode(writer)?;
        wire::check_number_limits(self.custom_permission_flags.0, Some(0), None)?;
        Ok(())
    }
}

impl wire::Decode for RequestPermissions {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_player_id_raw_id = <wire::I64LE as wire::Decode>::decode(reader)?;
        let player_permission_level = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let custom_permission_flags = {
            let value = <wire::U16LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        Ok(Self {
            target_player_id_raw_id,
            player_permission_level,
            custom_permission_flags,
        })
    }
}

/// ToastRequest is a packet sent from the server to the client to display a toast to the top of the screen.
/// These toasts are the same as the ones seen when, for example, loading a new resource pack or obtaining an
/// achievement.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToastRequest {
    /// `title` is the title of the toast.
    pub title: wire::Str,
    /// `content` is the message that the toast may contain alongside the title.
    pub content: wire::Str,
}

impl ToastRequest {
    pub const ID: u32 = 186;
}
impl wire::Encode for ToastRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.title.encode(writer)?;
        self.content.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ToastRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let title = reader.read_str(crate::fields::TOAST_REQUEST_TITLE)?;
        let content = reader.read_str(crate::fields::TOAST_REQUEST_CONTENT)?;
        Ok(Self { title, content })
    }
}

/// UpdateAbilities is a packet sent from the server to the client to update the abilities of the player. It,
/// along with the UpdateAdventureSettings packet, are replacements of the AdventureSettings packet since
/// v1.19.10.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateAbilities {
    /// `data` represents various data about the abilities of a player, such as ability layers or permissions.
    pub data: SerializedAbilitiesData,
}

impl UpdateAbilities {
    pub const ID: u32 = 187;
}
impl wire::Encode for UpdateAbilities {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for UpdateAbilities {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let data = <SerializedAbilitiesData as wire::Decode>::decode(reader)?;
        Ok(Self { data })
    }
}

/// UpdateAdventureSettings is a packet sent from the server to the client to update the adventure settings of
/// the player. It, along with the UpdateAbilities packet, are replacements of the AdventureSettings packet
/// since v1.19.10.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateAdventureSettings {
    pub adventure_settings: AdventureSettings,
}

impl UpdateAdventureSettings {
    pub const ID: u32 = 188;
}
impl wire::Encode for UpdateAdventureSettings {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.adventure_settings.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for UpdateAdventureSettings {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let adventure_settings = <AdventureSettings as wire::Decode>::decode(reader)?;
        Ok(Self { adventure_settings })
    }
}

/// DeathInfo is a packet sent from the server to the client expected to be sent when a player dies. It
/// contains messages related to the player's death, which are shown on the death screen as of v1.19.10.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeathInfo {
    /// `death_cause_attack_name` is the cause of the player's death, such as "suffocation" or "suicide".
    pub death_cause_attack_name: wire::Str,
    /// `death_cause_message_list` is a list of death messages to be shown on the death screen.
    pub death_cause_message_list: Vec<wire::Str>,
}

impl DeathInfo {
    pub const ID: u32 = 189;
}
impl wire::Encode for DeathInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.death_cause_attack_name.encode(writer)?;
        writer.write_len_var_u32(self.death_cause_message_list.len())?;
        for item in self.death_cause_message_list.iter() {
            item.encode(writer)?;
        }
        Ok(())
    }
}

impl wire::Decode for DeathInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let death_cause_attack_name =
            reader.read_str(crate::fields::DEATH_INFO_DEATH_CAUSE_ATTACK_NAME)?;
        let death_cause_message_list = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::DEATH_INFO_DEATH_CAUSE_MESSAGE_LIST,
                declared,
                1,
            )?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    reader.read_str(crate::fields::DEATH_INFO_DEATH_CAUSE_MESSAGE_LIST_ITEM)?,
                )?;
            }
            out
        };
        Ok(Self {
            death_cause_attack_name,
            death_cause_message_list,
        })
    }
}

/// EditorNetwork is a packet sent from the server to the client and vise-versa to communicate editor-mode
/// related information. It carries a single compound tag containing the relevant information.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditorNetwork {
    /// `route_to_manager` ...
    pub route_to_manager: bool,
    /// `payload` is a network little endian compound tag holding data relevant to the editor.
    pub payload: wire::NetworkNbt,
}

impl EditorNetwork {
    pub const ID: u32 = 190;
}
impl wire::Encode for EditorNetwork {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.route_to_manager.encode(writer)?;
        self.payload.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for EditorNetwork {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let route_to_manager = <bool as wire::Decode>::decode(reader)?;
        let payload = <wire::NetworkNbt as wire::Decode>::decode(reader)?;
        Ok(Self {
            route_to_manager,
            payload,
        })
    }
}

/// FeatureRegistry is a packet used to notify the client about the world generation features the server is
/// currently using. This is used in combination with the client-side world generation system introduced in
/// v1.19.20, allowing the client to completely generate the chunks of the world without having to rely on the
/// server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeatureRegistry {
    /// `features_data_list` is a slice of all registered world generation features.
    pub features_data_list: Vec<FeatureRegistryFeatureBinaryJsonFormat>,
}

impl FeatureRegistry {
    pub const ID: u32 = 191;
}
impl wire::Encode for FeatureRegistry {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.features_data_list.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for FeatureRegistry {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let features_data_list = wire::decode_collection::<FeatureRegistryFeatureBinaryJsonFormat>(
            reader,
            crate::fields::FEATURE_REGISTRY_FEATURES_DATA_LIST,
            2,
        )?;
        Ok(Self { features_data_list })
    }
}

/// ServerStats is a packet sent from the server to the client to update the client on server statistics. It
/// is purely used for telemetry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerStats {
    /// `server_time` ...
    pub server_time: wire::F32LE,
    /// `network_time` ...
    pub network_time: wire::F32LE,
}

impl ServerStats {
    pub const ID: u32 = 192;
}
impl wire::Encode for ServerStats {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.server_time.encode(writer)?;
        self.network_time.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerStats {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let server_time = <wire::F32LE as wire::Decode>::decode(reader)?;
        let network_time = <wire::F32LE as wire::Decode>::decode(reader)?;
        Ok(Self {
            server_time,
            network_time,
        })
    }
}

/// RequestNetworkSettings is sent by the client to request network settings, such as compression, from the
/// server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RequestNetworkSettings {
    /// `client_network_version` is the protocol version of the player. The player is disconnected if the protocol
    /// is incompatible with the protocol of the server.
    pub client_network_version: wire::I32BE,
}

impl RequestNetworkSettings {
    pub const ID: u32 = 193;
}
impl wire::Encode for RequestNetworkSettings {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.client_network_version.encode(writer)?;
        wire::check_number_limits(self.client_network_version.0, Some(2193), Some(2193))?;
        Ok(())
    }
}

impl wire::Decode for RequestNetworkSettings {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let client_network_version = {
            let value = <wire::I32BE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(2193), Some(2193))?;
            value
        };
        Ok(Self {
            client_network_version,
        })
    }
}

/// GameTestRequest ...
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GameTestRequest {
    /// `max_tests_per_batch` ...
    pub max_tests_per_batch: wire::ZigZag32,
    pub repeat_count: wire::ZigZag32,
    /// `rotation` represents the rotation of the test. It is one of the constants above.
    pub rotation: Rotation,
    pub stop_on_failure: bool,
    pub test_pos: BlockPos,
    /// `tests_per_row` ...
    pub tests_per_row: wire::ZigZag32,
    /// Name represents the name of the test.
    pub test_name: wire::Str,
}

impl GameTestRequest {
    pub const ID: u32 = 194;
}
impl wire::Encode for GameTestRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.max_tests_per_batch.encode(writer)?;
        self.repeat_count.encode(writer)?;
        self.rotation.encode(writer)?;
        self.stop_on_failure.encode(writer)?;
        self.test_pos.encode(writer)?;
        self.tests_per_row.encode(writer)?;
        self.test_name.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for GameTestRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let max_tests_per_batch = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let repeat_count = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let rotation = <Rotation as wire::Decode>::decode(reader)?;
        let stop_on_failure = <bool as wire::Decode>::decode(reader)?;
        let test_pos = <BlockPos as wire::Decode>::decode(reader)?;
        let tests_per_row = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let test_name = reader.read_str(crate::fields::GAME_TEST_REQUEST_TEST_NAME)?;
        Ok(Self {
            max_tests_per_batch,
            repeat_count,
            rotation,
            stop_on_failure,
            test_pos,
            tests_per_row,
            test_name,
        })
    }
}

/// GameTestResults is a packet sent in response to the GameTestRequest packet, with a boolean indicating
/// whether the test was successful or not, and an error string if the test failed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GameTestResults {
    /// `succeeded` indicates whether the test succeeded or not.
    pub succeeded: bool,
    /// `error` is the error that occurred. If Succeeded is true, this field is empty.
    pub error: wire::Str,
    /// Name represents the name of the test.
    pub test_name: wire::Str,
}

impl GameTestResults {
    pub const ID: u32 = 195;
}
impl wire::Encode for GameTestResults {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.succeeded.encode(writer)?;
        self.error.encode(writer)?;
        self.test_name.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for GameTestResults {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let succeeded = <bool as wire::Decode>::decode(reader)?;
        let error = reader.read_str(crate::fields::GAME_TEST_RESULTS_ERROR)?;
        let test_name = reader.read_str(crate::fields::GAME_TEST_RESULTS_TEST_NAME)?;
        Ok(Self {
            succeeded,
            error,
            test_name,
        })
    }
}

/// UpdateClientInputLocks is sent by the server to the client to lock specific player inputs such as camera
/// rotation, movement, jumping, sneaking, mounting or individual directional movement.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateClientInputLocks {
    /// `input_lock_component_data` is a set of flags that specify which client inputs are disabled, such as
    /// whether the player can move, rotate the camera, jump, sneak or mount/dismount entities. It is a
    /// combination of the ClientInputLock constants above.
    pub input_lock_component_data: wire::VarUInt,
}

impl UpdateClientInputLocks {
    pub const ID: u32 = 196;
}
impl wire::Encode for UpdateClientInputLocks {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.input_lock_component_data.encode(writer)?;
        wire::check_number_limits(self.input_lock_component_data.0, Some(0), None)?;
        Ok(())
    }
}

impl wire::Decode for UpdateClientInputLocks {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let input_lock_component_data = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        Ok(Self {
            input_lock_component_data,
        })
    }
}

/// CameraPresets gives the client a list of custom camera presets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraPresets {
    pub camera_presets: CameraPresetList,
}

impl CameraPresets {
    pub const ID: u32 = 198;
}
impl wire::Encode for CameraPresets {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.camera_presets.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraPresets {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let camera_presets = <CameraPresetList as wire::Decode>::decode(reader)?;
        Ok(Self { camera_presets })
    }
}

/// UnlockedRecipes gives the client a list of recipes that have been unlocked, restricting the recipes that
/// appear in the recipe book.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnlockedRecipes {
    /// UnlockType is the type of unlock that the packet represents, and can either be adding or removing a list
    /// of recipes. It is one of the constants listed above.
    pub packet_type: PacketType,
    /// Recipes is a list of recipe names that have been unlocked.
    pub unlocked_recipes_list: Vec<wire::Str>,
}

impl UnlockedRecipes {
    pub const ID: u32 = 199;
}
impl wire::Encode for UnlockedRecipes {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.packet_type.encode(writer)?;
        writer.write_len_var_u32(self.unlocked_recipes_list.len())?;
        for item in self.unlocked_recipes_list.iter() {
            item.encode(writer)?;
        }
        Ok(())
    }
}

impl wire::Decode for UnlockedRecipes {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let packet_type = <PacketType as wire::Decode>::decode(reader)?;
        let unlocked_recipes_list = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::UNLOCKED_RECIPES_UNLOCKED_RECIPES_LIST,
                declared,
                1,
            )?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    reader.read_str(crate::fields::UNLOCKED_RECIPES_UNLOCKED_RECIPES_LIST_ITEM)?,
                )?;
            }
            out
        };
        Ok(Self {
            packet_type,
            unlocked_recipes_list,
        })
    }
}

/// CameraInstruction gives a custom camera specific instructions to operate.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraInstruction {
    pub camera_instruction: CameraInstructionData,
}

impl CameraInstruction {
    pub const ID: u32 = 300;
}
impl wire::Encode for CameraInstruction {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.camera_instruction.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraInstruction {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let camera_instruction = <CameraInstructionData as wire::Decode>::decode(reader)?;
        Ok(Self { camera_instruction })
    }
}

/// TrimData is sent by the server to the client when they first join the server. It contains a list of all
/// the patterns and materials that can be applied via armour trims.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrimData {
    /// `trim_pattern_list` is a list of patterns that can be applied to armour. Each pattern has its own style
    /// and texture that is defined through resource packs.
    pub trim_pattern_list: Vec<TrimPattern>,
    /// `trim_material_list` is a list of materials that can be applied to armour. These are mostly different ores
    /// that have different colours for more customization.
    pub trim_material_list: Vec<TrimMaterial>,
}

impl TrimData {
    pub const ID: u32 = 302;
}
impl wire::Encode for TrimData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.trim_pattern_list.as_slice())?;
        wire::encode_collection(writer, self.trim_material_list.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for TrimData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let trim_pattern_list = wire::decode_collection::<TrimPattern>(
            reader,
            crate::fields::TRIM_DATA_TRIM_PATTERN_LIST,
            2,
        )?;
        let trim_material_list = wire::decode_collection::<TrimMaterial>(
            reader,
            crate::fields::TRIM_DATA_TRIM_MATERIAL_LIST,
            3,
        )?;
        Ok(Self {
            trim_pattern_list,
            trim_material_list,
        })
    }
}

/// OpenSign is sent by the server to open a sign for editing. As of 1.19.80, the player can interact with a
/// sign to edit the text on both sides instead of just the front.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenSign {
    /// `pos` is the position of the sign to edit. The client uses this position to get the data of the sign,
    /// including the existing text and formatting etc.
    pub pos: BlockPos,
    /// `is_front_side` dictates whether the front side of the sign should be opened for editing. If false, the
    /// back side is assumed to be edited.
    pub is_front_side: bool,
}

impl OpenSign {
    pub const ID: u32 = 303;
}
impl wire::Encode for OpenSign {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.pos.encode(writer)?;
        self.is_front_side.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for OpenSign {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let pos = <BlockPos as wire::Decode>::decode(reader)?;
        let is_front_side = <bool as wire::Decode>::decode(reader)?;
        Ok(Self { pos, is_front_side })
    }
}

/// AgentAnimation is an Education Edition packet sent from the server to the client to make an agent perform
/// an animation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentAnimation {
    /// Animation is the ID of the animation that the agent should perform. As of its implementation, there are no
    /// IDs that can be used in the regular client.
    pub agent_animation: AgentAnimationType,
    /// EntityRuntimeID is the runtime ID of the entity. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub runtime_id: ActorRuntimeID,
}

impl AgentAnimation {
    pub const ID: u32 = 304;
}
impl wire::Encode for AgentAnimation {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.agent_animation.encode(writer)?;
        self.runtime_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AgentAnimation {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let agent_animation = <AgentAnimationType as wire::Decode>::decode(reader)?;
        let runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        Ok(Self {
            agent_animation,
            runtime_id,
        })
    }
}

/// RefreshEntitlements is sent by the client to the server to refresh the entitlements of the player.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RefreshEntitlements {}

impl RefreshEntitlements {
    pub const ID: u32 = 305;
}
impl wire::Encode for RefreshEntitlements {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        let _ = writer;
        Ok(())
    }
}

impl wire::Decode for RefreshEntitlements {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let _ = reader;
        Ok(Self {})
    }
}

/// PlayerToggleCrafterSlotRequest is sent by the client when it tries to toggle the state of a slot within a
/// Crafter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerToggleCrafterSlotRequest {
    /// `pos_x` is the X position of the Crafter that is being modified.
    pub pos_x: wire::I32LE,
    /// `pos_y` is the Y position of the Crafter that is being modified.
    pub pos_y: wire::I32LE,
    /// `pos_z` is the Z position of the Crafter that is being modified.
    pub pos_z: wire::I32LE,
    /// Slot is the index of the slot that was toggled. This should be a value between 0 and 8.
    pub slot_index: wire::U8,
    /// Disabled is the new state of the slot. If true, the slot is disabled, if false, the slot is enabled.
    pub is_disabled: bool,
}

impl PlayerToggleCrafterSlotRequest {
    pub const ID: u32 = 306;
}
impl wire::Encode for PlayerToggleCrafterSlotRequest {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.pos_x.encode(writer)?;
        self.pos_y.encode(writer)?;
        self.pos_z.encode(writer)?;
        self.slot_index.encode(writer)?;
        wire::check_number_limits(self.slot_index.0, Some(0), Some(255))?;
        self.is_disabled.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerToggleCrafterSlotRequest {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let pos_x = <wire::I32LE as wire::Decode>::decode(reader)?;
        let pos_y = <wire::I32LE as wire::Decode>::decode(reader)?;
        let pos_z = <wire::I32LE as wire::Decode>::decode(reader)?;
        let slot_index = {
            let value = <wire::U8 as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), Some(255))?;
            value
        };
        let is_disabled = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            pos_x,
            pos_y,
            pos_z,
            slot_index,
            is_disabled,
        })
    }
}

/// SetPlayerInventoryOptions is a bidirectional packet that can be used to update the inventory options of a
/// player.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetPlayerInventoryOptions {
    pub inventory_options: InventoryOptions,
}

impl SetPlayerInventoryOptions {
    pub const ID: u32 = 307;
}
impl wire::Encode for SetPlayerInventoryOptions {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.inventory_options.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetPlayerInventoryOptions {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let inventory_options = <InventoryOptions as wire::Decode>::decode(reader)?;
        Ok(Self { inventory_options })
    }
}

/// SetHud is sent by the server to set the visibility of individual HUD elements on the client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetHud {
    /// Elements is a list of HUD elements that are being modified. The values can be any of the HudElement
    /// constants above.
    pub hud_element: Vec<HudElement>,
    /// Visibility represents the new visibility of the specified Elements. It can be any of the HudVisibility
    /// constants above.
    pub hud_visible: HudVisibility,
}

impl SetHud {
    pub const ID: u32 = 308;
}
impl wire::Encode for SetHud {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.hud_element.as_slice())?;
        self.hud_visible.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetHud {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let hud_element =
            wire::decode_collection::<HudElement>(reader, crate::fields::SET_HUD_HUD_ELEMENT, 1)?;
        let hud_visible = <HudVisibility as wire::Decode>::decode(reader)?;
        Ok(Self {
            hud_element,
            hud_visible,
        })
    }
}

/// AwardAchievement is sent by the server to award an achievement to a player.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AwardAchievement {
    /// `achievement_id` is the ID of the achievement that should be awarded to the player. The values for these
    /// IDs are currently unknown.
    pub achievement_id: wire::I32LE,
}

impl AwardAchievement {
    pub const ID: u32 = 309;
}
impl wire::Encode for AwardAchievement {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.achievement_id.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for AwardAchievement {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let achievement_id = <wire::I32LE as wire::Decode>::decode(reader)?;
        Ok(Self { achievement_id })
    }
}

/// ClientBoundCloseForm is sent by the server to clear the entire form stack of the client. This means that
/// all forms that are currently open will be closed. This does not affect inventories and other containers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientboundCloseForm {}

impl ClientboundCloseForm {
    pub const ID: u32 = 310;
}
impl wire::Encode for ClientboundCloseForm {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        let _ = writer;
        Ok(())
    }
}

impl wire::Decode for ClientboundCloseForm {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let _ = reader;
        Ok(Self {})
    }
}

/// ServerBoundLoadingScreen is sent by the client to tell the server about the state of the loading screen
/// that the client is currently displaying.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerboundLoadingScreen {
    /// Type is the type of the loading screen event. It is one of the constants that may be found above.
    pub loading_screen_packet_type: ServerboundLoadingScreenType,
    /// `loading_screen_id` is the ID of the screen that was previously sent by the server in the ChangeDimension
    /// packet. The server should validate that the ID matches the last one it sent.
    /// Wire presence: optional value is preceded by a presence marker.
    pub loading_screen_id: Option<wire::U32LE>,
}

impl ServerboundLoadingScreen {
    pub const ID: u32 = 312;
}
impl wire::Encode for ServerboundLoadingScreen {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.loading_screen_packet_type.encode(writer)?;
        match &self.loading_screen_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ServerboundLoadingScreen {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let loading_screen_packet_type =
            <ServerboundLoadingScreenType as wire::Decode>::decode(reader)?;
        let loading_screen_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::U32LE as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            loading_screen_packet_type,
            loading_screen_id,
        })
    }
}

/// JigsawStructureData is sent by the server to let the client know all the rules for jigsaw structures.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JigsawStructureData {
    pub jigsaw_structure_data_tag: wire::NetworkNbt,
}

impl JigsawStructureData {
    pub const ID: u32 = 313;
}
impl wire::Encode for JigsawStructureData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.jigsaw_structure_data_tag.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for JigsawStructureData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let jigsaw_structure_data_tag = <wire::NetworkNbt as wire::Decode>::decode(reader)?;
        Ok(Self {
            jigsaw_structure_data_tag,
        })
    }
}

/// CurrentStructureFeature is sent by the server to let the client know the name of the structure feature
/// that the player is currently occupying.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CurrentStructureFeature {
    /// `current_structure_feature` is the identifier of the structure feature that the player is currently
    /// occupying. If the player is not occupying any structure feature, this field is empty.
    pub current_structure_feature: wire::Str,
}

impl CurrentStructureFeature {
    pub const ID: u32 = 314;
}
impl wire::Encode for CurrentStructureFeature {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.current_structure_feature.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CurrentStructureFeature {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let current_structure_feature =
            reader.read_str(crate::fields::CURRENT_STRUCTURE_FEATURE_CURRENT_STRUCTURE_FEATURE)?;
        Ok(Self {
            current_structure_feature,
        })
    }
}

/// ServerBoundDiagnostics is sent by the client to tell the server about the performance diagnostics of the
/// client. It is sent by the client roughly every 500ms or 10 in-game ticks when the "Creator > Enable Client
/// Diagnostics" setting is enabled.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerboundDiagnostics {
    /// AverageFramesPerSecond is the average amount of frames per second that the client has been running at.
    pub avg_fps: wire::F32LE,
    /// AverageServerSimTickTime is the average time that the server spends simulating a single tick in
    /// milliseconds.
    pub avg_server_sim_tick_time_ms: wire::F32LE,
    /// AverageClientSimTickTime is the average time that the client spends simulating a single tick in
    /// milliseconds.
    pub avg_client_sim_tick_time_ms: wire::F32LE,
    /// AverageBeginFrameTime is the average time that the client spends beginning a frame in milliseconds.
    pub avg_begin_frame_time_ms: wire::F32LE,
    /// AverageInputTime is the average time that the client spends processing input in milliseconds.
    pub avg_input_time_ms: wire::F32LE,
    /// AverageRenderTime is the average time that the client spends rendering in milliseconds.
    pub avg_render_time_ms: wire::F32LE,
    /// AverageEndFrameTime is the average time that the client spends ending a frame in milliseconds.
    pub avg_end_frame_time_ms: wire::F32LE,
    /// AverageRemainderTimePercent is the average percentage of time that the client spends on tasks that are not
    /// accounted for.
    pub avg_remainder_time_percent: wire::F32LE,
    /// AverageUnaccountedTimePercent is the average percentage of time that the client spends on unaccounted
    /// tasks.
    pub avg_unaccounted_time_percent: wire::F32LE,
    /// `memory_category_values` is a list of memory category counters sent by the client.
    pub memory_category_values: Vec<MemoryCategoryCounter>,
    /// `entity_diagnostics` is a list of entity timing entries sent by the client.
    pub entity_diagnostics: Vec<ECSProfilingDiagnosticsEntityDiagnosticTimingInfo>,
    /// `system_diagnostics` is a list of system timing entries sent by the client.
    pub system_diagnostics: Vec<ECSProfilingDiagnosticsSystemDiagnosticTimingInfo>,
    /// `system_categories` maps diagnostics category names to system indices.
    pub system_categories: Vec<ECSProfilingDiagnosticsSystemCategory>,
    /// `whisker_scopes` is a list of whisker profiler scope diagnostic summaries sent by the client.
    pub whisker_scopes: Vec<BedrockProfileWhiskerDiagnosticsScopeDataSummary>,
}

impl ServerboundDiagnostics {
    pub const ID: u32 = 315;
}
impl wire::Encode for ServerboundDiagnostics {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.avg_fps.encode(writer)?;
        self.avg_server_sim_tick_time_ms.encode(writer)?;
        self.avg_client_sim_tick_time_ms.encode(writer)?;
        self.avg_begin_frame_time_ms.encode(writer)?;
        self.avg_input_time_ms.encode(writer)?;
        self.avg_render_time_ms.encode(writer)?;
        self.avg_end_frame_time_ms.encode(writer)?;
        self.avg_remainder_time_percent.encode(writer)?;
        self.avg_unaccounted_time_percent.encode(writer)?;
        wire::encode_collection(writer, self.memory_category_values.as_slice())?;
        wire::encode_collection(writer, self.entity_diagnostics.as_slice())?;
        wire::encode_collection(writer, self.system_diagnostics.as_slice())?;
        wire::encode_collection(writer, self.system_categories.as_slice())?;
        wire::encode_collection(writer, self.whisker_scopes.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for ServerboundDiagnostics {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let avg_fps = <wire::F32LE as wire::Decode>::decode(reader)?;
        let avg_server_sim_tick_time_ms = <wire::F32LE as wire::Decode>::decode(reader)?;
        let avg_client_sim_tick_time_ms = <wire::F32LE as wire::Decode>::decode(reader)?;
        let avg_begin_frame_time_ms = <wire::F32LE as wire::Decode>::decode(reader)?;
        let avg_input_time_ms = <wire::F32LE as wire::Decode>::decode(reader)?;
        let avg_render_time_ms = <wire::F32LE as wire::Decode>::decode(reader)?;
        let avg_end_frame_time_ms = <wire::F32LE as wire::Decode>::decode(reader)?;
        let avg_remainder_time_percent = <wire::F32LE as wire::Decode>::decode(reader)?;
        let avg_unaccounted_time_percent = <wire::F32LE as wire::Decode>::decode(reader)?;
        let memory_category_values = wire::decode_collection::<MemoryCategoryCounter>(
            reader,
            crate::fields::SERVERBOUND_DIAGNOSTICS_MEMORY_CATEGORY_VALUES,
            9,
        )?;
        let entity_diagnostics =
            wire::decode_collection::<ECSProfilingDiagnosticsEntityDiagnosticTimingInfo>(
                reader,
                crate::fields::SERVERBOUND_DIAGNOSTICS_ENTITY_DIAGNOSTICS,
                13,
            )?;
        let system_diagnostics =
            wire::decode_collection::<ECSProfilingDiagnosticsSystemDiagnosticTimingInfo>(
                reader,
                crate::fields::SERVERBOUND_DIAGNOSTICS_SYSTEM_DIAGNOSTICS,
                18,
            )?;
        let system_categories = wire::decode_collection::<ECSProfilingDiagnosticsSystemCategory>(
            reader,
            crate::fields::SERVERBOUND_DIAGNOSTICS_SYSTEM_CATEGORIES,
            9,
        )?;
        let whisker_scopes =
            wire::decode_collection::<BedrockProfileWhiskerDiagnosticsScopeDataSummary>(
                reader,
                crate::fields::SERVERBOUND_DIAGNOSTICS_WHISKER_SCOPES,
                26,
            )?;
        Ok(Self {
            avg_fps,
            avg_server_sim_tick_time_ms,
            avg_client_sim_tick_time_ms,
            avg_begin_frame_time_ms,
            avg_input_time_ms,
            avg_render_time_ms,
            avg_end_frame_time_ms,
            avg_remainder_time_percent,
            avg_unaccounted_time_percent,
            memory_category_values,
            entity_diagnostics,
            system_diagnostics,
            system_categories,
            whisker_scopes,
        })
    }
}

/// CameraAimAssist is sent by the server to the client to set up aim assist for the client's camera.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraAimAssist {
    /// `preset_id` is the ID of the preset that has previously been defined in the CameraAimAssistPresets packet.
    pub preset_id: wire::Str,
    /// `view_angle` is the maximum angle around the playes's cursor that the aim assist should check for a
    /// target, if TargetMode is set to protocol.AimAssistTargetModeAngle.
    pub view_angle: glam::Vec2,
    /// `distance` is the maximum distance from the player's cursor should check for a target, if TargetMode is
    /// set to protocol.AimAssistTargetModeDistance.
    pub distance: wire::F32LE,
    /// `target_mode` is the mode that the camera should use for detecting targets. This is currently one of
    /// protocol.AimAssistTargetModeAngle or protocol.AimAssistTargetModeDistance.
    pub target_mode: TargetMode,
    /// `action` is the action that should be performed with the aim assist. This is one of the constants above.
    pub action: CameraAimAssistAction,
    /// `show_debug_render` specifies if debug render should be shown.
    pub show_debug_render: bool,
}

impl CameraAimAssist {
    pub const ID: u32 = 316;
}
impl wire::Encode for CameraAimAssist {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.preset_id.encode(writer)?;
        self.view_angle.encode(writer)?;
        self.distance.encode(writer)?;
        wire::check_number_limits(self.distance.0, Some(1.0), Some(16.0))?;
        self.target_mode.encode(writer)?;
        self.action.encode(writer)?;
        self.show_debug_render.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraAimAssist {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let preset_id = reader.read_str(crate::fields::CAMERA_AIM_ASSIST_PRESET_ID)?;
        let view_angle = <glam::Vec2 as wire::Decode>::decode(reader)?;
        let distance = {
            let value = <wire::F32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(1.0), Some(16.0))?;
            value
        };
        let target_mode = <TargetMode as wire::Decode>::decode(reader)?;
        let action = <CameraAimAssistAction as wire::Decode>::decode(reader)?;
        let show_debug_render = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            preset_id,
            view_angle,
            distance,
            target_mode,
            action,
            show_debug_render,
        })
    }
}

/// ContainerRegistryCleanup is sent by the server to trigger a client-side cleanup of the dynamic container
/// registry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContainerRegistryCleanup {
    /// `removed_containers` is a list of protocol.FullContainerName's that should be removed from the client-side
    /// container registry.
    pub removed_containers: Vec<FullContainerName>,
}

impl ContainerRegistryCleanup {
    pub const ID: u32 = 317;
}
impl wire::Encode for ContainerRegistryCleanup {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.removed_containers.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for ContainerRegistryCleanup {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let removed_containers = wire::decode_collection::<FullContainerName>(
            reader,
            crate::fields::CONTAINER_REGISTRY_CLEANUP_REMOVED_CONTAINERS,
            2,
        )?;
        Ok(Self { removed_containers })
    }
}

/// MovementEffect is sent by the server to the client to update specific movement effects to allow the client
/// to predict its movement. For example, fireworks used during gliding will send this packet to tell the
/// client the exact duration of the boost.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MovementEffect {
    /// EntityRuntimeID is the runtime ID of the entity. The runtime ID is unique for each world session, and
    /// entities are generally identified in packets using this runtime ID.
    pub target_runtime_id: ActorRuntimeID,
    /// Type is the type of movement effect being updated. It is one of the constants found above.
    pub effect_id: MovementEffectType,
    /// Duration is the duration of the effect, measured in ticks.
    pub effect_duration: wire::ZigZag32,
    /// `tick` is the server tick at which the packet was sent. It is used in relation to
    /// CorrectPlayerMovePrediction.
    pub tick: PlayerInputTick,
}

impl MovementEffect {
    pub const ID: u32 = 318;
}
impl wire::Encode for MovementEffect {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_runtime_id.encode(writer)?;
        self.effect_id.encode(writer)?;
        self.effect_duration.encode(writer)?;
        self.tick.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for MovementEffect {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_runtime_id = <ActorRuntimeID as wire::Decode>::decode(reader)?;
        let effect_id = <MovementEffectType as wire::Decode>::decode(reader)?;
        let effect_duration = <wire::ZigZag32 as wire::Decode>::decode(reader)?;
        let tick = <PlayerInputTick as wire::Decode>::decode(reader)?;
        Ok(Self {
            target_runtime_id,
            effect_id,
            effect_duration,
            tick,
        })
    }
}

/// CameraAimAssistPresets is sent by the server to the client to provide a list of categories and presets
/// that can be used when sending a CameraAimAssist packet or a CameraInstruction including aim assist.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraAimAssistPresets {
    /// `camera_aim_assist_presets` is a list of categories which can be referenced by one of the Presets.
    pub camera_aim_assist_presets: Vec<CameraAimAssistCategoryDefinition>,
    /// `camera_aim_assist_categories` is a list of presets which define a base for how aim assist should behave
    pub camera_aim_assist_categories: Vec<CameraAimAssistPresetDefinition>,
    /// `operation` is the operation to perform with the presets. It is one of the constants above.
    pub operation: CameraAimAssistPresetOperation,
}

impl CameraAimAssistPresets {
    pub const ID: u32 = 320;
}
impl wire::Encode for CameraAimAssistPresets {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.camera_aim_assist_presets.as_slice())?;
        wire::encode_collection(writer, self.camera_aim_assist_categories.as_slice())?;
        self.operation.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for CameraAimAssistPresets {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let camera_aim_assist_presets = wire::decode_collection::<CameraAimAssistCategoryDefinition>(
            reader,
            crate::fields::CAMERA_AIM_ASSIST_PRESETS_CAMERA_AIM_ASSIST_PRESETS,
            7,
        )?;
        let camera_aim_assist_categories =
            wire::decode_collection::<CameraAimAssistPresetDefinition>(
                reader,
                crate::fields::CAMERA_AIM_ASSIST_PRESETS_CAMERA_AIM_ASSIST_CATEGORIES,
                9,
            )?;
        let operation = <CameraAimAssistPresetOperation as wire::Decode>::decode(reader)?;
        Ok(Self {
            camera_aim_assist_presets,
            camera_aim_assist_categories,
            operation,
        })
    }
}

/// ClientCameraAimAssist is sent by the server to send a player animation from one player to all viewers of
/// that player. It is used for a couple of actions, such as arm swimming and critical hits.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientCameraAimAssist {
    /// `camera_preset_id` is the identifier of the preset to use which was previously defined in the
    /// CameraAimAssistPresets packet.
    pub camera_preset_id: wire::Str,
    /// `action` is the action to perform with the aim assist. It is one of the constants above.
    pub action: ClientCameraAimAssistAction,
    /// `allow_aim_assist` specifies the client can use aim assist or not.
    pub allow_aim_assist: bool,
}

impl ClientCameraAimAssist {
    pub const ID: u32 = 321;
}
impl wire::Encode for ClientCameraAimAssist {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.camera_preset_id.encode(writer)?;
        self.action.encode(writer)?;
        self.allow_aim_assist.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ClientCameraAimAssist {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let camera_preset_id =
            reader.read_str(crate::fields::CLIENT_CAMERA_AIM_ASSIST_CAMERA_PRESET_ID)?;
        let action = <ClientCameraAimAssistAction as wire::Decode>::decode(reader)?;
        let allow_aim_assist = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            camera_preset_id,
            action,
            allow_aim_assist,
        })
    }
}

/// ClientMovementPredictionSync is sent by the client to the server periodically if the client has received
/// movement corrections from the server, containing information about client-predictions that are relevant to
/// movement.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientMovementPredictionSync {
    pub actor_data_flag: ActorDataFlagComponent,
    pub actor_bounding_box: ActorDataBoundingBoxComponent,
    pub movement_attributes: [wire::F32LE; 9],
    /// EntityUniqueID is the unique ID of the entity that the prediction data applies to.
    pub actor_unique_id: ActorUniqueID,
    pub actor_flying_state: bool,
}

impl ClientMovementPredictionSync {
    pub const ID: u32 = 322;
}
impl wire::Encode for ClientMovementPredictionSync {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.actor_data_flag.encode(writer)?;
        self.actor_bounding_box.encode(writer)?;
        for item in self.movement_attributes.iter() {
            item.encode(writer)?;
        }
        self.actor_unique_id.encode(writer)?;
        self.actor_flying_state.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ClientMovementPredictionSync {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let actor_data_flag = <ActorDataFlagComponent as wire::Decode>::decode(reader)?;
        let actor_bounding_box = <ActorDataBoundingBoxComponent as wire::Decode>::decode(reader)?;
        let movement_attributes = [
            <wire::F32LE as wire::Decode>::decode(reader)?,
            <wire::F32LE as wire::Decode>::decode(reader)?,
            <wire::F32LE as wire::Decode>::decode(reader)?,
            <wire::F32LE as wire::Decode>::decode(reader)?,
            <wire::F32LE as wire::Decode>::decode(reader)?,
            <wire::F32LE as wire::Decode>::decode(reader)?,
            <wire::F32LE as wire::Decode>::decode(reader)?,
            <wire::F32LE as wire::Decode>::decode(reader)?,
            <wire::F32LE as wire::Decode>::decode(reader)?,
        ];
        let actor_unique_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let actor_flying_state = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            actor_data_flag,
            actor_bounding_box,
            movement_attributes,
            actor_unique_id,
            actor_flying_state,
        })
    }
}

/// UpdateClientOptions is sent by the client when some of the client's options are updated, such as the
/// graphics mode.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateClientOptions {
    /// `graphics_mode_change` is the graphics mode that the client is using. It is one of the constants above.
    /// Wire presence: optional value is preceded by a presence marker.
    pub graphics_mode_change: Option<GraphicsMode>,
    /// `filter_profanity_change` is if the client only uses filtered messages or not.
    /// Wire presence: optional value is preceded by a presence marker.
    pub filter_profanity_change: Option<bool>,
}

impl UpdateClientOptions {
    pub const ID: u32 = 323;
}
impl wire::Encode for UpdateClientOptions {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        match &self.graphics_mode_change {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.filter_profanity_change {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for UpdateClientOptions {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let graphics_mode_change = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<GraphicsMode as wire::Decode>::decode(reader)?)
            }
        };
        let filter_profanity_change = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<bool as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            graphics_mode_change,
            filter_profanity_change,
        })
    }
}

/// PlayerVideoCapture packet is sent by the server to start or stop video recording for a player. This packet
/// only works on development builds and has no effect on retail builds. When recording, the client will save
/// individual frames to '/LocalCache/minecraftpe' in the format specified below.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerVideoCapture {
    /// `action` is the action to perform with the video capture. It is one of the constants above.
    pub action: PlayerVideoCaptureData,
}

impl PlayerVideoCapture {
    pub const ID: u32 = 324;
}
impl wire::Encode for PlayerVideoCapture {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.action.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerVideoCapture {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let action = <PlayerVideoCaptureData as wire::Decode>::decode(reader)?;
        Ok(Self { action })
    }
}

/// PlayerUpdateEntityOverrides is sent by the server to modify an entity's properties individually.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerUpdateEntityOverrides {
    /// EntityUniqueID is the unique ID of the entity. The unique ID is a value that remains consistent across
    /// different sessions of the same world, but most servers simply fill the runtime ID of the entity out for
    /// this field.
    pub target_id: ActorUniqueID,
    /// `property_index` is the index of the property to modify. The index is unique for each property of an
    /// entity.
    pub property_index: wire::VarUInt,
    pub update: PlayerUpdateEntityOverridesData,
}

impl PlayerUpdateEntityOverrides {
    pub const ID: u32 = 325;
}
impl wire::Encode for PlayerUpdateEntityOverrides {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_id.encode(writer)?;
        self.property_index.encode(writer)?;
        wire::check_number_limits(self.property_index.0, Some(0), None)?;
        self.update.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerUpdateEntityOverrides {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let property_index = {
            let value = <wire::VarUInt as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let update = <PlayerUpdateEntityOverridesData as wire::Decode>::decode(reader)?;
        Ok(Self {
            target_id,
            property_index,
            update,
        })
    }
}

/// PlayerLocation is sent by the server to the client to either update a player's position on the locator
/// bar, or remove them completely. The client will determine how to render the player on the locator bar
/// based on their own distance to Position.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerLocation {
    /// EntityUniqueID is the unique ID of the entity. The unique ID is a value that remains consistent across
    /// different sessions of the same world.
    pub target_actor_id: ActorUniqueID,
    pub location: PlayerLocationData,
}

impl PlayerLocation {
    pub const ID: u32 = 326;
}
impl wire::Encode for PlayerLocation {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.target_actor_id.encode(writer)?;
        self.location.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PlayerLocation {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let target_actor_id = <ActorUniqueID as wire::Decode>::decode(reader)?;
        let location = <PlayerLocationData as wire::Decode>::decode(reader)?;
        Ok(Self {
            target_actor_id,
            location,
        })
    }
}

/// ClientBoundControlSchemeSet is sent by the server upon the client's request or the usage of the vanilla
/// /controlscheme command. It is used to set the control scheme of the client, often used in combination with
/// custom cameras.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientboundControlSchemeSet {
    /// `control_scheme` is the control scheme that the client should use. It is one of the following: -
    /// ControlSchemeLockedPlayerRelativeStrafe is the default behaviour, this cannot be set when the client is in
    /// a custom camera. - ControlSchemeCameraRelative makes movement relative to the camera's transform, with the
    /// client's rotation being relative to the client's movement. - ControlSchemeCameraRelativeStrafe makes
    /// movement relative to the camera's transform, with the client's rotation being locked. -
    /// ControlSchemePlayerRelative makes movement relative to the player's transform, meaning holding left/right
    /// will make the player turn in a circle. - ControlSchemePlayerRelativeStrafe makes movement the same as the
    /// default behaviour, but can be used in a custom camera.
    pub control_scheme: ControlScheme,
}

impl ClientboundControlSchemeSet {
    pub const ID: u32 = 327;
}
impl wire::Encode for ClientboundControlSchemeSet {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.control_scheme.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ClientboundControlSchemeSet {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let control_scheme = <ControlScheme as wire::Decode>::decode(reader)?;
        Ok(Self { control_scheme })
    }
}

/// PrimitiveShapes is a packet sent by the server to instruct the client to render one or more shapes in the
/// world. Shapes can be added, removed or updated based on the data provided individually.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrimitiveShapes {
    /// `array_of_primitive_shapes_can_be_a_mix_of_new_updated_or_removed` is a list of shapes to draw on the
    /// client-side.
    pub array_of_primitive_shapes_can_be_a_mix_of_new_updated_or_removed: Vec<PrimitiveShape>,
}

impl PrimitiveShapes {
    pub const ID: u32 = 328;
}
impl wire::Encode for PrimitiveShapes {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection_limits(
            writer,
            self.array_of_primitive_shapes_can_be_a_mix_of_new_updated_or_removed
                .as_slice(),
            0,
            1048576,
        )?;
        Ok(())
    }
}

impl wire::Decode for PrimitiveShapes {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let array_of_primitive_shapes_can_be_a_mix_of_new_updated_or_removed = wire::decode_collection_limits::<PrimitiveShape>(reader, crate::fields::PRIMITIVE_SHAPES_ARRAY_OF_PRIMITIVE_SHAPES_CAN_BE_A_MIX_OF_NEW_UPDATED_OR_REMOVED, 11, 0, 1048576)?;
        Ok(Self {
            array_of_primitive_shapes_can_be_a_mix_of_new_updated_or_removed,
        })
    }
}

/// ServerBoundPackSettingChange is sent by the client to the server when it changes a setting for a specific
/// pack in the pack settings UI.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerboundPackSettingChange {
    /// `pack_id` is the UUID of the pack.
    pub pack_id: uuid::Uuid,
    pub pack_setting_name: wire::Str,
    /// PackSetting is the new setting value applied to the pack.
    pub pack_setting_value: ServerboundPackSettingChangePackSettingValue,
}

impl ServerboundPackSettingChange {
    pub const ID: u32 = 329;
}
impl wire::Encode for ServerboundPackSettingChange {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.pack_id.encode(writer)?;
        wire::encode_string_limits(writer, &self.pack_setting_name, 0, 128)?;
        self.pack_setting_value.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerboundPackSettingChange {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let pack_id = <uuid::Uuid as wire::Decode>::decode(reader)?;
        let pack_setting_name = wire::decode_string_limits(
            reader,
            crate::fields::SERVERBOUND_PACK_SETTING_CHANGE_PACK_SETTING_NAME,
            0,
            128,
        )?;
        let pack_setting_value =
            <ServerboundPackSettingChangePackSettingValue as wire::Decode>::decode(reader)?;
        Ok(Self {
            pack_id,
            pack_setting_name,
            pack_setting_value,
        })
    }
}

/// ClientBoundDataStore is sent by the server to update, change or remove data store entries on the client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientboundDataStore {
    /// `updates` is an array of data store changes. Each entry has its own change type discriminator.
    pub updates: Vec<BedrockDDUI>,
}

impl ClientboundDataStore {
    pub const ID: u32 = 330;
}
impl wire::Encode for ClientboundDataStore {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection_limits(writer, self.updates.as_slice(), 0, 500)?;
        Ok(())
    }
}

impl wire::Decode for ClientboundDataStore {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let updates = wire::decode_collection_limits::<BedrockDDUI>(
            reader,
            crate::fields::CLIENTBOUND_DATA_STORE_UPDATES,
            2,
            0,
            500,
        )?;
        Ok(Self { updates })
    }
}

/// GraphicsOverrideParameter is sent by the server to override graphics parameters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphicsOverrideParameter {
    pub parameter_keyframe_values: Vec<(wire::F32LE, glam::Vec3)>,
    /// `float_value` is an optional single float graphics parameter to be overridden.
    /// Wire presence: optional value is preceded by a presence marker.
    pub float_value: Option<wire::F32LE>,
    /// `vec3_value` is an optional single Vec3 graphics parameter to be overridden.
    /// Wire presence: optional value is preceded by a presence marker.
    pub vec3_value: Option<glam::Vec3>,
    /// `biome_identifier` is the identifier of the biome for which the parameters apply.
    pub biome_identifier: wire::Str,
    /// `player_identifier` is the optional identifier of the player for which the override parameter applies.
    /// Wire presence: optional value is preceded by a presence marker.
    pub player_identifier: Option<wire::Str>,
    pub identifier_for_parameter: GraphicsOverrideParameterType,
    pub reset_parameter: bool,
}

impl GraphicsOverrideParameter {
    pub const ID: u32 = 331;
}
impl wire::Encode for GraphicsOverrideParameter {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_map_limits(writer, self.parameter_keyframe_values.as_slice(), 0, 255)?;
        match &self.float_value {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        match &self.vec3_value {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        wire::encode_string_limits(writer, &self.biome_identifier, 0, 255)?;
        match &self.player_identifier {
            Some(value) => {
                writer.write_u8(1)?;
                wire::encode_string_limits(writer, value, 0, 255)?;
            }
            None => writer.write_u8(0)?,
        }
        self.identifier_for_parameter.encode(writer)?;
        self.reset_parameter.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for GraphicsOverrideParameter {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let parameter_keyframe_values = wire::decode_map_limits::<wire::F32LE, glam::Vec3>(
            reader,
            crate::fields::GRAPHICS_OVERRIDE_PARAMETER_PARAMETER_KEYFRAME_VALUES,
            16,
            0,
            255,
        )?;
        let float_value = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::F32LE as wire::Decode>::decode(reader)?)
            }
        };
        let vec3_value = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<glam::Vec3 as wire::Decode>::decode(reader)?)
            }
        };
        let biome_identifier = wire::decode_string_limits(
            reader,
            crate::fields::GRAPHICS_OVERRIDE_PARAMETER_BIOME_IDENTIFIER,
            0,
            255,
        )?;
        let player_identifier = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(wire::decode_string_limits(
                    reader,
                    crate::fields::GRAPHICS_OVERRIDE_PARAMETER_PLAYER_IDENTIFIER,
                    0,
                    255,
                )?)
            }
        };
        let identifier_for_parameter =
            <GraphicsOverrideParameterType as wire::Decode>::decode(reader)?;
        let reset_parameter = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            parameter_keyframe_values,
            float_value,
            vec3_value,
            biome_identifier,
            player_identifier,
            identifier_for_parameter,
            reset_parameter,
        })
    }
}

/// ServerBoundDataStore is sent by the client to update a data store property on the server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerboundDataStore {
    /// `update` contains the data store update.
    pub update: BedrockDDUIDataStoreUpdate,
}

impl ServerboundDataStore {
    pub const ID: u32 = 332;
}
impl wire::Encode for ServerboundDataStore {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.update.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerboundDataStore {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let update = <BedrockDDUIDataStoreUpdate as wire::Decode>::decode(reader)?;
        Ok(Self { update })
    }
}

/// ClientBoundDataDrivenUIShowScreen is sent by the server to show a data-driven UI screen on the client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientboundDataDrivenUIShowScreen {
    /// `screen_id` is the identifier of the screen to show.
    pub screen_id: wire::Str,
    /// `form_id` is a unique instance ID for the form, used for scripting to identify specific screen instances.
    pub form_id: wire::U32LE,
    /// `data_instance_id` is an optional data ID associated with the screen.
    /// Wire presence: optional value is preceded by a presence marker.
    pub data_instance_id: Option<wire::U32LE>,
}

impl ClientboundDataDrivenUIShowScreen {
    pub const ID: u32 = 333;
}
impl wire::Encode for ClientboundDataDrivenUIShowScreen {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_string_limits(writer, &self.screen_id, 0, 500)?;
        self.form_id.encode(writer)?;
        wire::check_number_limits(self.form_id.0, Some(0), None)?;
        match &self.data_instance_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ClientboundDataDrivenUIShowScreen {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let screen_id = wire::decode_string_limits(
            reader,
            crate::fields::CLIENTBOUND_DATA_DRIVEN_UI_SHOW_SCREEN_SCREEN_ID,
            0,
            500,
        )?;
        let form_id = {
            let value = <wire::U32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let data_instance_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::U32LE as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            screen_id,
            form_id,
            data_instance_id,
        })
    }
}

/// ClientBoundDataDrivenUICloseScreen is sent by the server to close a data-driven UI screen on the client.
/// If FormID is not set, all data-driven UI screens are closed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientboundDataDrivenUICloseScreen {
    /// `form_id` is the optional unique instance ID of the form to close. If not set, all forms are closed.
    /// Wire presence: optional value is preceded by a presence marker.
    pub form_id: Option<wire::U32LE>,
}

impl ClientboundDataDrivenUICloseScreen {
    pub const ID: u32 = 334;
}
impl wire::Encode for ClientboundDataDrivenUICloseScreen {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        match &self.form_id {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ClientboundDataDrivenUICloseScreen {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let form_id = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<wire::U32LE as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self { form_id })
    }
}

/// ClientBoundDataDrivenUIReload is sent by the server to reload the data-driven UI on the client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientboundDataDrivenUIReload {}

impl ClientboundDataDrivenUIReload {
    pub const ID: u32 = 335;
}
impl wire::Encode for ClientboundDataDrivenUIReload {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        let _ = writer;
        Ok(())
    }
}

impl wire::Decode for ClientboundDataDrivenUIReload {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let _ = reader;
        Ok(Self {})
    }
}

/// ClientBoundTextureShift is sent by the server to control texture shift animations on the client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientboundTextureShift {
    /// `action_id` is the texture shift action to perform. It is one of the constants above.
    pub action_id: ClientboundTextureShiftAction,
    /// `collection_name` is the name of the texture shift collection.
    pub collection_name: wire::Str,
    /// `from_step` is the step to shift from.
    pub from_step: wire::Str,
    /// `to_step` is the step to shift to.
    pub to_step: wire::Str,
    /// `all_steps` is a list of all steps in the texture shift.
    pub all_steps: Vec<wire::Str>,
    /// CurrentLengthTicks is the current length of the shift in ticks.
    pub current_length_in_ticks: wire::VarULong,
    /// TotalLengthTicks is the total length of the shift in ticks.
    pub total_length_in_ticks: wire::VarULong,
    /// `enabled` specifies if the texture shift is enabled.
    pub enabled: bool,
}

impl ClientboundTextureShift {
    pub const ID: u32 = 336;
}
impl wire::Encode for ClientboundTextureShift {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.action_id.encode(writer)?;
        self.collection_name.encode(writer)?;
        self.from_step.encode(writer)?;
        self.to_step.encode(writer)?;
        writer.write_len_var_u32(self.all_steps.len())?;
        for item in self.all_steps.iter() {
            item.encode(writer)?;
        }
        self.current_length_in_ticks.encode(writer)?;
        wire::check_number_limits(self.current_length_in_ticks.0, Some(0), None)?;
        self.total_length_in_ticks.encode(writer)?;
        wire::check_number_limits(self.total_length_in_ticks.0, Some(0), None)?;
        self.enabled.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ClientboundTextureShift {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let action_id = <ClientboundTextureShiftAction as wire::Decode>::decode(reader)?;
        let collection_name =
            reader.read_str(crate::fields::CLIENTBOUND_TEXTURE_SHIFT_COLLECTION_NAME)?;
        let from_step = reader.read_str(crate::fields::CLIENTBOUND_TEXTURE_SHIFT_FROM_STEP)?;
        let to_step = reader.read_str(crate::fields::CLIENTBOUND_TEXTURE_SHIFT_TO_STEP)?;
        let all_steps = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(
                crate::fields::CLIENTBOUND_TEXTURE_SHIFT_ALL_STEPS,
                declared,
                1,
            )?;
            let mut out: Vec<wire::Str> = wire::alloc_vec(count)?;
            for _ in 0..count {
                wire::push(
                    &mut out,
                    reader.read_str(crate::fields::CLIENTBOUND_TEXTURE_SHIFT_ALL_STEPS_ITEM)?,
                )?;
            }
            out
        };
        let current_length_in_ticks = {
            let value = <wire::VarULong as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let total_length_in_ticks = {
            let value = <wire::VarULong as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let enabled = <bool as wire::Decode>::decode(reader)?;
        Ok(Self {
            action_id,
            collection_name,
            from_step,
            to_step,
            all_steps,
            current_length_in_ticks,
            total_length_in_ticks,
            enabled,
        })
    }
}

/// VoxelShapes is sent by the server to send voxel shape data to the client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VoxelShapes {
    /// `shapes` is a list of voxel shapes.
    pub shapes: Vec<VoxelShapesSerializableVoxelShape>,
    /// `name_map` is a map of shape names to IDs.
    pub name_map: Vec<(wire::Str, VoxelShapesRegistryHandle)>,
    /// `custom_shape_count` is the number of custom shapes.
    pub custom_shape_count: wire::U16LE,
}

impl VoxelShapes {
    pub const ID: u32 = 337;
}
impl wire::Encode for VoxelShapes {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.shapes.as_slice())?;
        writer.write_len_var_u32(self.name_map.len())?;
        for (key, value) in self.name_map.iter() {
            key.encode(writer)?;
            value.encode(writer)?;
        }
        self.custom_shape_count.encode(writer)?;
        wire::check_number_limits(self.custom_shape_count.0, Some(0), None)?;
        Ok(())
    }
}

impl wire::Decode for VoxelShapes {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let shapes = wire::decode_collection::<VoxelShapesSerializableVoxelShape>(
            reader,
            crate::fields::VOXEL_SHAPES_SHAPES,
            7,
        )?;
        let name_map = {
            let declared = u64::from(reader.read_var_u32()?);
            let count = reader.checked_count(crate::fields::VOXEL_SHAPES_NAME_MAP, declared, 3)?;
            let mut out: Vec<(wire::Str, VoxelShapesRegistryHandle)> = wire::alloc_vec(count)?;
            for _ in 0..count {
                let key = reader.read_str(crate::fields::VOXEL_SHAPES_NAME_MAP_KEY)?;
                let value = <VoxelShapesRegistryHandle as wire::Decode>::decode(reader)?;
                wire::push(&mut out, (key, value))?;
            }
            out
        };
        let custom_shape_count = {
            let value = <wire::U16LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        Ok(Self {
            shapes,
            name_map,
            custom_shape_count,
        })
    }
}

/// CameraSpline is sent by the server to define camera spline paths.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraSpline {
    /// `camera_data_splines` is a list of camera spline definitions.
    pub camera_data_splines: Vec<CameraSplineDefinition>,
}

impl CameraSpline {
    pub const ID: u32 = 338;
}
impl wire::Encode for CameraSpline {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(writer, self.camera_data_splines.as_slice())?;
        Ok(())
    }
}

impl wire::Decode for CameraSpline {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let camera_data_splines = wire::decode_collection::<CameraSplineDefinition>(
            reader,
            crate::fields::CAMERA_SPLINE_CAMERA_DATA_SPLINES,
            9,
        )?;
        Ok(Self {
            camera_data_splines,
        })
    }
}

/// CameraAimAssistActorPriority is sent by the server to define actor-specific aim assist priorities.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraAimAssistActorPriority {
    /// `camera_aim_assist_actor_priority_list` is a list of aim assist actor priority entries.
    pub camera_aim_assist_actor_priority_list: Vec<CameraAimAssistActorPriorityData>,
}

impl CameraAimAssistActorPriority {
    pub const ID: u32 = 339;
}
impl wire::Encode for CameraAimAssistActorPriority {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection(
            writer,
            self.camera_aim_assist_actor_priority_list.as_slice(),
        )?;
        Ok(())
    }
}

impl wire::Decode for CameraAimAssistActorPriority {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let camera_aim_assist_actor_priority_list = wire::decode_collection::<
            CameraAimAssistActorPriorityData,
        >(
            reader,
            crate::fields::CAMERA_AIM_ASSIST_ACTOR_PRIORITY_CAMERA_AIM_ASSIST_ACTOR_PRIORITY_LIST,
            16,
        )?;
        Ok(Self {
            camera_aim_assist_actor_priority_list,
        })
    }
}

/// ResourcePacksReadyForValidation is sent by the client to inform the server that the client has finished
/// loading resource packs and is ready for validation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourcePacksReadyForValidation {}

impl ResourcePacksReadyForValidation {
    pub const ID: u32 = 340;
}
impl wire::Encode for ResourcePacksReadyForValidation {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        let _ = writer;
        Ok(())
    }
}

impl wire::Decode for ResourcePacksReadyForValidation {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let _ = reader;
        Ok(Self {})
    }
}

/// LocatorBar is sent by the server to add, remove or update waypoints on the client's locator bar.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocatorBar {
    /// `waypoints` is a slice of waypoints to add, remove or update.
    pub waypoints: Vec<LocatorBarWaypoint>,
}

impl LocatorBar {
    pub const ID: u32 = 341;
}
impl wire::Encode for LocatorBar {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_collection_limits(writer, self.waypoints.as_slice(), 0, 40000)?;
        Ok(())
    }
}

impl wire::Decode for LocatorBar {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let waypoints = wire::decode_collection_limits::<LocatorBarWaypoint>(
            reader,
            crate::fields::LOCATOR_BAR_WAYPOINTS,
            28,
            0,
            40000,
        )?;
        Ok(Self { waypoints })
    }
}

/// PartyChanged is sent by the client to the server to indicate that the player's party ID has changed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartyChanged {
    /// Wire presence: optional value is preceded by a presence marker.
    pub party_info: Option<PlayerPartyInfo>,
}

impl PartyChanged {
    pub const ID: u32 = 342;
}
impl wire::Encode for PartyChanged {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        match &self.party_info {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for PartyChanged {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let party_info = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<PlayerPartyInfo as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self { party_info })
    }
}

/// ServerBoundDataDrivenScreenClosed is sent by the client when a data-driven UI screen is closed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerboundDataDrivenScreenClosed {
    /// `form_id` is the unique instance ID of the form that was closed.
    pub form_id: wire::U32LE,
    /// `close_reason` is the reason the screen was closed. It is one of the DataDrivenScreenCloseReason
    /// constants.
    pub close_reason: wire::Str,
}

impl ServerboundDataDrivenScreenClosed {
    pub const ID: u32 = 343;
}
impl wire::Encode for ServerboundDataDrivenScreenClosed {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.form_id.encode(writer)?;
        wire::check_number_limits(self.form_id.0, Some(0), None)?;
        self.close_reason.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ServerboundDataDrivenScreenClosed {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let form_id = {
            let value = <wire::U32LE as wire::Decode>::decode(reader)?;
            wire::validate_number_limits(value.0, Some(0), None)?;
            value
        };
        let close_reason =
            reader.read_str(crate::fields::SERVERBOUND_DATA_DRIVEN_SCREEN_CLOSED_CLOSE_REASON)?;
        Ok(Self {
            form_id,
            close_reason,
        })
    }
}

/// SyncWorldClocks is sent by the server to initialise and synchronise world clocks with the client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncWorldClocks {
    pub data: SyncWorldClocksData,
}

impl SyncWorldClocks {
    pub const ID: u32 = 344;
}
impl wire::Encode for SyncWorldClocks {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SyncWorldClocks {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let data = <SyncWorldClocksData as wire::Decode>::decode(reader)?;
        Ok(Self { data })
    }
}

/// ClientBoundAttributeLayerSync is sent by the server to synchronise attribute layers with the client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientboundAttributeLayerSync {
    pub data: AttributeLayerSyncData,
}

impl ClientboundAttributeLayerSync {
    pub const ID: u32 = 345;
}
impl wire::Encode for ClientboundAttributeLayerSync {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.data.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ClientboundAttributeLayerSync {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let data = <AttributeLayerSyncData as wire::Decode>::decode(reader)?;
        Ok(Self { data })
    }
}

/// ServerStoreInfo is sent by the server to provide the client with a store entry point. Like the
/// ShowStoreOffer packet, this only has an effect on partnered servers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerStoreInfo {
    /// `client_store_entry_point_configuration` is the store info to set, or nothing to fall back to the default.
    /// Wire presence: optional value is preceded by a presence marker.
    pub client_store_entry_point_configuration:
        Option<ServerConfigurationClientStoreEntryPointConfiguration>,
}

impl ServerStoreInfo {
    pub const ID: u32 = 346;
}
impl wire::Encode for ServerStoreInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        match &self.client_store_entry_point_configuration {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ServerStoreInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let client_store_entry_point_configuration = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<ServerConfigurationClientStoreEntryPointConfiguration as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            client_store_entry_point_configuration,
        })
    }
}

/// ServerPresenceInfo is sent by the server to provide the client with presence info.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerPresenceInfo {
    /// `presence_configuration` is the presence info to set, or nothing to fall back to the default.
    /// Wire presence: optional value is preceded by a presence marker.
    pub presence_configuration: Option<ServerConfigurationPresenceConfiguration>,
}

impl ServerPresenceInfo {
    pub const ID: u32 = 347;
}
impl wire::Encode for ServerPresenceInfo {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        match &self.presence_configuration {
            Some(value) => {
                writer.write_u8(1)?;
                value.encode(writer)?;
            }
            None => writer.write_u8(0)?,
        }
        Ok(())
    }
}

impl wire::Decode for ServerPresenceInfo {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let presence_configuration = {
            if reader.read_u8()? == 0 {
                None
            } else {
                Some(<ServerConfigurationPresenceConfiguration as wire::Decode>::decode(reader)?)
            }
        };
        Ok(Self {
            presence_configuration,
        })
    }
}

/// ClientboundUpdateSoundData is sent by the server to update a sound that is currently playing, identified
/// by the handle that the server sent in the PlaySound packet that started it. Each optional field is a
/// Cereal union slot that may hold any SoundDataUpdate variant; its name does not constrain the variant on
/// the wire.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientboundUpdateSoundData {
    /// `server_sound_handle` is the server-side handle of the sound to update.
    pub server_sound_handle: ServerSoundHandle,
    pub stop: SoundDataEvent,
    pub set_volume: SoundDataEvent,
    pub set_pitch: SoundDataEvent,
    pub fade: SoundDataEvent,
    pub seek_to: SoundDataEvent,
    pub pause: SoundDataEvent,
    pub resume: SoundDataEvent,
}

impl ClientboundUpdateSoundData {
    pub const ID: u32 = 348;
}
impl wire::Encode for ClientboundUpdateSoundData {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.server_sound_handle.encode(writer)?;
        self.stop.encode(writer)?;
        self.set_volume.encode(writer)?;
        self.set_pitch.encode(writer)?;
        self.fade.encode(writer)?;
        self.seek_to.encode(writer)?;
        self.pause.encode(writer)?;
        self.resume.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for ClientboundUpdateSoundData {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let server_sound_handle = <ServerSoundHandle as wire::Decode>::decode(reader)?;
        let stop = <SoundDataEvent as wire::Decode>::decode(reader)?;
        let set_volume = <SoundDataEvent as wire::Decode>::decode(reader)?;
        let set_pitch = <SoundDataEvent as wire::Decode>::decode(reader)?;
        let fade = <SoundDataEvent as wire::Decode>::decode(reader)?;
        let seek_to = <SoundDataEvent as wire::Decode>::decode(reader)?;
        let pause = <SoundDataEvent as wire::Decode>::decode(reader)?;
        let resume = <SoundDataEvent as wire::Decode>::decode(reader)?;
        Ok(Self {
            server_sound_handle,
            stop,
            set_volume,
            set_pitch,
            fade,
            seek_to,
            pause,
            resume,
        })
    }
}

/// SendPartyDestinationCookie is sent by the server to a client with a party destination cookie.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SendPartyDestinationCookie {
    /// `cookie` is the opaque party destination cookie.
    pub cookie: wire::Str,
    /// `intent` is the intent of the cookie. It is one of the PartyDestinationCookieIntent constants.
    pub intent: wire::Str,
    /// `destination_name` is the name of the destination the cookie refers to.
    pub destination_name: wire::Str,
}

impl SendPartyDestinationCookie {
    pub const ID: u32 = 349;
}
impl wire::Encode for SendPartyDestinationCookie {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_string_limits(writer, &self.cookie, 0, 2048)?;
        self.intent.encode(writer)?;
        wire::encode_string_limits(writer, &self.destination_name, 0, 64)?;
        Ok(())
    }
}

impl wire::Decode for SendPartyDestinationCookie {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let cookie = wire::decode_string_limits(
            reader,
            crate::fields::SEND_PARTY_DESTINATION_COOKIE_COOKIE,
            0,
            2048,
        )?;
        let intent = reader.read_str(crate::fields::SEND_PARTY_DESTINATION_COOKIE_INTENT)?;
        let destination_name = wire::decode_string_limits(
            reader,
            crate::fields::SEND_PARTY_DESTINATION_COOKIE_DESTINATION_NAME,
            0,
            64,
        )?;
        Ok(Self {
            cookie,
            intent,
            destination_name,
        })
    }
}

/// PartyDestinationCookieResponse is sent by the client to the server in response to a
/// SendPartyDestinationCookie packet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartyDestinationCookieResponse {
    /// `cookie` is the opaque party destination cookie echoed back from the SendPartyDestinationCookie packet.
    pub cookie: wire::Str,
    /// `accepted` is true if the client accepted the party destination.
    pub accepted: bool,
}

impl PartyDestinationCookieResponse {
    pub const ID: u32 = 350;
}
impl wire::Encode for PartyDestinationCookieResponse {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        wire::encode_string_limits(writer, &self.cookie, 0, 2048)?;
        self.accepted.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for PartyDestinationCookieResponse {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let cookie = wire::decode_string_limits(
            reader,
            crate::fields::PARTY_DESTINATION_COOKIE_RESPONSE_COOKIE,
            0,
            2048,
        )?;
        let accepted = <bool as wire::Decode>::decode(reader)?;
        Ok(Self { cookie, accepted })
    }
}

/// SetPlayerFurnaceOptions is a bidirectional packet that can be used to update the options a player has
/// selected in a furnace-like container's UI.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetPlayerFurnaceOptions {
    /// `furnace_type` is the type of furnace that the options apply to. It is one of the FurnaceType constants
    /// above.
    pub furnace_type: FurnaceType,
    /// `furnace_options` holds the options that the player has selected for the furnace type above.
    pub furnace_options: FurnaceOptions,
}

impl SetPlayerFurnaceOptions {
    pub const ID: u32 = 351;
}
impl wire::Encode for SetPlayerFurnaceOptions {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.furnace_type.encode(writer)?;
        self.furnace_options.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for SetPlayerFurnaceOptions {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let furnace_type = <FurnaceType as wire::Decode>::decode(reader)?;
        let furnace_options = <FurnaceOptions as wire::Decode>::decode(reader)?;
        Ok(Self {
            furnace_type,
            furnace_options,
        })
    }
}

/// RecordStarted is sent by the server to notify the client that a record started playing at a specific block
/// position, such as when a music disc is inserted into a jukebox.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecordStarted {
    /// Position is the position of the block that the record started playing at.
    pub block_position: BlockPos,
    pub server_sound_handle: ServerSoundHandle,
}

impl RecordStarted {
    pub const ID: u32 = 352;
}
impl wire::Encode for RecordStarted {
    fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        self.block_position.encode(writer)?;
        self.server_sound_handle.encode(writer)?;
        Ok(())
    }
}

impl wire::Decode for RecordStarted {
    fn decode(reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        let block_position = <BlockPos as wire::Decode>::decode(reader)?;
        let server_sound_handle = <ServerSoundHandle as wire::Decode>::decode(reader)?;
        Ok(Self {
            block_position,
            server_sound_handle,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u32)]
pub enum PacketId {
    Login = 1,
    PlayStatus = 2,
    ServerToClientHandshake = 3,
    ClientToServerHandshake = 4,
    Disconnect = 5,
    ResourcePacksInfo = 6,
    ResourcePackStack = 7,
    ResourcePackClientResponse = 8,
    Text = 9,
    SetTime = 10,
    StartGame = 11,
    AddPlayer = 12,
    AddActor = 13,
    RemoveActor = 14,
    AddItemActor = 15,
    ServerPlayerPostMovePosition = 16,
    TakeItemActor = 17,
    MoveActorAbsolute = 18,
    MovePlayer = 19,
    UpdateBlock = 21,
    AddPainting = 22,
    LevelEvent = 25,
    BlockEvent = 26,
    ActorEvent = 27,
    MobEffect = 28,
    UpdateAttributes = 29,
    InventoryTransaction = 30,
    MobEquipment = 31,
    MobArmorEquipment = 32,
    Interact = 33,
    BlockPickRequest = 34,
    ActorPickRequest = 35,
    PlayerAction = 36,
    HurtArmor = 38,
    SetActorData = 39,
    SetActorMotion = 40,
    SetActorLink = 41,
    SetHealth = 42,
    SetSpawnPosition = 43,
    Animate = 44,
    Respawn = 45,
    ContainerOpen = 46,
    ContainerClose = 47,
    PlayerHotbar = 48,
    InventoryContent = 49,
    InventorySlot = 50,
    ContainerSetData = 51,
    CraftingData = 52,
    GuiDataPickItem = 54,
    BlockActorData = 56,
    LevelChunk = 58,
    SetCommandsEnabled = 59,
    SetDifficulty = 60,
    ChangeDimension = 61,
    SetPlayerGameType = 62,
    PlayerList = 63,
    SimpleEvent = 64,
    LegacyTelemetryEvent = 65,
    SpawnExperienceOrb = 66,
    ClientboundMapItemData = 67,
    MapInfoRequest = 68,
    RequestChunkRadius = 69,
    ChunkRadiusUpdated = 70,
    GameRulesChanged = 72,
    Camera = 73,
    BossEvent = 74,
    ShowCredits = 75,
    AvailableCommands = 76,
    CommandRequest = 77,
    CommandBlockUpdate = 78,
    CommandOutput = 79,
    UpdateTrade = 80,
    UpdateEquip = 81,
    ResourcePackDataInfo = 82,
    ResourcePackChunkData = 83,
    ResourcePackChunkRequest = 84,
    Transfer = 85,
    PlaySound = 86,
    StopSound = 87,
    SetTitle = 88,
    AddBehaviorTree = 89,
    StructureBlockUpdate = 90,
    ShowStoreOffer = 91,
    PurchaseReceipt = 92,
    PlayerSkin = 93,
    SubClientLogin = 94,
    AutomationClientConnect = 95,
    SetLastHurtBy = 96,
    BookEdit = 97,
    NpcRequest = 98,
    PhotoTransfer = 99,
    ModalFormRequest = 100,
    ModalFormResponse = 101,
    ServerSettingsRequest = 102,
    ServerSettingsResponse = 103,
    ShowProfile = 104,
    SetDefaultGameType = 105,
    RemoveObjective = 106,
    SetDisplayObjective = 107,
    SetScore = 108,
    LabTable = 109,
    UpdateBlockSynced = 110,
    MoveActorDelta = 111,
    SetScoreboardIdentity = 112,
    SetLocalPlayerAsInitialized = 113,
    UpdateSoftEnum = 114,
    NetworkStackLatency = 115,
    SpawnParticleEffect = 118,
    AvailableActorIdentifiers = 119,
    NetworkChunkPublisherUpdate = 121,
    BiomeDefinitionList = 122,
    LevelSoundEvent = 123,
    LevelEventGeneric = 124,
    LecternUpdate = 125,
    ClientCacheStatus = 129,
    OnScreenTextureAnimation = 130,
    MapCreateLockedCopy = 131,
    StructureTemplateDataRequest = 132,
    StructureTemplateDataResponse = 133,
    ClientCacheBlobStatus = 135,
    ClientCacheMissResponse = 136,
    EducationSettings = 137,
    Emote = 138,
    MultiplayerSettings = 139,
    SettingsCommand = 140,
    AnvilDamage = 141,
    CompletedUsingItem = 142,
    NetworkSettings = 143,
    PlayerAuthInput = 144,
    CreativeContent = 145,
    PlayerEnchantOptions = 146,
    ItemStackRequest = 147,
    ItemStackResponse = 148,
    PlayerArmorDamage = 149,
    CodeBuilder = 150,
    UpdatePlayerGameType = 151,
    EmoteList = 152,
    PositionTrackingDBServerBroadcast = 153,
    PositionTrackingDBClientRequest = 154,
    DebugInfo = 155,
    PacketViolationWarning = 156,
    MotionPredictionHints = 157,
    AnimateEntity = 158,
    CameraShake = 159,
    PlayerFog = 160,
    CorrectPlayerMovePrediction = 161,
    ItemRegistry = 162,
    ClientboundDebugRenderer = 164,
    SyncActorProperty = 165,
    AddVolumeEntity = 166,
    RemoveVolumeEntity = 167,
    SimulationType = 168,
    NpcDialogue = 169,
    EduUriResource = 170,
    CreatePhoto = 171,
    UpdateSubChunkBlocks = 172,
    SubChunk = 174,
    SubChunkRequest = 175,
    PlayerStartItemCooldown = 176,
    ScriptMessage = 177,
    CodeBuilderSource = 178,
    TickingAreasLoadStatus = 179,
    DimensionData = 180,
    AgentActionEvent = 181,
    ChangeMobProperty = 182,
    LessonProgress = 183,
    RequestAbility = 184,
    RequestPermissions = 185,
    ToastRequest = 186,
    UpdateAbilities = 187,
    UpdateAdventureSettings = 188,
    DeathInfo = 189,
    EditorNetwork = 190,
    FeatureRegistry = 191,
    ServerStats = 192,
    RequestNetworkSettings = 193,
    GameTestRequest = 194,
    GameTestResults = 195,
    UpdateClientInputLocks = 196,
    CameraPresets = 198,
    UnlockedRecipes = 199,
    CameraInstruction = 300,
    TrimData = 302,
    OpenSign = 303,
    AgentAnimation = 304,
    RefreshEntitlements = 305,
    PlayerToggleCrafterSlotRequest = 306,
    SetPlayerInventoryOptions = 307,
    SetHud = 308,
    AwardAchievement = 309,
    ClientboundCloseForm = 310,
    ServerboundLoadingScreen = 312,
    JigsawStructureData = 313,
    CurrentStructureFeature = 314,
    ServerboundDiagnostics = 315,
    CameraAimAssist = 316,
    ContainerRegistryCleanup = 317,
    MovementEffect = 318,
    CameraAimAssistPresets = 320,
    ClientCameraAimAssist = 321,
    ClientMovementPredictionSync = 322,
    UpdateClientOptions = 323,
    PlayerVideoCapture = 324,
    PlayerUpdateEntityOverrides = 325,
    PlayerLocation = 326,
    ClientboundControlSchemeSet = 327,
    PrimitiveShapes = 328,
    ServerboundPackSettingChange = 329,
    ClientboundDataStore = 330,
    GraphicsOverrideParameter = 331,
    ServerboundDataStore = 332,
    ClientboundDataDrivenUIShowScreen = 333,
    ClientboundDataDrivenUICloseScreen = 334,
    ClientboundDataDrivenUIReload = 335,
    ClientboundTextureShift = 336,
    VoxelShapes = 337,
    CameraSpline = 338,
    CameraAimAssistActorPriority = 339,
    ResourcePacksReadyForValidation = 340,
    LocatorBar = 341,
    PartyChanged = 342,
    ServerboundDataDrivenScreenClosed = 343,
    SyncWorldClocks = 344,
    ClientboundAttributeLayerSync = 345,
    ServerStoreInfo = 346,
    ServerPresenceInfo = 347,
    ClientboundUpdateSoundData = 348,
    SendPartyDestinationCookie = 349,
    PartyDestinationCookieResponse = 350,
    SetPlayerFurnaceOptions = 351,
    RecordStarted = 352,
}

impl PacketId {
    pub fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            1 => Some(Self::Login),
            2 => Some(Self::PlayStatus),
            3 => Some(Self::ServerToClientHandshake),
            4 => Some(Self::ClientToServerHandshake),
            5 => Some(Self::Disconnect),
            6 => Some(Self::ResourcePacksInfo),
            7 => Some(Self::ResourcePackStack),
            8 => Some(Self::ResourcePackClientResponse),
            9 => Some(Self::Text),
            10 => Some(Self::SetTime),
            11 => Some(Self::StartGame),
            12 => Some(Self::AddPlayer),
            13 => Some(Self::AddActor),
            14 => Some(Self::RemoveActor),
            15 => Some(Self::AddItemActor),
            16 => Some(Self::ServerPlayerPostMovePosition),
            17 => Some(Self::TakeItemActor),
            18 => Some(Self::MoveActorAbsolute),
            19 => Some(Self::MovePlayer),
            21 => Some(Self::UpdateBlock),
            22 => Some(Self::AddPainting),
            25 => Some(Self::LevelEvent),
            26 => Some(Self::BlockEvent),
            27 => Some(Self::ActorEvent),
            28 => Some(Self::MobEffect),
            29 => Some(Self::UpdateAttributes),
            30 => Some(Self::InventoryTransaction),
            31 => Some(Self::MobEquipment),
            32 => Some(Self::MobArmorEquipment),
            33 => Some(Self::Interact),
            34 => Some(Self::BlockPickRequest),
            35 => Some(Self::ActorPickRequest),
            36 => Some(Self::PlayerAction),
            38 => Some(Self::HurtArmor),
            39 => Some(Self::SetActorData),
            40 => Some(Self::SetActorMotion),
            41 => Some(Self::SetActorLink),
            42 => Some(Self::SetHealth),
            43 => Some(Self::SetSpawnPosition),
            44 => Some(Self::Animate),
            45 => Some(Self::Respawn),
            46 => Some(Self::ContainerOpen),
            47 => Some(Self::ContainerClose),
            48 => Some(Self::PlayerHotbar),
            49 => Some(Self::InventoryContent),
            50 => Some(Self::InventorySlot),
            51 => Some(Self::ContainerSetData),
            52 => Some(Self::CraftingData),
            54 => Some(Self::GuiDataPickItem),
            56 => Some(Self::BlockActorData),
            58 => Some(Self::LevelChunk),
            59 => Some(Self::SetCommandsEnabled),
            60 => Some(Self::SetDifficulty),
            61 => Some(Self::ChangeDimension),
            62 => Some(Self::SetPlayerGameType),
            63 => Some(Self::PlayerList),
            64 => Some(Self::SimpleEvent),
            65 => Some(Self::LegacyTelemetryEvent),
            66 => Some(Self::SpawnExperienceOrb),
            67 => Some(Self::ClientboundMapItemData),
            68 => Some(Self::MapInfoRequest),
            69 => Some(Self::RequestChunkRadius),
            70 => Some(Self::ChunkRadiusUpdated),
            72 => Some(Self::GameRulesChanged),
            73 => Some(Self::Camera),
            74 => Some(Self::BossEvent),
            75 => Some(Self::ShowCredits),
            76 => Some(Self::AvailableCommands),
            77 => Some(Self::CommandRequest),
            78 => Some(Self::CommandBlockUpdate),
            79 => Some(Self::CommandOutput),
            80 => Some(Self::UpdateTrade),
            81 => Some(Self::UpdateEquip),
            82 => Some(Self::ResourcePackDataInfo),
            83 => Some(Self::ResourcePackChunkData),
            84 => Some(Self::ResourcePackChunkRequest),
            85 => Some(Self::Transfer),
            86 => Some(Self::PlaySound),
            87 => Some(Self::StopSound),
            88 => Some(Self::SetTitle),
            89 => Some(Self::AddBehaviorTree),
            90 => Some(Self::StructureBlockUpdate),
            91 => Some(Self::ShowStoreOffer),
            92 => Some(Self::PurchaseReceipt),
            93 => Some(Self::PlayerSkin),
            94 => Some(Self::SubClientLogin),
            95 => Some(Self::AutomationClientConnect),
            96 => Some(Self::SetLastHurtBy),
            97 => Some(Self::BookEdit),
            98 => Some(Self::NpcRequest),
            99 => Some(Self::PhotoTransfer),
            100 => Some(Self::ModalFormRequest),
            101 => Some(Self::ModalFormResponse),
            102 => Some(Self::ServerSettingsRequest),
            103 => Some(Self::ServerSettingsResponse),
            104 => Some(Self::ShowProfile),
            105 => Some(Self::SetDefaultGameType),
            106 => Some(Self::RemoveObjective),
            107 => Some(Self::SetDisplayObjective),
            108 => Some(Self::SetScore),
            109 => Some(Self::LabTable),
            110 => Some(Self::UpdateBlockSynced),
            111 => Some(Self::MoveActorDelta),
            112 => Some(Self::SetScoreboardIdentity),
            113 => Some(Self::SetLocalPlayerAsInitialized),
            114 => Some(Self::UpdateSoftEnum),
            115 => Some(Self::NetworkStackLatency),
            118 => Some(Self::SpawnParticleEffect),
            119 => Some(Self::AvailableActorIdentifiers),
            121 => Some(Self::NetworkChunkPublisherUpdate),
            122 => Some(Self::BiomeDefinitionList),
            123 => Some(Self::LevelSoundEvent),
            124 => Some(Self::LevelEventGeneric),
            125 => Some(Self::LecternUpdate),
            129 => Some(Self::ClientCacheStatus),
            130 => Some(Self::OnScreenTextureAnimation),
            131 => Some(Self::MapCreateLockedCopy),
            132 => Some(Self::StructureTemplateDataRequest),
            133 => Some(Self::StructureTemplateDataResponse),
            135 => Some(Self::ClientCacheBlobStatus),
            136 => Some(Self::ClientCacheMissResponse),
            137 => Some(Self::EducationSettings),
            138 => Some(Self::Emote),
            139 => Some(Self::MultiplayerSettings),
            140 => Some(Self::SettingsCommand),
            141 => Some(Self::AnvilDamage),
            142 => Some(Self::CompletedUsingItem),
            143 => Some(Self::NetworkSettings),
            144 => Some(Self::PlayerAuthInput),
            145 => Some(Self::CreativeContent),
            146 => Some(Self::PlayerEnchantOptions),
            147 => Some(Self::ItemStackRequest),
            148 => Some(Self::ItemStackResponse),
            149 => Some(Self::PlayerArmorDamage),
            150 => Some(Self::CodeBuilder),
            151 => Some(Self::UpdatePlayerGameType),
            152 => Some(Self::EmoteList),
            153 => Some(Self::PositionTrackingDBServerBroadcast),
            154 => Some(Self::PositionTrackingDBClientRequest),
            155 => Some(Self::DebugInfo),
            156 => Some(Self::PacketViolationWarning),
            157 => Some(Self::MotionPredictionHints),
            158 => Some(Self::AnimateEntity),
            159 => Some(Self::CameraShake),
            160 => Some(Self::PlayerFog),
            161 => Some(Self::CorrectPlayerMovePrediction),
            162 => Some(Self::ItemRegistry),
            164 => Some(Self::ClientboundDebugRenderer),
            165 => Some(Self::SyncActorProperty),
            166 => Some(Self::AddVolumeEntity),
            167 => Some(Self::RemoveVolumeEntity),
            168 => Some(Self::SimulationType),
            169 => Some(Self::NpcDialogue),
            170 => Some(Self::EduUriResource),
            171 => Some(Self::CreatePhoto),
            172 => Some(Self::UpdateSubChunkBlocks),
            174 => Some(Self::SubChunk),
            175 => Some(Self::SubChunkRequest),
            176 => Some(Self::PlayerStartItemCooldown),
            177 => Some(Self::ScriptMessage),
            178 => Some(Self::CodeBuilderSource),
            179 => Some(Self::TickingAreasLoadStatus),
            180 => Some(Self::DimensionData),
            181 => Some(Self::AgentActionEvent),
            182 => Some(Self::ChangeMobProperty),
            183 => Some(Self::LessonProgress),
            184 => Some(Self::RequestAbility),
            185 => Some(Self::RequestPermissions),
            186 => Some(Self::ToastRequest),
            187 => Some(Self::UpdateAbilities),
            188 => Some(Self::UpdateAdventureSettings),
            189 => Some(Self::DeathInfo),
            190 => Some(Self::EditorNetwork),
            191 => Some(Self::FeatureRegistry),
            192 => Some(Self::ServerStats),
            193 => Some(Self::RequestNetworkSettings),
            194 => Some(Self::GameTestRequest),
            195 => Some(Self::GameTestResults),
            196 => Some(Self::UpdateClientInputLocks),
            198 => Some(Self::CameraPresets),
            199 => Some(Self::UnlockedRecipes),
            300 => Some(Self::CameraInstruction),
            302 => Some(Self::TrimData),
            303 => Some(Self::OpenSign),
            304 => Some(Self::AgentAnimation),
            305 => Some(Self::RefreshEntitlements),
            306 => Some(Self::PlayerToggleCrafterSlotRequest),
            307 => Some(Self::SetPlayerInventoryOptions),
            308 => Some(Self::SetHud),
            309 => Some(Self::AwardAchievement),
            310 => Some(Self::ClientboundCloseForm),
            312 => Some(Self::ServerboundLoadingScreen),
            313 => Some(Self::JigsawStructureData),
            314 => Some(Self::CurrentStructureFeature),
            315 => Some(Self::ServerboundDiagnostics),
            316 => Some(Self::CameraAimAssist),
            317 => Some(Self::ContainerRegistryCleanup),
            318 => Some(Self::MovementEffect),
            320 => Some(Self::CameraAimAssistPresets),
            321 => Some(Self::ClientCameraAimAssist),
            322 => Some(Self::ClientMovementPredictionSync),
            323 => Some(Self::UpdateClientOptions),
            324 => Some(Self::PlayerVideoCapture),
            325 => Some(Self::PlayerUpdateEntityOverrides),
            326 => Some(Self::PlayerLocation),
            327 => Some(Self::ClientboundControlSchemeSet),
            328 => Some(Self::PrimitiveShapes),
            329 => Some(Self::ServerboundPackSettingChange),
            330 => Some(Self::ClientboundDataStore),
            331 => Some(Self::GraphicsOverrideParameter),
            332 => Some(Self::ServerboundDataStore),
            333 => Some(Self::ClientboundDataDrivenUIShowScreen),
            334 => Some(Self::ClientboundDataDrivenUICloseScreen),
            335 => Some(Self::ClientboundDataDrivenUIReload),
            336 => Some(Self::ClientboundTextureShift),
            337 => Some(Self::VoxelShapes),
            338 => Some(Self::CameraSpline),
            339 => Some(Self::CameraAimAssistActorPriority),
            340 => Some(Self::ResourcePacksReadyForValidation),
            341 => Some(Self::LocatorBar),
            342 => Some(Self::PartyChanged),
            343 => Some(Self::ServerboundDataDrivenScreenClosed),
            344 => Some(Self::SyncWorldClocks),
            345 => Some(Self::ClientboundAttributeLayerSync),
            346 => Some(Self::ServerStoreInfo),
            347 => Some(Self::ServerPresenceInfo),
            348 => Some(Self::ClientboundUpdateSoundData),
            349 => Some(Self::SendPartyDestinationCookie),
            350 => Some(Self::PartyDestinationCookieResponse),
            351 => Some(Self::SetPlayerFurnaceOptions),
            352 => Some(Self::RecordStarted),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Packet {
    Login(Login),
    PlayStatus(PlayStatus),
    ServerToClientHandshake(ServerToClientHandshake),
    ClientToServerHandshake(ClientToServerHandshake),
    Disconnect(Disconnect),
    ResourcePacksInfo(ResourcePacksInfo),
    ResourcePackStack(ResourcePackStack),
    ResourcePackClientResponse(ResourcePackClientResponse),
    Text(Text),
    SetTime(SetTime),
    StartGame(Box<StartGame>),
    AddPlayer(Box<AddPlayer>),
    AddActor(Box<AddActor>),
    RemoveActor(RemoveActor),
    AddItemActor(AddItemActor),
    ServerPlayerPostMovePosition(ServerPlayerPostMovePosition),
    TakeItemActor(TakeItemActor),
    MoveActorAbsolute(MoveActorAbsolute),
    MovePlayer(Box<MovePlayer>),
    UpdateBlock(UpdateBlock),
    AddPainting(AddPainting),
    LevelEvent(LevelEvent),
    BlockEvent(BlockEvent),
    ActorEvent(ActorEvent),
    MobEffect(Box<MobEffect>),
    UpdateAttributes(UpdateAttributes),
    InventoryTransaction(InventoryTransaction),
    MobEquipment(MobEquipment),
    MobArmorEquipment(MobArmorEquipment),
    Interact(Interact),
    BlockPickRequest(BlockPickRequest),
    ActorPickRequest(ActorPickRequest),
    PlayerAction(PlayerAction),
    HurtArmor(HurtArmor),
    SetActorData(SetActorData),
    SetActorMotion(SetActorMotion),
    SetActorLink(SetActorLink),
    SetHealth(SetHealth),
    SetSpawnPosition(SetSpawnPosition),
    Animate(Animate),
    Respawn(Respawn),
    ContainerOpen(ContainerOpen),
    ContainerClose(ContainerClose),
    PlayerHotbar(PlayerHotbar),
    InventoryContent(InventoryContent),
    InventorySlot(InventorySlot),
    ContainerSetData(ContainerSetData),
    CraftingData(Box<CraftingData>),
    GuiDataPickItem(GuiDataPickItem),
    BlockActorData(BlockActorData),
    LevelChunk(LevelChunk),
    SetCommandsEnabled(SetCommandsEnabled),
    SetDifficulty(SetDifficulty),
    ChangeDimension(ChangeDimension),
    SetPlayerGameType(SetPlayerGameType),
    PlayerList(PlayerList),
    SimpleEvent(SimpleEvent),
    LegacyTelemetryEvent(LegacyTelemetryEvent),
    SpawnExperienceOrb(SpawnExperienceOrb),
    ClientboundMapItemData(Box<ClientboundMapItemData>),
    MapInfoRequest(MapInfoRequest),
    RequestChunkRadius(RequestChunkRadius),
    ChunkRadiusUpdated(ChunkRadiusUpdated),
    GameRulesChanged(GameRulesChanged),
    Camera(Camera),
    BossEvent(BossEvent),
    ShowCredits(ShowCredits),
    AvailableCommands(Box<AvailableCommands>),
    CommandRequest(CommandRequest),
    CommandBlockUpdate(Box<CommandBlockUpdate>),
    CommandOutput(CommandOutput),
    UpdateTrade(Box<UpdateTrade>),
    UpdateEquip(UpdateEquip),
    ResourcePackDataInfo(ResourcePackDataInfo),
    ResourcePackChunkData(ResourcePackChunkData),
    ResourcePackChunkRequest(ResourcePackChunkRequest),
    Transfer(Transfer),
    PlaySound(Box<PlaySound>),
    StopSound(StopSound),
    SetTitle(Box<SetTitle>),
    AddBehaviorTree(AddBehaviorTree),
    StructureBlockUpdate(StructureBlockUpdate),
    ShowStoreOffer(ShowStoreOffer),
    PurchaseReceipt(PurchaseReceipt),
    PlayerSkin(PlayerSkin),
    SubClientLogin(SubClientLogin),
    AutomationClientConnect(AutomationClientConnect),
    SetLastHurtBy(SetLastHurtBy),
    BookEdit(BookEdit),
    NpcRequest(NpcRequest),
    PhotoTransfer(PhotoTransfer),
    ModalFormRequest(ModalFormRequest),
    ModalFormResponse(ModalFormResponse),
    ServerSettingsRequest(ServerSettingsRequest),
    ServerSettingsResponse(ServerSettingsResponse),
    ShowProfile(ShowProfile),
    SetDefaultGameType(SetDefaultGameType),
    RemoveObjective(RemoveObjective),
    SetDisplayObjective(SetDisplayObjective),
    SetScore(SetScore),
    LabTable(LabTable),
    UpdateBlockSynced(UpdateBlockSynced),
    MoveActorDelta(MoveActorDelta),
    SetScoreboardIdentity(SetScoreboardIdentity),
    SetLocalPlayerAsInitialized(SetLocalPlayerAsInitialized),
    UpdateSoftEnum(UpdateSoftEnum),
    NetworkStackLatency(NetworkStackLatency),
    SpawnParticleEffect(SpawnParticleEffect),
    AvailableActorIdentifiers(AvailableActorIdentifiers),
    NetworkChunkPublisherUpdate(NetworkChunkPublisherUpdate),
    BiomeDefinitionList(BiomeDefinitionList),
    LevelSoundEvent(Box<LevelSoundEvent>),
    LevelEventGeneric(LevelEventGeneric),
    LecternUpdate(LecternUpdate),
    ClientCacheStatus(ClientCacheStatus),
    OnScreenTextureAnimation(OnScreenTextureAnimation),
    MapCreateLockedCopy(MapCreateLockedCopy),
    StructureTemplateDataRequest(StructureTemplateDataRequest),
    StructureTemplateDataResponse(StructureTemplateDataResponse),
    ClientCacheBlobStatus(ClientCacheBlobStatus),
    ClientCacheMissResponse(ClientCacheMissResponse),
    EducationSettings(EducationSettings),
    Emote(Emote),
    MultiplayerSettings(MultiplayerSettings),
    SettingsCommand(SettingsCommand),
    AnvilDamage(AnvilDamage),
    CompletedUsingItem(CompletedUsingItem),
    NetworkSettings(NetworkSettings),
    PlayerAuthInput(Box<PlayerAuthInput>),
    CreativeContent(CreativeContent),
    PlayerEnchantOptions(PlayerEnchantOptions),
    ItemStackRequest(ItemStackRequest),
    ItemStackResponse(ItemStackResponse),
    PlayerArmorDamage(PlayerArmorDamage),
    CodeBuilder(CodeBuilder),
    UpdatePlayerGameType(UpdatePlayerGameType),
    EmoteList(EmoteList),
    PositionTrackingDBServerBroadcast(PositionTrackingDBServerBroadcast),
    PositionTrackingDBClientRequest(PositionTrackingDBClientRequest),
    DebugInfo(DebugInfo),
    PacketViolationWarning(PacketViolationWarning),
    MotionPredictionHints(MotionPredictionHints),
    AnimateEntity(AnimateEntity),
    CameraShake(CameraShake),
    PlayerFog(PlayerFog),
    CorrectPlayerMovePrediction(CorrectPlayerMovePrediction),
    ItemRegistry(ItemRegistry),
    ClientboundDebugRenderer(ClientboundDebugRenderer),
    SyncActorProperty(SyncActorProperty),
    AddVolumeEntity(Box<AddVolumeEntity>),
    RemoveVolumeEntity(RemoveVolumeEntity),
    SimulationType(SimulationType),
    NpcDialogue(NpcDialogue),
    EduUriResource(EduUriResource),
    CreatePhoto(CreatePhoto),
    UpdateSubChunkBlocks(UpdateSubChunkBlocks),
    SubChunk(SubChunk),
    SubChunkRequest(SubChunkRequest),
    PlayerStartItemCooldown(PlayerStartItemCooldown),
    ScriptMessage(ScriptMessage),
    CodeBuilderSource(CodeBuilderSource),
    TickingAreasLoadStatus(TickingAreasLoadStatus),
    DimensionData(DimensionData),
    AgentActionEvent(AgentActionEvent),
    ChangeMobProperty(ChangeMobProperty),
    LessonProgress(LessonProgress),
    RequestAbility(RequestAbility),
    RequestPermissions(RequestPermissions),
    ToastRequest(ToastRequest),
    UpdateAbilities(UpdateAbilities),
    UpdateAdventureSettings(UpdateAdventureSettings),
    DeathInfo(DeathInfo),
    EditorNetwork(EditorNetwork),
    FeatureRegistry(FeatureRegistry),
    ServerStats(ServerStats),
    RequestNetworkSettings(RequestNetworkSettings),
    GameTestRequest(GameTestRequest),
    GameTestResults(GameTestResults),
    UpdateClientInputLocks(UpdateClientInputLocks),
    CameraPresets(CameraPresets),
    UnlockedRecipes(UnlockedRecipes),
    CameraInstruction(CameraInstruction),
    TrimData(TrimData),
    OpenSign(OpenSign),
    AgentAnimation(AgentAnimation),
    RefreshEntitlements(RefreshEntitlements),
    PlayerToggleCrafterSlotRequest(PlayerToggleCrafterSlotRequest),
    SetPlayerInventoryOptions(SetPlayerInventoryOptions),
    SetHud(SetHud),
    AwardAchievement(AwardAchievement),
    ClientboundCloseForm(ClientboundCloseForm),
    ServerboundLoadingScreen(ServerboundLoadingScreen),
    JigsawStructureData(JigsawStructureData),
    CurrentStructureFeature(CurrentStructureFeature),
    ServerboundDiagnostics(Box<ServerboundDiagnostics>),
    CameraAimAssist(CameraAimAssist),
    ContainerRegistryCleanup(ContainerRegistryCleanup),
    MovementEffect(MovementEffect),
    CameraAimAssistPresets(CameraAimAssistPresets),
    ClientCameraAimAssist(ClientCameraAimAssist),
    ClientMovementPredictionSync(ClientMovementPredictionSync),
    UpdateClientOptions(UpdateClientOptions),
    PlayerVideoCapture(PlayerVideoCapture),
    PlayerUpdateEntityOverrides(PlayerUpdateEntityOverrides),
    PlayerLocation(PlayerLocation),
    ClientboundControlSchemeSet(ClientboundControlSchemeSet),
    PrimitiveShapes(PrimitiveShapes),
    ServerboundPackSettingChange(ServerboundPackSettingChange),
    ClientboundDataStore(ClientboundDataStore),
    GraphicsOverrideParameter(GraphicsOverrideParameter),
    ServerboundDataStore(ServerboundDataStore),
    ClientboundDataDrivenUIShowScreen(ClientboundDataDrivenUIShowScreen),
    ClientboundDataDrivenUICloseScreen(ClientboundDataDrivenUICloseScreen),
    ClientboundDataDrivenUIReload(ClientboundDataDrivenUIReload),
    ClientboundTextureShift(Box<ClientboundTextureShift>),
    VoxelShapes(VoxelShapes),
    CameraSpline(CameraSpline),
    CameraAimAssistActorPriority(CameraAimAssistActorPriority),
    ResourcePacksReadyForValidation(ResourcePacksReadyForValidation),
    LocatorBar(LocatorBar),
    PartyChanged(PartyChanged),
    ServerboundDataDrivenScreenClosed(ServerboundDataDrivenScreenClosed),
    SyncWorldClocks(SyncWorldClocks),
    ClientboundAttributeLayerSync(ClientboundAttributeLayerSync),
    ServerStoreInfo(ServerStoreInfo),
    ServerPresenceInfo(ServerPresenceInfo),
    ClientboundUpdateSoundData(Box<ClientboundUpdateSoundData>),
    SendPartyDestinationCookie(SendPartyDestinationCookie),
    PartyDestinationCookieResponse(PartyDestinationCookieResponse),
    SetPlayerFurnaceOptions(SetPlayerFurnaceOptions),
    RecordStarted(RecordStarted),
}

impl From<Login> for Packet {
    fn from(value: Login) -> Self {
        Self::Login(value)
    }
}

impl From<PlayStatus> for Packet {
    fn from(value: PlayStatus) -> Self {
        Self::PlayStatus(value)
    }
}

impl From<ServerToClientHandshake> for Packet {
    fn from(value: ServerToClientHandshake) -> Self {
        Self::ServerToClientHandshake(value)
    }
}

impl From<ClientToServerHandshake> for Packet {
    fn from(value: ClientToServerHandshake) -> Self {
        Self::ClientToServerHandshake(value)
    }
}

impl From<Disconnect> for Packet {
    fn from(value: Disconnect) -> Self {
        Self::Disconnect(value)
    }
}

impl From<ResourcePacksInfo> for Packet {
    fn from(value: ResourcePacksInfo) -> Self {
        Self::ResourcePacksInfo(value)
    }
}

impl From<ResourcePackStack> for Packet {
    fn from(value: ResourcePackStack) -> Self {
        Self::ResourcePackStack(value)
    }
}

impl From<ResourcePackClientResponse> for Packet {
    fn from(value: ResourcePackClientResponse) -> Self {
        Self::ResourcePackClientResponse(value)
    }
}

impl From<Text> for Packet {
    fn from(value: Text) -> Self {
        Self::Text(value)
    }
}

impl From<SetTime> for Packet {
    fn from(value: SetTime) -> Self {
        Self::SetTime(value)
    }
}

impl From<StartGame> for Packet {
    fn from(value: StartGame) -> Self {
        Self::StartGame(Box::new(value))
    }
}

impl From<AddPlayer> for Packet {
    fn from(value: AddPlayer) -> Self {
        Self::AddPlayer(Box::new(value))
    }
}

impl From<AddActor> for Packet {
    fn from(value: AddActor) -> Self {
        Self::AddActor(Box::new(value))
    }
}

impl From<RemoveActor> for Packet {
    fn from(value: RemoveActor) -> Self {
        Self::RemoveActor(value)
    }
}

impl From<AddItemActor> for Packet {
    fn from(value: AddItemActor) -> Self {
        Self::AddItemActor(value)
    }
}

impl From<ServerPlayerPostMovePosition> for Packet {
    fn from(value: ServerPlayerPostMovePosition) -> Self {
        Self::ServerPlayerPostMovePosition(value)
    }
}

impl From<TakeItemActor> for Packet {
    fn from(value: TakeItemActor) -> Self {
        Self::TakeItemActor(value)
    }
}

impl From<MoveActorAbsolute> for Packet {
    fn from(value: MoveActorAbsolute) -> Self {
        Self::MoveActorAbsolute(value)
    }
}

impl From<MovePlayer> for Packet {
    fn from(value: MovePlayer) -> Self {
        Self::MovePlayer(Box::new(value))
    }
}

impl From<UpdateBlock> for Packet {
    fn from(value: UpdateBlock) -> Self {
        Self::UpdateBlock(value)
    }
}

impl From<AddPainting> for Packet {
    fn from(value: AddPainting) -> Self {
        Self::AddPainting(value)
    }
}

impl From<LevelEvent> for Packet {
    fn from(value: LevelEvent) -> Self {
        Self::LevelEvent(value)
    }
}

impl From<BlockEvent> for Packet {
    fn from(value: BlockEvent) -> Self {
        Self::BlockEvent(value)
    }
}

impl From<ActorEvent> for Packet {
    fn from(value: ActorEvent) -> Self {
        Self::ActorEvent(value)
    }
}

impl From<MobEffect> for Packet {
    fn from(value: MobEffect) -> Self {
        Self::MobEffect(Box::new(value))
    }
}

impl From<UpdateAttributes> for Packet {
    fn from(value: UpdateAttributes) -> Self {
        Self::UpdateAttributes(value)
    }
}

impl From<InventoryTransaction> for Packet {
    fn from(value: InventoryTransaction) -> Self {
        Self::InventoryTransaction(value)
    }
}

impl From<MobEquipment> for Packet {
    fn from(value: MobEquipment) -> Self {
        Self::MobEquipment(value)
    }
}

impl From<MobArmorEquipment> for Packet {
    fn from(value: MobArmorEquipment) -> Self {
        Self::MobArmorEquipment(value)
    }
}

impl From<Interact> for Packet {
    fn from(value: Interact) -> Self {
        Self::Interact(value)
    }
}

impl From<BlockPickRequest> for Packet {
    fn from(value: BlockPickRequest) -> Self {
        Self::BlockPickRequest(value)
    }
}

impl From<ActorPickRequest> for Packet {
    fn from(value: ActorPickRequest) -> Self {
        Self::ActorPickRequest(value)
    }
}

impl From<PlayerAction> for Packet {
    fn from(value: PlayerAction) -> Self {
        Self::PlayerAction(value)
    }
}

impl From<HurtArmor> for Packet {
    fn from(value: HurtArmor) -> Self {
        Self::HurtArmor(value)
    }
}

impl From<SetActorData> for Packet {
    fn from(value: SetActorData) -> Self {
        Self::SetActorData(value)
    }
}

impl From<SetActorMotion> for Packet {
    fn from(value: SetActorMotion) -> Self {
        Self::SetActorMotion(value)
    }
}

impl From<SetActorLink> for Packet {
    fn from(value: SetActorLink) -> Self {
        Self::SetActorLink(value)
    }
}

impl From<SetHealth> for Packet {
    fn from(value: SetHealth) -> Self {
        Self::SetHealth(value)
    }
}

impl From<SetSpawnPosition> for Packet {
    fn from(value: SetSpawnPosition) -> Self {
        Self::SetSpawnPosition(value)
    }
}

impl From<Animate> for Packet {
    fn from(value: Animate) -> Self {
        Self::Animate(value)
    }
}

impl From<Respawn> for Packet {
    fn from(value: Respawn) -> Self {
        Self::Respawn(value)
    }
}

impl From<ContainerOpen> for Packet {
    fn from(value: ContainerOpen) -> Self {
        Self::ContainerOpen(value)
    }
}

impl From<ContainerClose> for Packet {
    fn from(value: ContainerClose) -> Self {
        Self::ContainerClose(value)
    }
}

impl From<PlayerHotbar> for Packet {
    fn from(value: PlayerHotbar) -> Self {
        Self::PlayerHotbar(value)
    }
}

impl From<InventoryContent> for Packet {
    fn from(value: InventoryContent) -> Self {
        Self::InventoryContent(value)
    }
}

impl From<InventorySlot> for Packet {
    fn from(value: InventorySlot) -> Self {
        Self::InventorySlot(value)
    }
}

impl From<ContainerSetData> for Packet {
    fn from(value: ContainerSetData) -> Self {
        Self::ContainerSetData(value)
    }
}

impl From<CraftingData> for Packet {
    fn from(value: CraftingData) -> Self {
        Self::CraftingData(Box::new(value))
    }
}

impl From<GuiDataPickItem> for Packet {
    fn from(value: GuiDataPickItem) -> Self {
        Self::GuiDataPickItem(value)
    }
}

impl From<BlockActorData> for Packet {
    fn from(value: BlockActorData) -> Self {
        Self::BlockActorData(value)
    }
}

impl From<LevelChunk> for Packet {
    fn from(value: LevelChunk) -> Self {
        Self::LevelChunk(value)
    }
}

impl From<SetCommandsEnabled> for Packet {
    fn from(value: SetCommandsEnabled) -> Self {
        Self::SetCommandsEnabled(value)
    }
}

impl From<SetDifficulty> for Packet {
    fn from(value: SetDifficulty) -> Self {
        Self::SetDifficulty(value)
    }
}

impl From<ChangeDimension> for Packet {
    fn from(value: ChangeDimension) -> Self {
        Self::ChangeDimension(value)
    }
}

impl From<SetPlayerGameType> for Packet {
    fn from(value: SetPlayerGameType) -> Self {
        Self::SetPlayerGameType(value)
    }
}

impl From<PlayerList> for Packet {
    fn from(value: PlayerList) -> Self {
        Self::PlayerList(value)
    }
}

impl From<SimpleEvent> for Packet {
    fn from(value: SimpleEvent) -> Self {
        Self::SimpleEvent(value)
    }
}

impl From<LegacyTelemetryEvent> for Packet {
    fn from(value: LegacyTelemetryEvent) -> Self {
        Self::LegacyTelemetryEvent(value)
    }
}

impl From<SpawnExperienceOrb> for Packet {
    fn from(value: SpawnExperienceOrb) -> Self {
        Self::SpawnExperienceOrb(value)
    }
}

impl From<ClientboundMapItemData> for Packet {
    fn from(value: ClientboundMapItemData) -> Self {
        Self::ClientboundMapItemData(Box::new(value))
    }
}

impl From<MapInfoRequest> for Packet {
    fn from(value: MapInfoRequest) -> Self {
        Self::MapInfoRequest(value)
    }
}

impl From<RequestChunkRadius> for Packet {
    fn from(value: RequestChunkRadius) -> Self {
        Self::RequestChunkRadius(value)
    }
}

impl From<ChunkRadiusUpdated> for Packet {
    fn from(value: ChunkRadiusUpdated) -> Self {
        Self::ChunkRadiusUpdated(value)
    }
}

impl From<GameRulesChanged> for Packet {
    fn from(value: GameRulesChanged) -> Self {
        Self::GameRulesChanged(value)
    }
}

impl From<Camera> for Packet {
    fn from(value: Camera) -> Self {
        Self::Camera(value)
    }
}

impl From<BossEvent> for Packet {
    fn from(value: BossEvent) -> Self {
        Self::BossEvent(value)
    }
}

impl From<ShowCredits> for Packet {
    fn from(value: ShowCredits) -> Self {
        Self::ShowCredits(value)
    }
}

impl From<AvailableCommands> for Packet {
    fn from(value: AvailableCommands) -> Self {
        Self::AvailableCommands(Box::new(value))
    }
}

impl From<CommandRequest> for Packet {
    fn from(value: CommandRequest) -> Self {
        Self::CommandRequest(value)
    }
}

impl From<CommandBlockUpdate> for Packet {
    fn from(value: CommandBlockUpdate) -> Self {
        Self::CommandBlockUpdate(Box::new(value))
    }
}

impl From<CommandOutput> for Packet {
    fn from(value: CommandOutput) -> Self {
        Self::CommandOutput(value)
    }
}

impl From<UpdateTrade> for Packet {
    fn from(value: UpdateTrade) -> Self {
        Self::UpdateTrade(Box::new(value))
    }
}

impl From<UpdateEquip> for Packet {
    fn from(value: UpdateEquip) -> Self {
        Self::UpdateEquip(value)
    }
}

impl From<ResourcePackDataInfo> for Packet {
    fn from(value: ResourcePackDataInfo) -> Self {
        Self::ResourcePackDataInfo(value)
    }
}

impl From<ResourcePackChunkData> for Packet {
    fn from(value: ResourcePackChunkData) -> Self {
        Self::ResourcePackChunkData(value)
    }
}

impl From<ResourcePackChunkRequest> for Packet {
    fn from(value: ResourcePackChunkRequest) -> Self {
        Self::ResourcePackChunkRequest(value)
    }
}

impl From<Transfer> for Packet {
    fn from(value: Transfer) -> Self {
        Self::Transfer(value)
    }
}

impl From<PlaySound> for Packet {
    fn from(value: PlaySound) -> Self {
        Self::PlaySound(Box::new(value))
    }
}

impl From<StopSound> for Packet {
    fn from(value: StopSound) -> Self {
        Self::StopSound(value)
    }
}

impl From<SetTitle> for Packet {
    fn from(value: SetTitle) -> Self {
        Self::SetTitle(Box::new(value))
    }
}

impl From<AddBehaviorTree> for Packet {
    fn from(value: AddBehaviorTree) -> Self {
        Self::AddBehaviorTree(value)
    }
}

impl From<StructureBlockUpdate> for Packet {
    fn from(value: StructureBlockUpdate) -> Self {
        Self::StructureBlockUpdate(value)
    }
}

impl From<ShowStoreOffer> for Packet {
    fn from(value: ShowStoreOffer) -> Self {
        Self::ShowStoreOffer(value)
    }
}

impl From<PurchaseReceipt> for Packet {
    fn from(value: PurchaseReceipt) -> Self {
        Self::PurchaseReceipt(value)
    }
}

impl From<PlayerSkin> for Packet {
    fn from(value: PlayerSkin) -> Self {
        Self::PlayerSkin(value)
    }
}

impl From<SubClientLogin> for Packet {
    fn from(value: SubClientLogin) -> Self {
        Self::SubClientLogin(value)
    }
}

impl From<AutomationClientConnect> for Packet {
    fn from(value: AutomationClientConnect) -> Self {
        Self::AutomationClientConnect(value)
    }
}

impl From<SetLastHurtBy> for Packet {
    fn from(value: SetLastHurtBy) -> Self {
        Self::SetLastHurtBy(value)
    }
}

impl From<BookEdit> for Packet {
    fn from(value: BookEdit) -> Self {
        Self::BookEdit(value)
    }
}

impl From<NpcRequest> for Packet {
    fn from(value: NpcRequest) -> Self {
        Self::NpcRequest(value)
    }
}

impl From<PhotoTransfer> for Packet {
    fn from(value: PhotoTransfer) -> Self {
        Self::PhotoTransfer(value)
    }
}

impl From<ModalFormRequest> for Packet {
    fn from(value: ModalFormRequest) -> Self {
        Self::ModalFormRequest(value)
    }
}

impl From<ModalFormResponse> for Packet {
    fn from(value: ModalFormResponse) -> Self {
        Self::ModalFormResponse(value)
    }
}

impl From<ServerSettingsRequest> for Packet {
    fn from(value: ServerSettingsRequest) -> Self {
        Self::ServerSettingsRequest(value)
    }
}

impl From<ServerSettingsResponse> for Packet {
    fn from(value: ServerSettingsResponse) -> Self {
        Self::ServerSettingsResponse(value)
    }
}

impl From<ShowProfile> for Packet {
    fn from(value: ShowProfile) -> Self {
        Self::ShowProfile(value)
    }
}

impl From<SetDefaultGameType> for Packet {
    fn from(value: SetDefaultGameType) -> Self {
        Self::SetDefaultGameType(value)
    }
}

impl From<RemoveObjective> for Packet {
    fn from(value: RemoveObjective) -> Self {
        Self::RemoveObjective(value)
    }
}

impl From<SetDisplayObjective> for Packet {
    fn from(value: SetDisplayObjective) -> Self {
        Self::SetDisplayObjective(value)
    }
}

impl From<SetScore> for Packet {
    fn from(value: SetScore) -> Self {
        Self::SetScore(value)
    }
}

impl From<LabTable> for Packet {
    fn from(value: LabTable) -> Self {
        Self::LabTable(value)
    }
}

impl From<UpdateBlockSynced> for Packet {
    fn from(value: UpdateBlockSynced) -> Self {
        Self::UpdateBlockSynced(value)
    }
}

impl From<MoveActorDelta> for Packet {
    fn from(value: MoveActorDelta) -> Self {
        Self::MoveActorDelta(value)
    }
}

impl From<SetScoreboardIdentity> for Packet {
    fn from(value: SetScoreboardIdentity) -> Self {
        Self::SetScoreboardIdentity(value)
    }
}

impl From<SetLocalPlayerAsInitialized> for Packet {
    fn from(value: SetLocalPlayerAsInitialized) -> Self {
        Self::SetLocalPlayerAsInitialized(value)
    }
}

impl From<UpdateSoftEnum> for Packet {
    fn from(value: UpdateSoftEnum) -> Self {
        Self::UpdateSoftEnum(value)
    }
}

impl From<NetworkStackLatency> for Packet {
    fn from(value: NetworkStackLatency) -> Self {
        Self::NetworkStackLatency(value)
    }
}

impl From<SpawnParticleEffect> for Packet {
    fn from(value: SpawnParticleEffect) -> Self {
        Self::SpawnParticleEffect(value)
    }
}

impl From<AvailableActorIdentifiers> for Packet {
    fn from(value: AvailableActorIdentifiers) -> Self {
        Self::AvailableActorIdentifiers(value)
    }
}

impl From<NetworkChunkPublisherUpdate> for Packet {
    fn from(value: NetworkChunkPublisherUpdate) -> Self {
        Self::NetworkChunkPublisherUpdate(value)
    }
}

impl From<BiomeDefinitionList> for Packet {
    fn from(value: BiomeDefinitionList) -> Self {
        Self::BiomeDefinitionList(value)
    }
}

impl From<LevelSoundEvent> for Packet {
    fn from(value: LevelSoundEvent) -> Self {
        Self::LevelSoundEvent(Box::new(value))
    }
}

impl From<LevelEventGeneric> for Packet {
    fn from(value: LevelEventGeneric) -> Self {
        Self::LevelEventGeneric(value)
    }
}

impl From<LecternUpdate> for Packet {
    fn from(value: LecternUpdate) -> Self {
        Self::LecternUpdate(value)
    }
}

impl From<ClientCacheStatus> for Packet {
    fn from(value: ClientCacheStatus) -> Self {
        Self::ClientCacheStatus(value)
    }
}

impl From<OnScreenTextureAnimation> for Packet {
    fn from(value: OnScreenTextureAnimation) -> Self {
        Self::OnScreenTextureAnimation(value)
    }
}

impl From<MapCreateLockedCopy> for Packet {
    fn from(value: MapCreateLockedCopy) -> Self {
        Self::MapCreateLockedCopy(value)
    }
}

impl From<StructureTemplateDataRequest> for Packet {
    fn from(value: StructureTemplateDataRequest) -> Self {
        Self::StructureTemplateDataRequest(value)
    }
}

impl From<StructureTemplateDataResponse> for Packet {
    fn from(value: StructureTemplateDataResponse) -> Self {
        Self::StructureTemplateDataResponse(value)
    }
}

impl From<ClientCacheBlobStatus> for Packet {
    fn from(value: ClientCacheBlobStatus) -> Self {
        Self::ClientCacheBlobStatus(value)
    }
}

impl From<ClientCacheMissResponse> for Packet {
    fn from(value: ClientCacheMissResponse) -> Self {
        Self::ClientCacheMissResponse(value)
    }
}

impl From<EducationSettings> for Packet {
    fn from(value: EducationSettings) -> Self {
        Self::EducationSettings(value)
    }
}

impl From<Emote> for Packet {
    fn from(value: Emote) -> Self {
        Self::Emote(value)
    }
}

impl From<MultiplayerSettings> for Packet {
    fn from(value: MultiplayerSettings) -> Self {
        Self::MultiplayerSettings(value)
    }
}

impl From<SettingsCommand> for Packet {
    fn from(value: SettingsCommand) -> Self {
        Self::SettingsCommand(value)
    }
}

impl From<AnvilDamage> for Packet {
    fn from(value: AnvilDamage) -> Self {
        Self::AnvilDamage(value)
    }
}

impl From<CompletedUsingItem> for Packet {
    fn from(value: CompletedUsingItem) -> Self {
        Self::CompletedUsingItem(value)
    }
}

impl From<NetworkSettings> for Packet {
    fn from(value: NetworkSettings) -> Self {
        Self::NetworkSettings(value)
    }
}

impl From<PlayerAuthInput> for Packet {
    fn from(value: PlayerAuthInput) -> Self {
        Self::PlayerAuthInput(Box::new(value))
    }
}

impl From<CreativeContent> for Packet {
    fn from(value: CreativeContent) -> Self {
        Self::CreativeContent(value)
    }
}

impl From<PlayerEnchantOptions> for Packet {
    fn from(value: PlayerEnchantOptions) -> Self {
        Self::PlayerEnchantOptions(value)
    }
}

impl From<ItemStackRequest> for Packet {
    fn from(value: ItemStackRequest) -> Self {
        Self::ItemStackRequest(value)
    }
}

impl From<ItemStackResponse> for Packet {
    fn from(value: ItemStackResponse) -> Self {
        Self::ItemStackResponse(value)
    }
}

impl From<PlayerArmorDamage> for Packet {
    fn from(value: PlayerArmorDamage) -> Self {
        Self::PlayerArmorDamage(value)
    }
}

impl From<CodeBuilder> for Packet {
    fn from(value: CodeBuilder) -> Self {
        Self::CodeBuilder(value)
    }
}

impl From<UpdatePlayerGameType> for Packet {
    fn from(value: UpdatePlayerGameType) -> Self {
        Self::UpdatePlayerGameType(value)
    }
}

impl From<EmoteList> for Packet {
    fn from(value: EmoteList) -> Self {
        Self::EmoteList(value)
    }
}

impl From<PositionTrackingDBServerBroadcast> for Packet {
    fn from(value: PositionTrackingDBServerBroadcast) -> Self {
        Self::PositionTrackingDBServerBroadcast(value)
    }
}

impl From<PositionTrackingDBClientRequest> for Packet {
    fn from(value: PositionTrackingDBClientRequest) -> Self {
        Self::PositionTrackingDBClientRequest(value)
    }
}

impl From<DebugInfo> for Packet {
    fn from(value: DebugInfo) -> Self {
        Self::DebugInfo(value)
    }
}

impl From<PacketViolationWarning> for Packet {
    fn from(value: PacketViolationWarning) -> Self {
        Self::PacketViolationWarning(value)
    }
}

impl From<MotionPredictionHints> for Packet {
    fn from(value: MotionPredictionHints) -> Self {
        Self::MotionPredictionHints(value)
    }
}

impl From<AnimateEntity> for Packet {
    fn from(value: AnimateEntity) -> Self {
        Self::AnimateEntity(value)
    }
}

impl From<CameraShake> for Packet {
    fn from(value: CameraShake) -> Self {
        Self::CameraShake(value)
    }
}

impl From<PlayerFog> for Packet {
    fn from(value: PlayerFog) -> Self {
        Self::PlayerFog(value)
    }
}

impl From<CorrectPlayerMovePrediction> for Packet {
    fn from(value: CorrectPlayerMovePrediction) -> Self {
        Self::CorrectPlayerMovePrediction(value)
    }
}

impl From<ItemRegistry> for Packet {
    fn from(value: ItemRegistry) -> Self {
        Self::ItemRegistry(value)
    }
}

impl From<ClientboundDebugRenderer> for Packet {
    fn from(value: ClientboundDebugRenderer) -> Self {
        Self::ClientboundDebugRenderer(value)
    }
}

impl From<SyncActorProperty> for Packet {
    fn from(value: SyncActorProperty) -> Self {
        Self::SyncActorProperty(value)
    }
}

impl From<AddVolumeEntity> for Packet {
    fn from(value: AddVolumeEntity) -> Self {
        Self::AddVolumeEntity(Box::new(value))
    }
}

impl From<RemoveVolumeEntity> for Packet {
    fn from(value: RemoveVolumeEntity) -> Self {
        Self::RemoveVolumeEntity(value)
    }
}

impl From<SimulationType> for Packet {
    fn from(value: SimulationType) -> Self {
        Self::SimulationType(value)
    }
}

impl From<NpcDialogue> for Packet {
    fn from(value: NpcDialogue) -> Self {
        Self::NpcDialogue(value)
    }
}

impl From<EduUriResource> for Packet {
    fn from(value: EduUriResource) -> Self {
        Self::EduUriResource(value)
    }
}

impl From<CreatePhoto> for Packet {
    fn from(value: CreatePhoto) -> Self {
        Self::CreatePhoto(value)
    }
}

impl From<UpdateSubChunkBlocks> for Packet {
    fn from(value: UpdateSubChunkBlocks) -> Self {
        Self::UpdateSubChunkBlocks(value)
    }
}

impl From<SubChunk> for Packet {
    fn from(value: SubChunk) -> Self {
        Self::SubChunk(value)
    }
}

impl From<SubChunkRequest> for Packet {
    fn from(value: SubChunkRequest) -> Self {
        Self::SubChunkRequest(value)
    }
}

impl From<PlayerStartItemCooldown> for Packet {
    fn from(value: PlayerStartItemCooldown) -> Self {
        Self::PlayerStartItemCooldown(value)
    }
}

impl From<ScriptMessage> for Packet {
    fn from(value: ScriptMessage) -> Self {
        Self::ScriptMessage(value)
    }
}

impl From<CodeBuilderSource> for Packet {
    fn from(value: CodeBuilderSource) -> Self {
        Self::CodeBuilderSource(value)
    }
}

impl From<TickingAreasLoadStatus> for Packet {
    fn from(value: TickingAreasLoadStatus) -> Self {
        Self::TickingAreasLoadStatus(value)
    }
}

impl From<DimensionData> for Packet {
    fn from(value: DimensionData) -> Self {
        Self::DimensionData(value)
    }
}

impl From<AgentActionEvent> for Packet {
    fn from(value: AgentActionEvent) -> Self {
        Self::AgentActionEvent(value)
    }
}

impl From<ChangeMobProperty> for Packet {
    fn from(value: ChangeMobProperty) -> Self {
        Self::ChangeMobProperty(value)
    }
}

impl From<LessonProgress> for Packet {
    fn from(value: LessonProgress) -> Self {
        Self::LessonProgress(value)
    }
}

impl From<RequestAbility> for Packet {
    fn from(value: RequestAbility) -> Self {
        Self::RequestAbility(value)
    }
}

impl From<RequestPermissions> for Packet {
    fn from(value: RequestPermissions) -> Self {
        Self::RequestPermissions(value)
    }
}

impl From<ToastRequest> for Packet {
    fn from(value: ToastRequest) -> Self {
        Self::ToastRequest(value)
    }
}

impl From<UpdateAbilities> for Packet {
    fn from(value: UpdateAbilities) -> Self {
        Self::UpdateAbilities(value)
    }
}

impl From<UpdateAdventureSettings> for Packet {
    fn from(value: UpdateAdventureSettings) -> Self {
        Self::UpdateAdventureSettings(value)
    }
}

impl From<DeathInfo> for Packet {
    fn from(value: DeathInfo) -> Self {
        Self::DeathInfo(value)
    }
}

impl From<EditorNetwork> for Packet {
    fn from(value: EditorNetwork) -> Self {
        Self::EditorNetwork(value)
    }
}

impl From<FeatureRegistry> for Packet {
    fn from(value: FeatureRegistry) -> Self {
        Self::FeatureRegistry(value)
    }
}

impl From<ServerStats> for Packet {
    fn from(value: ServerStats) -> Self {
        Self::ServerStats(value)
    }
}

impl From<RequestNetworkSettings> for Packet {
    fn from(value: RequestNetworkSettings) -> Self {
        Self::RequestNetworkSettings(value)
    }
}

impl From<GameTestRequest> for Packet {
    fn from(value: GameTestRequest) -> Self {
        Self::GameTestRequest(value)
    }
}

impl From<GameTestResults> for Packet {
    fn from(value: GameTestResults) -> Self {
        Self::GameTestResults(value)
    }
}

impl From<UpdateClientInputLocks> for Packet {
    fn from(value: UpdateClientInputLocks) -> Self {
        Self::UpdateClientInputLocks(value)
    }
}

impl From<CameraPresets> for Packet {
    fn from(value: CameraPresets) -> Self {
        Self::CameraPresets(value)
    }
}

impl From<UnlockedRecipes> for Packet {
    fn from(value: UnlockedRecipes) -> Self {
        Self::UnlockedRecipes(value)
    }
}

impl From<CameraInstruction> for Packet {
    fn from(value: CameraInstruction) -> Self {
        Self::CameraInstruction(value)
    }
}

impl From<TrimData> for Packet {
    fn from(value: TrimData) -> Self {
        Self::TrimData(value)
    }
}

impl From<OpenSign> for Packet {
    fn from(value: OpenSign) -> Self {
        Self::OpenSign(value)
    }
}

impl From<AgentAnimation> for Packet {
    fn from(value: AgentAnimation) -> Self {
        Self::AgentAnimation(value)
    }
}

impl From<RefreshEntitlements> for Packet {
    fn from(value: RefreshEntitlements) -> Self {
        Self::RefreshEntitlements(value)
    }
}

impl From<PlayerToggleCrafterSlotRequest> for Packet {
    fn from(value: PlayerToggleCrafterSlotRequest) -> Self {
        Self::PlayerToggleCrafterSlotRequest(value)
    }
}

impl From<SetPlayerInventoryOptions> for Packet {
    fn from(value: SetPlayerInventoryOptions) -> Self {
        Self::SetPlayerInventoryOptions(value)
    }
}

impl From<SetHud> for Packet {
    fn from(value: SetHud) -> Self {
        Self::SetHud(value)
    }
}

impl From<AwardAchievement> for Packet {
    fn from(value: AwardAchievement) -> Self {
        Self::AwardAchievement(value)
    }
}

impl From<ClientboundCloseForm> for Packet {
    fn from(value: ClientboundCloseForm) -> Self {
        Self::ClientboundCloseForm(value)
    }
}

impl From<ServerboundLoadingScreen> for Packet {
    fn from(value: ServerboundLoadingScreen) -> Self {
        Self::ServerboundLoadingScreen(value)
    }
}

impl From<JigsawStructureData> for Packet {
    fn from(value: JigsawStructureData) -> Self {
        Self::JigsawStructureData(value)
    }
}

impl From<CurrentStructureFeature> for Packet {
    fn from(value: CurrentStructureFeature) -> Self {
        Self::CurrentStructureFeature(value)
    }
}

impl From<ServerboundDiagnostics> for Packet {
    fn from(value: ServerboundDiagnostics) -> Self {
        Self::ServerboundDiagnostics(Box::new(value))
    }
}

impl From<CameraAimAssist> for Packet {
    fn from(value: CameraAimAssist) -> Self {
        Self::CameraAimAssist(value)
    }
}

impl From<ContainerRegistryCleanup> for Packet {
    fn from(value: ContainerRegistryCleanup) -> Self {
        Self::ContainerRegistryCleanup(value)
    }
}

impl From<MovementEffect> for Packet {
    fn from(value: MovementEffect) -> Self {
        Self::MovementEffect(value)
    }
}

impl From<CameraAimAssistPresets> for Packet {
    fn from(value: CameraAimAssistPresets) -> Self {
        Self::CameraAimAssistPresets(value)
    }
}

impl From<ClientCameraAimAssist> for Packet {
    fn from(value: ClientCameraAimAssist) -> Self {
        Self::ClientCameraAimAssist(value)
    }
}

impl From<ClientMovementPredictionSync> for Packet {
    fn from(value: ClientMovementPredictionSync) -> Self {
        Self::ClientMovementPredictionSync(value)
    }
}

impl From<UpdateClientOptions> for Packet {
    fn from(value: UpdateClientOptions) -> Self {
        Self::UpdateClientOptions(value)
    }
}

impl From<PlayerVideoCapture> for Packet {
    fn from(value: PlayerVideoCapture) -> Self {
        Self::PlayerVideoCapture(value)
    }
}

impl From<PlayerUpdateEntityOverrides> for Packet {
    fn from(value: PlayerUpdateEntityOverrides) -> Self {
        Self::PlayerUpdateEntityOverrides(value)
    }
}

impl From<PlayerLocation> for Packet {
    fn from(value: PlayerLocation) -> Self {
        Self::PlayerLocation(value)
    }
}

impl From<ClientboundControlSchemeSet> for Packet {
    fn from(value: ClientboundControlSchemeSet) -> Self {
        Self::ClientboundControlSchemeSet(value)
    }
}

impl From<PrimitiveShapes> for Packet {
    fn from(value: PrimitiveShapes) -> Self {
        Self::PrimitiveShapes(value)
    }
}

impl From<ServerboundPackSettingChange> for Packet {
    fn from(value: ServerboundPackSettingChange) -> Self {
        Self::ServerboundPackSettingChange(value)
    }
}

impl From<ClientboundDataStore> for Packet {
    fn from(value: ClientboundDataStore) -> Self {
        Self::ClientboundDataStore(value)
    }
}

impl From<GraphicsOverrideParameter> for Packet {
    fn from(value: GraphicsOverrideParameter) -> Self {
        Self::GraphicsOverrideParameter(value)
    }
}

impl From<ServerboundDataStore> for Packet {
    fn from(value: ServerboundDataStore) -> Self {
        Self::ServerboundDataStore(value)
    }
}

impl From<ClientboundDataDrivenUIShowScreen> for Packet {
    fn from(value: ClientboundDataDrivenUIShowScreen) -> Self {
        Self::ClientboundDataDrivenUIShowScreen(value)
    }
}

impl From<ClientboundDataDrivenUICloseScreen> for Packet {
    fn from(value: ClientboundDataDrivenUICloseScreen) -> Self {
        Self::ClientboundDataDrivenUICloseScreen(value)
    }
}

impl From<ClientboundDataDrivenUIReload> for Packet {
    fn from(value: ClientboundDataDrivenUIReload) -> Self {
        Self::ClientboundDataDrivenUIReload(value)
    }
}

impl From<ClientboundTextureShift> for Packet {
    fn from(value: ClientboundTextureShift) -> Self {
        Self::ClientboundTextureShift(Box::new(value))
    }
}

impl From<VoxelShapes> for Packet {
    fn from(value: VoxelShapes) -> Self {
        Self::VoxelShapes(value)
    }
}

impl From<CameraSpline> for Packet {
    fn from(value: CameraSpline) -> Self {
        Self::CameraSpline(value)
    }
}

impl From<CameraAimAssistActorPriority> for Packet {
    fn from(value: CameraAimAssistActorPriority) -> Self {
        Self::CameraAimAssistActorPriority(value)
    }
}

impl From<ResourcePacksReadyForValidation> for Packet {
    fn from(value: ResourcePacksReadyForValidation) -> Self {
        Self::ResourcePacksReadyForValidation(value)
    }
}

impl From<LocatorBar> for Packet {
    fn from(value: LocatorBar) -> Self {
        Self::LocatorBar(value)
    }
}

impl From<PartyChanged> for Packet {
    fn from(value: PartyChanged) -> Self {
        Self::PartyChanged(value)
    }
}

impl From<ServerboundDataDrivenScreenClosed> for Packet {
    fn from(value: ServerboundDataDrivenScreenClosed) -> Self {
        Self::ServerboundDataDrivenScreenClosed(value)
    }
}

impl From<SyncWorldClocks> for Packet {
    fn from(value: SyncWorldClocks) -> Self {
        Self::SyncWorldClocks(value)
    }
}

impl From<ClientboundAttributeLayerSync> for Packet {
    fn from(value: ClientboundAttributeLayerSync) -> Self {
        Self::ClientboundAttributeLayerSync(value)
    }
}

impl From<ServerStoreInfo> for Packet {
    fn from(value: ServerStoreInfo) -> Self {
        Self::ServerStoreInfo(value)
    }
}

impl From<ServerPresenceInfo> for Packet {
    fn from(value: ServerPresenceInfo) -> Self {
        Self::ServerPresenceInfo(value)
    }
}

impl From<ClientboundUpdateSoundData> for Packet {
    fn from(value: ClientboundUpdateSoundData) -> Self {
        Self::ClientboundUpdateSoundData(Box::new(value))
    }
}

impl From<SendPartyDestinationCookie> for Packet {
    fn from(value: SendPartyDestinationCookie) -> Self {
        Self::SendPartyDestinationCookie(value)
    }
}

impl From<PartyDestinationCookieResponse> for Packet {
    fn from(value: PartyDestinationCookieResponse) -> Self {
        Self::PartyDestinationCookieResponse(value)
    }
}

impl From<SetPlayerFurnaceOptions> for Packet {
    fn from(value: SetPlayerFurnaceOptions) -> Self {
        Self::SetPlayerFurnaceOptions(value)
    }
}

impl From<RecordStarted> for Packet {
    fn from(value: RecordStarted) -> Self {
        Self::RecordStarted(value)
    }
}

impl PacketId {
    pub const fn direction(self) -> Direction {
        match self {
            Self::Login => Direction::Bidirectional,
            Self::PlayStatus => Direction::Clientbound,
            Self::ServerToClientHandshake => Direction::Clientbound,
            Self::ClientToServerHandshake => Direction::Bidirectional,
            Self::Disconnect => Direction::Clientbound,
            Self::ResourcePacksInfo => Direction::Clientbound,
            Self::ResourcePackStack => Direction::Clientbound,
            Self::ResourcePackClientResponse => Direction::Bidirectional,
            Self::Text => Direction::Bidirectional,
            Self::SetTime => Direction::Clientbound,
            Self::StartGame => Direction::Clientbound,
            Self::AddPlayer => Direction::Clientbound,
            Self::AddActor => Direction::Clientbound,
            Self::RemoveActor => Direction::Clientbound,
            Self::AddItemActor => Direction::Clientbound,
            Self::ServerPlayerPostMovePosition => Direction::Clientbound,
            Self::TakeItemActor => Direction::Clientbound,
            Self::MoveActorAbsolute => Direction::Clientbound,
            Self::MovePlayer => Direction::Bidirectional,
            Self::UpdateBlock => Direction::Bidirectional,
            Self::AddPainting => Direction::Clientbound,
            Self::LevelEvent => Direction::Clientbound,
            Self::BlockEvent => Direction::Clientbound,
            Self::ActorEvent => Direction::Bidirectional,
            Self::MobEffect => Direction::Clientbound,
            Self::UpdateAttributes => Direction::Clientbound,
            Self::InventoryTransaction => Direction::Bidirectional,
            Self::MobEquipment => Direction::Bidirectional,
            Self::MobArmorEquipment => Direction::Clientbound,
            Self::Interact => Direction::Bidirectional,
            Self::BlockPickRequest => Direction::Bidirectional,
            Self::ActorPickRequest => Direction::Bidirectional,
            Self::PlayerAction => Direction::Bidirectional,
            Self::HurtArmor => Direction::Clientbound,
            Self::SetActorData => Direction::Clientbound,
            Self::SetActorMotion => Direction::Clientbound,
            Self::SetActorLink => Direction::Bidirectional,
            Self::SetHealth => Direction::Clientbound,
            Self::SetSpawnPosition => Direction::Clientbound,
            Self::Animate => Direction::Bidirectional,
            Self::Respawn => Direction::Bidirectional,
            Self::ContainerOpen => Direction::Bidirectional,
            Self::ContainerClose => Direction::Bidirectional,
            Self::PlayerHotbar => Direction::Clientbound,
            Self::InventoryContent => Direction::Clientbound,
            Self::InventorySlot => Direction::Clientbound,
            Self::ContainerSetData => Direction::Clientbound,
            Self::CraftingData => Direction::Clientbound,
            Self::GuiDataPickItem => Direction::Clientbound,
            Self::BlockActorData => Direction::Bidirectional,
            Self::LevelChunk => Direction::Clientbound,
            Self::SetCommandsEnabled => Direction::Clientbound,
            Self::SetDifficulty => Direction::Bidirectional,
            Self::ChangeDimension => Direction::Clientbound,
            Self::SetPlayerGameType => Direction::Bidirectional,
            Self::PlayerList => Direction::Clientbound,
            Self::SimpleEvent => Direction::Bidirectional,
            Self::LegacyTelemetryEvent => Direction::Clientbound,
            Self::SpawnExperienceOrb => Direction::Bidirectional,
            Self::ClientboundMapItemData => Direction::Clientbound,
            Self::MapInfoRequest => Direction::Bidirectional,
            Self::RequestChunkRadius => Direction::Bidirectional,
            Self::ChunkRadiusUpdated => Direction::Clientbound,
            Self::GameRulesChanged => Direction::Clientbound,
            Self::Camera => Direction::Clientbound,
            Self::BossEvent => Direction::Bidirectional,
            Self::ShowCredits => Direction::Bidirectional,
            Self::AvailableCommands => Direction::Clientbound,
            Self::CommandRequest => Direction::Bidirectional,
            Self::CommandBlockUpdate => Direction::Bidirectional,
            Self::CommandOutput => Direction::Clientbound,
            Self::UpdateTrade => Direction::Clientbound,
            Self::UpdateEquip => Direction::Clientbound,
            Self::ResourcePackDataInfo => Direction::Clientbound,
            Self::ResourcePackChunkData => Direction::Clientbound,
            Self::ResourcePackChunkRequest => Direction::Bidirectional,
            Self::Transfer => Direction::Clientbound,
            Self::PlaySound => Direction::Clientbound,
            Self::StopSound => Direction::Clientbound,
            Self::SetTitle => Direction::Clientbound,
            Self::AddBehaviorTree => Direction::Clientbound,
            Self::StructureBlockUpdate => Direction::Bidirectional,
            Self::ShowStoreOffer => Direction::Clientbound,
            Self::PurchaseReceipt => Direction::Bidirectional,
            Self::PlayerSkin => Direction::Bidirectional,
            Self::SubClientLogin => Direction::Bidirectional,
            Self::AutomationClientConnect => Direction::Bidirectional,
            Self::SetLastHurtBy => Direction::Clientbound,
            Self::BookEdit => Direction::Bidirectional,
            Self::NpcRequest => Direction::Bidirectional,
            Self::PhotoTransfer => Direction::Clientbound,
            Self::ModalFormRequest => Direction::Clientbound,
            Self::ModalFormResponse => Direction::Bidirectional,
            Self::ServerSettingsRequest => Direction::Bidirectional,
            Self::ServerSettingsResponse => Direction::Clientbound,
            Self::ShowProfile => Direction::Clientbound,
            Self::SetDefaultGameType => Direction::Bidirectional,
            Self::RemoveObjective => Direction::Clientbound,
            Self::SetDisplayObjective => Direction::Clientbound,
            Self::SetScore => Direction::Clientbound,
            Self::LabTable => Direction::Bidirectional,
            Self::UpdateBlockSynced => Direction::Clientbound,
            Self::MoveActorDelta => Direction::Clientbound,
            Self::SetScoreboardIdentity => Direction::Clientbound,
            Self::SetLocalPlayerAsInitialized => Direction::Bidirectional,
            Self::UpdateSoftEnum => Direction::Clientbound,
            Self::NetworkStackLatency => Direction::Bidirectional,
            Self::SpawnParticleEffect => Direction::Clientbound,
            Self::AvailableActorIdentifiers => Direction::Clientbound,
            Self::NetworkChunkPublisherUpdate => Direction::Clientbound,
            Self::BiomeDefinitionList => Direction::Clientbound,
            Self::LevelSoundEvent => Direction::Bidirectional,
            Self::LevelEventGeneric => Direction::Clientbound,
            Self::LecternUpdate => Direction::Bidirectional,
            Self::ClientCacheStatus => Direction::Bidirectional,
            Self::OnScreenTextureAnimation => Direction::Clientbound,
            Self::MapCreateLockedCopy => Direction::Bidirectional,
            Self::StructureTemplateDataRequest => Direction::Bidirectional,
            Self::StructureTemplateDataResponse => Direction::Bidirectional,
            Self::ClientCacheBlobStatus => Direction::Bidirectional,
            Self::ClientCacheMissResponse => Direction::Clientbound,
            Self::EducationSettings => Direction::Clientbound,
            Self::Emote => Direction::Bidirectional,
            Self::MultiplayerSettings => Direction::Bidirectional,
            Self::SettingsCommand => Direction::Bidirectional,
            Self::AnvilDamage => Direction::Bidirectional,
            Self::CompletedUsingItem => Direction::Clientbound,
            Self::NetworkSettings => Direction::Clientbound,
            Self::PlayerAuthInput => Direction::Bidirectional,
            Self::CreativeContent => Direction::Clientbound,
            Self::PlayerEnchantOptions => Direction::Clientbound,
            Self::ItemStackRequest => Direction::Bidirectional,
            Self::ItemStackResponse => Direction::Clientbound,
            Self::PlayerArmorDamage => Direction::Clientbound,
            Self::CodeBuilder => Direction::Clientbound,
            Self::UpdatePlayerGameType => Direction::Bidirectional,
            Self::EmoteList => Direction::Bidirectional,
            Self::PositionTrackingDBServerBroadcast => Direction::Clientbound,
            Self::PositionTrackingDBClientRequest => Direction::Bidirectional,
            Self::DebugInfo => Direction::Bidirectional,
            Self::PacketViolationWarning => Direction::Bidirectional,
            Self::MotionPredictionHints => Direction::Clientbound,
            Self::AnimateEntity => Direction::Clientbound,
            Self::CameraShake => Direction::Clientbound,
            Self::PlayerFog => Direction::Clientbound,
            Self::CorrectPlayerMovePrediction => Direction::Clientbound,
            Self::ItemRegistry => Direction::Clientbound,
            Self::ClientboundDebugRenderer => Direction::Clientbound,
            Self::SyncActorProperty => Direction::Clientbound,
            Self::AddVolumeEntity => Direction::Clientbound,
            Self::RemoveVolumeEntity => Direction::Clientbound,
            Self::SimulationType => Direction::Clientbound,
            Self::NpcDialogue => Direction::Clientbound,
            Self::EduUriResource => Direction::Clientbound,
            Self::CreatePhoto => Direction::Bidirectional,
            Self::UpdateSubChunkBlocks => Direction::Clientbound,
            Self::SubChunk => Direction::Clientbound,
            Self::SubChunkRequest => Direction::Bidirectional,
            Self::PlayerStartItemCooldown => Direction::Clientbound,
            Self::ScriptMessage => Direction::Bidirectional,
            Self::CodeBuilderSource => Direction::Bidirectional,
            Self::TickingAreasLoadStatus => Direction::Clientbound,
            Self::DimensionData => Direction::Clientbound,
            Self::AgentActionEvent => Direction::Clientbound,
            Self::ChangeMobProperty => Direction::Clientbound,
            Self::LessonProgress => Direction::Clientbound,
            Self::RequestAbility => Direction::Bidirectional,
            Self::RequestPermissions => Direction::Bidirectional,
            Self::ToastRequest => Direction::Clientbound,
            Self::UpdateAbilities => Direction::Clientbound,
            Self::UpdateAdventureSettings => Direction::Clientbound,
            Self::DeathInfo => Direction::Clientbound,
            Self::EditorNetwork => Direction::Bidirectional,
            Self::FeatureRegistry => Direction::Clientbound,
            Self::ServerStats => Direction::Clientbound,
            Self::RequestNetworkSettings => Direction::Bidirectional,
            Self::GameTestRequest => Direction::Clientbound,
            Self::GameTestResults => Direction::Bidirectional,
            Self::UpdateClientInputLocks => Direction::Clientbound,
            Self::CameraPresets => Direction::Clientbound,
            Self::UnlockedRecipes => Direction::Clientbound,
            Self::CameraInstruction => Direction::Clientbound,
            Self::TrimData => Direction::Clientbound,
            Self::OpenSign => Direction::Bidirectional,
            Self::AgentAnimation => Direction::Clientbound,
            Self::RefreshEntitlements => Direction::Clientbound,
            Self::PlayerToggleCrafterSlotRequest => Direction::Bidirectional,
            Self::SetPlayerInventoryOptions => Direction::Bidirectional,
            Self::SetHud => Direction::Clientbound,
            Self::AwardAchievement => Direction::Clientbound,
            Self::ClientboundCloseForm => Direction::Clientbound,
            Self::ServerboundLoadingScreen => Direction::Bidirectional,
            Self::JigsawStructureData => Direction::Clientbound,
            Self::CurrentStructureFeature => Direction::Clientbound,
            Self::ServerboundDiagnostics => Direction::Bidirectional,
            Self::CameraAimAssist => Direction::Clientbound,
            Self::ContainerRegistryCleanup => Direction::Clientbound,
            Self::MovementEffect => Direction::Clientbound,
            Self::CameraAimAssistPresets => Direction::Clientbound,
            Self::ClientCameraAimAssist => Direction::Bidirectional,
            Self::ClientMovementPredictionSync => Direction::Bidirectional,
            Self::UpdateClientOptions => Direction::Bidirectional,
            Self::PlayerVideoCapture => Direction::Clientbound,
            Self::PlayerUpdateEntityOverrides => Direction::Clientbound,
            Self::PlayerLocation => Direction::Clientbound,
            Self::ClientboundControlSchemeSet => Direction::Clientbound,
            Self::PrimitiveShapes => Direction::Clientbound,
            Self::ServerboundPackSettingChange => Direction::Bidirectional,
            Self::ClientboundDataStore => Direction::Clientbound,
            Self::GraphicsOverrideParameter => Direction::Clientbound,
            Self::ServerboundDataStore => Direction::Serverbound,
            Self::ClientboundDataDrivenUIShowScreen => Direction::Clientbound,
            Self::ClientboundDataDrivenUICloseScreen => Direction::Clientbound,
            Self::ClientboundDataDrivenUIReload => Direction::Clientbound,
            Self::ClientboundTextureShift => Direction::Clientbound,
            Self::VoxelShapes => Direction::Clientbound,
            Self::CameraSpline => Direction::Clientbound,
            Self::CameraAimAssistActorPriority => Direction::Clientbound,
            Self::ResourcePacksReadyForValidation => Direction::Serverbound,
            Self::LocatorBar => Direction::Clientbound,
            Self::PartyChanged => Direction::Serverbound,
            Self::ServerboundDataDrivenScreenClosed => Direction::Serverbound,
            Self::SyncWorldClocks => Direction::Clientbound,
            Self::ClientboundAttributeLayerSync => Direction::Clientbound,
            Self::ServerStoreInfo => Direction::Clientbound,
            Self::ServerPresenceInfo => Direction::Clientbound,
            Self::ClientboundUpdateSoundData => Direction::Clientbound,
            Self::SendPartyDestinationCookie => Direction::Clientbound,
            Self::PartyDestinationCookieResponse => Direction::Serverbound,
            Self::SetPlayerFurnaceOptions => Direction::Bidirectional,
            Self::RecordStarted => Direction::Clientbound,
        }
    }

    /// Every packet id in the canonical manifest, in id order.
    pub const ALL: &'static [PacketId] = &[
        Self::Login,
        Self::PlayStatus,
        Self::ServerToClientHandshake,
        Self::ClientToServerHandshake,
        Self::Disconnect,
        Self::ResourcePacksInfo,
        Self::ResourcePackStack,
        Self::ResourcePackClientResponse,
        Self::Text,
        Self::SetTime,
        Self::StartGame,
        Self::AddPlayer,
        Self::AddActor,
        Self::RemoveActor,
        Self::AddItemActor,
        Self::ServerPlayerPostMovePosition,
        Self::TakeItemActor,
        Self::MoveActorAbsolute,
        Self::MovePlayer,
        Self::UpdateBlock,
        Self::AddPainting,
        Self::LevelEvent,
        Self::BlockEvent,
        Self::ActorEvent,
        Self::MobEffect,
        Self::UpdateAttributes,
        Self::InventoryTransaction,
        Self::MobEquipment,
        Self::MobArmorEquipment,
        Self::Interact,
        Self::BlockPickRequest,
        Self::ActorPickRequest,
        Self::PlayerAction,
        Self::HurtArmor,
        Self::SetActorData,
        Self::SetActorMotion,
        Self::SetActorLink,
        Self::SetHealth,
        Self::SetSpawnPosition,
        Self::Animate,
        Self::Respawn,
        Self::ContainerOpen,
        Self::ContainerClose,
        Self::PlayerHotbar,
        Self::InventoryContent,
        Self::InventorySlot,
        Self::ContainerSetData,
        Self::CraftingData,
        Self::GuiDataPickItem,
        Self::BlockActorData,
        Self::LevelChunk,
        Self::SetCommandsEnabled,
        Self::SetDifficulty,
        Self::ChangeDimension,
        Self::SetPlayerGameType,
        Self::PlayerList,
        Self::SimpleEvent,
        Self::LegacyTelemetryEvent,
        Self::SpawnExperienceOrb,
        Self::ClientboundMapItemData,
        Self::MapInfoRequest,
        Self::RequestChunkRadius,
        Self::ChunkRadiusUpdated,
        Self::GameRulesChanged,
        Self::Camera,
        Self::BossEvent,
        Self::ShowCredits,
        Self::AvailableCommands,
        Self::CommandRequest,
        Self::CommandBlockUpdate,
        Self::CommandOutput,
        Self::UpdateTrade,
        Self::UpdateEquip,
        Self::ResourcePackDataInfo,
        Self::ResourcePackChunkData,
        Self::ResourcePackChunkRequest,
        Self::Transfer,
        Self::PlaySound,
        Self::StopSound,
        Self::SetTitle,
        Self::AddBehaviorTree,
        Self::StructureBlockUpdate,
        Self::ShowStoreOffer,
        Self::PurchaseReceipt,
        Self::PlayerSkin,
        Self::SubClientLogin,
        Self::AutomationClientConnect,
        Self::SetLastHurtBy,
        Self::BookEdit,
        Self::NpcRequest,
        Self::PhotoTransfer,
        Self::ModalFormRequest,
        Self::ModalFormResponse,
        Self::ServerSettingsRequest,
        Self::ServerSettingsResponse,
        Self::ShowProfile,
        Self::SetDefaultGameType,
        Self::RemoveObjective,
        Self::SetDisplayObjective,
        Self::SetScore,
        Self::LabTable,
        Self::UpdateBlockSynced,
        Self::MoveActorDelta,
        Self::SetScoreboardIdentity,
        Self::SetLocalPlayerAsInitialized,
        Self::UpdateSoftEnum,
        Self::NetworkStackLatency,
        Self::SpawnParticleEffect,
        Self::AvailableActorIdentifiers,
        Self::NetworkChunkPublisherUpdate,
        Self::BiomeDefinitionList,
        Self::LevelSoundEvent,
        Self::LevelEventGeneric,
        Self::LecternUpdate,
        Self::ClientCacheStatus,
        Self::OnScreenTextureAnimation,
        Self::MapCreateLockedCopy,
        Self::StructureTemplateDataRequest,
        Self::StructureTemplateDataResponse,
        Self::ClientCacheBlobStatus,
        Self::ClientCacheMissResponse,
        Self::EducationSettings,
        Self::Emote,
        Self::MultiplayerSettings,
        Self::SettingsCommand,
        Self::AnvilDamage,
        Self::CompletedUsingItem,
        Self::NetworkSettings,
        Self::PlayerAuthInput,
        Self::CreativeContent,
        Self::PlayerEnchantOptions,
        Self::ItemStackRequest,
        Self::ItemStackResponse,
        Self::PlayerArmorDamage,
        Self::CodeBuilder,
        Self::UpdatePlayerGameType,
        Self::EmoteList,
        Self::PositionTrackingDBServerBroadcast,
        Self::PositionTrackingDBClientRequest,
        Self::DebugInfo,
        Self::PacketViolationWarning,
        Self::MotionPredictionHints,
        Self::AnimateEntity,
        Self::CameraShake,
        Self::PlayerFog,
        Self::CorrectPlayerMovePrediction,
        Self::ItemRegistry,
        Self::ClientboundDebugRenderer,
        Self::SyncActorProperty,
        Self::AddVolumeEntity,
        Self::RemoveVolumeEntity,
        Self::SimulationType,
        Self::NpcDialogue,
        Self::EduUriResource,
        Self::CreatePhoto,
        Self::UpdateSubChunkBlocks,
        Self::SubChunk,
        Self::SubChunkRequest,
        Self::PlayerStartItemCooldown,
        Self::ScriptMessage,
        Self::CodeBuilderSource,
        Self::TickingAreasLoadStatus,
        Self::DimensionData,
        Self::AgentActionEvent,
        Self::ChangeMobProperty,
        Self::LessonProgress,
        Self::RequestAbility,
        Self::RequestPermissions,
        Self::ToastRequest,
        Self::UpdateAbilities,
        Self::UpdateAdventureSettings,
        Self::DeathInfo,
        Self::EditorNetwork,
        Self::FeatureRegistry,
        Self::ServerStats,
        Self::RequestNetworkSettings,
        Self::GameTestRequest,
        Self::GameTestResults,
        Self::UpdateClientInputLocks,
        Self::CameraPresets,
        Self::UnlockedRecipes,
        Self::CameraInstruction,
        Self::TrimData,
        Self::OpenSign,
        Self::AgentAnimation,
        Self::RefreshEntitlements,
        Self::PlayerToggleCrafterSlotRequest,
        Self::SetPlayerInventoryOptions,
        Self::SetHud,
        Self::AwardAchievement,
        Self::ClientboundCloseForm,
        Self::ServerboundLoadingScreen,
        Self::JigsawStructureData,
        Self::CurrentStructureFeature,
        Self::ServerboundDiagnostics,
        Self::CameraAimAssist,
        Self::ContainerRegistryCleanup,
        Self::MovementEffect,
        Self::CameraAimAssistPresets,
        Self::ClientCameraAimAssist,
        Self::ClientMovementPredictionSync,
        Self::UpdateClientOptions,
        Self::PlayerVideoCapture,
        Self::PlayerUpdateEntityOverrides,
        Self::PlayerLocation,
        Self::ClientboundControlSchemeSet,
        Self::PrimitiveShapes,
        Self::ServerboundPackSettingChange,
        Self::ClientboundDataStore,
        Self::GraphicsOverrideParameter,
        Self::ServerboundDataStore,
        Self::ClientboundDataDrivenUIShowScreen,
        Self::ClientboundDataDrivenUICloseScreen,
        Self::ClientboundDataDrivenUIReload,
        Self::ClientboundTextureShift,
        Self::VoxelShapes,
        Self::CameraSpline,
        Self::CameraAimAssistActorPriority,
        Self::ResourcePacksReadyForValidation,
        Self::LocatorBar,
        Self::PartyChanged,
        Self::ServerboundDataDrivenScreenClosed,
        Self::SyncWorldClocks,
        Self::ClientboundAttributeLayerSync,
        Self::ServerStoreInfo,
        Self::ServerPresenceInfo,
        Self::ClientboundUpdateSoundData,
        Self::SendPartyDestinationCookie,
        Self::PartyDestinationCookieResponse,
        Self::SetPlayerFurnaceOptions,
        Self::RecordStarted,
    ];
}

impl Packet {
    pub const fn id(&self) -> PacketId {
        match self {
            Self::Login(..) => PacketId::Login,
            Self::PlayStatus(..) => PacketId::PlayStatus,
            Self::ServerToClientHandshake(..) => PacketId::ServerToClientHandshake,
            Self::ClientToServerHandshake(..) => PacketId::ClientToServerHandshake,
            Self::Disconnect(..) => PacketId::Disconnect,
            Self::ResourcePacksInfo(..) => PacketId::ResourcePacksInfo,
            Self::ResourcePackStack(..) => PacketId::ResourcePackStack,
            Self::ResourcePackClientResponse(..) => PacketId::ResourcePackClientResponse,
            Self::Text(..) => PacketId::Text,
            Self::SetTime(..) => PacketId::SetTime,
            Self::StartGame(..) => PacketId::StartGame,
            Self::AddPlayer(..) => PacketId::AddPlayer,
            Self::AddActor(..) => PacketId::AddActor,
            Self::RemoveActor(..) => PacketId::RemoveActor,
            Self::AddItemActor(..) => PacketId::AddItemActor,
            Self::ServerPlayerPostMovePosition(..) => PacketId::ServerPlayerPostMovePosition,
            Self::TakeItemActor(..) => PacketId::TakeItemActor,
            Self::MoveActorAbsolute(..) => PacketId::MoveActorAbsolute,
            Self::MovePlayer(..) => PacketId::MovePlayer,
            Self::UpdateBlock(..) => PacketId::UpdateBlock,
            Self::AddPainting(..) => PacketId::AddPainting,
            Self::LevelEvent(..) => PacketId::LevelEvent,
            Self::BlockEvent(..) => PacketId::BlockEvent,
            Self::ActorEvent(..) => PacketId::ActorEvent,
            Self::MobEffect(..) => PacketId::MobEffect,
            Self::UpdateAttributes(..) => PacketId::UpdateAttributes,
            Self::InventoryTransaction(..) => PacketId::InventoryTransaction,
            Self::MobEquipment(..) => PacketId::MobEquipment,
            Self::MobArmorEquipment(..) => PacketId::MobArmorEquipment,
            Self::Interact(..) => PacketId::Interact,
            Self::BlockPickRequest(..) => PacketId::BlockPickRequest,
            Self::ActorPickRequest(..) => PacketId::ActorPickRequest,
            Self::PlayerAction(..) => PacketId::PlayerAction,
            Self::HurtArmor(..) => PacketId::HurtArmor,
            Self::SetActorData(..) => PacketId::SetActorData,
            Self::SetActorMotion(..) => PacketId::SetActorMotion,
            Self::SetActorLink(..) => PacketId::SetActorLink,
            Self::SetHealth(..) => PacketId::SetHealth,
            Self::SetSpawnPosition(..) => PacketId::SetSpawnPosition,
            Self::Animate(..) => PacketId::Animate,
            Self::Respawn(..) => PacketId::Respawn,
            Self::ContainerOpen(..) => PacketId::ContainerOpen,
            Self::ContainerClose(..) => PacketId::ContainerClose,
            Self::PlayerHotbar(..) => PacketId::PlayerHotbar,
            Self::InventoryContent(..) => PacketId::InventoryContent,
            Self::InventorySlot(..) => PacketId::InventorySlot,
            Self::ContainerSetData(..) => PacketId::ContainerSetData,
            Self::CraftingData(..) => PacketId::CraftingData,
            Self::GuiDataPickItem(..) => PacketId::GuiDataPickItem,
            Self::BlockActorData(..) => PacketId::BlockActorData,
            Self::LevelChunk(..) => PacketId::LevelChunk,
            Self::SetCommandsEnabled(..) => PacketId::SetCommandsEnabled,
            Self::SetDifficulty(..) => PacketId::SetDifficulty,
            Self::ChangeDimension(..) => PacketId::ChangeDimension,
            Self::SetPlayerGameType(..) => PacketId::SetPlayerGameType,
            Self::PlayerList(..) => PacketId::PlayerList,
            Self::SimpleEvent(..) => PacketId::SimpleEvent,
            Self::LegacyTelemetryEvent(..) => PacketId::LegacyTelemetryEvent,
            Self::SpawnExperienceOrb(..) => PacketId::SpawnExperienceOrb,
            Self::ClientboundMapItemData(..) => PacketId::ClientboundMapItemData,
            Self::MapInfoRequest(..) => PacketId::MapInfoRequest,
            Self::RequestChunkRadius(..) => PacketId::RequestChunkRadius,
            Self::ChunkRadiusUpdated(..) => PacketId::ChunkRadiusUpdated,
            Self::GameRulesChanged(..) => PacketId::GameRulesChanged,
            Self::Camera(..) => PacketId::Camera,
            Self::BossEvent(..) => PacketId::BossEvent,
            Self::ShowCredits(..) => PacketId::ShowCredits,
            Self::AvailableCommands(..) => PacketId::AvailableCommands,
            Self::CommandRequest(..) => PacketId::CommandRequest,
            Self::CommandBlockUpdate(..) => PacketId::CommandBlockUpdate,
            Self::CommandOutput(..) => PacketId::CommandOutput,
            Self::UpdateTrade(..) => PacketId::UpdateTrade,
            Self::UpdateEquip(..) => PacketId::UpdateEquip,
            Self::ResourcePackDataInfo(..) => PacketId::ResourcePackDataInfo,
            Self::ResourcePackChunkData(..) => PacketId::ResourcePackChunkData,
            Self::ResourcePackChunkRequest(..) => PacketId::ResourcePackChunkRequest,
            Self::Transfer(..) => PacketId::Transfer,
            Self::PlaySound(..) => PacketId::PlaySound,
            Self::StopSound(..) => PacketId::StopSound,
            Self::SetTitle(..) => PacketId::SetTitle,
            Self::AddBehaviorTree(..) => PacketId::AddBehaviorTree,
            Self::StructureBlockUpdate(..) => PacketId::StructureBlockUpdate,
            Self::ShowStoreOffer(..) => PacketId::ShowStoreOffer,
            Self::PurchaseReceipt(..) => PacketId::PurchaseReceipt,
            Self::PlayerSkin(..) => PacketId::PlayerSkin,
            Self::SubClientLogin(..) => PacketId::SubClientLogin,
            Self::AutomationClientConnect(..) => PacketId::AutomationClientConnect,
            Self::SetLastHurtBy(..) => PacketId::SetLastHurtBy,
            Self::BookEdit(..) => PacketId::BookEdit,
            Self::NpcRequest(..) => PacketId::NpcRequest,
            Self::PhotoTransfer(..) => PacketId::PhotoTransfer,
            Self::ModalFormRequest(..) => PacketId::ModalFormRequest,
            Self::ModalFormResponse(..) => PacketId::ModalFormResponse,
            Self::ServerSettingsRequest(..) => PacketId::ServerSettingsRequest,
            Self::ServerSettingsResponse(..) => PacketId::ServerSettingsResponse,
            Self::ShowProfile(..) => PacketId::ShowProfile,
            Self::SetDefaultGameType(..) => PacketId::SetDefaultGameType,
            Self::RemoveObjective(..) => PacketId::RemoveObjective,
            Self::SetDisplayObjective(..) => PacketId::SetDisplayObjective,
            Self::SetScore(..) => PacketId::SetScore,
            Self::LabTable(..) => PacketId::LabTable,
            Self::UpdateBlockSynced(..) => PacketId::UpdateBlockSynced,
            Self::MoveActorDelta(..) => PacketId::MoveActorDelta,
            Self::SetScoreboardIdentity(..) => PacketId::SetScoreboardIdentity,
            Self::SetLocalPlayerAsInitialized(..) => PacketId::SetLocalPlayerAsInitialized,
            Self::UpdateSoftEnum(..) => PacketId::UpdateSoftEnum,
            Self::NetworkStackLatency(..) => PacketId::NetworkStackLatency,
            Self::SpawnParticleEffect(..) => PacketId::SpawnParticleEffect,
            Self::AvailableActorIdentifiers(..) => PacketId::AvailableActorIdentifiers,
            Self::NetworkChunkPublisherUpdate(..) => PacketId::NetworkChunkPublisherUpdate,
            Self::BiomeDefinitionList(..) => PacketId::BiomeDefinitionList,
            Self::LevelSoundEvent(..) => PacketId::LevelSoundEvent,
            Self::LevelEventGeneric(..) => PacketId::LevelEventGeneric,
            Self::LecternUpdate(..) => PacketId::LecternUpdate,
            Self::ClientCacheStatus(..) => PacketId::ClientCacheStatus,
            Self::OnScreenTextureAnimation(..) => PacketId::OnScreenTextureAnimation,
            Self::MapCreateLockedCopy(..) => PacketId::MapCreateLockedCopy,
            Self::StructureTemplateDataRequest(..) => PacketId::StructureTemplateDataRequest,
            Self::StructureTemplateDataResponse(..) => PacketId::StructureTemplateDataResponse,
            Self::ClientCacheBlobStatus(..) => PacketId::ClientCacheBlobStatus,
            Self::ClientCacheMissResponse(..) => PacketId::ClientCacheMissResponse,
            Self::EducationSettings(..) => PacketId::EducationSettings,
            Self::Emote(..) => PacketId::Emote,
            Self::MultiplayerSettings(..) => PacketId::MultiplayerSettings,
            Self::SettingsCommand(..) => PacketId::SettingsCommand,
            Self::AnvilDamage(..) => PacketId::AnvilDamage,
            Self::CompletedUsingItem(..) => PacketId::CompletedUsingItem,
            Self::NetworkSettings(..) => PacketId::NetworkSettings,
            Self::PlayerAuthInput(..) => PacketId::PlayerAuthInput,
            Self::CreativeContent(..) => PacketId::CreativeContent,
            Self::PlayerEnchantOptions(..) => PacketId::PlayerEnchantOptions,
            Self::ItemStackRequest(..) => PacketId::ItemStackRequest,
            Self::ItemStackResponse(..) => PacketId::ItemStackResponse,
            Self::PlayerArmorDamage(..) => PacketId::PlayerArmorDamage,
            Self::CodeBuilder(..) => PacketId::CodeBuilder,
            Self::UpdatePlayerGameType(..) => PacketId::UpdatePlayerGameType,
            Self::EmoteList(..) => PacketId::EmoteList,
            Self::PositionTrackingDBServerBroadcast(..) => {
                PacketId::PositionTrackingDBServerBroadcast
            }
            Self::PositionTrackingDBClientRequest(..) => PacketId::PositionTrackingDBClientRequest,
            Self::DebugInfo(..) => PacketId::DebugInfo,
            Self::PacketViolationWarning(..) => PacketId::PacketViolationWarning,
            Self::MotionPredictionHints(..) => PacketId::MotionPredictionHints,
            Self::AnimateEntity(..) => PacketId::AnimateEntity,
            Self::CameraShake(..) => PacketId::CameraShake,
            Self::PlayerFog(..) => PacketId::PlayerFog,
            Self::CorrectPlayerMovePrediction(..) => PacketId::CorrectPlayerMovePrediction,
            Self::ItemRegistry(..) => PacketId::ItemRegistry,
            Self::ClientboundDebugRenderer(..) => PacketId::ClientboundDebugRenderer,
            Self::SyncActorProperty(..) => PacketId::SyncActorProperty,
            Self::AddVolumeEntity(..) => PacketId::AddVolumeEntity,
            Self::RemoveVolumeEntity(..) => PacketId::RemoveVolumeEntity,
            Self::SimulationType(..) => PacketId::SimulationType,
            Self::NpcDialogue(..) => PacketId::NpcDialogue,
            Self::EduUriResource(..) => PacketId::EduUriResource,
            Self::CreatePhoto(..) => PacketId::CreatePhoto,
            Self::UpdateSubChunkBlocks(..) => PacketId::UpdateSubChunkBlocks,
            Self::SubChunk(..) => PacketId::SubChunk,
            Self::SubChunkRequest(..) => PacketId::SubChunkRequest,
            Self::PlayerStartItemCooldown(..) => PacketId::PlayerStartItemCooldown,
            Self::ScriptMessage(..) => PacketId::ScriptMessage,
            Self::CodeBuilderSource(..) => PacketId::CodeBuilderSource,
            Self::TickingAreasLoadStatus(..) => PacketId::TickingAreasLoadStatus,
            Self::DimensionData(..) => PacketId::DimensionData,
            Self::AgentActionEvent(..) => PacketId::AgentActionEvent,
            Self::ChangeMobProperty(..) => PacketId::ChangeMobProperty,
            Self::LessonProgress(..) => PacketId::LessonProgress,
            Self::RequestAbility(..) => PacketId::RequestAbility,
            Self::RequestPermissions(..) => PacketId::RequestPermissions,
            Self::ToastRequest(..) => PacketId::ToastRequest,
            Self::UpdateAbilities(..) => PacketId::UpdateAbilities,
            Self::UpdateAdventureSettings(..) => PacketId::UpdateAdventureSettings,
            Self::DeathInfo(..) => PacketId::DeathInfo,
            Self::EditorNetwork(..) => PacketId::EditorNetwork,
            Self::FeatureRegistry(..) => PacketId::FeatureRegistry,
            Self::ServerStats(..) => PacketId::ServerStats,
            Self::RequestNetworkSettings(..) => PacketId::RequestNetworkSettings,
            Self::GameTestRequest(..) => PacketId::GameTestRequest,
            Self::GameTestResults(..) => PacketId::GameTestResults,
            Self::UpdateClientInputLocks(..) => PacketId::UpdateClientInputLocks,
            Self::CameraPresets(..) => PacketId::CameraPresets,
            Self::UnlockedRecipes(..) => PacketId::UnlockedRecipes,
            Self::CameraInstruction(..) => PacketId::CameraInstruction,
            Self::TrimData(..) => PacketId::TrimData,
            Self::OpenSign(..) => PacketId::OpenSign,
            Self::AgentAnimation(..) => PacketId::AgentAnimation,
            Self::RefreshEntitlements(..) => PacketId::RefreshEntitlements,
            Self::PlayerToggleCrafterSlotRequest(..) => PacketId::PlayerToggleCrafterSlotRequest,
            Self::SetPlayerInventoryOptions(..) => PacketId::SetPlayerInventoryOptions,
            Self::SetHud(..) => PacketId::SetHud,
            Self::AwardAchievement(..) => PacketId::AwardAchievement,
            Self::ClientboundCloseForm(..) => PacketId::ClientboundCloseForm,
            Self::ServerboundLoadingScreen(..) => PacketId::ServerboundLoadingScreen,
            Self::JigsawStructureData(..) => PacketId::JigsawStructureData,
            Self::CurrentStructureFeature(..) => PacketId::CurrentStructureFeature,
            Self::ServerboundDiagnostics(..) => PacketId::ServerboundDiagnostics,
            Self::CameraAimAssist(..) => PacketId::CameraAimAssist,
            Self::ContainerRegistryCleanup(..) => PacketId::ContainerRegistryCleanup,
            Self::MovementEffect(..) => PacketId::MovementEffect,
            Self::CameraAimAssistPresets(..) => PacketId::CameraAimAssistPresets,
            Self::ClientCameraAimAssist(..) => PacketId::ClientCameraAimAssist,
            Self::ClientMovementPredictionSync(..) => PacketId::ClientMovementPredictionSync,
            Self::UpdateClientOptions(..) => PacketId::UpdateClientOptions,
            Self::PlayerVideoCapture(..) => PacketId::PlayerVideoCapture,
            Self::PlayerUpdateEntityOverrides(..) => PacketId::PlayerUpdateEntityOverrides,
            Self::PlayerLocation(..) => PacketId::PlayerLocation,
            Self::ClientboundControlSchemeSet(..) => PacketId::ClientboundControlSchemeSet,
            Self::PrimitiveShapes(..) => PacketId::PrimitiveShapes,
            Self::ServerboundPackSettingChange(..) => PacketId::ServerboundPackSettingChange,
            Self::ClientboundDataStore(..) => PacketId::ClientboundDataStore,
            Self::GraphicsOverrideParameter(..) => PacketId::GraphicsOverrideParameter,
            Self::ServerboundDataStore(..) => PacketId::ServerboundDataStore,
            Self::ClientboundDataDrivenUIShowScreen(..) => {
                PacketId::ClientboundDataDrivenUIShowScreen
            }
            Self::ClientboundDataDrivenUICloseScreen(..) => {
                PacketId::ClientboundDataDrivenUICloseScreen
            }
            Self::ClientboundDataDrivenUIReload(..) => PacketId::ClientboundDataDrivenUIReload,
            Self::ClientboundTextureShift(..) => PacketId::ClientboundTextureShift,
            Self::VoxelShapes(..) => PacketId::VoxelShapes,
            Self::CameraSpline(..) => PacketId::CameraSpline,
            Self::CameraAimAssistActorPriority(..) => PacketId::CameraAimAssistActorPriority,
            Self::ResourcePacksReadyForValidation(..) => PacketId::ResourcePacksReadyForValidation,
            Self::LocatorBar(..) => PacketId::LocatorBar,
            Self::PartyChanged(..) => PacketId::PartyChanged,
            Self::ServerboundDataDrivenScreenClosed(..) => {
                PacketId::ServerboundDataDrivenScreenClosed
            }
            Self::SyncWorldClocks(..) => PacketId::SyncWorldClocks,
            Self::ClientboundAttributeLayerSync(..) => PacketId::ClientboundAttributeLayerSync,
            Self::ServerStoreInfo(..) => PacketId::ServerStoreInfo,
            Self::ServerPresenceInfo(..) => PacketId::ServerPresenceInfo,
            Self::ClientboundUpdateSoundData(..) => PacketId::ClientboundUpdateSoundData,
            Self::SendPartyDestinationCookie(..) => PacketId::SendPartyDestinationCookie,
            Self::PartyDestinationCookieResponse(..) => PacketId::PartyDestinationCookieResponse,
            Self::SetPlayerFurnaceOptions(..) => PacketId::SetPlayerFurnaceOptions,
            Self::RecordStarted(..) => PacketId::RecordStarted,
        }
    }

    pub fn encode(&self, writer: &mut wire::Writer<'_>) -> wire::EncodeResult<()> {
        match self {
            Self::Login(value) => wire::Encode::encode(value, writer),
            Self::PlayStatus(value) => wire::Encode::encode(value, writer),
            Self::ServerToClientHandshake(value) => wire::Encode::encode(value, writer),
            Self::ClientToServerHandshake(value) => wire::Encode::encode(value, writer),
            Self::Disconnect(value) => wire::Encode::encode(value, writer),
            Self::ResourcePacksInfo(value) => wire::Encode::encode(value, writer),
            Self::ResourcePackStack(value) => wire::Encode::encode(value, writer),
            Self::ResourcePackClientResponse(value) => wire::Encode::encode(value, writer),
            Self::Text(value) => wire::Encode::encode(value, writer),
            Self::SetTime(value) => wire::Encode::encode(value, writer),
            Self::StartGame(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::AddPlayer(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::AddActor(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::RemoveActor(value) => wire::Encode::encode(value, writer),
            Self::AddItemActor(value) => wire::Encode::encode(value, writer),
            Self::ServerPlayerPostMovePosition(value) => wire::Encode::encode(value, writer),
            Self::TakeItemActor(value) => wire::Encode::encode(value, writer),
            Self::MoveActorAbsolute(value) => wire::Encode::encode(value, writer),
            Self::MovePlayer(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::UpdateBlock(value) => wire::Encode::encode(value, writer),
            Self::AddPainting(value) => wire::Encode::encode(value, writer),
            Self::LevelEvent(value) => wire::Encode::encode(value, writer),
            Self::BlockEvent(value) => wire::Encode::encode(value, writer),
            Self::ActorEvent(value) => wire::Encode::encode(value, writer),
            Self::MobEffect(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::UpdateAttributes(value) => wire::Encode::encode(value, writer),
            Self::InventoryTransaction(value) => wire::Encode::encode(value, writer),
            Self::MobEquipment(value) => wire::Encode::encode(value, writer),
            Self::MobArmorEquipment(value) => wire::Encode::encode(value, writer),
            Self::Interact(value) => wire::Encode::encode(value, writer),
            Self::BlockPickRequest(value) => wire::Encode::encode(value, writer),
            Self::ActorPickRequest(value) => wire::Encode::encode(value, writer),
            Self::PlayerAction(value) => wire::Encode::encode(value, writer),
            Self::HurtArmor(value) => wire::Encode::encode(value, writer),
            Self::SetActorData(value) => wire::Encode::encode(value, writer),
            Self::SetActorMotion(value) => wire::Encode::encode(value, writer),
            Self::SetActorLink(value) => wire::Encode::encode(value, writer),
            Self::SetHealth(value) => wire::Encode::encode(value, writer),
            Self::SetSpawnPosition(value) => wire::Encode::encode(value, writer),
            Self::Animate(value) => wire::Encode::encode(value, writer),
            Self::Respawn(value) => wire::Encode::encode(value, writer),
            Self::ContainerOpen(value) => wire::Encode::encode(value, writer),
            Self::ContainerClose(value) => wire::Encode::encode(value, writer),
            Self::PlayerHotbar(value) => wire::Encode::encode(value, writer),
            Self::InventoryContent(value) => wire::Encode::encode(value, writer),
            Self::InventorySlot(value) => wire::Encode::encode(value, writer),
            Self::ContainerSetData(value) => wire::Encode::encode(value, writer),
            Self::CraftingData(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::GuiDataPickItem(value) => wire::Encode::encode(value, writer),
            Self::BlockActorData(value) => wire::Encode::encode(value, writer),
            Self::LevelChunk(value) => wire::Encode::encode(value, writer),
            Self::SetCommandsEnabled(value) => wire::Encode::encode(value, writer),
            Self::SetDifficulty(value) => wire::Encode::encode(value, writer),
            Self::ChangeDimension(value) => wire::Encode::encode(value, writer),
            Self::SetPlayerGameType(value) => wire::Encode::encode(value, writer),
            Self::PlayerList(value) => wire::Encode::encode(value, writer),
            Self::SimpleEvent(value) => wire::Encode::encode(value, writer),
            Self::LegacyTelemetryEvent(value) => wire::Encode::encode(value, writer),
            Self::SpawnExperienceOrb(value) => wire::Encode::encode(value, writer),
            Self::ClientboundMapItemData(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::MapInfoRequest(value) => wire::Encode::encode(value, writer),
            Self::RequestChunkRadius(value) => wire::Encode::encode(value, writer),
            Self::ChunkRadiusUpdated(value) => wire::Encode::encode(value, writer),
            Self::GameRulesChanged(value) => wire::Encode::encode(value, writer),
            Self::Camera(value) => wire::Encode::encode(value, writer),
            Self::BossEvent(value) => wire::Encode::encode(value, writer),
            Self::ShowCredits(value) => wire::Encode::encode(value, writer),
            Self::AvailableCommands(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::CommandRequest(value) => wire::Encode::encode(value, writer),
            Self::CommandBlockUpdate(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::CommandOutput(value) => wire::Encode::encode(value, writer),
            Self::UpdateTrade(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::UpdateEquip(value) => wire::Encode::encode(value, writer),
            Self::ResourcePackDataInfo(value) => wire::Encode::encode(value, writer),
            Self::ResourcePackChunkData(value) => wire::Encode::encode(value, writer),
            Self::ResourcePackChunkRequest(value) => wire::Encode::encode(value, writer),
            Self::Transfer(value) => wire::Encode::encode(value, writer),
            Self::PlaySound(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::StopSound(value) => wire::Encode::encode(value, writer),
            Self::SetTitle(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::AddBehaviorTree(value) => wire::Encode::encode(value, writer),
            Self::StructureBlockUpdate(value) => wire::Encode::encode(value, writer),
            Self::ShowStoreOffer(value) => wire::Encode::encode(value, writer),
            Self::PurchaseReceipt(value) => wire::Encode::encode(value, writer),
            Self::PlayerSkin(value) => wire::Encode::encode(value, writer),
            Self::SubClientLogin(value) => wire::Encode::encode(value, writer),
            Self::AutomationClientConnect(value) => wire::Encode::encode(value, writer),
            Self::SetLastHurtBy(value) => wire::Encode::encode(value, writer),
            Self::BookEdit(value) => wire::Encode::encode(value, writer),
            Self::NpcRequest(value) => wire::Encode::encode(value, writer),
            Self::PhotoTransfer(value) => wire::Encode::encode(value, writer),
            Self::ModalFormRequest(value) => wire::Encode::encode(value, writer),
            Self::ModalFormResponse(value) => wire::Encode::encode(value, writer),
            Self::ServerSettingsRequest(value) => wire::Encode::encode(value, writer),
            Self::ServerSettingsResponse(value) => wire::Encode::encode(value, writer),
            Self::ShowProfile(value) => wire::Encode::encode(value, writer),
            Self::SetDefaultGameType(value) => wire::Encode::encode(value, writer),
            Self::RemoveObjective(value) => wire::Encode::encode(value, writer),
            Self::SetDisplayObjective(value) => wire::Encode::encode(value, writer),
            Self::SetScore(value) => wire::Encode::encode(value, writer),
            Self::LabTable(value) => wire::Encode::encode(value, writer),
            Self::UpdateBlockSynced(value) => wire::Encode::encode(value, writer),
            Self::MoveActorDelta(value) => wire::Encode::encode(value, writer),
            Self::SetScoreboardIdentity(value) => wire::Encode::encode(value, writer),
            Self::SetLocalPlayerAsInitialized(value) => wire::Encode::encode(value, writer),
            Self::UpdateSoftEnum(value) => wire::Encode::encode(value, writer),
            Self::NetworkStackLatency(value) => wire::Encode::encode(value, writer),
            Self::SpawnParticleEffect(value) => wire::Encode::encode(value, writer),
            Self::AvailableActorIdentifiers(value) => wire::Encode::encode(value, writer),
            Self::NetworkChunkPublisherUpdate(value) => wire::Encode::encode(value, writer),
            Self::BiomeDefinitionList(value) => wire::Encode::encode(value, writer),
            Self::LevelSoundEvent(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::LevelEventGeneric(value) => wire::Encode::encode(value, writer),
            Self::LecternUpdate(value) => wire::Encode::encode(value, writer),
            Self::ClientCacheStatus(value) => wire::Encode::encode(value, writer),
            Self::OnScreenTextureAnimation(value) => wire::Encode::encode(value, writer),
            Self::MapCreateLockedCopy(value) => wire::Encode::encode(value, writer),
            Self::StructureTemplateDataRequest(value) => wire::Encode::encode(value, writer),
            Self::StructureTemplateDataResponse(value) => wire::Encode::encode(value, writer),
            Self::ClientCacheBlobStatus(value) => wire::Encode::encode(value, writer),
            Self::ClientCacheMissResponse(value) => wire::Encode::encode(value, writer),
            Self::EducationSettings(value) => wire::Encode::encode(value, writer),
            Self::Emote(value) => wire::Encode::encode(value, writer),
            Self::MultiplayerSettings(value) => wire::Encode::encode(value, writer),
            Self::SettingsCommand(value) => wire::Encode::encode(value, writer),
            Self::AnvilDamage(value) => wire::Encode::encode(value, writer),
            Self::CompletedUsingItem(value) => wire::Encode::encode(value, writer),
            Self::NetworkSettings(value) => wire::Encode::encode(value, writer),
            Self::PlayerAuthInput(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::CreativeContent(value) => wire::Encode::encode(value, writer),
            Self::PlayerEnchantOptions(value) => wire::Encode::encode(value, writer),
            Self::ItemStackRequest(value) => wire::Encode::encode(value, writer),
            Self::ItemStackResponse(value) => wire::Encode::encode(value, writer),
            Self::PlayerArmorDamage(value) => wire::Encode::encode(value, writer),
            Self::CodeBuilder(value) => wire::Encode::encode(value, writer),
            Self::UpdatePlayerGameType(value) => wire::Encode::encode(value, writer),
            Self::EmoteList(value) => wire::Encode::encode(value, writer),
            Self::PositionTrackingDBServerBroadcast(value) => wire::Encode::encode(value, writer),
            Self::PositionTrackingDBClientRequest(value) => wire::Encode::encode(value, writer),
            Self::DebugInfo(value) => wire::Encode::encode(value, writer),
            Self::PacketViolationWarning(value) => wire::Encode::encode(value, writer),
            Self::MotionPredictionHints(value) => wire::Encode::encode(value, writer),
            Self::AnimateEntity(value) => wire::Encode::encode(value, writer),
            Self::CameraShake(value) => wire::Encode::encode(value, writer),
            Self::PlayerFog(value) => wire::Encode::encode(value, writer),
            Self::CorrectPlayerMovePrediction(value) => wire::Encode::encode(value, writer),
            Self::ItemRegistry(value) => wire::Encode::encode(value, writer),
            Self::ClientboundDebugRenderer(value) => wire::Encode::encode(value, writer),
            Self::SyncActorProperty(value) => wire::Encode::encode(value, writer),
            Self::AddVolumeEntity(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::RemoveVolumeEntity(value) => wire::Encode::encode(value, writer),
            Self::SimulationType(value) => wire::Encode::encode(value, writer),
            Self::NpcDialogue(value) => wire::Encode::encode(value, writer),
            Self::EduUriResource(value) => wire::Encode::encode(value, writer),
            Self::CreatePhoto(value) => wire::Encode::encode(value, writer),
            Self::UpdateSubChunkBlocks(value) => wire::Encode::encode(value, writer),
            Self::SubChunk(value) => wire::Encode::encode(value, writer),
            Self::SubChunkRequest(value) => wire::Encode::encode(value, writer),
            Self::PlayerStartItemCooldown(value) => wire::Encode::encode(value, writer),
            Self::ScriptMessage(value) => wire::Encode::encode(value, writer),
            Self::CodeBuilderSource(value) => wire::Encode::encode(value, writer),
            Self::TickingAreasLoadStatus(value) => wire::Encode::encode(value, writer),
            Self::DimensionData(value) => wire::Encode::encode(value, writer),
            Self::AgentActionEvent(value) => wire::Encode::encode(value, writer),
            Self::ChangeMobProperty(value) => wire::Encode::encode(value, writer),
            Self::LessonProgress(value) => wire::Encode::encode(value, writer),
            Self::RequestAbility(value) => wire::Encode::encode(value, writer),
            Self::RequestPermissions(value) => wire::Encode::encode(value, writer),
            Self::ToastRequest(value) => wire::Encode::encode(value, writer),
            Self::UpdateAbilities(value) => wire::Encode::encode(value, writer),
            Self::UpdateAdventureSettings(value) => wire::Encode::encode(value, writer),
            Self::DeathInfo(value) => wire::Encode::encode(value, writer),
            Self::EditorNetwork(value) => wire::Encode::encode(value, writer),
            Self::FeatureRegistry(value) => wire::Encode::encode(value, writer),
            Self::ServerStats(value) => wire::Encode::encode(value, writer),
            Self::RequestNetworkSettings(value) => wire::Encode::encode(value, writer),
            Self::GameTestRequest(value) => wire::Encode::encode(value, writer),
            Self::GameTestResults(value) => wire::Encode::encode(value, writer),
            Self::UpdateClientInputLocks(value) => wire::Encode::encode(value, writer),
            Self::CameraPresets(value) => wire::Encode::encode(value, writer),
            Self::UnlockedRecipes(value) => wire::Encode::encode(value, writer),
            Self::CameraInstruction(value) => wire::Encode::encode(value, writer),
            Self::TrimData(value) => wire::Encode::encode(value, writer),
            Self::OpenSign(value) => wire::Encode::encode(value, writer),
            Self::AgentAnimation(value) => wire::Encode::encode(value, writer),
            Self::RefreshEntitlements(value) => wire::Encode::encode(value, writer),
            Self::PlayerToggleCrafterSlotRequest(value) => wire::Encode::encode(value, writer),
            Self::SetPlayerInventoryOptions(value) => wire::Encode::encode(value, writer),
            Self::SetHud(value) => wire::Encode::encode(value, writer),
            Self::AwardAchievement(value) => wire::Encode::encode(value, writer),
            Self::ClientboundCloseForm(value) => wire::Encode::encode(value, writer),
            Self::ServerboundLoadingScreen(value) => wire::Encode::encode(value, writer),
            Self::JigsawStructureData(value) => wire::Encode::encode(value, writer),
            Self::CurrentStructureFeature(value) => wire::Encode::encode(value, writer),
            Self::ServerboundDiagnostics(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::CameraAimAssist(value) => wire::Encode::encode(value, writer),
            Self::ContainerRegistryCleanup(value) => wire::Encode::encode(value, writer),
            Self::MovementEffect(value) => wire::Encode::encode(value, writer),
            Self::CameraAimAssistPresets(value) => wire::Encode::encode(value, writer),
            Self::ClientCameraAimAssist(value) => wire::Encode::encode(value, writer),
            Self::ClientMovementPredictionSync(value) => wire::Encode::encode(value, writer),
            Self::UpdateClientOptions(value) => wire::Encode::encode(value, writer),
            Self::PlayerVideoCapture(value) => wire::Encode::encode(value, writer),
            Self::PlayerUpdateEntityOverrides(value) => wire::Encode::encode(value, writer),
            Self::PlayerLocation(value) => wire::Encode::encode(value, writer),
            Self::ClientboundControlSchemeSet(value) => wire::Encode::encode(value, writer),
            Self::PrimitiveShapes(value) => wire::Encode::encode(value, writer),
            Self::ServerboundPackSettingChange(value) => wire::Encode::encode(value, writer),
            Self::ClientboundDataStore(value) => wire::Encode::encode(value, writer),
            Self::GraphicsOverrideParameter(value) => wire::Encode::encode(value, writer),
            Self::ServerboundDataStore(value) => wire::Encode::encode(value, writer),
            Self::ClientboundDataDrivenUIShowScreen(value) => wire::Encode::encode(value, writer),
            Self::ClientboundDataDrivenUICloseScreen(value) => wire::Encode::encode(value, writer),
            Self::ClientboundDataDrivenUIReload(value) => wire::Encode::encode(value, writer),
            Self::ClientboundTextureShift(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::VoxelShapes(value) => wire::Encode::encode(value, writer),
            Self::CameraSpline(value) => wire::Encode::encode(value, writer),
            Self::CameraAimAssistActorPriority(value) => wire::Encode::encode(value, writer),
            Self::ResourcePacksReadyForValidation(value) => wire::Encode::encode(value, writer),
            Self::LocatorBar(value) => wire::Encode::encode(value, writer),
            Self::PartyChanged(value) => wire::Encode::encode(value, writer),
            Self::ServerboundDataDrivenScreenClosed(value) => wire::Encode::encode(value, writer),
            Self::SyncWorldClocks(value) => wire::Encode::encode(value, writer),
            Self::ClientboundAttributeLayerSync(value) => wire::Encode::encode(value, writer),
            Self::ServerStoreInfo(value) => wire::Encode::encode(value, writer),
            Self::ServerPresenceInfo(value) => wire::Encode::encode(value, writer),
            Self::ClientboundUpdateSoundData(value) => wire::Encode::encode(value.as_ref(), writer),
            Self::SendPartyDestinationCookie(value) => wire::Encode::encode(value, writer),
            Self::PartyDestinationCookieResponse(value) => wire::Encode::encode(value, writer),
            Self::SetPlayerFurnaceOptions(value) => wire::Encode::encode(value, writer),
            Self::RecordStarted(value) => wire::Encode::encode(value, writer),
        }
    }

    /// Decodes a packet body by id, rejecting ids the sender may not use.
    pub fn decode_from(
        id: u32,
        sender: Peer,
        reader: &mut wire::Reader<'_>,
    ) -> wire::DecodeResult<Self> {
        let packet = PacketId::from_raw(id).ok_or(wire::DecodeError::UnknownPacketId(id))?;
        if !packet.direction().permits(sender) {
            return Err(wire::DecodeError::UnexpectedDirection(id));
        }
        Self::decode_body(packet, reader)
    }

    /// Decodes a packet body and rejects bytes left inside the declared entry.
    pub fn decode_exact_from(
        id: u32,
        sender: Peer,
        reader: &mut wire::Reader<'_>,
    ) -> wire::DecodeResult<Self> {
        let packet = Self::decode_from(id, sender, reader)?;
        reader.expect_consumed()?;
        Ok(packet)
    }

    pub fn decode_body(id: PacketId, reader: &mut wire::Reader<'_>) -> wire::DecodeResult<Self> {
        Ok(match id {
            PacketId::Login => Self::Login(<Login as wire::Decode>::decode(reader)?),
            PacketId::PlayStatus => Self::PlayStatus(<PlayStatus as wire::Decode>::decode(reader)?),
            PacketId::ServerToClientHandshake => Self::ServerToClientHandshake(
                <ServerToClientHandshake as wire::Decode>::decode(reader)?,
            ),
            PacketId::ClientToServerHandshake => Self::ClientToServerHandshake(
                <ClientToServerHandshake as wire::Decode>::decode(reader)?,
            ),
            PacketId::Disconnect => Self::Disconnect(<Disconnect as wire::Decode>::decode(reader)?),
            PacketId::ResourcePacksInfo => {
                Self::ResourcePacksInfo(<ResourcePacksInfo as wire::Decode>::decode(reader)?)
            }
            PacketId::ResourcePackStack => {
                Self::ResourcePackStack(<ResourcePackStack as wire::Decode>::decode(reader)?)
            }
            PacketId::ResourcePackClientResponse => Self::ResourcePackClientResponse(
                <ResourcePackClientResponse as wire::Decode>::decode(reader)?,
            ),
            PacketId::Text => Self::Text(<Text as wire::Decode>::decode(reader)?),
            PacketId::SetTime => Self::SetTime(<SetTime as wire::Decode>::decode(reader)?),
            PacketId::StartGame => {
                Self::StartGame(Box::new(<StartGame as wire::Decode>::decode(reader)?))
            }
            PacketId::AddPlayer => {
                Self::AddPlayer(Box::new(<AddPlayer as wire::Decode>::decode(reader)?))
            }
            PacketId::AddActor => {
                Self::AddActor(Box::new(<AddActor as wire::Decode>::decode(reader)?))
            }
            PacketId::RemoveActor => {
                Self::RemoveActor(<RemoveActor as wire::Decode>::decode(reader)?)
            }
            PacketId::AddItemActor => {
                Self::AddItemActor(<AddItemActor as wire::Decode>::decode(reader)?)
            }
            PacketId::ServerPlayerPostMovePosition => Self::ServerPlayerPostMovePosition(
                <ServerPlayerPostMovePosition as wire::Decode>::decode(reader)?,
            ),
            PacketId::TakeItemActor => {
                Self::TakeItemActor(<TakeItemActor as wire::Decode>::decode(reader)?)
            }
            PacketId::MoveActorAbsolute => {
                Self::MoveActorAbsolute(<MoveActorAbsolute as wire::Decode>::decode(reader)?)
            }
            PacketId::MovePlayer => {
                Self::MovePlayer(Box::new(<MovePlayer as wire::Decode>::decode(reader)?))
            }
            PacketId::UpdateBlock => {
                Self::UpdateBlock(<UpdateBlock as wire::Decode>::decode(reader)?)
            }
            PacketId::AddPainting => {
                Self::AddPainting(<AddPainting as wire::Decode>::decode(reader)?)
            }
            PacketId::LevelEvent => Self::LevelEvent(<LevelEvent as wire::Decode>::decode(reader)?),
            PacketId::BlockEvent => Self::BlockEvent(<BlockEvent as wire::Decode>::decode(reader)?),
            PacketId::ActorEvent => Self::ActorEvent(<ActorEvent as wire::Decode>::decode(reader)?),
            PacketId::MobEffect => {
                Self::MobEffect(Box::new(<MobEffect as wire::Decode>::decode(reader)?))
            }
            PacketId::UpdateAttributes => {
                Self::UpdateAttributes(<UpdateAttributes as wire::Decode>::decode(reader)?)
            }
            PacketId::InventoryTransaction => {
                Self::InventoryTransaction(<InventoryTransaction as wire::Decode>::decode(reader)?)
            }
            PacketId::MobEquipment => {
                Self::MobEquipment(<MobEquipment as wire::Decode>::decode(reader)?)
            }
            PacketId::MobArmorEquipment => {
                Self::MobArmorEquipment(<MobArmorEquipment as wire::Decode>::decode(reader)?)
            }
            PacketId::Interact => Self::Interact(<Interact as wire::Decode>::decode(reader)?),
            PacketId::BlockPickRequest => {
                Self::BlockPickRequest(<BlockPickRequest as wire::Decode>::decode(reader)?)
            }
            PacketId::ActorPickRequest => {
                Self::ActorPickRequest(<ActorPickRequest as wire::Decode>::decode(reader)?)
            }
            PacketId::PlayerAction => {
                Self::PlayerAction(<PlayerAction as wire::Decode>::decode(reader)?)
            }
            PacketId::HurtArmor => Self::HurtArmor(<HurtArmor as wire::Decode>::decode(reader)?),
            PacketId::SetActorData => {
                Self::SetActorData(<SetActorData as wire::Decode>::decode(reader)?)
            }
            PacketId::SetActorMotion => {
                Self::SetActorMotion(<SetActorMotion as wire::Decode>::decode(reader)?)
            }
            PacketId::SetActorLink => {
                Self::SetActorLink(<SetActorLink as wire::Decode>::decode(reader)?)
            }
            PacketId::SetHealth => Self::SetHealth(<SetHealth as wire::Decode>::decode(reader)?),
            PacketId::SetSpawnPosition => {
                Self::SetSpawnPosition(<SetSpawnPosition as wire::Decode>::decode(reader)?)
            }
            PacketId::Animate => Self::Animate(<Animate as wire::Decode>::decode(reader)?),
            PacketId::Respawn => Self::Respawn(<Respawn as wire::Decode>::decode(reader)?),
            PacketId::ContainerOpen => {
                Self::ContainerOpen(<ContainerOpen as wire::Decode>::decode(reader)?)
            }
            PacketId::ContainerClose => {
                Self::ContainerClose(<ContainerClose as wire::Decode>::decode(reader)?)
            }
            PacketId::PlayerHotbar => {
                Self::PlayerHotbar(<PlayerHotbar as wire::Decode>::decode(reader)?)
            }
            PacketId::InventoryContent => {
                Self::InventoryContent(<InventoryContent as wire::Decode>::decode(reader)?)
            }
            PacketId::InventorySlot => {
                Self::InventorySlot(<InventorySlot as wire::Decode>::decode(reader)?)
            }
            PacketId::ContainerSetData => {
                Self::ContainerSetData(<ContainerSetData as wire::Decode>::decode(reader)?)
            }
            PacketId::CraftingData => {
                Self::CraftingData(Box::new(<CraftingData as wire::Decode>::decode(reader)?))
            }
            PacketId::GuiDataPickItem => {
                Self::GuiDataPickItem(<GuiDataPickItem as wire::Decode>::decode(reader)?)
            }
            PacketId::BlockActorData => {
                Self::BlockActorData(<BlockActorData as wire::Decode>::decode(reader)?)
            }
            PacketId::LevelChunk => Self::LevelChunk(<LevelChunk as wire::Decode>::decode(reader)?),
            PacketId::SetCommandsEnabled => {
                Self::SetCommandsEnabled(<SetCommandsEnabled as wire::Decode>::decode(reader)?)
            }
            PacketId::SetDifficulty => {
                Self::SetDifficulty(<SetDifficulty as wire::Decode>::decode(reader)?)
            }
            PacketId::ChangeDimension => {
                Self::ChangeDimension(<ChangeDimension as wire::Decode>::decode(reader)?)
            }
            PacketId::SetPlayerGameType => {
                Self::SetPlayerGameType(<SetPlayerGameType as wire::Decode>::decode(reader)?)
            }
            PacketId::PlayerList => Self::PlayerList(<PlayerList as wire::Decode>::decode(reader)?),
            PacketId::SimpleEvent => {
                Self::SimpleEvent(<SimpleEvent as wire::Decode>::decode(reader)?)
            }
            PacketId::LegacyTelemetryEvent => {
                Self::LegacyTelemetryEvent(<LegacyTelemetryEvent as wire::Decode>::decode(reader)?)
            }
            PacketId::SpawnExperienceOrb => {
                Self::SpawnExperienceOrb(<SpawnExperienceOrb as wire::Decode>::decode(reader)?)
            }
            PacketId::ClientboundMapItemData => Self::ClientboundMapItemData(Box::new(
                <ClientboundMapItemData as wire::Decode>::decode(reader)?,
            )),
            PacketId::MapInfoRequest => {
                Self::MapInfoRequest(<MapInfoRequest as wire::Decode>::decode(reader)?)
            }
            PacketId::RequestChunkRadius => {
                Self::RequestChunkRadius(<RequestChunkRadius as wire::Decode>::decode(reader)?)
            }
            PacketId::ChunkRadiusUpdated => {
                Self::ChunkRadiusUpdated(<ChunkRadiusUpdated as wire::Decode>::decode(reader)?)
            }
            PacketId::GameRulesChanged => {
                Self::GameRulesChanged(<GameRulesChanged as wire::Decode>::decode(reader)?)
            }
            PacketId::Camera => Self::Camera(<Camera as wire::Decode>::decode(reader)?),
            PacketId::BossEvent => Self::BossEvent(<BossEvent as wire::Decode>::decode(reader)?),
            PacketId::ShowCredits => {
                Self::ShowCredits(<ShowCredits as wire::Decode>::decode(reader)?)
            }
            PacketId::AvailableCommands => Self::AvailableCommands(Box::new(
                <AvailableCommands as wire::Decode>::decode(reader)?,
            )),
            PacketId::CommandRequest => {
                Self::CommandRequest(<CommandRequest as wire::Decode>::decode(reader)?)
            }
            PacketId::CommandBlockUpdate => Self::CommandBlockUpdate(Box::new(
                <CommandBlockUpdate as wire::Decode>::decode(reader)?,
            )),
            PacketId::CommandOutput => {
                Self::CommandOutput(<CommandOutput as wire::Decode>::decode(reader)?)
            }
            PacketId::UpdateTrade => {
                Self::UpdateTrade(Box::new(<UpdateTrade as wire::Decode>::decode(reader)?))
            }
            PacketId::UpdateEquip => {
                Self::UpdateEquip(<UpdateEquip as wire::Decode>::decode(reader)?)
            }
            PacketId::ResourcePackDataInfo => {
                Self::ResourcePackDataInfo(<ResourcePackDataInfo as wire::Decode>::decode(reader)?)
            }
            PacketId::ResourcePackChunkData => Self::ResourcePackChunkData(
                <ResourcePackChunkData as wire::Decode>::decode(reader)?,
            ),
            PacketId::ResourcePackChunkRequest => Self::ResourcePackChunkRequest(
                <ResourcePackChunkRequest as wire::Decode>::decode(reader)?,
            ),
            PacketId::Transfer => Self::Transfer(<Transfer as wire::Decode>::decode(reader)?),
            PacketId::PlaySound => {
                Self::PlaySound(Box::new(<PlaySound as wire::Decode>::decode(reader)?))
            }
            PacketId::StopSound => Self::StopSound(<StopSound as wire::Decode>::decode(reader)?),
            PacketId::SetTitle => {
                Self::SetTitle(Box::new(<SetTitle as wire::Decode>::decode(reader)?))
            }
            PacketId::AddBehaviorTree => {
                Self::AddBehaviorTree(<AddBehaviorTree as wire::Decode>::decode(reader)?)
            }
            PacketId::StructureBlockUpdate => {
                Self::StructureBlockUpdate(<StructureBlockUpdate as wire::Decode>::decode(reader)?)
            }
            PacketId::ShowStoreOffer => {
                Self::ShowStoreOffer(<ShowStoreOffer as wire::Decode>::decode(reader)?)
            }
            PacketId::PurchaseReceipt => {
                Self::PurchaseReceipt(<PurchaseReceipt as wire::Decode>::decode(reader)?)
            }
            PacketId::PlayerSkin => Self::PlayerSkin(<PlayerSkin as wire::Decode>::decode(reader)?),
            PacketId::SubClientLogin => {
                Self::SubClientLogin(<SubClientLogin as wire::Decode>::decode(reader)?)
            }
            PacketId::AutomationClientConnect => Self::AutomationClientConnect(
                <AutomationClientConnect as wire::Decode>::decode(reader)?,
            ),
            PacketId::SetLastHurtBy => {
                Self::SetLastHurtBy(<SetLastHurtBy as wire::Decode>::decode(reader)?)
            }
            PacketId::BookEdit => Self::BookEdit(<BookEdit as wire::Decode>::decode(reader)?),
            PacketId::NpcRequest => Self::NpcRequest(<NpcRequest as wire::Decode>::decode(reader)?),
            PacketId::PhotoTransfer => {
                Self::PhotoTransfer(<PhotoTransfer as wire::Decode>::decode(reader)?)
            }
            PacketId::ModalFormRequest => {
                Self::ModalFormRequest(<ModalFormRequest as wire::Decode>::decode(reader)?)
            }
            PacketId::ModalFormResponse => {
                Self::ModalFormResponse(<ModalFormResponse as wire::Decode>::decode(reader)?)
            }
            PacketId::ServerSettingsRequest => Self::ServerSettingsRequest(
                <ServerSettingsRequest as wire::Decode>::decode(reader)?,
            ),
            PacketId::ServerSettingsResponse => Self::ServerSettingsResponse(
                <ServerSettingsResponse as wire::Decode>::decode(reader)?,
            ),
            PacketId::ShowProfile => {
                Self::ShowProfile(<ShowProfile as wire::Decode>::decode(reader)?)
            }
            PacketId::SetDefaultGameType => {
                Self::SetDefaultGameType(<SetDefaultGameType as wire::Decode>::decode(reader)?)
            }
            PacketId::RemoveObjective => {
                Self::RemoveObjective(<RemoveObjective as wire::Decode>::decode(reader)?)
            }
            PacketId::SetDisplayObjective => {
                Self::SetDisplayObjective(<SetDisplayObjective as wire::Decode>::decode(reader)?)
            }
            PacketId::SetScore => Self::SetScore(<SetScore as wire::Decode>::decode(reader)?),
            PacketId::LabTable => Self::LabTable(<LabTable as wire::Decode>::decode(reader)?),
            PacketId::UpdateBlockSynced => {
                Self::UpdateBlockSynced(<UpdateBlockSynced as wire::Decode>::decode(reader)?)
            }
            PacketId::MoveActorDelta => {
                Self::MoveActorDelta(<MoveActorDelta as wire::Decode>::decode(reader)?)
            }
            PacketId::SetScoreboardIdentity => Self::SetScoreboardIdentity(
                <SetScoreboardIdentity as wire::Decode>::decode(reader)?,
            ),
            PacketId::SetLocalPlayerAsInitialized => Self::SetLocalPlayerAsInitialized(
                <SetLocalPlayerAsInitialized as wire::Decode>::decode(reader)?,
            ),
            PacketId::UpdateSoftEnum => {
                Self::UpdateSoftEnum(<UpdateSoftEnum as wire::Decode>::decode(reader)?)
            }
            PacketId::NetworkStackLatency => {
                Self::NetworkStackLatency(<NetworkStackLatency as wire::Decode>::decode(reader)?)
            }
            PacketId::SpawnParticleEffect => {
                Self::SpawnParticleEffect(<SpawnParticleEffect as wire::Decode>::decode(reader)?)
            }
            PacketId::AvailableActorIdentifiers => Self::AvailableActorIdentifiers(
                <AvailableActorIdentifiers as wire::Decode>::decode(reader)?,
            ),
            PacketId::NetworkChunkPublisherUpdate => Self::NetworkChunkPublisherUpdate(
                <NetworkChunkPublisherUpdate as wire::Decode>::decode(reader)?,
            ),
            PacketId::BiomeDefinitionList => {
                Self::BiomeDefinitionList(<BiomeDefinitionList as wire::Decode>::decode(reader)?)
            }
            PacketId::LevelSoundEvent => {
                Self::LevelSoundEvent(Box::new(<LevelSoundEvent as wire::Decode>::decode(reader)?))
            }
            PacketId::LevelEventGeneric => {
                Self::LevelEventGeneric(<LevelEventGeneric as wire::Decode>::decode(reader)?)
            }
            PacketId::LecternUpdate => {
                Self::LecternUpdate(<LecternUpdate as wire::Decode>::decode(reader)?)
            }
            PacketId::ClientCacheStatus => {
                Self::ClientCacheStatus(<ClientCacheStatus as wire::Decode>::decode(reader)?)
            }
            PacketId::OnScreenTextureAnimation => Self::OnScreenTextureAnimation(
                <OnScreenTextureAnimation as wire::Decode>::decode(reader)?,
            ),
            PacketId::MapCreateLockedCopy => {
                Self::MapCreateLockedCopy(<MapCreateLockedCopy as wire::Decode>::decode(reader)?)
            }
            PacketId::StructureTemplateDataRequest => Self::StructureTemplateDataRequest(
                <StructureTemplateDataRequest as wire::Decode>::decode(reader)?,
            ),
            PacketId::StructureTemplateDataResponse => Self::StructureTemplateDataResponse(
                <StructureTemplateDataResponse as wire::Decode>::decode(reader)?,
            ),
            PacketId::ClientCacheBlobStatus => Self::ClientCacheBlobStatus(
                <ClientCacheBlobStatus as wire::Decode>::decode(reader)?,
            ),
            PacketId::ClientCacheMissResponse => Self::ClientCacheMissResponse(
                <ClientCacheMissResponse as wire::Decode>::decode(reader)?,
            ),
            PacketId::EducationSettings => {
                Self::EducationSettings(<EducationSettings as wire::Decode>::decode(reader)?)
            }
            PacketId::Emote => Self::Emote(<Emote as wire::Decode>::decode(reader)?),
            PacketId::MultiplayerSettings => {
                Self::MultiplayerSettings(<MultiplayerSettings as wire::Decode>::decode(reader)?)
            }
            PacketId::SettingsCommand => {
                Self::SettingsCommand(<SettingsCommand as wire::Decode>::decode(reader)?)
            }
            PacketId::AnvilDamage => {
                Self::AnvilDamage(<AnvilDamage as wire::Decode>::decode(reader)?)
            }
            PacketId::CompletedUsingItem => {
                Self::CompletedUsingItem(<CompletedUsingItem as wire::Decode>::decode(reader)?)
            }
            PacketId::NetworkSettings => {
                Self::NetworkSettings(<NetworkSettings as wire::Decode>::decode(reader)?)
            }
            PacketId::PlayerAuthInput => {
                Self::PlayerAuthInput(Box::new(<PlayerAuthInput as wire::Decode>::decode(reader)?))
            }
            PacketId::CreativeContent => {
                Self::CreativeContent(<CreativeContent as wire::Decode>::decode(reader)?)
            }
            PacketId::PlayerEnchantOptions => {
                Self::PlayerEnchantOptions(<PlayerEnchantOptions as wire::Decode>::decode(reader)?)
            }
            PacketId::ItemStackRequest => {
                Self::ItemStackRequest(<ItemStackRequest as wire::Decode>::decode(reader)?)
            }
            PacketId::ItemStackResponse => {
                Self::ItemStackResponse(<ItemStackResponse as wire::Decode>::decode(reader)?)
            }
            PacketId::PlayerArmorDamage => {
                Self::PlayerArmorDamage(<PlayerArmorDamage as wire::Decode>::decode(reader)?)
            }
            PacketId::CodeBuilder => {
                Self::CodeBuilder(<CodeBuilder as wire::Decode>::decode(reader)?)
            }
            PacketId::UpdatePlayerGameType => {
                Self::UpdatePlayerGameType(<UpdatePlayerGameType as wire::Decode>::decode(reader)?)
            }
            PacketId::EmoteList => Self::EmoteList(<EmoteList as wire::Decode>::decode(reader)?),
            PacketId::PositionTrackingDBServerBroadcast => Self::PositionTrackingDBServerBroadcast(
                <PositionTrackingDBServerBroadcast as wire::Decode>::decode(reader)?,
            ),
            PacketId::PositionTrackingDBClientRequest => Self::PositionTrackingDBClientRequest(
                <PositionTrackingDBClientRequest as wire::Decode>::decode(reader)?,
            ),
            PacketId::DebugInfo => Self::DebugInfo(<DebugInfo as wire::Decode>::decode(reader)?),
            PacketId::PacketViolationWarning => Self::PacketViolationWarning(
                <PacketViolationWarning as wire::Decode>::decode(reader)?,
            ),
            PacketId::MotionPredictionHints => Self::MotionPredictionHints(
                <MotionPredictionHints as wire::Decode>::decode(reader)?,
            ),
            PacketId::AnimateEntity => {
                Self::AnimateEntity(<AnimateEntity as wire::Decode>::decode(reader)?)
            }
            PacketId::CameraShake => {
                Self::CameraShake(<CameraShake as wire::Decode>::decode(reader)?)
            }
            PacketId::PlayerFog => Self::PlayerFog(<PlayerFog as wire::Decode>::decode(reader)?),
            PacketId::CorrectPlayerMovePrediction => Self::CorrectPlayerMovePrediction(
                <CorrectPlayerMovePrediction as wire::Decode>::decode(reader)?,
            ),
            PacketId::ItemRegistry => {
                Self::ItemRegistry(<ItemRegistry as wire::Decode>::decode(reader)?)
            }
            PacketId::ClientboundDebugRenderer => Self::ClientboundDebugRenderer(
                <ClientboundDebugRenderer as wire::Decode>::decode(reader)?,
            ),
            PacketId::SyncActorProperty => {
                Self::SyncActorProperty(<SyncActorProperty as wire::Decode>::decode(reader)?)
            }
            PacketId::AddVolumeEntity => {
                Self::AddVolumeEntity(Box::new(<AddVolumeEntity as wire::Decode>::decode(reader)?))
            }
            PacketId::RemoveVolumeEntity => {
                Self::RemoveVolumeEntity(<RemoveVolumeEntity as wire::Decode>::decode(reader)?)
            }
            PacketId::SimulationType => {
                Self::SimulationType(<SimulationType as wire::Decode>::decode(reader)?)
            }
            PacketId::NpcDialogue => {
                Self::NpcDialogue(<NpcDialogue as wire::Decode>::decode(reader)?)
            }
            PacketId::EduUriResource => {
                Self::EduUriResource(<EduUriResource as wire::Decode>::decode(reader)?)
            }
            PacketId::CreatePhoto => {
                Self::CreatePhoto(<CreatePhoto as wire::Decode>::decode(reader)?)
            }
            PacketId::UpdateSubChunkBlocks => {
                Self::UpdateSubChunkBlocks(<UpdateSubChunkBlocks as wire::Decode>::decode(reader)?)
            }
            PacketId::SubChunk => Self::SubChunk(<SubChunk as wire::Decode>::decode(reader)?),
            PacketId::SubChunkRequest => {
                Self::SubChunkRequest(<SubChunkRequest as wire::Decode>::decode(reader)?)
            }
            PacketId::PlayerStartItemCooldown => Self::PlayerStartItemCooldown(
                <PlayerStartItemCooldown as wire::Decode>::decode(reader)?,
            ),
            PacketId::ScriptMessage => {
                Self::ScriptMessage(<ScriptMessage as wire::Decode>::decode(reader)?)
            }
            PacketId::CodeBuilderSource => {
                Self::CodeBuilderSource(<CodeBuilderSource as wire::Decode>::decode(reader)?)
            }
            PacketId::TickingAreasLoadStatus => Self::TickingAreasLoadStatus(
                <TickingAreasLoadStatus as wire::Decode>::decode(reader)?,
            ),
            PacketId::DimensionData => {
                Self::DimensionData(<DimensionData as wire::Decode>::decode(reader)?)
            }
            PacketId::AgentActionEvent => {
                Self::AgentActionEvent(<AgentActionEvent as wire::Decode>::decode(reader)?)
            }
            PacketId::ChangeMobProperty => {
                Self::ChangeMobProperty(<ChangeMobProperty as wire::Decode>::decode(reader)?)
            }
            PacketId::LessonProgress => {
                Self::LessonProgress(<LessonProgress as wire::Decode>::decode(reader)?)
            }
            PacketId::RequestAbility => {
                Self::RequestAbility(<RequestAbility as wire::Decode>::decode(reader)?)
            }
            PacketId::RequestPermissions => {
                Self::RequestPermissions(<RequestPermissions as wire::Decode>::decode(reader)?)
            }
            PacketId::ToastRequest => {
                Self::ToastRequest(<ToastRequest as wire::Decode>::decode(reader)?)
            }
            PacketId::UpdateAbilities => {
                Self::UpdateAbilities(<UpdateAbilities as wire::Decode>::decode(reader)?)
            }
            PacketId::UpdateAdventureSettings => Self::UpdateAdventureSettings(
                <UpdateAdventureSettings as wire::Decode>::decode(reader)?,
            ),
            PacketId::DeathInfo => Self::DeathInfo(<DeathInfo as wire::Decode>::decode(reader)?),
            PacketId::EditorNetwork => {
                Self::EditorNetwork(<EditorNetwork as wire::Decode>::decode(reader)?)
            }
            PacketId::FeatureRegistry => {
                Self::FeatureRegistry(<FeatureRegistry as wire::Decode>::decode(reader)?)
            }
            PacketId::ServerStats => {
                Self::ServerStats(<ServerStats as wire::Decode>::decode(reader)?)
            }
            PacketId::RequestNetworkSettings => Self::RequestNetworkSettings(
                <RequestNetworkSettings as wire::Decode>::decode(reader)?,
            ),
            PacketId::GameTestRequest => {
                Self::GameTestRequest(<GameTestRequest as wire::Decode>::decode(reader)?)
            }
            PacketId::GameTestResults => {
                Self::GameTestResults(<GameTestResults as wire::Decode>::decode(reader)?)
            }
            PacketId::UpdateClientInputLocks => Self::UpdateClientInputLocks(
                <UpdateClientInputLocks as wire::Decode>::decode(reader)?,
            ),
            PacketId::CameraPresets => {
                Self::CameraPresets(<CameraPresets as wire::Decode>::decode(reader)?)
            }
            PacketId::UnlockedRecipes => {
                Self::UnlockedRecipes(<UnlockedRecipes as wire::Decode>::decode(reader)?)
            }
            PacketId::CameraInstruction => {
                Self::CameraInstruction(<CameraInstruction as wire::Decode>::decode(reader)?)
            }
            PacketId::TrimData => Self::TrimData(<TrimData as wire::Decode>::decode(reader)?),
            PacketId::OpenSign => Self::OpenSign(<OpenSign as wire::Decode>::decode(reader)?),
            PacketId::AgentAnimation => {
                Self::AgentAnimation(<AgentAnimation as wire::Decode>::decode(reader)?)
            }
            PacketId::RefreshEntitlements => {
                Self::RefreshEntitlements(<RefreshEntitlements as wire::Decode>::decode(reader)?)
            }
            PacketId::PlayerToggleCrafterSlotRequest => Self::PlayerToggleCrafterSlotRequest(
                <PlayerToggleCrafterSlotRequest as wire::Decode>::decode(reader)?,
            ),
            PacketId::SetPlayerInventoryOptions => Self::SetPlayerInventoryOptions(
                <SetPlayerInventoryOptions as wire::Decode>::decode(reader)?,
            ),
            PacketId::SetHud => Self::SetHud(<SetHud as wire::Decode>::decode(reader)?),
            PacketId::AwardAchievement => {
                Self::AwardAchievement(<AwardAchievement as wire::Decode>::decode(reader)?)
            }
            PacketId::ClientboundCloseForm => {
                Self::ClientboundCloseForm(<ClientboundCloseForm as wire::Decode>::decode(reader)?)
            }
            PacketId::ServerboundLoadingScreen => Self::ServerboundLoadingScreen(
                <ServerboundLoadingScreen as wire::Decode>::decode(reader)?,
            ),
            PacketId::JigsawStructureData => {
                Self::JigsawStructureData(<JigsawStructureData as wire::Decode>::decode(reader)?)
            }
            PacketId::CurrentStructureFeature => Self::CurrentStructureFeature(
                <CurrentStructureFeature as wire::Decode>::decode(reader)?,
            ),
            PacketId::ServerboundDiagnostics => Self::ServerboundDiagnostics(Box::new(
                <ServerboundDiagnostics as wire::Decode>::decode(reader)?,
            )),
            PacketId::CameraAimAssist => {
                Self::CameraAimAssist(<CameraAimAssist as wire::Decode>::decode(reader)?)
            }
            PacketId::ContainerRegistryCleanup => Self::ContainerRegistryCleanup(
                <ContainerRegistryCleanup as wire::Decode>::decode(reader)?,
            ),
            PacketId::MovementEffect => {
                Self::MovementEffect(<MovementEffect as wire::Decode>::decode(reader)?)
            }
            PacketId::CameraAimAssistPresets => Self::CameraAimAssistPresets(
                <CameraAimAssistPresets as wire::Decode>::decode(reader)?,
            ),
            PacketId::ClientCameraAimAssist => Self::ClientCameraAimAssist(
                <ClientCameraAimAssist as wire::Decode>::decode(reader)?,
            ),
            PacketId::ClientMovementPredictionSync => Self::ClientMovementPredictionSync(
                <ClientMovementPredictionSync as wire::Decode>::decode(reader)?,
            ),
            PacketId::UpdateClientOptions => {
                Self::UpdateClientOptions(<UpdateClientOptions as wire::Decode>::decode(reader)?)
            }
            PacketId::PlayerVideoCapture => {
                Self::PlayerVideoCapture(<PlayerVideoCapture as wire::Decode>::decode(reader)?)
            }
            PacketId::PlayerUpdateEntityOverrides => Self::PlayerUpdateEntityOverrides(
                <PlayerUpdateEntityOverrides as wire::Decode>::decode(reader)?,
            ),
            PacketId::PlayerLocation => {
                Self::PlayerLocation(<PlayerLocation as wire::Decode>::decode(reader)?)
            }
            PacketId::ClientboundControlSchemeSet => Self::ClientboundControlSchemeSet(
                <ClientboundControlSchemeSet as wire::Decode>::decode(reader)?,
            ),
            PacketId::PrimitiveShapes => {
                Self::PrimitiveShapes(<PrimitiveShapes as wire::Decode>::decode(reader)?)
            }
            PacketId::ServerboundPackSettingChange => Self::ServerboundPackSettingChange(
                <ServerboundPackSettingChange as wire::Decode>::decode(reader)?,
            ),
            PacketId::ClientboundDataStore => {
                Self::ClientboundDataStore(<ClientboundDataStore as wire::Decode>::decode(reader)?)
            }
            PacketId::GraphicsOverrideParameter => Self::GraphicsOverrideParameter(
                <GraphicsOverrideParameter as wire::Decode>::decode(reader)?,
            ),
            PacketId::ServerboundDataStore => {
                Self::ServerboundDataStore(<ServerboundDataStore as wire::Decode>::decode(reader)?)
            }
            PacketId::ClientboundDataDrivenUIShowScreen => Self::ClientboundDataDrivenUIShowScreen(
                <ClientboundDataDrivenUIShowScreen as wire::Decode>::decode(reader)?,
            ),
            PacketId::ClientboundDataDrivenUICloseScreen => {
                Self::ClientboundDataDrivenUICloseScreen(
                    <ClientboundDataDrivenUICloseScreen as wire::Decode>::decode(reader)?,
                )
            }
            PacketId::ClientboundDataDrivenUIReload => Self::ClientboundDataDrivenUIReload(
                <ClientboundDataDrivenUIReload as wire::Decode>::decode(reader)?,
            ),
            PacketId::ClientboundTextureShift => Self::ClientboundTextureShift(Box::new(
                <ClientboundTextureShift as wire::Decode>::decode(reader)?,
            )),
            PacketId::VoxelShapes => {
                Self::VoxelShapes(<VoxelShapes as wire::Decode>::decode(reader)?)
            }
            PacketId::CameraSpline => {
                Self::CameraSpline(<CameraSpline as wire::Decode>::decode(reader)?)
            }
            PacketId::CameraAimAssistActorPriority => Self::CameraAimAssistActorPriority(
                <CameraAimAssistActorPriority as wire::Decode>::decode(reader)?,
            ),
            PacketId::ResourcePacksReadyForValidation => Self::ResourcePacksReadyForValidation(
                <ResourcePacksReadyForValidation as wire::Decode>::decode(reader)?,
            ),
            PacketId::LocatorBar => Self::LocatorBar(<LocatorBar as wire::Decode>::decode(reader)?),
            PacketId::PartyChanged => {
                Self::PartyChanged(<PartyChanged as wire::Decode>::decode(reader)?)
            }
            PacketId::ServerboundDataDrivenScreenClosed => Self::ServerboundDataDrivenScreenClosed(
                <ServerboundDataDrivenScreenClosed as wire::Decode>::decode(reader)?,
            ),
            PacketId::SyncWorldClocks => {
                Self::SyncWorldClocks(<SyncWorldClocks as wire::Decode>::decode(reader)?)
            }
            PacketId::ClientboundAttributeLayerSync => Self::ClientboundAttributeLayerSync(
                <ClientboundAttributeLayerSync as wire::Decode>::decode(reader)?,
            ),
            PacketId::ServerStoreInfo => {
                Self::ServerStoreInfo(<ServerStoreInfo as wire::Decode>::decode(reader)?)
            }
            PacketId::ServerPresenceInfo => {
                Self::ServerPresenceInfo(<ServerPresenceInfo as wire::Decode>::decode(reader)?)
            }
            PacketId::ClientboundUpdateSoundData => Self::ClientboundUpdateSoundData(Box::new(
                <ClientboundUpdateSoundData as wire::Decode>::decode(reader)?,
            )),
            PacketId::SendPartyDestinationCookie => Self::SendPartyDestinationCookie(
                <SendPartyDestinationCookie as wire::Decode>::decode(reader)?,
            ),
            PacketId::PartyDestinationCookieResponse => Self::PartyDestinationCookieResponse(
                <PartyDestinationCookieResponse as wire::Decode>::decode(reader)?,
            ),
            PacketId::SetPlayerFurnaceOptions => Self::SetPlayerFurnaceOptions(
                <SetPlayerFurnaceOptions as wire::Decode>::decode(reader)?,
            ),
            PacketId::RecordStarted => {
                Self::RecordStarted(<RecordStarted as wire::Decode>::decode(reader)?)
            }
        })
    }
}

/// A decoded batch entry: a packet this crate defines, or the exact bytes of
/// one it does not.
#[derive(Clone, Debug, PartialEq)]
pub enum Decoded {
    Known(Packet),
    Unknown(UnknownPacket),
}

/// A packet id absent from this manifest, with its entry preserved byte-exact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownPacket {
    pub header: wire::PacketHeader,
    pub body: bytes::Bytes,
    /// The complete entry as received. The header keeps only the id and
    /// subclient bits, so forwarding uses these bytes instead.
    pub frame: bytes::Bytes,
}

impl UnknownPacket {
    /// Appends the entry exactly as received; on error `out` is unchanged.
    pub fn encode_frame(&self, out: &mut Vec<u8>) -> wire::EncodeResult<()> {
        wire::Writer::new(out).write_all(&self.frame)
    }
}

impl Packet {
    /// Decodes one batch entry sent by `sender`. Unknown ids yield
    /// [`Decoded::Unknown`]; a known id the sender may not use, a malformed
    /// body, or bytes left inside the entry fail this entry only.
    pub fn decode_frame(
        frame: &wire::RawFrame,
        sender: Peer,
        limits: &wire::Limits,
    ) -> wire::DecodeResult<Decoded> {
        let header = frame.header();
        let body = frame.body();
        let Some(id) = PacketId::from_raw(header.id) else {
            return Ok(Decoded::Unknown(UnknownPacket {
                header,
                body,
                frame: frame.frame().clone(),
            }));
        };
        if !id.direction().permits(sender) {
            return Err(wire::DecodeError::UnexpectedDirection(header.id));
        }
        let mut reader = wire::Reader::from_shared(&body).with_limits(limits);
        let packet = Self::decode_body(id, &mut reader)?;
        reader.expect_consumed()?;
        Ok(Decoded::Known(packet))
    }

    /// Appends this packet as one batch entry sent by `sender`; on error
    /// `out` is restored to its prior length.
    pub fn encode_frame(
        &self,
        sender: Peer,
        sender_subclient: u8,
        target_subclient: u8,
        out: &mut Vec<u8>,
    ) -> wire::EncodeResult<()> {
        let id = self.id();
        if !id.direction().permits(sender) {
            return Err(wire::EncodeError::UnexpectedDirection(id as u32));
        }
        let header = wire::PacketHeader {
            id: id as u32,
            sender_subclient,
            target_subclient,
        };
        wire::encode_frame_with(out, header, usize::MAX, |writer| self.encode(writer))
    }
}
