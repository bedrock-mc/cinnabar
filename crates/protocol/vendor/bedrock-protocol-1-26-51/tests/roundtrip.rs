// Code generated from canonical protocol manifest v2. DO NOT EDIT.

use bedrock_protocol_1_26_51::packets::*;
use bedrock_protocol_1_26_51::wire::{self, Decode, Encode, Peer};

fn roundtrip<T>(name: &str)
where
    T: Encode + Decode + Default + PartialEq + std::fmt::Debug + Into<Packet>,
{
    let value = T::default();
    let bytes = match value.encode_to_vec() {
        Ok(bytes) => bytes,
        Err(wire::EncodeError::SchemaConstraint(_)) => return,
        Err(error) => panic!("{name} could not encode its default: {error}"),
    };
    match T::decode_exact(&bytes) {
        Ok(decoded) => assert_eq!(decoded, value, "{name} did not survive a round trip"),
        Err(error) => panic!("{name} could not decode its own encoding: {error}"),
    }

    let packet: Packet = value.into();
    let sender = if packet.id().direction().permits(Peer::Server) {
        Peer::Server
    } else {
        Peer::Client
    };
    let mut out = Vec::new();
    packet
        .encode_frame(sender, 1, 2, &mut out)
        .unwrap_or_else(|error| panic!("{name} could not encode a frame: {error}"));
    let frames = wire::Frames::new(out.into(), 1)
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|error| panic!("{name} framing failed: {error}"));
    assert_eq!(frames.len(), 1, "{name} did not encode exactly one frame");
    assert_eq!(frames[0].header().sender_subclient, 1);
    assert_eq!(frames[0].header().target_subclient, 2);
    let decoded = Packet::decode_frame(&frames[0], sender, &wire::Limits::receiving_from(sender));
    assert_eq!(
        decoded,
        Ok(Decoded::Known(packet)),
        "{name} did not survive a frame round trip"
    );
}

#[test]
fn every_schema_valid_packet_default_round_trips() {
    roundtrip::<Login>("Login");
    roundtrip::<PlayStatus>("PlayStatus");
    roundtrip::<ServerToClientHandshake>("ServerToClientHandshake");
    roundtrip::<ClientToServerHandshake>("ClientToServerHandshake");
    roundtrip::<Disconnect>("Disconnect");
    roundtrip::<ResourcePacksInfo>("ResourcePacksInfo");
    roundtrip::<ResourcePackStack>("ResourcePackStack");
    roundtrip::<ResourcePackClientResponse>("ResourcePackClientResponse");
    roundtrip::<Text>("Text");
    roundtrip::<SetTime>("SetTime");
    roundtrip::<StartGame>("StartGame");
    roundtrip::<AddPlayer>("AddPlayer");
    roundtrip::<AddActor>("AddActor");
    roundtrip::<RemoveActor>("RemoveActor");
    roundtrip::<AddItemActor>("AddItemActor");
    roundtrip::<ServerPlayerPostMovePosition>("ServerPlayerPostMovePosition");
    roundtrip::<TakeItemActor>("TakeItemActor");
    roundtrip::<MoveActorAbsolute>("MoveActorAbsolute");
    roundtrip::<MovePlayer>("MovePlayer");
    roundtrip::<UpdateBlock>("UpdateBlock");
    roundtrip::<AddPainting>("AddPainting");
    roundtrip::<LevelEvent>("LevelEvent");
    roundtrip::<BlockEvent>("BlockEvent");
    roundtrip::<ActorEvent>("ActorEvent");
    roundtrip::<MobEffect>("MobEffect");
    roundtrip::<UpdateAttributes>("UpdateAttributes");
    roundtrip::<InventoryTransaction>("InventoryTransaction");
    roundtrip::<MobEquipment>("MobEquipment");
    roundtrip::<MobArmorEquipment>("MobArmorEquipment");
    roundtrip::<Interact>("Interact");
    roundtrip::<BlockPickRequest>("BlockPickRequest");
    roundtrip::<ActorPickRequest>("ActorPickRequest");
    roundtrip::<PlayerAction>("PlayerAction");
    roundtrip::<HurtArmor>("HurtArmor");
    roundtrip::<SetActorData>("SetActorData");
    roundtrip::<SetActorMotion>("SetActorMotion");
    roundtrip::<SetActorLink>("SetActorLink");
    roundtrip::<SetHealth>("SetHealth");
    roundtrip::<SetSpawnPosition>("SetSpawnPosition");
    roundtrip::<Animate>("Animate");
    roundtrip::<Respawn>("Respawn");
    roundtrip::<ContainerOpen>("ContainerOpen");
    roundtrip::<ContainerClose>("ContainerClose");
    roundtrip::<PlayerHotbar>("PlayerHotbar");
    roundtrip::<InventoryContent>("InventoryContent");
    roundtrip::<InventorySlot>("InventorySlot");
    roundtrip::<ContainerSetData>("ContainerSetData");
    roundtrip::<CraftingData>("CraftingData");
    roundtrip::<GuiDataPickItem>("GuiDataPickItem");
    roundtrip::<BlockActorData>("BlockActorData");
    roundtrip::<LevelChunk>("LevelChunk");
    roundtrip::<SetCommandsEnabled>("SetCommandsEnabled");
    roundtrip::<SetDifficulty>("SetDifficulty");
    roundtrip::<ChangeDimension>("ChangeDimension");
    roundtrip::<SetPlayerGameType>("SetPlayerGameType");
    roundtrip::<PlayerList>("PlayerList");
    roundtrip::<SimpleEvent>("SimpleEvent");
    roundtrip::<LegacyTelemetryEvent>("LegacyTelemetryEvent");
    roundtrip::<SpawnExperienceOrb>("SpawnExperienceOrb");
    roundtrip::<ClientboundMapItemData>("ClientboundMapItemData");
    roundtrip::<MapInfoRequest>("MapInfoRequest");
    roundtrip::<RequestChunkRadius>("RequestChunkRadius");
    roundtrip::<ChunkRadiusUpdated>("ChunkRadiusUpdated");
    roundtrip::<GameRulesChanged>("GameRulesChanged");
    roundtrip::<Camera>("Camera");
    roundtrip::<BossEvent>("BossEvent");
    roundtrip::<ShowCredits>("ShowCredits");
    roundtrip::<AvailableCommands>("AvailableCommands");
    roundtrip::<CommandRequest>("CommandRequest");
    roundtrip::<CommandBlockUpdate>("CommandBlockUpdate");
    roundtrip::<CommandOutput>("CommandOutput");
    roundtrip::<UpdateTrade>("UpdateTrade");
    roundtrip::<UpdateEquip>("UpdateEquip");
    roundtrip::<ResourcePackDataInfo>("ResourcePackDataInfo");
    roundtrip::<ResourcePackChunkData>("ResourcePackChunkData");
    roundtrip::<ResourcePackChunkRequest>("ResourcePackChunkRequest");
    roundtrip::<Transfer>("Transfer");
    roundtrip::<PlaySound>("PlaySound");
    roundtrip::<StopSound>("StopSound");
    roundtrip::<SetTitle>("SetTitle");
    roundtrip::<AddBehaviorTree>("AddBehaviorTree");
    roundtrip::<StructureBlockUpdate>("StructureBlockUpdate");
    roundtrip::<ShowStoreOffer>("ShowStoreOffer");
    roundtrip::<PurchaseReceipt>("PurchaseReceipt");
    roundtrip::<PlayerSkin>("PlayerSkin");
    roundtrip::<SubClientLogin>("SubClientLogin");
    roundtrip::<AutomationClientConnect>("AutomationClientConnect");
    roundtrip::<SetLastHurtBy>("SetLastHurtBy");
    roundtrip::<BookEdit>("BookEdit");
    roundtrip::<NpcRequest>("NpcRequest");
    roundtrip::<PhotoTransfer>("PhotoTransfer");
    roundtrip::<ModalFormRequest>("ModalFormRequest");
    roundtrip::<ModalFormResponse>("ModalFormResponse");
    roundtrip::<ServerSettingsRequest>("ServerSettingsRequest");
    roundtrip::<ServerSettingsResponse>("ServerSettingsResponse");
    roundtrip::<ShowProfile>("ShowProfile");
    roundtrip::<SetDefaultGameType>("SetDefaultGameType");
    roundtrip::<RemoveObjective>("RemoveObjective");
    roundtrip::<SetDisplayObjective>("SetDisplayObjective");
    roundtrip::<SetScore>("SetScore");
    roundtrip::<LabTable>("LabTable");
    roundtrip::<UpdateBlockSynced>("UpdateBlockSynced");
    roundtrip::<MoveActorDelta>("MoveActorDelta");
    roundtrip::<SetScoreboardIdentity>("SetScoreboardIdentity");
    roundtrip::<SetLocalPlayerAsInitialized>("SetLocalPlayerAsInitialized");
    roundtrip::<UpdateSoftEnum>("UpdateSoftEnum");
    roundtrip::<NetworkStackLatency>("NetworkStackLatency");
    roundtrip::<SpawnParticleEffect>("SpawnParticleEffect");
    roundtrip::<AvailableActorIdentifiers>("AvailableActorIdentifiers");
    roundtrip::<NetworkChunkPublisherUpdate>("NetworkChunkPublisherUpdate");
    roundtrip::<BiomeDefinitionList>("BiomeDefinitionList");
    roundtrip::<LevelSoundEvent>("LevelSoundEvent");
    roundtrip::<LevelEventGeneric>("LevelEventGeneric");
    roundtrip::<LecternUpdate>("LecternUpdate");
    roundtrip::<ClientCacheStatus>("ClientCacheStatus");
    roundtrip::<OnScreenTextureAnimation>("OnScreenTextureAnimation");
    roundtrip::<MapCreateLockedCopy>("MapCreateLockedCopy");
    roundtrip::<StructureTemplateDataRequest>("StructureTemplateDataRequest");
    roundtrip::<StructureTemplateDataResponse>("StructureTemplateDataResponse");
    roundtrip::<ClientCacheBlobStatus>("ClientCacheBlobStatus");
    roundtrip::<ClientCacheMissResponse>("ClientCacheMissResponse");
    roundtrip::<EducationSettings>("EducationSettings");
    roundtrip::<Emote>("Emote");
    roundtrip::<MultiplayerSettings>("MultiplayerSettings");
    roundtrip::<SettingsCommand>("SettingsCommand");
    roundtrip::<AnvilDamage>("AnvilDamage");
    roundtrip::<CompletedUsingItem>("CompletedUsingItem");
    roundtrip::<NetworkSettings>("NetworkSettings");
    roundtrip::<PlayerAuthInput>("PlayerAuthInput");
    roundtrip::<CreativeContent>("CreativeContent");
    roundtrip::<PlayerEnchantOptions>("PlayerEnchantOptions");
    roundtrip::<ItemStackRequest>("ItemStackRequest");
    roundtrip::<ItemStackResponse>("ItemStackResponse");
    roundtrip::<PlayerArmorDamage>("PlayerArmorDamage");
    roundtrip::<CodeBuilder>("CodeBuilder");
    roundtrip::<UpdatePlayerGameType>("UpdatePlayerGameType");
    roundtrip::<EmoteList>("EmoteList");
    roundtrip::<PositionTrackingDBServerBroadcast>("PositionTrackingDBServerBroadcast");
    roundtrip::<PositionTrackingDBClientRequest>("PositionTrackingDBClientRequest");
    roundtrip::<DebugInfo>("DebugInfo");
    roundtrip::<PacketViolationWarning>("PacketViolationWarning");
    roundtrip::<MotionPredictionHints>("MotionPredictionHints");
    roundtrip::<AnimateEntity>("AnimateEntity");
    roundtrip::<CameraShake>("CameraShake");
    roundtrip::<PlayerFog>("PlayerFog");
    roundtrip::<CorrectPlayerMovePrediction>("CorrectPlayerMovePrediction");
    roundtrip::<ItemRegistry>("ItemRegistry");
    roundtrip::<ClientboundDebugRenderer>("ClientboundDebugRenderer");
    roundtrip::<SyncActorProperty>("SyncActorProperty");
    roundtrip::<AddVolumeEntity>("AddVolumeEntity");
    roundtrip::<RemoveVolumeEntity>("RemoveVolumeEntity");
    roundtrip::<SimulationType>("SimulationType");
    roundtrip::<NpcDialogue>("NpcDialogue");
    roundtrip::<EduUriResource>("EduUriResource");
    roundtrip::<CreatePhoto>("CreatePhoto");
    roundtrip::<UpdateSubChunkBlocks>("UpdateSubChunkBlocks");
    roundtrip::<SubChunk>("SubChunk");
    roundtrip::<SubChunkRequest>("SubChunkRequest");
    roundtrip::<PlayerStartItemCooldown>("PlayerStartItemCooldown");
    roundtrip::<ScriptMessage>("ScriptMessage");
    roundtrip::<CodeBuilderSource>("CodeBuilderSource");
    roundtrip::<TickingAreasLoadStatus>("TickingAreasLoadStatus");
    roundtrip::<DimensionData>("DimensionData");
    roundtrip::<AgentActionEvent>("AgentActionEvent");
    roundtrip::<ChangeMobProperty>("ChangeMobProperty");
    roundtrip::<LessonProgress>("LessonProgress");
    roundtrip::<RequestAbility>("RequestAbility");
    roundtrip::<RequestPermissions>("RequestPermissions");
    roundtrip::<ToastRequest>("ToastRequest");
    roundtrip::<UpdateAbilities>("UpdateAbilities");
    roundtrip::<UpdateAdventureSettings>("UpdateAdventureSettings");
    roundtrip::<DeathInfo>("DeathInfo");
    roundtrip::<EditorNetwork>("EditorNetwork");
    roundtrip::<FeatureRegistry>("FeatureRegistry");
    roundtrip::<ServerStats>("ServerStats");
    roundtrip::<RequestNetworkSettings>("RequestNetworkSettings");
    roundtrip::<GameTestRequest>("GameTestRequest");
    roundtrip::<GameTestResults>("GameTestResults");
    roundtrip::<UpdateClientInputLocks>("UpdateClientInputLocks");
    roundtrip::<CameraPresets>("CameraPresets");
    roundtrip::<UnlockedRecipes>("UnlockedRecipes");
    roundtrip::<CameraInstruction>("CameraInstruction");
    roundtrip::<TrimData>("TrimData");
    roundtrip::<OpenSign>("OpenSign");
    roundtrip::<AgentAnimation>("AgentAnimation");
    roundtrip::<RefreshEntitlements>("RefreshEntitlements");
    roundtrip::<PlayerToggleCrafterSlotRequest>("PlayerToggleCrafterSlotRequest");
    roundtrip::<SetPlayerInventoryOptions>("SetPlayerInventoryOptions");
    roundtrip::<SetHud>("SetHud");
    roundtrip::<AwardAchievement>("AwardAchievement");
    roundtrip::<ClientboundCloseForm>("ClientboundCloseForm");
    roundtrip::<ServerboundLoadingScreen>("ServerboundLoadingScreen");
    roundtrip::<JigsawStructureData>("JigsawStructureData");
    roundtrip::<CurrentStructureFeature>("CurrentStructureFeature");
    roundtrip::<ServerboundDiagnostics>("ServerboundDiagnostics");
    roundtrip::<CameraAimAssist>("CameraAimAssist");
    roundtrip::<ContainerRegistryCleanup>("ContainerRegistryCleanup");
    roundtrip::<MovementEffect>("MovementEffect");
    roundtrip::<CameraAimAssistPresets>("CameraAimAssistPresets");
    roundtrip::<ClientCameraAimAssist>("ClientCameraAimAssist");
    roundtrip::<ClientMovementPredictionSync>("ClientMovementPredictionSync");
    roundtrip::<UpdateClientOptions>("UpdateClientOptions");
    roundtrip::<PlayerVideoCapture>("PlayerVideoCapture");
    roundtrip::<PlayerUpdateEntityOverrides>("PlayerUpdateEntityOverrides");
    roundtrip::<PlayerLocation>("PlayerLocation");
    roundtrip::<ClientboundControlSchemeSet>("ClientboundControlSchemeSet");
    roundtrip::<PrimitiveShapes>("PrimitiveShapes");
    roundtrip::<ServerboundPackSettingChange>("ServerboundPackSettingChange");
    roundtrip::<ClientboundDataStore>("ClientboundDataStore");
    roundtrip::<GraphicsOverrideParameter>("GraphicsOverrideParameter");
    roundtrip::<ServerboundDataStore>("ServerboundDataStore");
    roundtrip::<ClientboundDataDrivenUIShowScreen>("ClientboundDataDrivenUIShowScreen");
    roundtrip::<ClientboundDataDrivenUICloseScreen>("ClientboundDataDrivenUICloseScreen");
    roundtrip::<ClientboundDataDrivenUIReload>("ClientboundDataDrivenUIReload");
    roundtrip::<ClientboundTextureShift>("ClientboundTextureShift");
    roundtrip::<VoxelShapes>("VoxelShapes");
    roundtrip::<CameraSpline>("CameraSpline");
    roundtrip::<CameraAimAssistActorPriority>("CameraAimAssistActorPriority");
    roundtrip::<ResourcePacksReadyForValidation>("ResourcePacksReadyForValidation");
    roundtrip::<LocatorBar>("LocatorBar");
    roundtrip::<PartyChanged>("PartyChanged");
    roundtrip::<ServerboundDataDrivenScreenClosed>("ServerboundDataDrivenScreenClosed");
    roundtrip::<SyncWorldClocks>("SyncWorldClocks");
    roundtrip::<ClientboundAttributeLayerSync>("ClientboundAttributeLayerSync");
    roundtrip::<ServerStoreInfo>("ServerStoreInfo");
    roundtrip::<ServerPresenceInfo>("ServerPresenceInfo");
    roundtrip::<ClientboundUpdateSoundData>("ClientboundUpdateSoundData");
    roundtrip::<SendPartyDestinationCookie>("SendPartyDestinationCookie");
    roundtrip::<PartyDestinationCookieResponse>("PartyDestinationCookieResponse");
    roundtrip::<SetPlayerFurnaceOptions>("SetPlayerFurnaceOptions");
    roundtrip::<RecordStarted>("RecordStarted");
}

/// The direction table must reject a packet from the peer that cannot send it,
/// on the id, before any field is read.
#[test]
fn every_packet_rejects_the_wrong_sender() {
    for &id in PacketId::ALL {
        let raw = id as u32;
        let wrong = match id.direction() {
            wire::Direction::Bidirectional => continue,
            wire::Direction::Clientbound => Peer::Client,
            wire::Direction::Serverbound => Peer::Server,
        };
        let mut reader = wire::Reader::new(&[]);
        assert!(
            matches!(
                Packet::decode_from(raw, wrong, &mut reader),
                Err(wire::DecodeError::UnexpectedDirection(_))
            ),
            "{id:?} accepted a packet from the wrong peer"
        );
    }
}

/// An id this manifest lacks keeps its exact body, and the next entry still
/// decodes from its own boundary.
#[test]
fn unknown_packets_keep_exact_boundaries() {
    let unknown = (0..=wire::PacketHeader::ID_MASK)
        .find(|id| PacketId::from_raw(*id).is_none())
        .expect("a free packet id");
    let mut batch = vec![wire::BATCH_HEADER];
    for body in [&[1u8, 2, 3][..], &[][..]] {
        wire::encode_frame_with(
            &mut batch,
            wire::PacketHeader::new(unknown),
            usize::MAX,
            |writer| writer.write_all(body),
        )
        .unwrap();
    }
    let limits = wire::Limits::unbounded();
    let decoded = wire::Frames::from_batch(batch.into(), 16)
        .unwrap()
        .map(|frame| Packet::decode_frame(&frame.unwrap(), Peer::Server, &limits).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(decoded.len(), 2);
    match &decoded[0] {
        Decoded::Unknown(packet) => {
            assert_eq!(packet.header.id, unknown);
            assert_eq!(packet.body.as_ref(), &[1, 2, 3]);
        }
        other => panic!("unknown id decoded as {other:?}"),
    }
    assert!(matches!(&decoded[1], Decoded::Unknown(packet) if packet.body.is_empty()));
}

/// Header bits above the target subclient are ignored on decode but must
/// survive forwarding an unknown entry.
#[test]
fn unknown_packets_forward_byte_for_byte() {
    let unknown = (0..=wire::PacketHeader::ID_MASK)
        .find(|id| PacketId::from_raw(*id).is_none())
        .expect("a free packet id");
    let mut header = Vec::new();
    wire::Writer::new(&mut header)
        .write_var_u32(unknown | (1 << 14))
        .unwrap();
    let mut entry = Vec::new();
    let mut writer = wire::Writer::new(&mut entry);
    writer.write_var_u32(header.len() as u32 + 1).unwrap();
    writer.write_all(&header).unwrap();
    writer.write_u8(0xab).unwrap();
    let frame = wire::Frames::new(entry.clone().into(), 1)
        .next()
        .unwrap()
        .unwrap();
    let decoded = Packet::decode_frame(&frame, Peer::Server, &wire::Limits::unbounded());
    let Ok(Decoded::Unknown(packet)) = &decoded else {
        panic!("unknown id decoded as {decoded:?}");
    };
    assert_eq!(packet.header.id, unknown);
    let mut out = Vec::new();
    packet.encode_frame(&mut out).unwrap();
    assert_eq!(out, entry);
}
