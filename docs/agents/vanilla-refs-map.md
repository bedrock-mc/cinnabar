# Vanilla reference map

Agent cross-reference index: for each file, the vanilla symbols and addresses its behaviour was matched against, removed from source comments by #126. Use it to locate the matching vanilla code; keep source comments free of these references. Entries go stale as code moves.


## app/src/block_entities/describe.rs
- // Current renderSkull selects the model from the backing block type;

## app/src/block_entities/system.rs
- // Current SkullBlockRenderer supplies BlockSource light at
- // the skull's BlockPos to mob_head's ordinary entity material.

## app/src/block_selection.rs
- /// StairBlock::getOutline deliberately returns a full

## app/src/camera/tests/projection.rs
- // Vanilla getFov uses normalized viewport fractions; bx::mtxProjRh

## app/src/environment.rs
- // Current native ClientLevel creates its clock module;
- // registerWorldClock initializes the
- // daylight clock to zero. StartGame current tick instead initializes
- // LevelData's elapsed tick counter. SetTime buffered
- // during loading is applied by onPlayerReady.

## app/src/environment/atmosphere.rs
- // Ordinary preRenderParameters supplies coefficient1; optional platform
- // Ordinary native renderer supplies flag1 to update.
- // buildImage uses it for both ambient stages around gamma;

## app/src/environment/renderer_clock.rs
- //! LevelRenderer's local tick counter.

## app/src/environment/seasonal_foliage.rs
- /// Bounded per-frame Weather::tick samples, previous/current rain in native order.

## app/src/environment/time_override/tests.rs
- // Current ordinary renderer supplies flag1 to LightTexture
- // update; buildImage applies both ambient stages.

## app/src/environment/weather.rs
- // Native Weather consumers interpolate the previous/current tick states;
- // only ClientLevel::_subTick explicitly requests alpha zero for its rate.
- /// Native Weather+0x4c, independent of the interpolated displayed rain.
- /// Native Weather+0x38, used without interpolation by the sky's rain admission.

## app/src/environment/weather_fog.rs
- //! Native WeatherRenderer fog accumulator.
- /// Called once per LevelRenderer tick, not once per rendered frame and not
- /// gated by doWeatherCycle. Native doRainUpdate reads Weather rain at alpha

## app/src/environment/world_clocks.rs
- //! WorldClock::tick and RegistryClient::tick respectively
- //! gate advancement on the clock's pause state and global doDaylightCycle.
- //! Level::getTime's lookup uses its canonical pre-registered hash
- // Legacy handler compares Level::getTime before calling Level::setTime; identical integer times are not re-anchored.
- // Native compares the integer WorldClock time, rather than resetting the

## app/src/item_use.rs
- //! Follows `ClientInputCallbacks::handleBuildAction`, `GameMode::baseUseItem`,
- //! `GameMode::releaseUsingItem` and `Player::completeUsingItem`; projectiles, food effects and
- //! ammunition stay server-owned.
- // Native CrossbowItem::getMaxUseDuration remains its charge duration
- /// `releaseUsing` checks the offhand for either projectile first, then inventory
- /// arrows, and synthesizes an arrow only in creative (09a157e0).

## app/src/menu/input.rs
- /// as vanilla's `TextEditComponent` shows its caret again after typing.
- // VanillaClientInputMappingFactory uses fixed F1/F8 shortcuts.

## app/src/particles/ambient.rs
- //! Other block animateTick callbacks and unclassified native materials remain unsupported.
- /// Vanilla LevelRenderer::tick calls animateTick once per

## app/src/particles/ambient/portal.rs
- // Native PortalAxis::X (1) selects the north/south effect. Unknown (0)

## app/src/particles/drive.rs
- // CommonGameModeMessenger emits local destruction before a server echo.

## app/src/runtime/visibility.rs
- /// Whether the culler hides the box from `low` to `high` in `dimension`: as vanilla's
- /// `isAABBVisible`, only when the cache matches graph `generation` and every sub-chunk the

## core/catalog/home.go
- Live events removed: `/api/v1.0/config/public` (GatheringServiceGetPublicGatheringsRequestHandler) exists only in the 26.30 Edu reconstruction; it is absent from the iOS 1.26.50.04 binary strings and the 1.26.50.26 Windows reconstruction, and the live service returns 404.

## core/catalog/profile_overview.go
- // Vanilla reference: OreUI J b2, Ik, Rk, xk (docs/profile-parity.md).

## core/proxy/resource_pack_admission.go
- ConnectStageRealm      ConnectStage = "realm"      // RealmsConnectProgressHandler: the Realm lookup
- ConnectStageConnecting ConnectStage = "connecting" // GameServerConnectProgressHandler
- ConnectStagePacks      ConnectStage = "packs"      // ResourcePackProgressHandler

## core/proxy/targets.go
- remoteServerNetwork / gophertunnel AddressNetwork: MinecraftGame::joinMultiplayerWithAddress (bool false via joinRemoteServerWithAddress; ConnectionType 1/2/8) -> ClientNetworkSystem::probeTransportLayer (URL list, port 0 => 19132 0x4abc) -> TransportProber::start (3 s "TransportProber::timeout") / _tryNextUrl (GET {url}/v1/join, Method variant index 2, 2xx) -> $_0 continuation: error => _joinMultiplayerAfterTransportLayerDetermined(..., 0 RakNet), success => host replaced by URL, TransportLayer 2 (NetherNet). 26.50 adds an https-only fast path and TofuServerIdentityVerifier for http:// results.
- HTTP signaling: ClientNetherNetConnector::connect (types 1/2/8 build HttpSignalingClient, remote id from HttpSignalingClientAnon::createRandomNetworkID), HttpSignalingClient::SendSignal (POST "{}/v1/join/{}", application/sdp, body = payload after 2nd space; response => "CONNECTRESPONSE <id> <body>", error => ESessionError 0x1a), NetherNet::HttpSignalingServer::onRequest/_handleJoin (GET /v1/join => 200 "OK" in 26.30, JSON status in 26.50; 400 "Missing SDP offer in request body").
- No fallback after selection: NetworkSystem::onOutgoingConnectionFailed only notifies; RemoteConnectorComposite::getActiveConnector picks NetherNetConnector iff session transport == 2.
- Transfers: WorldTransferInitiator::initiateTransferToServer builds ConnectionType 8 -> WorldTransferHandler::handleTransferToServer -> ClientInstance::startExternalNetworkWorld("transferServer"), the Play-screen external-server entry.
- Undecodable batches are dropped, not fatal (gophertunnel ErrBatchDropped): CompressedNetworkPeer::_receivePacket returns DataStatus 2 for an unknown header byte or a zlib/snappy failure; 26.50 FUN_1404b0b50 also returns 2 when the byte is neither 0xff nor the negotiated algorithm. NetworkSystem::runEvents::$_0 (26.30) and FUN_1418a075c's loop (26.50) treat any non-zero receivePacket status as "stop this connection for the tick", with no disconnect.
- Server trust (core/proxy/server_trust.go, gophertunnel FirstUseTrust, oreui modal::server_trust_modal): FUN_1408bf550 (probe continuation) builds TofuServerIdentityVerifier (FUN_141190820/FUN_141190a20) with a callback, capturing the probed URL (the modal's %1$s); the callback FUN_1408c07a0 trusts "https://" URLs at once and otherwise pushes ServerTrustModalScreenController (FUN_1408c0c20, modal FUN_145501670: permissions.servertrust.title/message/button.trust/button.doNotTrust). TofuServerIdentityVerifier::verify (FUN_141190bb0) gets the a=identity `assertion` (empty when absent: FUN_1418cf960 returns nullopt, so verify is false) and parses {"fingerprints","token"}, taking the key from the token's cpk; known keys hit a sorted set (FUN_1411eda10 equal_range) and move to the end of the LRU vector, persisted by FUN_141190050 as {"keys":[...]} under the static key "trusted_server_public_keys" (loader FUN_14118f120); FUN_14118f8e0 inserts and evicts past 0xc80 bytes (100 keys). Negotiator side: FUN_140e246d0 hands FUN_140e0fd40's parsed a=identity (or none) to the verifier; a false result logs "Rejecting answer from %s: application declined the server identity".
- No Minecraft-layer AES over NetherNet: EncryptedNetworkPeer::enableEncryption returns early when the inner peer isEncrypted() (WebRTCNetworkPeer::isEncrypted returns true).
- // the Login's multiplayer token and key as the SDP identity, as vanilla's MinecraftIdentityAssertion does.
- // transport accepts identityless answers like vanilla's ClientNegotiator::onRemoteAnswer, while

## core/store/client_test.go
- // Authored to the reference client's inventory parser; not a captured payload.

## crates/assets/src/biome.rs
- // whose constructor defaults surfaceOpacity to .65. Loading that component
- // replaces alpha with its surfaceOpacity.
- // Native LeavesBlock::getRenderLayer:
- // snow eligibility and palette blend strength are independent. The

## crates/assets/src/biome_noise.rs
- /// Converts the full unsigned word to a float as Core::Random::nextFloat does.

## crates/assets/src/block_names.rs
- /// Current SkullBlock identities mapped to the legacy `SkullType` ordinal.

## crates/assets/src/entity.rs
- /// Geometry::_parseBones and the geometry 1.21 schema agree.

## crates/assets/src/gui_item.rs
- /// Static shield ModelPart geometry through its own native GUI matrix, in design pixels.
- /// ModelPart box UV layout, or explicit authored face UV dimensions.

## crates/assets/src/model.rs
- /// SeasonsAgnosticLeaves uses the same cutout/deep group layout, without a

## crates/assets/src/registry.rs
- /// Native BlockReplaceableComponent admission used by the seasonal scan.

## crates/assets/src/seasonal_foliage.rs
- //! 1.26.50.26 `SeasonsRenderer` palette generation
- //! creates covered evergreen/birch/default columns, then their exposed
- /// LeavesBlock::getRenderLayer cold limit.
- /// Native ClientLeavesSeasonColorUtils
- /// skips the main block's air/leaves properties before canBeBuiltOver.
- // canBeBuiltOver predicate: an extra leaf still needs replacement.

## crates/assets/src/sound_events.rs
- // Native BlockGraphics loads textures and `sound` from the same

## crates/assets/src/texture/legacy_terrain.rs
- /// TextureAtlas::updateTextureAtUVs samples a
- /// `2^level` square, normalizes bytes, averages all four channels equally and

## crates/bridge/src/account.rs
- /// The four Xbox title statistics requested by vanilla's PlayerStatisticsFacet.

## crates/chunk-pipeline/src/stream/cohort.rs
- // ClientLoadingProgressTickingSystem::mChunksNeededForLoadOffsets covers nine columns.

## crates/chunk-pipeline/src/stream/dimension_transfer.rs
- // Current client offset initializer 02ddc3a0 copies these 57 eight-byte
- // ChunkPos entries into a 0x1c8-byte vector. Level slot 0x830 (01313e50)
- // returns this list independently of the server's simulation radius.

## crates/chunk-pipeline/src/stream/meshing/types.rs
- /// Vanilla `BlockSource` reads an absent chunk at the dimension's default brightness

## crates/chunk-pipeline/src/stream/residency.rs
- // longer covers (`NetworkChunkSubscriber::moveRegion`), so overlap stays presented.

## crates/chunk-pipeline/src/stream/seasonal_foliage.rs
- //! ClientLevel::_subTick season rows.
- /// SeasonsRenderer::tick refreshes its palette at tick 0 and every hundred ticks.

## crates/chunk-pipeline/tests/it/entity_runtime/pose_defaults.rs
- //! Native BoneOrientation defaults are part of Molang `this`, not animation deltas.

## crates/client-presentation/src/actor_publication/hand.rs
- // The native first-person ActorRenderer root retains its 1/128-model-unit lift
- /// The arm's state at `partial_tick` between the rig's last two ticks, as `renderFirstPerson`
- /// interpolates it: the swing wraps forward past its end, and an eat or drink use of

## crates/client-presentation/src/audio/predicted.rs
- /// Seconds between block hit sounds while mining (`GameMode` 200 ms).
- /// `GameMode` spaces mining hit sounds 200 ms apart.

## crates/client-presentation/src/camera.rs
- /// Native `getNormalizedViewportSize` measures viewport fractions of the full

## crates/client-presentation/src/camera/bob.rs
- //! Walk view-bob and first-person hand sway, expressed as view-space effects, following the
- //! 26.30 reference's bobView and hand spring.

## crates/client-presentation/src/presentation/equipment/display.rs
- /// `ItemInHandRenderer::_applyDefaultItemTransforms` for a flat sprite in hand: the 1.5 scale
- /// Camera-space placement of the first-person held item, from `renderFirstPerson`'s own item
- /// Items whose icon vanilla turns half a revolution in first person (`isMirroredArt`).

## crates/client-presentation/src/presentation/equipment/first_person.rs
- /// Native TextureTessellator pixel-to-model conversion (current PE VA14ffa90e0).
- /// Native renderOffhandItem, not the main-hand swing stack. Blocks use
- // TextureTessellator writes positive column X, depth Y,

## crates/client-presentation/src/presentation/equipment/runtime.rs
- /// by `renderFirstPerson`'s own transforms for the arm's `hand` state.

## crates/client-presentation/src/presentation/equipment/runtime/modern.rs
- /// Native setupAttachableNoChecks copies the parent's complete matrix before the held

## crates/client-ui/src/sound_requests.rs
- /// `min_seconds_between_plays` (`SoundComponent`).

## crates/client-ui/src/ui_runtime/forms/engine_input.rs
- // InputComponent sends pointer deltas to active components;
- // ScrollViewComponent consumes them while capture is still down.

## crates/client-ui/src/ui_runtime/gameplay_authority.rs
- /// Bedrock's GuiData tick notices slot changes even between identical items.

## crates/client-ui/src/ui_runtime/inventory_actions.rs
- /// slot clicked with nothing held, as `CrafterScreenController::handleEvent`.

## crates/client-ui/src/ui_runtime/item_facts.rs
- /// Whether a stack glints as `Item::isGlint` decides: an `ench` list, the item's glint
- /// The format code and colour `Item::getHoverTextColor` gives a component item's name: its

## crates/client-ui/src/ui_runtime/presentation/forms/container_kinds.rs
- // Native ChestContainerManagerModel::_postInit
- // uses the container helper's size, rather than requiring exactly 27 or 54.

## crates/client-ui/src/ui_runtime/presentation/forms/engine.rs
- // An unresolved texture draws `mce::TexturePtr`'s default white texture.

## crates/client-ui/src/ui_runtime/presentation/forms/engine/fill_renderers.rs
- //! (1.26.50 `ProgressBarRenderer`) and `gradient_renderer` (`GradientRenderer`).

## crates/client-ui/src/ui_runtime/presentation/forms/engine/menu_renderers.rs
- //! name tag, after `SplashTextRenderer`, `PaperDollRenderer` and `NameTagRenderer`.
- /// Name tag backing: `BaseActorRenderer::NAME_TAG_BACKGROUND_COLOR`, black at alpha 0.25.

## crates/client-ui/src/ui_runtime/presentation/forms/engine/text_paint.rs
- //! Label painting after vanilla's `TextComponent`: one layout per label with

## crates/client-ui/src/ui_runtime/presentation/forms/engine/tooltip.rs
- //! Native HoverTextRenderer geometry, painted through retained JSON-UI nodes.
- // BitmapFont::getWrapHeight returns default scale × 10.
- // Font::getLineLength rounds the widest line upward first.
- // Native drawCached receives false for its shadow/outline switches.

## crates/client-ui/src/ui_runtime/presentation/forms/hud.rs
- /// Java sidebar background opacities (`getBackgroundColor(0.3)` / `(0.4)`).

## crates/client-ui/src/ui_runtime/presentation/forms/join_progress.rs
- /// `SceneFactory::createNetworkProgressScreen`'s screen.
- /// `SceneFactory::createRealmNetworkProgressScreen`'s screen.
- /// `Util::getFilesizeString`: MB to two places under 1 MiB and one above, GB from 1 GiB.

## crates/client-ui/src/ui_runtime/presentation/forms/menu_caret.rs
- //! The launcher text boxes' caret, after vanilla's `TextEditComponent` as the

## crates/client-ui/src/ui_runtime/presentation/forms/menu_screens.rs
- /// Settings selector index vars as 1.26.50's `SettingsScreenController`
- /// assigns them.
- /// `StartMenuScreenController::addStaticScreenVars` for a full-game, non-edu
- /// The pause store button on a third-party server, as `PauseScreenController`
- /// names it: "%s Store" with the server's store name, else the generic "Server".
- /// The static vars `SettingsScreenController` sets for the global settings a

## crates/client-ui/src/ui_runtime/presentation/forms/recipe_book.rs
- /// `CraftingScreenController::addStaticScreenVars`: radio indexes of the tabs

## crates/client-ui/src/ui_runtime/presentation/forms/server_pack.rs
- /// not the pack also replaces the image (`UITextureInfo::_loadNineslice`).

## crates/client-ui/src/ui_runtime/presentation/forms/settings_storage.rs
- /// Uses the binary megabyte and gigabyte units identified in Util::getFilesizeString.

## crates/client-ui/src/ui_runtime/presentation/forms/settings_support.rs
- /// Populates FeedbackPromptController's three bindings on the actual rating prompt.

## crates/client-ui/src/ui_runtime/presentation/forms/sign_editor.rs
- // `SignScreenController::addStaticScreenVars`: the wood's art and edit box.

## crates/client-ui/src/ui_runtime/presentation/forms/toast_screen.rs
- // `ToastScreenController::addStaticScreenVars`.

## crates/client-ui/src/ui_runtime/presentation/item_gui/shield.rs
- //! The shield GUI ModelPart path, not the first-person attachable animation.

## crates/client-ui/src/ui_runtime/presentation/paper_doll.rs
- //! HUD visibility follows HudPlayerRenderer::update.

## crates/client-ui/src/ui_runtime/presentation/player_preview.rs
- /// UI rendering retains the native ModelPart origin rather than the world feet origin.
- /// renderer's centre minus the pointer in GUI pixels
- /// (`LivePlayerRenderer::_getMousePosition`).
- /// follows `LivePlayerRenderer::render`: body `atan(dx / 40) * 20`, head
- /// renderer (`LivePlayerRenderer::render`) centres the eyes on the control at
- /// (`PaperDollRenderer::_render`) centres the model at `min(w / 20, h / 39)`
- // PaperDollRenderer sets variable.is_paperdoll=1. The vanilla player

## crates/client-ui/src/ui_runtime/presentation/player_preview/equipment.rs
- /// `setupAttachableNoChecks` preserves expression-bound ModelPart defaults:

## crates/client-ui/src/ui_runtime/presentation/publish/item_icons.rs
- //! Stack-aware icons: native CrossbowItem::getAnimationFrame feeds
- //! getIcon, whose nonzero frame N selects crossbow_pulling variant N-1.

## crates/client-ui/src/ui_runtime/presentation/session_icons.rs
- /// Native CrossbowItem::getIcon routes nonzero animation frames to the pulling

## crates/client-ui/src/ui_runtime/scene_stack.rs
- /// The world and its in-world overlays: the native `InGamePlayScreen`.
- /// `InGamePlayScreen`'s overrides of `BaseScreen`: it passes input through (the
- /// Whether the top scene captures the mouse (`currentScreenShouldStealMouse`);

## crates/client-ui/src/ui_runtime/screen_state.rs
- /// How long local toggles override the block entity, as `CrafterScreenController::tick`.

## crates/client-world/src/actor_animation/attachable.rs
- /// Native setupAttachableNoChecks distinguishes expression
- /// bindings from owner-name matches. Only the latter clear the authored default TRS;
- /// applyAnimations restores the former's ModelPart defaults afterward.

## crates/client-world/src/actor_animation/geometry.rs
- // ItemInHandRenderer's constructor starts both offhand observations at zero.

## crates/client-world/src/actor_animation/hud.rs
- /// Vanilla selects that separate component in Actor; the HUD forces
- /// third person before drawing the same actor in HudPlayerRenderer.

## crates/client-world/src/actor_animation/motion.rs
- // FishAnimationSystem tick consumes StateVector velocity in blocks/tick.
- /// FishAnimationComponent survives geometry/controller resets for this actor lifetime.

## crates/client-world/src/actor_animation/pose.rs
- // ModelPart loader uses 24, then the model
- // constructor negates native Y into BoneOrientation default position.
- // `this` reads BoneOrientation, not an animation-only delta. ModelPart's
- // defaults are copied into that orientation before channels add their values.
- // ModelPart uses an authored X/Z frame and a 24-pixel Y origin. A
- // BoneOrientation negates ModelPart's Y before exposing it to Molang.
- // Owner-name binding clears defaults; an explicit expression keeps ModelPart defaults.

## crates/client-world/src/actor_animation/tick.rs
- // Native ItemInHandRenderer::tick: ±0.4 clamp and cached
- // a mob: Actor::getInterpolatedBodyYaw returns 0, while
- // The native updater publishes FishAnimationComponent before pack scripts.

## crates/client-world/src/actor_store.rs
- /// Native StateVector units for tick-driven engine animation components.

## crates/client-world/src/actor_store/dropped.rs
- // ItemRenderer::render, current 1.26.50.26. The random phase belongs to
- // Native ActorRenderDispatcher uses StateVector origin,

## crates/client-world/src/actor_store/hurt.rs
- /// Native StateVector displacement per tick, distinct from query-derived movement speed.
- /// The current hurt came without damage, so it shows no red flash (`SkipRedFlashComponent`).
- // Native Actor::baseTick decrements only positive
- // Actor::handleEntityEvent, case 0x27.

## crates/client-world/src/actor_store/lifecycle.rs
- // `Player::handleMovePlayerPacket`: Reset sets the position directly and

## crates/client-world/src/actor_store/lifecycle/interpolation.rs
- // Native MovementInterpolator tick clears StateVector velocity

## crates/client-world/src/authority/contracts.rs
- /// Total/current factor for the identified native sprint modifier.

## crates/client-world/src/game_mode_capabilities.rs
- // Retained wire evidence, not a mining gate. Native instant destruction uses
- // Actor::isCreative, not the Instabuild ability (see game-mode-updates.md).
- // `Player::_setPlayerGameType` gives any id but survival the base GameMode, which

## crates/gameplay/src/block_use.rs
- /// Trapdoors and levers flip `open_bit` (`TrapDoorBlock::_useTrapDoor`); an

## crates/gameplay/src/item_use.rs
- /// `handleBuildAction` re-arms the next build action this long after an air use.
- /// `TypedClientNetId<ItemStackLegacyRequestIdTag>`'s process-wide counter.
- // Switching away stops the use without a release, as `Player::stopUsingItem`.
- // `completeUsingItem` finishes locally, without a release transaction.
- // CrossbowItem stores its loaded projectile for the next press's pose/action.
- // `baseUseItem` opens a legacy request scope on every air use.
- /// `TypedClientNetId::_generateNext`: even ids from -4 downward, restarting past the range.

## crates/gameplay/src/item_use/classify.rs
- /// `BowItem`/`TridentItem::getMaxUseDuration`.
- /// `CrossbowItem::getMaxUseDuration`: 25 ticks less 5 per Quick Charge level.
- /// Drink duration of `PotionItem` (and `OminousBottleItem`) and the milk `BucketItem`.
- /// `ItemUseSlowdownSystemImpl`'s factor for a use without `minecraft:use_modifiers`.
- /// `EnderpearlItem::getCooldownDuration`.
- /// Food points below full, as `FoodItemComponent::use` requires.
- /// A use's shared cooldown, as `Player::startItemCooldown` records it.
- /// Whether a vanilla hold use is eaten or drunk (`UseAnimation::Eat`/`Drink`), which the

## crates/gameplay/src/item_use/tests.rs
- /// A depleted use completes locally: the client sends nothing (`Player::completeUsingItem`).
- /// Server-owned lobby items still use `baseUseItem`, without a locally predicted hold.
- /// `TypedClientNetId::_generateNext` restarts at -4 once the counter leaves the negative range.

## crates/gameplay/src/movement.rs
- // LocalPlayer::sendInput copies end-of-tick StateVector motion.

## crates/gameplay/src/movement/collision_registries/connected.rs
- //! Connection-state collision boxes from FenceBlock and ThinFenceBlock.

## crates/gameplay/src/movement/collision_registries/flow.rs
- //! Native material/flow cache bindings; geometry flags do not establish them.
- // Material::_setupMaterials current 0x0379bed0: types 0 / 5 / 6.
- // DirtBlock 0x0a7b9820 / GrassBlockBase 0x0712ab00 use type 1.
- // IceBlock current 0x071305c0 chooses types 13 / 23 (both solid).
- // Current StoneBlock 0x0a5b7aa0 / SandBlock 0x08efbad0 use type 23;
- // recovered registerBlock wrappers are 0x0dfb6010 / 0x0dfb97a0.
- // GravelBlock 0x0712be60 also uses type 23; its vtable 0x1502a5290
- // selects the native falling_dust_gravel_particle producer.
- // These identified native classes use the default liquid detection
- // cache (mask 0) and BlockType directional virtual (always true).

## crates/gameplay/src/movement/collision_registries/selection.rs
- /// Visual bounds for plants; `BlockType::clip` picks these independently
- // TopSnowBlock::getVisualShape: full X/Z,
- // DeadBushBlock constructor overrides inherited
- // flower bounds with grass-sized bounds, including maxY=.8.
- // BushBlock uses
- // minXYZ=(0,0,0), maxX=1; ctor literals set maxY=.8 and maxZ=1.
- /// TorchBlock chooses the visual box by `torch_facing_direction`, independently

## crates/gameplay/src/movement/control_modes.rs
- /// Native SprintTrigger cannot stop an existing sprint while the previous

## crates/gameplay/src/movement/correction_shape.rs
- /// motion as already matching the retained frame (`getAdvanceFrameResult`).
- /// `MovePlayer` (`_onPlayerMovePacketReceived`, 16.0 read from the 26.30 client).
- /// replays from it with motion cleared as `MovePlayerInput` does; anything
- /// Server StateVector motion; `None` keeps the retained velocity.

## crates/gameplay/src/movement/encoding.rs
- // Raw jump-button carriers track the physical button exactly. Native
- // 0x07108cc0 also sets processed up; 0x070fcfd0 sends it as WantUp,
- // which the server's 0x0998fe80 reads independently of JumpDown.

## crates/gameplay/src/movement/integration_tests/simulation.rs
- // Ground friction distinguishes StateVector motion from displacement.

## crates/gameplay/src/movement/locomotion.rs
- // SprintTrigger runs before SwimTrigger and keeps the previous actor

## crates/gameplay/src/movement/locomotion/swimming_trigger.rs
- //! Current SwimTriggerSystem (0x09fd25a0), for unmounted desktop input.
- // PE VAs 0x14feff2ac, 0x14ffab6c8, 0x15013adc8 and 0x14ffd5070.

## crates/gameplay/src/movement/locomotion_tests.rs
- // Current SendPlayerInputPacket 0x070fcfd0 emits processed up/down as
- // WantUp/WantDown. ServerMoveInputHandler 0x0998fe80 reconstructs them
- // directly; raw JumpDown/Ascend alone do not populate these control lanes.
- // CurrentSwimAmount precedes SwimTrigger. The first dry tick still advances

## crates/gameplay/src/movement/physics.rs
- /// End-of-tick StateVector motion sent as PlayerAuthInput.PosDelta.

## crates/gameplay/src/movement/physics/correction.rs
- // MovePlayer changes spatial state without resetting jump input or
- // movement abilities (native MovePlayerInput RVAs 04b046c0/04b047e0).
- // Vanilla's correction input writes both position and StateVector
- // motion into the corrected frame before replaying later inputs.

## crates/gameplay/src/movement/physics/eye.rs
- // VanillaOffsetSystem dispatcher selects the
- // current-game-version 0x3eb33333 drop. The older-version branch is not our target.

## crates/gameplay/src/movement/physics/sprint_retention.rs
- /// Native SprintTrigger skips its stop action while the preceding swimming
- /// flag and this tick's body-water sensing are set (current RVA 0x0c5b6310).

## crates/gameplay/src/movement/physics/timeline.rs
- /// frame as `ReplayStateComponent::applyFrameCorrection` does. Non-finite
- /// frame as `applyFrameCorrection` does. Modes are only ended, never

## crates/gameplay/src/movement/speed_authority.rs
- /// Native LocalPlayer::setSprinting is edge-triggered; Mob adds/removes only
- /// its identified modifier. An attribute packet replaces that modifier set.

## crates/gameplay/src/movement/teleport_ack.rs
- //! Vanilla Player::handleMovePlayerPacket, mode 2,
- //! sets the action that setFromComponent maps to HandledTeleport.

## crates/gameplay/src/survival_mining.rs
- /// above it, it destroys once per block travelled (`GameMode::continueDestroyBlock`).
- // stopDestroyBlock clears the destroy delay.
- /// `ItemStackBase::hurtAndBreak` keeps damage below `Item::getDamageChance`.

## crates/gameplay/src/survival_mining/tests.rs
- /// Only zero hardness breaks on the start tick (`GameMode::startDestroyBlock`);
- /// stopDestroyBlock clears the delay, so a fresh press starts at once.

## crates/inventory/src/inventory_ledger/admission.rs
- // Native LegacyClientNetworkHandler::handle routes a response to the screen manager

## crates/inventory/src/inventory_ledger/crafting.rs
- // Native _makeCreateItemScopeCreative

## crates/inventory/src/inventory_ledger/crafting_close.rs
- //! ContainerManagerController::_closeContainers invokes
- //! _returnToPlayerOrDrop for every return-on-close input and cursor.

## crates/inventory/src/inventory_ledger/crafting_tests.rs
- /// Native _makeCreateItemScopeCreative declares

## crates/inventory/src/inventory_ledger/distribute/live.rs
- //! Incremental splitting, following ContainerManagerController::_handleSplitMultiple;

## crates/inventory/src/inventory_ledger/queue.rs
- // Native tryPushSlotPrediction returns MissingPrediction,
- // not HistoricPrediction, after a newer owner was removed.
- // Native tryPushSlotPrediction and the historic path

## crates/inventory/src/inventory_ledger/registry.rs
- // Native ItemStackBase::matchesItem compares block/aux identity; a

## crates/inventory/src/item_icon.rs
- /// CrossbowItem::getAnimationFrame, including loaded projectile art.

## crates/json-ui/src/anim/def.rs
- //! Animation definitions as `UIAnimationComponent::_createAnimation` reads them:

## crates/json-ui/src/anim/ease.rs
- //! The 32 easing curves of 1.26.50's `Easing` table (`mce::Math::ease*`), in the
- /// An easing curve, in the client's `EasingType` order.

## crates/json-ui/src/anim/paint.rs
- /// `UIAsepriteFlipbook::tick`: the frame whose span holds `ms` into the loop.

## crates/json-ui/src/anim/runtime.rs
- //! `UIAnimationComponent::_animationTick` does and keeping the values they write.
- /// Resources are ready at the first paint (`onResourcesLoaded`).
- /// `UIAnimFlipbook::tick`: at most one frame per tick, the remainder kept.

## crates/json-ui/src/bind.rs
- //! Data binding as the client's `DataBindingComponent` runs it: each control's
- // `SliderComponent::_createSteps` through the slider's own factory.

## crates/json-ui/src/bind/apply.rs
- //! `DataBindingComponent::_bind` for one control: each binding runs when its
- /// An edit box's content binding seeds the text vanilla's TextEditComponent then owns;

## crates/json-ui/src/bind/bag.rs
- //! A control's property bags at creation, as `UIControl::processPropertyBags`
- //! builds them: its own `property_bag` and `property_bag_for_children`, each
- /// A bag literal's members, each through `UIResolvedDef::_evaluate`: a

## crates/json-ui/src/bind/data.rs
- /// binds, as `ScreenController::bindGridSize` does: a `[columns, rows]` array.

## crates/json-ui/src/bind/native.rs
- //! `DataBindingComponent::_updateCustomComponentsPostBinding`: after a binding
- /// `_getDesiredValue<bool>`: only a JSON bool, else `default`.
- /// `_getDesiredValue<float>`: any number or bool, else `default`.
- /// `_getDesiredValue<int>`: only an integral JSON number, else `default`.
- // jsoncpp's `isInt` also takes an integral real in range.
- /// `Json::Value::asInt`: integers truncate to 32 bits, reals cast, bools 0/1.
- /// `_getDesiredValue<std::string>`: only a JSON string, else `default`.
- /// `PropertyBag::get<T>` on the bag itself: a present value of the right type.

## crates/json-ui/src/bind/spec.rs
- //! A control's `bindings` array parsed as `UIControlFactory::
- //! _populateDataBindingComponent` reads it: the binding type and condition, the
- // (`ScreenController::bindGridSize`) every refresh.
- /// `UIResolvedDef::getAsString`: the field through `_evaluate`, so a constant
- /// `UIResolvedDef::getAsBindingType`: absent is global, `none` binds nothing,
- /// `UIResolvedDef::getAsBindingCondition`: an unknown name logs and reads `none`.
- /// `UIResolvedDef::getAsPropetyEvaluation`: a `#name` is one property; a

## crates/json-ui/src/component.rs
- /// `isInteracted`: the press edge for pointer/gamepad, the release for touch.

## crates/json-ui/src/component/dispatch.rs
- //! Raw input through button mappings to components, as
- //! `InputComponent::handleButtonEvent` / `handlePointerLocation` do: controls
- /// Last button state per (control key, mapping index): `lastButtonState`.
- // `isInteracted`: the press edge, or the release on touch.
- // A button losing its hover lets go of its press (`ButtonComponent::receive`).
- /// `_shouldHandlePressedMapping` and the focused/global rules.
- // `GestureComponent`: its button's hold tracks motion; the release zeroes the deltas.
- /// `SliderComponent::receive` for a button event.
- // `_sendHoverScreenEvent` raises a hover mapping in the Up state: never a press.
- /// `_setChecked` plus the radio group's `_updateToggleGroupState`.
- /// The control's index in its named collection (`UIControl::findCollectionIndex`):

## crates/json-ui/src/component/edit.rs
- //! The text edit component (`TextEditComponent`) of an `edit_box`: what the
- //! factory reads, the retained text/caret state, and the character rules of
- //! `handleTextCharEvent` and `_textFitsInControl`.
- /// Seconds between caret blinks (`updateCaretBlink`).
- // `asInt` admits any integral number, else reads zero.
- /// Apply typed `input` (`handleTextCharEvent`): Enter without newlines ends

## crates/json-ui/src/component/selection_wheel.rs
- //! `SelectionWheelComponent`: pointer sectors and component-managed state children.
- //! Lens 26.30 `receive` (0x1024ea3d0), constructor (0x1024e9af0) and
- //! `_updateControlVisibility` (0x1024e9f30), corroborated by the pinned UI definitions.

## crates/json-ui/src/component/slider.rs
- //! The slider component (`SliderComponent`) as the 1.26.50 factory reads it,
- //! and its value arithmetic (`_updateSliderFromPosition`,
- //! `_updateSliderFromStepSize`).
- /// The value after `direction` small steps (`_updateSliderFromStepSize`).
- /// The step marks a step slider's component creates through its factory
- /// (`SliderComponent::_createSteps`): one per inner step, spaced across the

## crates/json-ui/src/component/sound.rs
- //! The sound component (`UIControlFactory::_populateSoundComponent`,
- //! `SoundComponent::receive`): a shorthand sound for every interacted button

## crates/json-ui/src/component/toggle.rs
- //! The toggle component (`ToggleComponent`) and the toggle manager
- //! (`ToggleManagerComponent`), as the 1.26.50 factory reads them.
- /// What a toggle does with a button event reaching it, as
- /// `ToggleComponent::receive`: the new state and whether a click set it, or

## crates/json-ui/src/env.rs
- //! Variable scopes and value evaluation as the vanilla client performs them
- //! (`UIEval::evalVariable`, `UIResolvedDef::_evaluate`). Each control pushes a
- /// `UIResolvedDef::_evaluate`: a string starting with `$` reads that variable

## crates/json-ui/src/expr.rs
- //! the client's `parseLayoutAxis` does: a lower-cased token stream of numbers,

## crates/json-ui/src/hud.rs
- /// A `#rrggbb` tint as the `[r, g, b, a]` array `bindColor` answers; other

## crates/json-ui/src/input.rs
- /// A control's press sound (`SoundComponent`): its `sound_name`, else the first

## crates/json-ui/src/input/focus.rs
- //! A control's focus component (`UIControlFactory::_populateFocusComponent`) and
- //! the focus containers enclosing it (`_populateFocusContainerComponent`).
- // A non-integer precedence reads as zero, as `Json::Value::isInt` gates it.

## crates/json-ui/src/input/mapping.rs
- //! A control's input component as the 1.26.50 factory builds it
- //! (`UIControlFactory::_populateInputComponent`): its button mappings, the

## crates/json-ui/src/input/navigate.rs
- //! Keyboard/gamepad focus movement over hit regions, after the 1.26.50
- //! `FocusManager`: default focus by precedence, identifier overrides, the
- //! directional sweep (`_sweepForControlDirectional`), scroll sections, and
- //! focus-container rules (`_handleFocusContainerLogic`).
- /// How far into its own edge the sweep starts (`_sweepToNextFocusObject`).
- /// `_sweepForControlDirectional`: the nearest candidate ahead of `current`'s

## crates/json-ui/src/label.rs
- //! A `label`'s text component, as vanilla's `TextComponent` reads it: glyph

## crates/json-ui/src/layout/scroll.rs
- //! Scroll views as the client's `ScrollViewComponent` lays them out: the named
- // `_updateScroll` scrolls only with all four references resolved, along

## crates/json-ui/src/layout/size.rs
- //! clamped before anything reads them, as the client's `LayoutVariable::satisfy`
- //! does.
- // Native `LayoutVariable::isSatisfiable` (26.30, 0x1027946f0) includes
- // min/max dependencies before `satisfy` clamps the ordinary size rule.

## crates/json-ui/src/lib.rs
- // `SceneFactory::_createSafeZoneSizeVar` at the desktop defaults (safe
- // zone 1, screen position 0) sizes every buffer zero along its axis.

## crates/json-ui/src/localize.rs
- //! Label localization as the client's `Localization::_get` applies it: text

## crates/json-ui/src/pack.rs
- /// jsoncpp's `asString`: text as is, bools and numbers spelled out, null empty.
- /// `UIModification::_findIndex` over the original elements: a name matches an

## crates/json-ui/src/predicate.rs
- // `getPropertyValue` without a bag yields the name as text.

## crates/json-ui/src/predicate/ops.rs
- //! `UiExpression::evaluate`: a value/operator stack where prefix `+`, `-` and

## crates/json-ui/src/predicate/tests.rs
- // An unbound `$var` is null as in `UIEval::evalVariable`; with no bag a `#name`

## crates/json-ui/src/predicate/token.rs
- //! Expression source to tokens, as `UIEval::evalExpression` splits it and
- //! `ExprToken::_parseToken` types each piece.
- /// Operator codes, as the client's `OperatorType` numbers them.
- /// `Json::Value::asBool`: nonzero, nonempty, or true.
- /// `Json::Value::asInt`: strings and compound values read 0.
- /// `Json::Value::asFloat`: strings and compound values read 0.
- /// `createTokenFromUIDefVal`: the token a JSON value becomes.
- /// `ExprToken::_parseToken`: keyword, quoted string, property, int, float,
- /// `Util::toBool`, case-insensitive: `true`/`false`, `yes`/`no`, `1`/`0`.
- /// `ExprToken::createStringToken` for an operator's text result: reparsed as a

## crates/json-ui/src/scene.rs
- /// A screen root's settings. Defaults are the parser's (`getAsBool` fallbacks).
- /// Read by `UIScene::ignoreAsTop`: the scene below still counts as topmost.
- /// bool (an unbound `$var`) keeps the default, as `getAsBool` does for null.
- /// Scenes in paint order (`forEachVisibleScreen`): from the highest scene that
- /// `ClientInstance::currentScreenShouldStealMouse`.
- /// `ClientInstance::isShowingMenu`.
- /// history alone when it holds none (`popScreensBackToFirstInstanceOf`).

## crates/json-ui/src/sprite.rs
- //! The `image` control's sprite, following vanilla's `SpriteComponent` draw
- //! dispatch: nine-slice first, then a clipped, tiled, filled (cover), kept-ratio

## crates/json-ui/src/widgets.rs
- /// its content sibling's name (`DropdownComponent`).
- /// The content's top as `DropdownComponent::_positionContent` places it:

## crates/json-ui/src/widgets/scroll_motion.rs
- //! Touch scrolling as 1.26.50 `ScrollViewComponent::_updateDynamicsAndScrollPosition`
- //! runs it: a held finger pulls the offset on a spring, a release flings it

## crates/json-ui/src/widgets/states.rs
- //! Which state controls a stateful control shows, as the components'
- //! `_updateControlVisibility` write them: each named target is the first
- /// `ToggleComponent`'s targets by `checked + 4·hover + 8·locked`; unused slots are empty.

## crates/json-ui/tests/it/bind_native.rs
- //! typed readers (`_getDesiredValue`), one case per post-binding target.
- // jsoncpp's `isInt` takes an integral real, as a controller count is.

## crates/json-ui/tests/it/bind_parity.rs
- //! client's `DataBindingComponent`, one case per audited behaviour.
- // B37: `ignoreCollectionItem` keeps a child out of the collection.
- // the text vanilla's TextEditComponent keeps, so typing shows as it happens. Vanilla's

## crates/json-ui/tests/it/layout_parity.rs
- // V08: a horizontally draggable box scrolls the content along x (`_updateScroll`

## crates/json-ui/tests/it/resolution.rs
- // token parses as an int from its leading digits (`Util::toNumber<int>`), so
- // An item index counts only under the collection's own panel; `ignoreCollectionItem` opts out.

## crates/json-ui/tests/it/tooltip.rs
- //! HoverTextRenderer receives its authored maximum width, not just # bindings.

## crates/json-ui/tests/scroll.rs
- //! Scroll views as vanilla's ScrollViewComponent runs them: the named viewport,

## crates/launcher/src/menu/profile.rs
- /// Bedrock truncates the service's minute value before passing it to DateHelper.

## crates/launcher/src/menu/settings_options/chat.rs
- /// Mirrors ChatUtils::canLanguageBeSmooth's four unsupported locales.
- /// Applies ChatUtils' one-decimal padding plus the source's nonzero epsilon.

## crates/launcher/src/menu/settings_options/control_bindings.rs
- // R:v/VanillaClientInputMappingFactory.cpp: key.emote defaults to B.
- // R:26.30 createInputMappingTemplates action0x34 uses native button7;
- // GamePadRemappingLayout's native sprite/name map identifies it as D-pad left.

## crates/launcher/src/menu/settings_options/definitions.rs
- // current OptionRegistry values are recovered; see plan.md.

## crates/launcher/src/menu/settings_options/emotes.rs
- /// R: native EmoteWheelScreenController equipped top/right/bottom/left slots.

## crates/launcher/src/menu/settings_options/keybindings.rs
- // KeyboardRemappingLayout replaces the list with one captured key.

## crates/launcher/src/menu/settings_support.rs
- /// Fixed destinations from general_section.json and AppPlatform::getFeedbackHelpLink.

## crates/meshing/src/chunk/cube_materials.rs
- /// GrassBlock::calcVariant samples only the block directly above:

## crates/meshing/src/chunk/leaves.rs
- //! LeavesBlock::_isDeep tests six neighbours;
- //! BlockOccluder::_updateRenderFace retains only one of the
- // Both native leaf types use this predicate; only ordinary LeavesBlock
- // adds the seasonal colour material flag.

## crates/meshing/src/chunk/seasonal_foliage.rs
- //! ClientLeavesSeasonColorUtils scans upward
- //! to the height map, skipping air/leaves and native exempt blocks. TopSnow

## crates/meshing/src/cloud.rs
- /// Current native TextureTessellator colour bake, before conversion

## crates/meshing/src/cloud_viewport.rs
- //! Current 1.26.50.26 tickClouds rebuilds after fifteen blocks of
- //! floor(samplePosition)>>4, and TextureTessellator emits unit caps

## crates/meshing/src/lighting.rs
- // BlockType::getShadeBrightness:
- // property 0x20 and Block+0x71, independently of Block+0xa3.
- // y+1 plane, rounding each channel. Admission uses BlockType+0x15c > 0.5,
- // AmbientOcclusionCalculator::calculateWithCache
- // averages four independent 0.2/1 shade samples. Its diagonal fallback
- /// PortalBlock takes the native flat path: boundary-adjacent light, own light

## crates/meshing/src/lighting/native_liquid.rs
- // independently of AmbientOcclusionCalculator's terrain sampling.

## crates/meshing/tests/it/mesh/snow_covered.rs
- /// block's own pass (tessellateTopSnowInWorld). A crossed

## crates/meshing/tests/it/support/liquid_contacts.rs
- // BlockType with Air, not its opacity. Deferred model 1 instead compares material;

## crates/pack-compiler/src/animation.rs
- /// Overlay-mask sources (grass sides) use the TextureAtlas::updateTextureAtUVs /
- /// _buildAtlasMips byte-space box mips, as every vanilla atlas tile does.

## crates/pack-compiler/src/compiler/lily_pad_textures.rs
- // TextureAtlas::updateTextureAtUVs multiplies RGB only.

## crates/pack-compiler/src/compiler/seasonal_leaves.rs
- // LeavesBlock::getRenderLayer chooses an
- // opaque deep material without changing getVariant's fancy texture.
- // SeasonsAgnosticLeavesBlock::getRenderLayer
- // also chooses layer5/7 by depth, but never seasonal layer9/10.
- // Exact current BlockReplaceableDescription registration witnesses:

## crates/pack-compiler/src/compiler/visuals/cross.rs
- /// Native row tessellation (MCSRC 06a21df0 / 06a98c10): four full-width

## crates/pack-compiler/src/compiler/visuals/portal_tests.rs
- // Current final PortalBlock registration supplies light, without opacity.

## crates/pack-compiler/src/compiler/visuals/signs.rs
- // Classic vanilla's SignModel declares a 24x12 board and its

## crates/pack-compiler/src/compiler/visuals/snowy_grass.rs
- // by GrassBlock::calcVariant.

## crates/pack-compiler/src/entity/animation/roots.rs
- // ActorResourceDefinitionGroup::upgrade_v1_8_to_v1_10
- // moves legacy controllers into a distinct animation alias before appending activation roots.

## crates/pack-compiler/src/entity/item.rs
- // Vanilla draws an item as its block only when it is that block's own BlockItem: an item
- /// torchflower) are that block's `BlockItem`, drawn from its first canonical state.

## crates/pack-compiler/src/entity/item/spawn_eggs.rs
- //! ActorPlacerItem resolves icons by actor identifier, not item-atlas key spelling.
- //! Current 26.50.26 getIconInfo reads the actor icon map. The
- //! ActorResourceDefinitionGroup loader reads description.spawn_egg
- //! texture/texture_index; ActorPlacerItem::isValidAuxValue accepts only zero.

## crates/pack-compiler/src/entity/item_bindings.rs
- //! Current Item::initClient reads components.minecraft:icon;

## crates/pack-compiler/src/entity/legacy_icons.rs
- //! Legacy vanilla icon routes: the atlas key and variant the retail client's
- //! `VanillaItems::initClientData` assigns to items without an icon component,
- //! with potion icons keyed by aux. Each row cites its call site in the 26.30
- //! client; keys absent from the pinned atlas are skipped, never invented.

## crates/pack-compiler/src/entity/native_bind_pose.rs
- // llama.geo.json has the same bind. Native GeometryGroup keeps same-identifier history;
- // Geometry::_parseBones reads missing bind fields through
- // JsonValueHierarchy::get, retaining the older shipped bind under the modern sample.

## crates/pack-compiler/src/entity/native_dragon_geometry.rs
- // These parts use parent-relative pivots with ModelPart's Y origin.

## crates/pack-compiler/src/icon/blocks.rs
- //! Vanilla's item renderer draws a block item flat when `BlockTessellator::canRender` rejects
- //! its shape; `BlockItem::getIconInfo` then shows the carried texture, down face, at the
- // The world key's variant stands in for the block's `getVariant`.

## crates/pack-compiler/src/icon/carried.rs
- // Matched 26.50 TextureJSONParser keeps the low RGB bytes
- // of the hexadecimal value and forces opacity to one. The matching
- // TextureAtlas::updateTextureAtUVs mixes original/tinted RGB

## crates/pack-compiler/src/icon/shield.rs
- //! ShieldRenderer GUI branch, not the first-person attachable animation or a flat UV sheet.
- // The retail ShieldModel loads its named root. Exotic animated/inherited model-part trees

## crates/pack-compiler/src/pack/block.rs
- /// The texture key vanilla's `BlockItem` icon reads: `carried_textures`, else `textures`,

## crates/pack-compiler/src/pack/fixed_tint.rs
- // TextureJSONParser delegates to colour parsing:

## crates/pack-compiler/tests/it/shield_icon.rs
- // A different legacy atlas image must never override the native ShieldModel branch.

## crates/pack-compiler/tests/it/water_appearance.rs
- //! WaterRenderAttributes retains its alpha when a biome only replaces RGB.
- // Current getWaterColor reads the default RGBA.

## crates/particles/src/ambient.rs
- /// Material::_setupMaterials: air(0) and plant(8)
- /// are neither solid nor liquid. TallGrass, Flower,

## crates/particles/src/ambient/fire_tests.rs
- // Independently rounded witnesses for native Random::nextFloat's double

## crates/particles/src/ambient/random.rs
- /// Current Random::nextFloat converts one unsigned MT word to double,

## crates/particles/src/ambient/sampler.rs
- // Target-version LevelRendererPlayer constructor starts
- // mode 2 at 100 samples with its previous camera position zeroed.

## crates/particles/src/emitter/manual.rs
- /// `LevelRendererPlayer::addBiomeTintedParticleEffect`
- /// keeps one emitter per colour and calls its manual emission method for every origin.

## crates/particles/src/system.rs
- /// `_addTerrainEffect` checks the selected effect's

## crates/particles/src/system/biome_tinted.rs
- /// Emits one biome-tinted particle at a block center. Vanilla 26.50's
- /// `LevelRendererPlayer::addBiomeTintedParticleEffect` caches

## crates/particles/src/tiles.rs
- /// `BlockDestructionParticlesComponent::getTextureInfo`
- /// resolves `down`, then `*`; the built-in texture fallback uses

## crates/particles/src/tiles/tint_tests.rs
- // SeasonsRenderer::getTintedColor clamps doubled palette RGB, whereas
- // RenderChunk's seasonal shader multiplies it into the texture unclamped.

## crates/particles/src/triggers.rs
- /// Default destruction count from vanilla BlockDestructionParticlesComponent.

## crates/protocol/src/actor/status.rs
- // The vanilla constructor and handler pass StateVector origin

## crates/protocol/src/interaction.rs
- /// Builds the click-air transaction vanilla's `GameMode::baseUseItem` sends: zero block and
- // `setPlayerContainer` stamps and records only a non-empty result.
- /// Builds the release-item transaction `GameMode::releaseUsingItem` sends when the use button

## crates/protocol/src/inventory/transaction.rs
- //! Vanilla client verification does not reject a stale fromItem.
- // Native UI output 50 defers an InventoryTransactionManager action;

## crates/protocol/src/item_capacity.rs
- /// The stack size `Item::Item` gives every item until its definition says otherwise
- /// (`mMaxStackSize`, 64): a server block's own `BlockItem` keeps it.

## crates/protocol/src/login.rs
- // Login has already initialized this session's registry. Native 1.26.50
- // ItemRegistry::matchServerItemIds returns once its
- // initialization state is complete, including for an empty/custom-only
- // repeat. Decode the wire first so malformed repeats remain fatal;

## crates/protocol/src/ui.rs
- /// Floors a coordinate as `LevelRendererPlayer::levelEvent` does.

## crates/protocol/src/ui/forms.rs
- // ServerFormBindingInformation::createBindingData
- // normalizes both representations through the same image value. Absent

## crates/protocol/src/world/clocks.rs
- /// The vanilla clock consumed by Level::getTime and the atmosphere renderer.
- /// The vanilla initializer constructs this exact
- /// name; Level::getTime uses the resulting hashed string.
- /// Native HashedString key for the built-in daylight clock. The current
- /// initializer assigns this key alongside OVERWORLD_CLOCK_NAME;
- /// registerWorldClock pre-registers it and Level::getTime's
- /// lookup searches that ID directly, not packet string names.
- /// Native RegistryClient initialization upserts registrations; it does not

## crates/protocol/src/world/events.rs
- /// Native ClientNetworkHandler 014b1c90 stores this phase without moving the actor.
- /// Native ClientNetworkHandler 014b1c90 applies this phase through LocalPlayer::respawn.

## crates/protocol/src/world/game_rules.rs
- /// Native GameRules slot 9 gates ClientLevel's seasonal palette accumulation.

## crates/protocol/tests/it/actors.rs
- // MoveActorDeltaData::parseDeltas merges into the previous absolute data,

## crates/protocol/tests/it/interaction_packets.rs
- /// Air use matches `GameMode::baseUseItem`: action 1, face 255, no trigger, no block.

## crates/protocol/tests/it/item_stack_requests.rs
- /// Current SparseContainerSetListenerClient::postSetItem stamps cells;
- /// ItemStackRequestActionHandler::_validateRequestSlot resolves them.

## crates/render-model/src/actor/bind_pose_tests.rs
- //! Cube-local bind poses from native ModelPart cube setup, not hierarchical bone rotations.

## crates/render-model/src/actor/geometry.rs
- // Native ModelPart cube setup adds the bind Euler angles
- // Geometry::_parseBoxFaceUV first copies the cube's

## crates/render-model/src/actor/texture_mesh.rs
- //! Native attachable raster extrusions (TextureMesh::compileQuads).

## crates/render-model/src/equipment/display.rs
- /// Native `_renderOffHandItem` has its own bone-frame offset and
- /// Legacy block grip, applied to the centred cube emitted by
- /// `_rebuildItem` through the (-.5,-.5,-.5) mesh offset.
- /// Items vanilla holds upright (its `isHandEquipped`): tools, weapons and rod-like items. The

## crates/render/src/actor.rs
- /// axis (`LevelRendererCamera::queueRenderEntities`, `min(radius, 72)`); players are added apart.

## crates/render/src/atmosphere.rs
- /// Cloud RGBA from `DimensionClientUtils::getCloudColor`: weather tint, day brightness,
- // Current DimensionClientUtils, classic non-custom branch:
- /// LevelRenderer::tick drives clouds even while daylight is paused.
- // getInterpolatedSkyColor also mixes precipitation fog before thunder.
- // That stage waits for Weather+0x4c/current-rain view inputs after profile resolution.

## crates/render/src/atmosphere.wgsl
- // LevelRendererCamera rotates the star mesh around +Z.
- // Current 1.26.50.26 buildSkyMesh has red0 at its centre and
- // red1 at this decagon rim. renderSky places its plane at Y256
- // orbit. buildSunAndMoonQuad maps−X→u1 and−Z→v0, hence fixed+Z
- // degrees (moon offset 180). Ordinary renderSunAndMoon admits the
- // Target renderSunAndMoon scales the stock celestial alpha by

## crates/render/src/atmosphere/liquid_distance.rs
- /// Incomplete: the native underwater/no-FrameBuilder branch uses a distinct

## crates/render/src/biome_tint.wgsl
- // interpolating the ordinary foliage lattice (FoliageTessellationPolicy).

## crates/render/src/block_entity/crack.rs
- /// Current extractCracks routes through the block

## crates/render/src/celestial.rs
- /// Colour helpers use the native table, unlike getSunDirection's full-precision sinf/cosf.
- /// Ordinary Dimension::getSkyDarken, consumed by builder.
- // DimensionClientUtils::getStarBrightness: weather participates
- /// Current getSunriseColor does not read weather. The sky and cloud

## crates/render/src/chunk/transparent/face_metric.rs
- // Current vanilla RenderChunkSorter perspective sort uses
- // Current CentroidPlusReverseBit packs the emitted-vertex

## crates/render/src/chunk/transparent/gamma_pass.rs
- //! Current client RendererSettings selects UNORM format 0x57, while

## crates/render/src/cloud.wgsl
- // TextureTessellator's exact face sequences preserve both outward

## crates/render/src/cloud_config.rs
- /// Current native recalculateRenderDistance removes a distance-dependent
- /// Vanilla stores packet radius + one chunk in Player::mChunkRadius;
- /// Current tickClouds's feature-disabled Fancy route. Advanced
- /// FrameBuilder quality selection is a separate, still-open parity gate.

## crates/render/src/cloud_render.rs
- // Native cloud PassState cull1 translates to BGFX CULL_CW:

## crates/render/src/dropped_item.rs
- /// Native TextureTessellator frame after the ordinary dropped-item default transform.

## crates/render/src/dropped_item/native.rs
- //! Current 1.26.50.26 ItemRenderer. Item-local
- /// `getRenderYOffset`'s table-index truncation and cubic ease-in are retained. Computing

## crates/render/src/lighting.wgsl
- // Current classic builder stores Color::toABGR, which
- // truncates RGB to bytes before LightTexture::getColorForUV reads them.

## crates/render/src/material.wgsl
- // enabled by BlockGraphics' pack-authored isotropic mask.
- // AmbientOcclusionCalculator raises the four-sample average * face

## crates/render/src/native_sunlight.rs
- /// Weather+0x38: current simulation rain, not the frame's interpolated rain.
- /// getSunIntensity; the caller supplies the narrow or broad threshold.
- /// getInterpolatedSkyColor: precipitation fog first, then thunder.
- /// Overworld::getFogColor; unlike cloud colour, fog uses full cosf.

## crates/render/src/screen_overlay.wgsl
- // Portal: native FullScreenEffectRenderer unit cube and the active atlas flipbook.

## crates/render/src/screen_overlay_portal.rs
- // Current getDestructionParticlesTexture 04e95920 chooses default face zero.

## crates/render/src/weather.rs
- //! Precipitation model after the vanilla `WeatherRenderer`: biome lattice, per-kind intensity,
- /// Weather::tick approaches its targets by this amount.
- /// Per-kind constants from the vanilla `paramsRain`/`paramsSnow` tables.
- /// Biome sample lattice around the player from the vanilla `precipitationOffsets` table.

## crates/render/tests/it/atmosphere.rs
- // Current getCloudColor, legacy (non-custom) branch.
- // Native admission reads Weather+0x38, independently of interpolated frame rain.

## crates/render/tests/it/block_selection_native.rs
- // Named native StairBlock::getOutline deliberately uses one full box.

## crates/render/tests/it/cloud_render/corners.rs
- // Current native TextureTessellator emits these four vertices per

## crates/render/tests/it/native_sky.rs
- /// Current 1.26.50.26 buildSkyMesh stores a black centre and white
- /// decagon rim. renderSky translates it to Y256 and scales it by2000.
- /// Current renderSunAndMoon admits orbital phase through 105/255.

## crates/render/tests/it/skull_lighting.rs
- // Native getColorForUV: /16 coordinates, clamp-linear sampling of RGB bytes.

## crates/sim/src/destroy.rs
- /// `WeaponItem::getDestroySpeed` gives bamboo the harvest divisor as its speed,

## crates/sim/src/math.rs
- /// Native look vector used by the swimming trigger (current RVA 0x09fd25a0).

## crates/sim/src/simulator.rs
- // FinalizeMove uses the native float epsilon.
- /// `bedsim v0.1.3` `ClimbSpeed`, cited there against `Mob::ascendLadder()`.
- // TravelTypeSensing (0x09fefcb0) selects water by WasInWater,
- // Current BedBlock restitution.
- /// `WaterTravelSystem`'s travel speed: the water base blended toward the ground

## crates/sim/src/simulator/collision.rs
- // Like `AutoStepSystem::getMaxCollisionVolume`, cover the raised path too.

## crates/sim/src/simulator/environment.rs
- // Current BlockSource::containsAnyLiquid (0x031a7a20)
- // reads getBlock's primary material, without secondary layers.

## crates/sim/src/simulator/flight.rs
- // HorizontalFlySpeedControl current RVA 0x03235360, PE VA 0x1501672a8.
- // VerticalFlySpeedControl current RVA 0x02c452b0, matching PE float lanes.
- // FlyDrag current RVA 0x03217940 reads this friction coefficient; retention
- // is its native float subtraction from one, independent of horizontal drag.
- /// Flying shares DefaultMoveSystems' ground-friction probe at starting AABB
- /// minimum y minus native float 0.1, including fractional support heights.
- // Current horizontal drag RVA 0x03203a50 clears each lane at its float
- // epsilon before applying friction; this is independent of vertical drag.

## crates/sim/src/simulator/mode.rs
- /// Current Player constructor's SneakingHeightChangeVersion value.
- /// Native horizontal pose uses collision width as height (RVA 0x02c33550).
- /// Native bounding-box input update shrinks all probe faces (RVA 0x09eeeb70).

## crates/sim/src/simulator/state.rs
- /// Native SwimAmountComponent blend retained across ticks and replay. Both
- /// Retained native swimming-or-crawling flag observed by SwimAmount before

## crates/sim/src/simulator/travel.rs
- //! Flight controls and liquid movement follow the current mcsrc client systems.

## crates/sim/src/simulator/water.rs
- //! Current-client liquid drag, jump ascent and swimming pitch steering.
- // Current UnderWaterSensingSystem (0x09fd7c00), PE VAs 0x150064950/0x150167290.
- /// CurrentSwimAmountSystem (0x099e64c0) precedes MobJumpSystem in the
- /// native category's registration order. Crawl flag 114 also advances the blend.
- /// MobJumpSystem (0x0a5dc2e0) suppresses every jump path while the blend
- // 1.26.50.26 RVA 0x0320fc20; PE VAs 0x14ff9c370 and 0x15005ea28.
- // MobJumpSystem equivalent 0x0a5dc2e0 reads PE VA 0x1500b5374.
- // WaterSinkInputSystem equivalent 0x0dc3db30 reads PE VA 0x150106adc.
- // SwimControl equivalent 0x09fd2140; PE VAs 0x150361800 and 0x14ffab668.
- // MobMovementClimbOutOfLiquid 0x09004df0; PE VA 0x14ffab698.
- /// Swimming pitch steering runs only without a held jump. The native dispatch
- /// excludes MobIsJumpingFlagComponent and lets MobJumpSystem handle ascent.
- // RVA 0x09fd2140: ordinary upward steering requires the liquid material
- // flag written by bounding-box input update, even if velocity was falling.

## crates/sim/src/world/current.rs
- //! Player liquid-current impulse from the preceding pose (native 0x0a5d5c40).
- // Native Range<int> uses floor(f32(upper + 1)) as its exclusive
- // LiquidPhysics selects lava when both material probes find contact.
- // Matching PE preserves (X² + Y²) + Z² despite the reconstructed
- // expression listing the operands in another order.

## crates/sim/src/world/flow.rs
- //! Native liquid-cell direction (LiquidBlockBase::_getFlow, current 0x0395d2f0).
- // Facing::PLANAR at PE 0x15013e0c7; face masks are 1 << Facing.
- /// Source-identified material and liquid detection facts, independent of meshes.
- /// `blocked_faces` is BlockLiquidDetectionComponent's cache byte at Block+0xb9.
- /// `allowed_faces` records the native directional virtual's admitted faces.
- // getLiquidBlock (+0x28): extra layer unless it is air, then primary.
- // Matching PE 14395d72d / 14395d793: (X² + Y²) + Z².

## crates/sim/src/world/liquid_probe.rs
- /// Native LiquidPhysics material probe, using the preceding collision pose.

## crates/sim/tests/it/embedment_convergence.rs
- // FinalizeMove reconstructs the centre of the native float AABB.

## crates/sim/tests/it/flight_native.rs
- //! Frozen float regressions from identified current-client flight controls and
- //! PE constants. These are source-derived cases, not captured live trajectories.
- // DefaultMoveSystems flying wrapper 0x099cce30 calls horizontalMovement
- // 0x099cc8b0, which samples starting feet minus 0.1 before collision.
- // VerticalFlySpeedControl 0x02c452b0 precedes movement; FlyDrag
- // 0x03217940 retains y independently. Each pair is (movement, velocity).
- // HorizontalFlySpeedControl 0x03235360 reads float ability 6 and its
- // sprint table. Flying descent does not slow the processed horizontal axis.

## crates/sim/tests/it/liquid_contact_native.rs
- //! LiquidBlocksFetch senses the preceding pose before SwimTrigger changes it.

## crates/sim/tests/it/liquid_exit_native.rs
- //! Current-client liquid exit regressions. The native system probes the actual
- //! resolved pose box in f32, for every liquid travel mode (0x09004df0).

## crates/sim/tests/it/liquid_native.rs
- //! Focused regressions derived from current 1.26.50.26 canonical movement
- //! bodies and their matching PE data. These use a synthetic fully wet world;
- // RVA 0x0320fc20: sprint is ActorData bit 3; y uses the independent
- // water retention. RVA 0x0322d5d0 then applies water gravity outside swim.
- // Dispatch 0x09fdd550 excludes MobIsJumping; jump 0x0a5dc2e0 uses
- // the default liquid impulse. Swimming skips gravity (0x0322d5d0).
- // Current 0x09fd2140 selects the faster steering rate below its dive
- // WaterSinkInputSystem 0x0dc3db30 adds the descent input in water,
- // Current 0x09eeeb70 floors attach7 and copies its material's liquid byte.

## crates/sim/tests/it/surfaces.rs
- /// Current BedBlock restitution is 0.75, without a one-block velocity cap.

## crates/sim/tests/it/swim_jump_native.rs
- //! Native SwimAmountComponent/ActorHeadInWater guards in MobJumpSystem.
- // CurrentSwimAmountSystem 0x099e64c0 is registered before MobJumpSystem
- // 0x0a5dc2e0 and the current swim trigger. The first entry tick retains

## crates/sim/tests/it/terrain.rs
- /// MaxAutoStepComponent starts at 0.5625, below this obstacle's top.

## crates/ui/src/geometry.rs
- /// Physical pixels per GUI pixel: Bedrock's desktop rule
- /// (`GuiData::calculateOptimalGuiScaleIndex`), `min(width/376, height/250)` in

## crates/ui/src/hud.rs
- /// `ToastMessage` defaults and `ToastManager`.

## crates/ui/src/text/palette.rs
- //! Vanilla UIDefRepository::_applyGlobalColorFormat reads RGB triples and updates
- //! ColorFormat's shared table.

## docs/core-join-startup.md
- | Initialization | Send the local runtime ID once after loading state `0x10`, stable dimension state `0`, and `isInWorldAndNotShowingAnyMenuScreens` are satisfied. This is local readiness after the screen closes, rather than a direct PlayerSpawn response. |
- but the path that does so remains unresolved. Loading state 4 completes (0x10) only
- once loaded chunks reach the needed count or, after a deadline, every
- `_mChunksNeededForLoadOffsets` chunk is loaded, and a position check passes.
- State 0x200 completes directly unless the view's `bool` argument is set;
- `stopLoading` sets 0x10 directly, through callers not resolved here.

## docs/evidence/2026-10-01-zeqa-movement.md
- Jumping: input update carries held processed jump into MoveInput bit
- `0x10`; `fillInputPacket` maps it to wire bit 6. StartJumping:
- Jump initiation checks canJump, performs the jump, then sets action `0x100`;
- `setFromComponent` maps that action to wire bit 31.
- MovePlayer mode 2: `Player::handleMovePlayerPacket` sets action
- `0x40000000`; `setFromComponent` maps it to HandledTeleport bit 37.
- Current `LegacyClientNetworkHandler::handle(SetActorMotionPacket)`:
- 1. For a local replay actor and a **nonzero packet tick**, creates a position-delta replay object, calls `ReplayStateComponent::applyFrameCorrection` at that packet tick, and clears replay-state byte 1.
- 2. For **tick zero**, bypasses the replay object/history APIs and invokes the actor motion virtual immediately. It never substitutes a local receive tick.
- Immediate motion writes only the incoming vector to StateVector velocity offsets
- 0x18/0x20. It does not call other functions or write history, input flags, ground
- state or rotation.

## crates/client-ui/src/ui_runtime/presentation/forms/oreui/modal.rs
- Index bundle modal `Ug` (`Ug.Overlay`, `Ug.Header` over title bar `gm`, `Ug.Content`, `Ug.Text`,
  `Ug.Buttons`) and the modal menu `SV`/`CV`/`wV`.

## crates/client-ui/src/ui_runtime/presentation/forms/oreui/widgets.rs
- `button_face`: pressable `sf`/`bf`/`hf`; menus theme `--pressableElevated*` nine-slices.
- `menu_item`: dropdown item `bV` (classes `gV`) in `MV`; check icon `Fp`.

## crates/client-ui/src/ui_runtime/presentation/forms/oreui/theme.rs
- Role table: theme `pD` colour roles over the palette constants defined beside `Zc`.

## docs/evidence/desktop-video-settings.md
- `GuiData::GUI_SCALE_VALUES` is `[1, 2, 3, 4, 5, 6, 7, 8]`. Desktop minimum

## docs/home-promo-investigation.md
- | Images | Associate fetched images by message and image ID with a local Core::Path. |

## docs/oreui.md
- The client picks a tech stack per screen (`ScreenTechStackSelector::getTechStackForScreen`): a
- non-zero dev override wins (1 OreUI, 2 JSON-UI), then a preference option, then the screen's
- `isSelected() && isSupported()`. Treatment toggles are true only when the service's treatment list
- names them, so they default off. The local install's `routes.json` lists screen routes.

## docs/reference/actor-animation-clocks.md
- # Native actor animation clocks
- broader native actor-animation parity gate: ordinary actors still evaluate Molang
- The definition constructor compiles the default expression
- tick still does not represent every independent native animation-player instance.
- The native query getters use render interpolation fraction `alpha`:
- phase input but does not implement these native render-time getters. No teleport distance cutoff
- or teleport-specific native animation reset was established by this investigation.

## docs/reference/arrow-rendering.md
- # Native arrow entity rendering
- Native geometry parsing starts with the supplied
- uses the native face-size defaults for all entities, with explicit signed
- `minecraft:arrow` receives this exact native two-sided contract. The lookup
- `ExpressionQueries::getTargetYRotationBase`
- special-cases the arrow actor type: `target_y_rotation` is absolute actor
- rotation yaw, interpolated by the native frame alpha, not a mob's clamped
- `Actor::getInterpolatedBodyYaw` returns zero for
- the base actor; the data-driven renderer uses that value for
- Native actor event handling, case `0x27`, assigns
- zero and interpolates bone poses for display. Exact native per-render-frame
- version-matched native flight/embedded-arrow comparison.

## docs/reference/block-break-particles.md
- plus 0.5, and reading `BlockDestructionParticlesComponent`'s particle count.
- Its current getter returns **100** without a count override.
- `addTerrainParticleEffect` selects `minecraft:block_destruct`.
- `_addTerrainEffect` supplies the count, its cube-root intensity, velocity scalar 1 and radius 0.5 for these

## docs/reference/block-placement-prediction.md
- # Native block placement prediction
- fix. This is a live functional prediction witness, not native Bedrock/BDS parity acceptance.
- actor-overlap tolerances, repeat timing and complete native material/side-effect behavior.
- native ordering parity is not claimed. No full placement parity gate is closed. Changes

## docs/reference/camera-fov.md
- `LevelRendererPlayer::getFov` and `getFovWithoutGameplay`
- scale the configured angle by
- `min(normalized_viewport.y / normalized_viewport.x, 1)`. `ClientInstance::getNormalizedViewportSize` divides each viewport dimension by the corresponding
- full-screen dimension from `GuiData::ScreenSizeData`. The values are `(1, 1)`
- `CameraAPI::tryGetFOV` converts that angle to radians.
- `dragon::rendering::Camera::createPerspective` supplies the
- viewport width/height separately to `bx::mtxProjRh`. That
- projection places `cot(FOV / 2)` on the vertical axis and divides it by aspect

## docs/reference/carried-block-textures.md
- # Native carried block textures
- | Mip construction | Pass the parsed overlay from tile offset `0x30` to atlas mip construction as overlay argument 15. |

## docs/reference/crossbow-use.md
- # Native crossbow use state
- `getMaxUseDuration` is 25 ticks minus five ticks per Quick Charge
- level; loading does not change that duration query to zero. `use`
- checks the stack's cached charged projectile. An uncharged item starts use; a
- `releaseUsing` computes normalized draw power from duration minus
- `chargedItem` and retains a cached item stack. The crossbow's duration-depleted
- virtual implementation dispatches release with zero remaining
- After its item-complete gameplay event, `Player::completeUsingItem` checks
- `Level::isClientSide` and skips the transaction/depletion branch on the client.
- The sub-client id is a separate field.
- Only the server-side branch constructs an `ItemReleaseInventoryTransaction`:
- the transaction fills selected slot, player position and action `Use` (1),
- invokes `useTimeDepleted` and writes the mutated stack. That branch also records
- button release still sends a release transaction. `getAnimationFrame` selects frame 4 for loaded arrows and frame 5 for fireworks
- The current `setIcon` override registers five icon records from
- the `crossbow_pulling` atlas key, in variants zero through four. The
- `getIconInfo` override uses the ordinary standby icon for frame
- zero; nonzero animation frame N addresses registered record N minus one.
- HUD capture now routes every charged stack's icon through that mapping. Native
- loaded NBT applies to hotbar, inventory, offhand, storage, and cursor cells.
- and rendering evidence, not a version-matched native frame comparison.
- `Player.UseItem` call instead. Consequently the fixture cannot establish native

## docs/reference/crouch-camera.md
- `VanillaOffsetSystem` runs on both sides.
- | Version-selected crouch drop | Current version uses float bits `0x3eb33333` (`0.35`); the legacy branch’s `0.125` is not this client. |
- The client tick receives
- the actor data flags and optional `IsHorizontalPoseFlagComponent`; sneaking is
- flag bit 1. `UpdateHorizontalPoseSystem::update` admits
- gliding, swimming and crawling flags. The sleeping branch has a distinct native
- `0.2` target; that branch and riding/dynamic offset inputs are not implemented by
- `SneakTriggerActionSystem` consumes the input start/stop-sneaking bits and
- sets/clears actor flag bit 1. Current `SneakingSystem` tick adapter uses native base multiplier `0.300000012`; Swift Sneak
- adds `0.150000006` per level and caps at one. This audit does not
- claim the existing collision/Swift Sneak simulation is fully native-conformant.
- targets follow the native priority and tick blend. Frame alpha interpolates the
- behavior-contract tests, not a native rendered-frame
- on the macOS/Metal client. A controlled native standing/held Shift/release and

## docs/reference/dropped-items.md
- # Native dropped-item rendering
- Current `ItemActor` constructor sets the
- collision width and height to 0.25 and the native Y-origin offset to half the
- height. Current `MoveActorAbsoluteData` constructor copies the
- actor's StateVector position directly. `AddItemActorPacket`
- constructor copies that cached movement position; its
- `LegacyClientNetworkHandler` item spawn handler creates the actor
- at that position and retains it as the last received position. The item `clientInitialize` does not subtract the collision offset.
- `clientInitialize` retains this direct position setter.
- The rendering caller is equally important: `ActorRenderDispatcher` calls `Actor::getInterpolatedRidingPosition`.
- For an unmounted item that calls `getInterpolatedPosition` which
- interpolates current and previous StateVector XYZ directly. The dispatcher places the passed XYZ into `ActorRenderData` offsets 0x10, 0x14
- and 0x18; `ItemRenderer::render` translates by those values
- without subtracting the item offset. The alternate dispatch also builds that direct camera-relative origin. This establishes a native
- item origin above collision feet, rather than an extra invented mesh lift.
- retain `NetworkOffset` because native `MoveActorDeltaData::parseDeltas` merges
- positions into the previous absolute data.
- `ItemRenderer::getRenderYOffset` uses time
- and baseline 0.1. Ordinary block models additionally rise by 0.2. The Math
- table initializer samples `sinf(index / 10430.3779296875)`;
- lookup truncates `angle * 10430.3779296875` and masks to 16 bits.
- On the first render only, an actor whose current native origin is less than
- zero over 0.4 seconds. `easeInCubic` establishes this as
- spin. The captured camera yaw contributes using the native degree-conversion
- `ItemRenderer::render` uses one, two, three and four copies for stack counts
- below 2, below 6, below 21 and at least 21 respectively. The renderer
- constructor owns its random XYZ copy table, shared across actors;
- the first copy stays centered. `_renderItemGroup` translates
- later copies by that table times `0.2 / groupScale` inside the spinning and
- The ordinary flat-item route uses group scale 0.3 and shared
- `ItemInHandRenderer` with the dropped-item flag, not a hand grip.
- and rotations to the raster frame. Combined with `TextureTessellator`, this maps a raster point `[column, depth, row]` to local
- The native legacy tessellation route queries the item's animation frame before
- its icon. Loaded crossbow sprites share the same `getAnimationFrame` /
- `getIconInfo` selector as HUD and inventory icons, using canonical charged
- brightness, native sine lookup, first-render camera capture/easing, rotated
- frame checks, not a version-matched native gallery or pickup-trajectory parity.
- transforms require their native model routes, not a flat GUI thumbnail. This
- Multi-layer/enchantment material behavior, native actor lighting/shader parity
- but the local generator is ours, not the native generator; platform sine
- native pickup-trajectory implementation.

## docs/reference/effective-movement-speed.md
- `Player::getSpeed` reads `minecraft:movement` from AttributeInstance current. `BaseAttributeMap::updateAttribute`
- replaces the previous modifier set with the packet's modifiers and
- `LocalPlayer::setSprinting` and
- `SprintTriggerSystem::setSprinting` return on an unchanged actor
- flag. `Mob::setSprinting` adds or removes only its identified sprint

## docs/reference/emote-wheel.md
- ## Identified references
- Lens's 26.30 reference and the current reconstructed Windows client identify:
- at `0x10096bc20` and full layout at `0x10096e1f0`, action `0x34`, keyboard B.
- The current Windows mapper at RVA `0x7395f10` names that action `key.emote`.
- `persona_common.emote_wheel_panel`: four cardinal slots. The controller's
- contextual global bindings query each originating control's `#index`.
- action `0x34` to native button 7, identified as D-pad Left by the sprite/name
- initializer at `0x102e15a30`. The controller mapper at `0x10097ef10` binds it
- to `button.emote`. Supplemental binding rows are appended to preserve saved IDs.
- `PersonaAppearance::setEmote` at `0x103c3b260`: bounds slots and swaps an
- already equipped piece into the requested slot.
- rectangle side; the inner boundary is excluded and the outer is included.
- Visibility at `0x1024e9f30` selects one state, and construction at `0x1024e9af0`
- starts with no hovered slice.
- 1.26.50.26 RVA `0x4f382c0`: animation controller playback and emoting status.
- This function does not directly change the camera perspective.
- `ClientInputUpdateSystem::updateStopEmotingRequest` at `0x1022bc6a0`:
- nonzero horizontal movement requests cancellation.
- doll visible with the existing hold timer.

## docs/reference/farmland-rendering.md
- incorrect generic diagonal cross. Wheat now uses the native four-row model,
- ## Current Bedrock source evidence
- The local MCSRC reconstruction at revision
- `da728f0ce4d7a5ae0be443b8abe03119858d923e` was inspected for the current
- `1.26.50.26` client. Recovered names aid navigation; canonical function records
- and the matching executable establish identity. The executable SHA-256 is
- `7d6cf9b2e4b01fce5d6283cc3deb65b877995a8fd1e146f967d8ac743369d628`.
- References below are RVAs under `current/1.26.50.26/`; source and executable
- payloads remain outside this repository.
- `src/__recovered/BlockTypeRegistry.h`, calls constructor `08715fd0`.
- visual shape `(0,0,0)..(1,15/16,1)`. Initializer `05efb820` sets those bounds;
- its raw reconstruction was checked against the matching PE disassembly and
- data, including the packed maximum Y/Z words.
- whether moisture is below one. The pinned pack binds dry to index one and
- wet to index zero, so moisture zero is dry and all other states are wet.
- `src/__recovered/BlockTessellator.cpp`, calls vtable slot `+0x330`.
- The matching FarmBlock vtable points that slot at `08716d90`, confirming
- that the selector is used during tessellation.
- `src/__unmapped/06.cpp`, builds graphics from pack names and registered
- block types; it does not establish the legacy sequential-ID formula.
- The older native gallery and UV limitations remain documented in
- `docs/evidence/phase-2-farmland-native-reference.md`. The current source confirms
- height and top selection; no new claim about calibrated source rows is made.
- The older named `BlockTessellator::tessellateRowInWorld` and
- `tessellateRowTexture` bodies identify the row path. The current canonical bodies
- and matching PE verify the coordinates and UV order:
- adds the float at VA `15014e720` to block Y; its verified value is `-0.0625`.
- Disassembly `06a21ff5..06a2201b` confirms the offset is passed to the helper.
- side for each. PE constants at VAs `14fec3380`, `1500eb850`, `14feff2e0`,
- `14ff1b150`, and `14fea4060` are respectively `0.5`, `-0.25`, `-0.5`,
- `0.25`, and `1`. These yield rows at X/Z 1/4 and 3/4, spanning 0..1.
- The resulting Y range is -1/16..15/16 relative to the wheat block.
- their world direction. Cinnabar represents the native reverse sides with its
- near-version witness to the analyzed preview, rather than an exact-version gate.

## docs/reference/first-person-offhand.md
- # Native first-person offhand placement
- `ItemInHandRenderer::renderOffhandItem` selects the cached offhand
- stack at renderer offset `0xd0`, builds its render key using animation frame `-1`, pushes
- the camera matrix, and draws the cached item. It does not
- re-enter the ordinary main-hand `renderItem` transform after applying its camera pose.
- Angles below are degrees. These are native tessellator-frame matrices, not an instruction
- For `Item::isHandEquipped() == false`:
- For `Item::isHandEquipped() == true`, unless the legacy Shield-blocking special case wins:
- | Native pixel-to-model scale | `0.0625` |
- The table shows equivalent angles; the native rotation constants use radians. The hand-equipped depth is `1.53125`,
- `_rebuildItem` stores `16/max(width,height)` in the cache. The flat offhand branch
- The flat branch therefore normalizes the native pixel geometry to one model unit on
- `TextureTessellator::tessellate` emits positive column
- normalized native point is `(-held.x, -held.z, height/max - held.y)`. Thus the basis
- display transform for presentation type `2`. The native default presentation array
- has zero translation/pivots, Y rotation `-135` degrees and scale
- `0.4`. Constructor negates Y/Z for type 2, yielding
- Current `renderFirstPerson` writes
- `context.player_offhand_arm_height` separately from `variable.player_arm_height`.
- `previous_offhand_height + (current_offhand_height-previous_offhand_height)*frame_alpha`,
- using renderer offsets `0x18c` and `0x188`. Main-hand heights use `0x184` and `0x180`.
- Native tick snapshots both hands independently, advances each toward
- and stack-specific native instant-update/equivalence predicates (the current retained
- equipment feed supplies identifiers, not the full native cached ItemStack comparison).
- matched native gallery or acceptance of the incomplete routes above.

## docs/reference/fish-rendering.md
- Native FishAnimationSystem copies the current phase to previous, then advances
- current by `1 + 0.1 * length(StateVector.velocity)` each tick. The native variable
- updater publishes `variable.AnimationAmount` and `variable.AnimationAmountPrev`

## docs/reference/flight-control-corrections.md
- The analyzed reference is the canonical reconstruction of client `1.26.50.26`
- in the private MCSRC workspace. The repository target game version is selected
- by `assets/bedrock-target.json`; the available client reconstruction is a preview
- build in that version family. Source bodies remain private and are not copied
- into the repository.
- ## Identified native behavior
- position, motion and AABB. It does not clear movement abilities or input mode.
- teleport route before advancing the live actor. Its directly reconstructed
- spatial operations do not reset the flight trigger or movement abilities.
- its full structural correspondence with the named reference counterpart
- `0x059aaf10`. The local flight state lives in the player input request and is
- toggled by input; the double-tap countdown is seven simulation ticks.
- named reference `0x059ab030`. Flight start/stop requests write boolean ability
- 9 and clear the flight countdown and fall distance.
- internal flight action bits 34/35 into wire input ordinals 42/43.
- mode. Its `PosDelta` uses end-of-tick motion, matching the native send path;
- ## Identified flight travel
- The named older `VerticalFlySpeedControlSystem::doFlySpeedControlSystem`
- counterpart identifies current RVA `0x02c452b0`. The current body and matching
- executable float data establish these control operations before movement:
- float `0.01`. Creative idle flight multiplies existing vertical motion by
- `0.375` only while neither vertical control is held.
- `HorizontalFlySpeedControl`, current RVA `0x03235360`, reads float ability 6 and
- the matching executable's `[2, 1]` sprint multiplier table. The keyboard
- vertical controls read float ability 7 in the vertical control body. Both
- custom speeds and explicit zero are retained.
- The current ability default constructor at RVA `0x001dd510` initializes
- protocol ability 13 (`FlySpeed`) to native float `0.05`, and ability 19
- (`VerticalFlySpeed`) to native float `1.0`. These match the fallback values
- `FlyDrag`, current RVA `0x03217940`, reads friction coefficient
- `0.3999999761581421`. The native float subtraction from one produces vertical
- retention `0.6000000238418579`, independently of horizontal hover friction.
- Horizontal drag, current RVA `0x03203a50`, multiplies the horizontal modifier
- by ordinary air friction and clears each horizontal lane at float epsilon.
- The flying travel wrapper at current RVA `0x099cce30` calls shared horizontal
- movement at `0x099cc8b0`. That movement reads ground contact before collision
- and selects the friction of the block at starting feet minus native float
- `0.1`. The current `MobTravelComponent` constructor at `0x036ac750` enables
- this ground-friction probe and leaves vertical generic friction disabled.
- The flying friction wrapper at `0x0321f020` therefore retains horizontal
- motion by `ground friction * hover modifier * air friction`, while separate
- flight drag retains vertical motion. Grounded flight now samples that exact
- support coordinate with the ordinary query bound and world-identity checks.
- Its regressions include hover, takeoff, landing and fractional support height;
- takeoff and landing use the starting contact state for that tick's friction.
- The current send-input mapper at RVA `0x070fcfd0` maps raw input bits 17/18 to
- wire `Ascend`/`Descend`, and raw bits 2/3 to wire
- `WantDownSlow`/`WantUpSlow`. The current local input updater at RVA `0x07108cc0`
- combines keyboard jump or raw ascend into processed up with mask `0x20080`,
- and keyboard sneak or raw descend into processed down with mask `0x40001`.
- Those processed controls are then sent as `WantUp`/`WantDown`, wire ordinals
- 16/17. The extra raw-input acceleration lanes in the native vertical
- controller correspond to the distinct slow controls, which Cinnabar does not
- emit.
- The current server input handler at RVA `0x0998fe80` reconstructs processed
- up/down directly from `WantUp`/`WantDown`. It reconstructs raw
- `Ascend`/`Descend` and `JumpDown`/`SneakDown` separately; those raw flags do not
- populate processed up/down. The handler is identified through the named
- `ServerMoveInputHandlerSystemUtils` adapter at RVA `0x099abbb0`, whose entity
- dispatch at `0x09990430` selects this body. The following input-lock operation
- at `0x00480c40` only clears restricted raw inputs.
- processed controls with held jump and sneak. The regression reconstructs the
- native server control mask from the outbound flags for up, down, both and
- released controls. The protocol regression verifies their named wire rows.
- double-tap detector remains an approximation of the native tick countdown;

## docs/reference/fox-rendering.md
- ## Current client evidence
- MCSRC's verified current export is preview 1.26.50.26. Its matching executable
- SHA-256 is `7d6cf9b2e4b01fce5d6283cc3deb65b877995a8fd1e146f967d8ac743369d628`.
- The named 26.30 reference supplies navigation; the contracts below were checked
- against current canonical bodies or the matching executable.
- reconstructed body is unavailable, so its bind-field read was verified by
- disassembly of the matching executable. VA `146aea6c5` loads
- `bind_pose_rotation`; `146aea71e` calls hierarchy getter `00b95650`.
- `146aea7d8` requires three array entries. `146aea841` through `146aea84b`
- store the converted bind radians separately at node offsets `0x38..0x40`.
- nodes when a field is absent from a replacement. A missing bind in a modern
- replacement therefore retains the base model's bind.
- passes bind rotation at `+0x38` separately to `01e62d50`. The latter combines
- cube and bind Euler angles and rotates the cube pivot about the part pivot.
- Children retain their authored pivots and animation frames.
- ## Native resource witness
- native side-by-side acceptance were not performed.

## docs/reference/game-mode-updates.md
- Current `UpdatePlayerGameType::getId` returns the same packet
- ID as our generated `McpePacketName::UpdatePlayerGameTypePacket`; we use the enum,
- not a copied numeric ID. The generated packet preserves a game type, signed actor
- `ClientNetworkHandler::handle(UpdatePlayerGameTypePacket)` matches the packet target against player-list **unique IDs**. A
- `Player::getPlayerGameType` resolves raw game type
- `5` through the level's default game type. `Player::setPlayerGameType` retains that raw default binding while using the effective mode for
- its mode-change work. Cinnabar reuses its existing player/default-mode reducer:
- functional regression, not historical replay or full native visual parity.
- `GameMode::startDestroyBlock` and
- `continueDestroyBlock` select the creative
- destruction route through `Actor::isCreative`.
- That predicate reads only the game-type component: Creative, or world-default
- `Player::getDestroyProgress` passes **Flying**
- into the destroy context. `PlayerDestroy::getDestroyProgress` and `getDestroySpeed`
- apply the speed, hardness,
- harvest and movement penalties; neither selects instant destruction from Instabuild.
- Current ability serialization independently distinguishes Flying
- at offset `+0x6c` from Instabuild at `+0x84`.
- Other ability grants and the passive evidence owner are unchanged. Native mode-layer
- `ClientPlayerRewindListener::_onUpdatePlayerGameTypePacketReceived` applies a tick-zero packet immediately. With a nonzero tick and
- an eligible replay timeline, it instead inserts `GameTypeReplay` at that tick and

## docs/reference/held-attachables.md
- # Native animated held items
- `ItemInHandRenderer` skips its legacy icon placement
- when an active attachable is present. First-person rendering evaluates the owner’s
- `setupAttachableNoChecks` composes the parent's matrix before the
- item's channels. Treating this as the third-person sprite grip was the bow bug.
- The owner-skeleton camera root also retains the native post-scale 1/128-model-unit
- Native `TextureTessellator` admits pixels with alpha at least 2.
- `compileQuads` composes position minus the authored bone pivot, Z/Y/X Euler
- rotation, negative local pivot, and texture/model scale, in that order. The loader performs the bone-pivot subtraction. Native model Y is converted
- Its animation frame comes from `RangedWeaponItem`: for elapsed
- native frame comparison or a complete visual parity gate.
- The packet handler calls the `ItemRegistryRef` wrapper, which invokes
- `ItemRegistry::matchServerItemIds`.
- Its initialization state at offset `0x331` gates execution: state 3 returns
- without changing the registry, and successful initialization finishes in state 3.
- Repeated packets are not runtime registry replacements.

## docs/reference/hud-paper-doll.md
- | Model origin | Retain the 24-pixel ModelPart origin scaled by the player model scale; swimming adds a 0.8 vertical adjustment. |
- native 24-pixel ModelPart origin, scaled by the player model scale; its control

## docs/reference/inventory-block-restacking.md
- Native occupied-stack compatibility is not a requirement for zero aux or zero
- block identity:
- wildcard), user data, restriction hashes and an additional field at `+0x70`.
- A present left-hand block pointer at `+0x18` must match the right-hand block
- pointer; merely being present is not a rejection. Its charged-item path has
- further checks and is outside this fix.
- other stack. It compares aux when the item's variant flag requires it, then
- user data, the restriction hashes and the additional `+0x70` field. This
- function does not itself compare the block pointer at `+0x18`.
- data and an empty compound tag can compare equal. Native support is broader
- than admitting only empty serialized data.
- The restriction hashes are CanDestroyHash at `+0x68` and CanPlaceOnHash at `+0x48`.
- The extra `+0x70` comparison is retained as an
- identified field, not assigned an unverified semantic name. For ordinary
- occupied stacks, count and server/sparse stack-network IDs are not semantic
- item equality keys; they remain necessary for quantities and request authority.
- aux checking enabled (`sameItemAndAux`). Recipe selection separately calls
- full-stack `matchesItem` when merging items into an occupied grid slot.
- non-null block pointer, including a wildcard block-type descriptor, before
- merge prediction path, even when native structural comparison would accept it.
- The scoped equality checks also do not reproduce native variant-flag exceptions,

## docs/reference/inventory-gui-geometry.md
- | Shield GUI transform | Shield GUI transform and ModelPart draw |
- rotation, so Y acts first on authored points. The native rotation angles have bits
- `0x406a927f` and `0x3f490fdb`; the scale/vertical offset have bits `0x41200000` and
- `0x4147ae14`. The displayed icon frame is 16 design pixels, not 16 physical pixels.
- The ordinary cube GUI path emits only Up, South and West, in that order. Their native
- Static ModelPart pivot-relative coordinates combine into `(x,24-y,z)` before the GUI matrix.
- See [shield inventory rules](shield-inventory-icon.md) for the ModelPart witnesses.
- builders therefore disable depth test/write and preserve native authored draw order.
- exact native model-material texture/color transfer-function formats and a controlled vanilla frame comparison remain
- shield NBT layers, native glint and custom rotated/inherited/animated shield ModelParts

## docs/reference/inventory-hover-tooltip.md
- The constructor accepts only an integer JSON value for `hover_text_max_width`.
- number leaves it unrestricted, as in the native constructor.

## docs/reference/inventory-normal-transactions.md
- cells, with the native cursor projection. Unknown or unreviewed sources/slots,
- The native UI output-50 deferred `InventoryTransactionManager` path, arbitrary

## docs/reference/inventory-recipe-admission.md
- those conditions, the state pointer remains null and candidate recipes still

## docs/reference/inventory-reopen.md
- `onContainerScreenClose` removes the pending screen when the retained screen queue

## docs/reference/inventory-sparse-prediction.md
- # Native sparse inventory prediction
- | `SparseContainer::getItem` | Return the absolute sparse item when the cell is predicted; otherwise return its backing item. |
- | `SparseContainer::setItem` | Save the new sparse item and invoke the set listener. |
- | `SparseContainerSetListenerClient::postSetItem` | Stamp every changed item with the current typed request id, including an emptied item, and register its container with the request. |
- | `SparseContainerClient::_networkUpdateItem` | Update the backing container without rebasing or subtracting an active prediction. |
- | `ItemStackRequestActionHandler::_validateRequestSlot` | Resolve odd-negative request references through request-id, container-runtime-id, and requested-slot assignments. A request id is not a globally unique item identity. |
- | `ItemStackNetManagerClient::handleItemStackResponse` | Find the issued request across retained screens; skip unknown ids. Process each answer immediately. |
- | `SparseContainerClient::tryPushSlotPrediction` | Requested slot locates the sparse item; actual slot receives the correction. Validate amount/net-id pairing. A later owner selects the historic path. A missing sparse cell is skipped. |
- | `SparseContainerClient::_pushHistoricPredictionItem` | Correct backing using the request's historic item without removing the newer active prediction. |
- | `SparseContainerClient::clearAllPredictions` | Remove remaining active cells whose stamp is the answered request, not older or later owners. |
- | `ItemStackNetManagerClient::_clearPredictiveContainerRequest` | Remove the answered historic snapshot and clear that request's active sparse cells. |
- | `ItemStackNetManagerBase::onContainerScreenClose` | Retire the oldest retained screen after close acknowledgement. Its late replies no longer own a retained screen. |
- the retained pre-empty item, matching native zeroed-out-item handling. Count/id
- proxy delta contracts to the identified native contracts.
- policies remain Cinnabar safety policies, not a claim that every native inventory

## docs/reference/item-particle-lighting.md
- ## Current-client lighting contracts
- `ItemInHandRenderer::renderItem` is the ordinary dropped-item
- tessellation consumer identified in [dropped-items.md](dropped-items.md).
- Its actor lighting setup supplies `(sky, block) / 16` to
- `LightTexture::getColorForUV`. The latter samples normalized byte
- Modern pack particles use the same RGB lookup. Current
- `ParticleSystemEngine::tick` constructs its
- 16-by-16 gameplay-light cache by calling `LightTexture::getColorForUV` for each pair of nibbles
- multiplied by `0.0625`.
- `ParticleEmitterActual::getGameplayLightForParticle` retrieves
- the four-channel cached entry for the particle's brightness pair.
- `getBrightnessPairForParticle` floors world particle coordinates,
- The ordinary native material color contract composes normalized gamma RGB;
- shaders undo the atlas decode, multiply native tint and RGB lighting, then
- Dropped items now use the byte-quantized `/16` lookup and native color
- native byte lookup in darkness, daylight, sky light at night, torchlight,
- version-matched native scene or performance parity gate.
- material handling. Native item AABB brightness sampling is not replaced by

## docs/reference/jsonui-review-fixes.md
- indices, share instance keys between binding and layout, seed native creation
- | Perspective | Register the perspective option’s callback and apply `_perspectiveOptionChanged` only for a perspective change. |
- | Font measurement | Obtain the current FontHandle for both measured text and drawing. `ui/ui_template_dialogs.json:9` defines the standard title label; the accepted open-font deviation still requires shared installed metrics. |
- These rules do not establish a native frame-time budget for the captured shop.

## docs/reference/liquid-currents.md
- The reference is the canonical reconstructed 1.26.50.26 client and its matching
- PE, SHA-256 `7d6cf9b2e4b01fce5d6283cc3deb65b877995a8fd1e146f967d8ac743369d628`.
- The older named 26.30 reconstruction supplies identities; current bodies and PE
- data supply the behavior below. Cinnabar's implementation is independently
- written from this evidence.
- Current `LiquidPhysicsSystem::_liquidBlockFetch` is **RVA 0x0a5d5c40**. Its
- adapter is **0x0a5e8d50**. Registration **0x072c5ec0** installs the flow-policy
- writer, then this block fetch, then lava, water and head sensing. Only afterward
- does it register player triggers and pose updates through **0x072bb110** and
- **0x072b6020**. Consequently current acts on retained velocity before jumping,
- lava uses `0.1` and `0.4`. Inverted axes clamp to the original center. Material
- gather **0x0a5dd330** traverses Y, then Z, then X, with a floored lower bound and
- exclusive `floor(f32(upper + 1))` range end. It gathers resolved-liquid cells
- using IConstBlockSource slot `+0x28`: the extra non-air block takes precedence,
- with primary fallback when extra is air. This is the getter **0x0319e4f0** in
- vtable `0x150161770`. When both probes contact their liquid, the fetch selects
- lava for current force.
- Local flow-policy writer **0x0a5e6f90** admits ordinary current when the prior
- flying ability is clear. `FlyTriggerSystem` intent **0x06dcdf80**, registered
- after swimming intent inside **0x072b6020**, and its action **0x06dce090** run
- later. The input therefore retains the preceding flow allowance for live ticks
- and replay, independently of the newly selected flight mode.
- with matching material and nonzero raw depth admits current. The entry's native
- boundary bits are `0x04`, `0x20`, `0x08` and `0x10`, respectively. Falling depths
- still admit current; collapsing them to an effective depth zero too early would
- incorrectly disable the force.
- Once admitted, every collected cell contributes its vector from
- **0x0395d2f0**, including source-depth-zero cells. That helper already normalizes
- each cell's result. The fetch sums these vectors in its native Y/Z/X cell order,
- normalizes the aggregate again, multiplies by the selected material's impulse,
- and adds it to the retained velocity. The force does not grow with the number
- of touching cells and is not an average of separately scaled impulses.
- Both normalization steps compute native float squared length as
- The matching PE confirms this grouping at VA `0x14395d72d` and `0x14395d793`
- inside the cell helper and `0x14a5d6826` inside the aggregate; the reconstructed
- expression's operand order differs. Matching PE values are:
- | VA | Native float | Meaning |
- | --- | ---: | --- |
- | `0x15005b1dc` | `0.0000999999975` | Normalization threshold |
- | `0x1500d3a40` | `0.0140000004` | Water impulse |
- | `0x1500d3a3c` | `0.00350000011` | Lava impulse |
- | `0x14fea4060` | `1` | Exclusive gather end addition |
- the raw state. A legacy world or a per-cell vector whose native obstruction facts
- are unavailable also returns no current authority, preserving the incomplete
- boundary instead of inventing a vector from collision geometry. Mounted ownership and
- non-player/item-actor liquid probes remain outside this player implementation.
- ## Per-cell direction and native obstruction facts
- `world/flow.rs` implements current **0x0395d2f0**, identified by the named
- 26.30 `LiquidBlockBase::_getFlow` at **0x0ab193c0**. Production registrations
- retain BREG `ModelStateField::LiquidDepth` without collapsing values 8–15.
- The helper uses raw depth for the falling branch and effective depth zero for
- those values when comparing neighboring levels.
- The matching PE's planar facing bytes at **0x15013e0c7** are `2, 5, 3, 4`
- (Z-, X+, Z+, X-). Its masks at **0x1502a38a8** are `1 << facing` and the
- opposite-facing table at **0x1500e01e0** swaps each adjacent facing pair.
- For a matching neighboring liquid, both primary blocks must admit the
- corresponding face. The contribution is `(neighbor depth - current depth)`
- times that direction. A rejected face follows the same fallback as an
- unmatched liquid: when the primary neighbor's material does not block motion,
- a matching liquid beneath it contributes
- `(below-neighbor depth - current depth + 8)` times that direction.
- The getters are intentionally distinct. Matching PE reads neighboring liquid
- depth through slot **+0x28** at **0x14395d42a**, primary material's motion byte
- through **+0x10** at **0x14395d451**, and below-neighbor depth through
- **+0x28** at **0x14395d48c**. Directional face reads use primary **+0x10**
- at **0x14395d535** and **0x14395d58e**, followed by BlockType's virtual
- **+0x88** at **0x14395d568** and **0x14395d5bd**. The falling-wall checks
- also read primary **+0x10**, at **0x14395d6cd** and **0x14395d703**.
- normalizes again. The matching constant is **0x150068e7c**. Each arithmetic
- step remains float; widening happens after the normalized vector is complete.
- Native material setup **0x0379bed0** establishes byte **+3** (`blocksMotion`)
- and byte **+5** (`isSolid`). Types 0, 5 and 6 have both clear; types 1, 13 and
- 23 have both set. The bindings below come from constructors, not PREG
- passability, render opacity, collision boxes, or BREG face coverage.
- | Binding | Current source association | Material type |
- | --- | --- | ---: |
- | Air | Native air material setup | 0 |
- | Water / flowing water | Liquid material setup; raw BREG LiquidDepth | 5 |
- | Lava / flowing lava | Liquid material setup; raw BREG LiquidDepth | 6 |
- | Dirt | DirtBlock constructor **0x0a7b9820** | 1 |
- | Grass | GrassBlockBase constructor **0x0712ab00** | 1 |
- | Stone | registerBlock<StoneBlock> **0x0dfb6010** → **0x0a5b7aa0** | 23 |
- | Sand | registerBlock<SandBlock> **0x0dfb97a0** → **0x08efbad0** | 23 |
- | Gravel | Constructor **0x0712be60**, vtable **0x1502a5290** | 23 |
- | Ice / packed ice | registerBlock<IceBlock> **0x0dfc49c0** → **0x071305c0** | 13 / 23 |
- Gravel's vtable resolves its dust producer to **0x0712c030**, which selects
- the matching PE's gravel particle identifier at **0x15064f6f4**. This binds
- the otherwise unnamed constructor to GravelBlock. The listed class vtables'
- directional slot +0x88 resolves to **0x00085800**, which returns true.
- LiquidBlock's current constructor is **0x0395a220**; its vtable
- **0x1501905d0** and LiquidBlockBase's **0x150190a00** share that default.
- The face cache at Block **+0xb9** is the packed
- BlockLiquidDetectionComponent mask, separate from render face coverage.
- Named 26.30 `BlockComponentDirectData::_finalizeInit` **0x0b15f8e0** places
- that component in direct data at Block **+0xb8**. The current component
- initializer **0x0ab6b560** initializes the packed value to `0x20000`, whose
- mask byte is zero, matching the ordinary default cache. Special directional
- overrides and liquid detection rules require their own source-established
- registrations.
- below-neighbor liquid is absent: both native fallback outcomes are zero in
- that case. A required unknown directional or material fact returns no flow
- authority. Water-like blocks without proven raw LiquidDepth, including bubble
- columns, also remain unavailable. No fluid-height inverse supplies those
- facts. This keeps waterlogged special shapes, remaining native materials,
- bubble-column forces and non-player probes explicitly outside the completed
- ordinary current behavior.

## docs/reference/liquid-movement.md
- The movement target comes from `assets/bedrock-target.json`. This investigation
- uses the canonical reconstructed **1.26.50.26 preview client**, Lens artifact 6,
- and the matching executable SHA-256
- `7d6cf9b2e4b01fce5d6283cc3deb65b877995a8fd1e146f967d8ac743369d628`.
- The named 26.30 reconstruction identifies call meanings; every formula below
- was checked in the current canonical body and matching PE data. The
- reconstruction is derived evidence, not Mojang's original source. Implementation
- and tests are independently written.
- Current liquid physics **RVA 0x0a5d5c40** forms the water-contact AABB by shrinking
- the horizontal axes by `0.001` and the vertical axis by `0.401`. Lava uses `0.1`
- and `0.4`. A shrink that would invert an axis clamps it to the original center.
- The material-cell scan **0x0a5dd330** floors the lower bound and includes cells
- whose integer coordinate is at most the upper bound. Ordinary contact does not
- compare the actor with the rendered liquid surface. Cinnabar's existing
- Liquid contact precedes this tick's pose change. Registration **0x072c5ec0**
- installs `LiquidBlocksFetch` (current source `07.cpp`, line 470993; PE vtable
- `0x150376d90`, tick **0x0a5e8d50**, callback **0x0a5d5c40**), then lava, water
- and head sensing, before calling the player swim-trigger and pose registration.
- The callback reads the current AABBShape and does not sweep it by StateVector
- velocity. The resulting WasInWater component then supplies jumping and travel
- selection after the new collision height has been applied.
- sensing. PE **0x0a5dd330** calls IConstBlockSource slot `+0x28`; matching vtable
- `0x150161770` selects **0x0319e4f0**, which reads the extra block through
- `+0x20` and falls back to the primary block through `+0x10` when its type is air.
- Initialization **0x02e431b0** identifies the comparison value `0x1551c6d40` as
- `minecraft:air`. Existing all-layer liquid flags cover ordinary primary liquid
- and waterlogged secondary water. Exact priority for a non-air secondary block
- that conflicts with a primary liquid remains a boundary of this model.
- ## Proven water forces
- Water travel acceleration **0x0dc3eeb0** blends the water movement attribute
- toward the effective ground movement attribute by capped Depth Strider level
- divided by its maximum level. The effective level is halved while airborne.
- Cinnabar's existing `water_travel_speed` already implements the ordinary-player
- branch of this rule.
- Water drag **0x0320fc20**, identified through its current adapter's
- `MobMovementDrag::tickApplyWaterDrag` signature, acts on retained velocity after
- movement. Its baseline horizontal retention is `0.9` while the actor's sprint
- flag is set, independently of swimming pose. Otherwise it reads optional
- WaterMovement or the default `0.8`. Depth Strider blends horizontal retention
- toward `0.546000063`. Vertical retention is always the independent default
- water value `0.800000012`; enchantment does not change it.
- The current player's water-gravity body **0x0322d5d0** installs `-0.005`
- (`0xbba3d70a`) only when actor flag 57, swimming, is clear. A swimmer has no
- ordinary water gravity. Levitation uses a separate native travel path.
- The ordinary held liquid-jump branch in **0x0a5dc2e0** adds `0.0399999991`
- to vertical velocity before collision resolution. Its default comes from PE
- VA `0x1500b5374`. The branch retains native float addition.
- WaterSinkInputSystem's current adapter **0x0dc47ac0** calls ticking wrapper
- **0x0dc3db60**, whose callback is **0x0dc3db30**. With WasInWater present,
- held sneak/descent input (MoveInput byte 0 bit 2 or byte `0x60` bit 3) adds
- `-0.0399999991` from PE VA `0x150106adc` to vertical velocity. Flying ability
- suppresses this water sink force. Ordinary water sneak therefore changes vertical
- motion before movement, in addition to reducing horizontal input.
- Swimming steering **0x09fd2140** reads the negative-pitch sine table and moves
- vertical velocity toward that target using rate `0.0599999987`, or
- `0.0850000009` when the target is below `-0.200000003`. Its current dispatcher
- **0x09fdd550** excludes `MobIsJumpingFlagComponent`; held jump therefore bypasses
- pitch steering and enters the jump system. All steering products and addition
- are native float operations. Taking the sine table at negative pitch is distinct
- from negating the positive-pitch lookup at non-cardinal angles.
- The current source chain establishes the point and material semantics:
- offset to **0x0284dd20**. Bounding-box size update **0x02c33d10**, and its
- single-entity counterpart **0x02c39d40**, copy that offset into OffsetsComponent
- bytes `0x38..0x40`.
- calculation **0x037517b0** reads precisely those three floats. For an ordinary
- unmounted player, they are zero and the point is the native player anchor
- minus VanillaOffset. The caller selects interpolation zero, retaining the
- previous pose offset rather than the camera's render-frame interpolation.
- byte 2 into PlayerInputRequest byte `0x21`. Material setup **0x0379bed0** sets
- that byte for water and lava and clears it for air and ordinary solids.
- Matching executable disassembly confirms the virtual default-block slot at
- `0x10`. The current BlockSource vtable at image VA `0x150161770` selects
- **0x0319e020**, which reads the default chunk storage through **0x037af620**.
- It does not scan secondary liquid storage or compare fluid height.
- unspecified for legacy traces. The native previous-offset selection is proven;
- client comparison. Native start/continue/stop and current registration order
- are recorded in [swimming-trigger.md](swimming-trigger.md).
- Relevant current PE float words were read through PE section mapping, with
- image base `0x140000000`:
- | Image VA | Value | Consumer |
- | --- | --- | --- |
- | `0x14ff9c370` | `0.899999976` | Sprint water horizontal drag |
- | `0x15005ea28` | `0.800000012` | Water vertical/default horizontal drag |
- | `0x150167298` | `0.546000063` | Depth Strider horizontal drag target |
- | `0x150361800` | `0.0599999987` | Ordinary swim steering rate |
- | `0x150361804` | `0.0850000009` | Diving swim steering rate |
- | `0x14ffab668` | `-0.200000003` | Dive steering threshold |
- | `0x1500b5374` | `0.0399999991` | Default held liquid jump |
- | `0x150106adc` | `-0.0399999991` | Held water descent |
- derived from these bodies and PE values in a synthetic submerged world. They
- cover sprint drag without a swimming pose, independent vertical retention with
- Depth Strider, jump ascent independent of look pitch, and dive steering followed
- by water drag without gravity. They are numeric source-derived regressions,
- not recorded vanilla trajectories or server-acceptance evidence.
- The surface regressions also cover the exact floored attach-cell boundary,
- material contact above a shallow rendered liquid surface, primary air with
- secondary water, and downward steering with a dry attach point.
- CurrentSwimAmountSystem **0x099e64c0** saves the preceding amount and adds
- native `0.100000001` while actor flag 57 (swimming) or flag 114 (crawling)
- is set, capped at one. Otherwise it adds `-0.100000001`, floored at zero.
- These constants are PE VAs `0x14ffab644` and `0x14ffab670`.
- The writer runs before this tick's swim trigger and MobJumpSystem. This follows
- the actual registration chain, rather than inferring an order from metadata:
- **0x072ce0d0** registers CurrentSwimAmount, then invokes **0x072c5ec0**;
- that invokes **0x072bb110**, which invokes **0x072b6020** to register the
- swim trigger before registering MobJump. Collection **0x028e64f0** appends
- each identity to its category's vector, and **0x028e8f00** traverses it forward.
- Current `07.cpp` call sites are lines 476769, 481041, 473675, 462530,
- 459956 and 463035. The source ordering matters at both transition edges.
- MobJumpSystem **0x0a5dc2e0** returns before all ordinary liquid/ground jump
- paths when `0 < SwimAmount < 1`, or when swimming is set but the
- ActorHeadInWater component is absent. If WasInWater is present, this return
- also zeroes retained vertical velocity. Dry partial-blend ticks retain that
- velocity and suppress the ground-jump request. The head component uses the
- primary-material/level comparison documented in swimming-trigger.md, not
- the steering guard's broader water-or-lava material boolean.
- The jump regression freezes all native float values through entry and exit,
- distinguishes wet and dry suppression, exercises native head sensing, and
- verifies that correction replay reconstructs the same gate.
- MobMovementClimbOutOfLiquid's current callback is **0x09004df0**, identified
- by its current adapter in `09.cpp` line 49309 and the named older body
- **0x06401d40**. Its filter requires LiquidTravelFlagComponent and
- HorizontalCollisionFlagComponent; it applies to water and lava independently
- of swimming/crawling pose.
- `(starting_y - current_y) + 0.600000024 + retained_velocity_y` vertically.
- Matching PE instructions perform each subtraction/addition and face translation
- in float. The raise constant is at VA `0x14ffab698`. It first checks primary
- The executable confirms BlockSource virtual slots `0x38` and `0xa0`.
- The matching vtable at `0x150161770` selects **0x031a7a20**
- (containsAnyLiquid, through its thunk) and **0x031a4560**
- (fetchCollisionShapes, with boolean one). The liquid test floors minimum
- faces, ceils exclusive maximum faces, reads primary getBlock material, and
- does not compare rendered liquid height or inspect secondary storage.
- resolver's actual AABB. Previously the walking probe reconstructed a standing
- box and swimming never received the escape impulse. The dedicated regression
- checks a low pose below an overhead obstruction, both liquids, primary versus
- secondary material, unavailable/conflicting probe rollback, and native float
- impulse. Dry retained swimming uses ordinary acceleration and drag in its low
- box: TravelTypeSensing **0x09fefcb0** selects WaterTravel by WasInWater, not by
- actor flag 57. These synthetic checks do not establish live server acceptance.
- Jump-controller and swimmer-specific paths remain separate. Its one-shot alternate liquid impulse is
- `0.0280000009` at PE VA `0x150376a10`. Those branches are not modeled by the
- ordinary fully submerged regression.
- optional WaterMovement alters unsprinted drag. Those component paths are not
- included in Cinnabar's input contract.
- are implemented from **0x0a5d5c40** and **0x0395d2f0**, with replay-captured
- preceding contact pose and flying policy. The source evidence and exact
- material getter boundaries are recorded in [liquid-currents.md](liquid-currents.md).
- Compile and live acceptance remain pending for this integration. Native
- directional obstruction facts that have not been established remain explicitly
- unavailable; mounted ownership and non-player liquid probes are separate paths.
- swim-entry/exit timing, surface behavior, bubble-column forces and waterlogged
- obstacles still need controlled vanilla and real-server comparisons.

## docs/reference/nametag-rendering.md
- # Native name-tag rendering rules
- The installed PlayCover app and the IPA named `Minecraft-1.26.50-for-iOS-mcpelife.ipa`
- contain the definitions in `data/resource_packs/vanilla/materials/ui3D.material`, lines
- 224–345, and readable Metal shaders in `data/renderer/materials/Nametag.material.bin`
- and `UIText.material.bin`. Their **internal version is 1.26.51.01**, not the filename's
- 1.26.50; they corroborate the material rules but are not a version-matched shader-pack witness.
- SHA-256 values: `ui3D.material`
- `2ca6efaa1e93d650c2476025cb4d6043a70a14516ae542ffcb4218dc8abeeaec`,
- `Nametag.material.bin`
- `cf0f7d60b85c42324fa2955c50a599405bdbaf238c0d800b9eeeefb17ff98d49`,
- `UIText.material.bin`
- `b1f6dc57ea38b7ececf55ec1209b5521e005e192bf05fe0b08ee6ea01e85eb2b`.

## docs/reference/player-preview-rendering.md
- same helper after its independent hand-frame offset. `_rebuildItem` uses mesh offset `(-.5,-.5,-.5)`, centering the cube basis. Native special-shape/custom presentation branches remain distinct.
- the authored default root origin with Y pivot minus the shared ModelPart height.
- pose VM and literal preview share `MODEL_PART_ORIGIN_Y` with the existing GUI ModelPart basis.
- `DataDrivenRenderer::render`
- uses Y rotation `wrap(180-body_yaw)` and then negates the first two matrix columns.
- steps. `animation.player.bob` uses `cos(life_time*103.2)*2.865+2.865` in degrees. The `PaperDollRenderer` constructor sets `variable.is_paperdoll=1`; the install-fetched

## docs/reference/player-skull-lighting.md
- and light-coordinate rules. The native installed client is a near-version
- | Light coordinates | Read `BlockSource::getLightColor` with minimum block light zero. Divide the two retained brightness levels by 16, sample `LightTexture::getColorForUV`, and publish RGB `TILE_LIGHT_COLOR`. |
- | Registration | Register `player_head` and its six companion head types as SkullBlock. |
- | Light sampling | SkullBlock sets light filter zero. Read the requested cell’s retained nibbles directly without choosing neighbouring cells. |
- native shading polynomial represented by `render_api::ACTOR_SHADE_COEFFICIENTS`,
- the correctly lit block-actor geometry. This is a separate native-head mismatch;
- heads. The correction derives these behaviors from the native material path;

## docs/reference/shield-blocking.md
- `query.blocking` reads `ActorDataFlagComponent` (type hash `0xc67426f3`),
- byte 9 bit 0, which is actor flag 72, and returns a boolean script argument.
- It does **not** call `Player::isBlocking`, derive blocking from the processed
- sneak state, or apply a local five-tick timer.
- `ServerPlayer::normalTick`
- sets flag 72. Its eligibility checks include cooldown, sneak/using state,
- Current `SneakTriggerSystem::doActionTick` updates the
- processed sneak/swim/crawl flags but does not set blocking 72.
- The separate damage-blocking predicate `Player::isBlocking` requires flag 72, an active Shield
- than four. `ShieldItem::inventoryTick` maintains that
- timestamp on the server only. `readUserData`/`writeUserData` transmit its trailing signed 64-bit value.
- `ShieldItem::use` is a no-op: starting ordinary ranged-item
- fixture therefore cannot establish a native Shield gameplay or animation
- 72. No client-side fallback bypasses the native metadata query.
- These are functional and rendered-frame checks, not a matched native gallery.

## docs/reference/shield-inventory-icon.md
- | Shield model | Load the authored `shield` ModelPart with default geometry and `ui_shield.skinning` material. |
- | ModelPart orientation | Use `24 - pivot.y` with box-local inverted Y and apply the supplied model unit. |
- ModelPart geometry uses `1/16` units. Matrices post-multiply, so the Y rotation acts first.
- | ModelPart Y orientation | `24` |
- shader behavior, custom rotated/animated/inherited ModelPart trees, exact hardware-MSAA

## docs/reference/swimming-trigger.md
- The target is the canonical reconstructed **1.26.50.26 preview client**, Lens
- artifact 6, executable SHA-256
- `7d6cf9b2e4b01fce5d6283cc3deb65b877995a8fd1e146f967d8ac743369d628`.
- The older named 26.30 reconstruction identifies systems; formulas, current
- dispatch and constants were checked in the current reconstruction and matching
- PE. The code in Cinnabar is independently written.
- ## Current identities and ordering
- `SwimTriggerSystem::doTick` is current **RVA 0x09fd25a0** (named older body
- `0x053bf000`). Current ticking adapter **0x09fe1160** supplies this callback;
- its assertion signature identifies the system and its component arguments.
- Registration **0x072b6020** installs it through category-one registration,
- before horizontal-pose and vanilla-offset updates.
- Current category-one registration proceeds through **0x072ce0d0**, which
- registers `CurrentSwimAmountSystem`, then calls **0x072c5ec0**. That registers
- `InWaterSensingSystem` and `UnderWaterSensingSystem`, then calls **0x072bb110**.
- The latter calls **0x072b6020** to register player input, swimming trigger and
- pose updates before registering jumping and movement. The common registration
- routine is **0x070be000**. The collection appends identities in **0x028e64f0**
- and its ticker traverses forward in **0x028e8f00**.
- ## Native start and continuation
- Entry requires the head-water component, flight disabled, swimming clear,
- sprint intent (`PlayerInputRequest +0x08`) and clear sprint-direction rejection
- (`+0x0a`). The direction rejection comes from current sprint intent
- **0x0c5b6310**. Its ordinary desktop direction checks require movement magnitude
- at least `sqrt(0.5)`, positive forward input and absolute sideways input at most
- `sqrt(0.5)`. A separate native stall check compares retained position/input with
- the current position, using `0.00005`; Cinnabar does not yet retain that complete
- the hunger-stop request (`+0x10`) is clear, the player is unmounted (`+0x0b`),
- the desktop/touch sprint cancellation (`+0x09`) is clear and body water contact
- is present. This continuation does not require the actor's sprint flag or
- positive forward input. The hunger request is produced in **0x0c5b1b60**:
- missing hunger or hunger at most six requests a stop when flight permission is
- absent. The application uses the existing shared hunger threshold.
- the swim. For air at attach seven, native retains the swim when
- surface behavior. The native lookup uses the float sine table for the negative
- When continuation fails, native emits stop-swimming only if the standing fit
- boolean (`+0x0c`) is true. There is no unconditional grounded stop in this
- function. A blocked standing probe therefore retains swimming and its low box,
- including on land; dry travel must still apply ordinary land/air forces.
- Cinnabar formerly relabeled this condition as crawling and also ended a swim
- whenever sprint or forward input cleared.
- Current sprint intent **0x0c5b6310** skips its stop action when the preceding
- swimming flag and current body-water flag are set. This preserves an existing
- sprint through backward/sideways movement, sneak and released sprint input;
- ordinary valid starts remain possible. Because sprint intent precedes swimming
- trigger, even the first stop-swimming tick retains sprint while the old box is
- wet. Cinnabar retains the actual flag with its controller frames, applies the
- same selection during correction replay, and shares that flag across water drag,
- `UnderWaterSensingSystem::doUnderWaterSensing` is current **0x09fd7c00**,
- dispatched by **0x09fd75f0**. It samples attach location seven at interpolation
- zero, requiring the primary water material. Its strict eye comparison uses
- then performs the native float comparison. The application passes its captured
- Bounding-box input update **0x09eeeb70** writes the standing/sneaking/low fit
- flags. Its standing probe uses the existing feet and standing collision height,
- with all faces inset by `0.01`. `sim::pose_fits` now shares this rule; its old
- Pose-size transform **0x02c33550** sets horizontal-pose height to collision
- width. Applying the request in **0x02c33600** preserves the previous AABB minimum
- Y and changes maximum Y to minimum Y plus requested height. It does not shift
- the feet or write StateVector Y. Vanilla offset affects the attach/camera point;
- standing-up itself does not justify changing the network feet anchor.
- ## PE constants and remaining scope
- | PE VA | Float | Consumer |
- | --- | ---: | --- |
- | `0x14feff2ac` | `0.707106769` | Swim and sprint movement threshold |
- | `0x14ffab6c8` | `0.150000006` | Upward swim-entry target limit |
- | `0x14ffd5070` | `57.2957764` | Native radians-to-degrees multiply |
- | `0x15013adc8` | `45` | Surface continuation angle |
- | `0x1503dcba0` | `0.0000499999987` | Sprint direction stall comparison |
- | `0x150064950` | `9` | Head-water liquid-level divisor |
- | `0x150167290` | `-0.111111112` | Head-water level offset |
- | `0x1500c2b70` | `0.00999999978` | Fit probe lower-face inset |
- | `0x1500dfa68` | `-0.00999999978` | Fit probe upper-face inset |
- The focused regressions cover native source/flowing head heights, primary-layer

## docs/reference/third-person-camera.md
- `CameraAPI::tryGetActorInterpolatedPosition` passes its render
- fraction to `VanillaOffsetSystem::getCameraPosition`. The latter
- starts from `Actor::getInterpolatedRidingPosition` and applies the interpolated
- eye/stance offsets. `Actor::getActorToWorldTransform` starts from
- the same interpolated riding position at the same render fraction.
- `LevelRendererPlayer::bobView` independently interpolates walk
- `CameraAttachSystem::_handleLookInput` uses polar/elevation and
- azimuth input, in that order. Its `invert_x_input` flips the polar component;
- pitch when moving to the opposite side of the subject. The look-at update
- (`CameraLookAtSystemUtil::_lookAtSystem`) uses global Y as its

## docs/tracking/vanilla-parity-gaps.md
- `animation.player.first_person.*` (public samples) + `ItemInHandRenderer` transforms.
- | 0 / 92 flags | yes | every `is_*` query, on-fire camera overlay, invisible body (NoDraw; armor and held items stay, as `shouldHideHeldItems` returns false), show/always-show name, sneak tag dimming, sleeping, riding layouts (saddled, baby, tamed, sheared) |
- Missing presentation that the flags drive: the entity flame billboard (`ActorRenderer::renderFlame`)

## plan.md
- Current TopSnow routes through ordinary terrain into AO/flat lighting. The real model-fragment Metal regression reproduced
- ice was CUTOUT despite source alpha 190/255. Native IceBlock selects
- local, uncommitted, not pushed): current LeavesBlock and
- Current TextureAtlas::updateTextureAtUVs and _buildAtlasMips establish
- orientation and sampler contracts. Current BlockType::getShadeBrightness
- AmbientOcclusionCalculator averages outward/side/diagonal shade and raises
- close, not a Rust panic. Named vanilla
- CraftingContainerManagerController::_makeCreateItemScopeCreative
- and the result-action constructor declare the
- vanilla Wolf::getTailAngle behaviour: angry overrides tame,
- not pushed): current SeasonsRenderer palette generation and native
- BlockReplaceableComponent coverage beyond the identified native ground plants
- Native animateTick/LeavesBlock ambient particle sampling, independent fixed-tick
- and broader block animateTick callbacks remain incomplete. Falling-leaf parity
- iOS 1.26.51 item archive; current Item::initClient performs
- component-icon loading. This is labeled a near-version asset witness, not an
- the named Geometry parser and official schema; real pig/cow/sheep model tests
- hotbar and hand, connected cold pig/cow bodies, and snowy grass sides. Current
- GrassBlock::calcVariant selects the alternate side for TopSnow,
- appends the sheared base head's cubes. Native Geometry::_parseBones appends cubes unless reset is authored. The shared inherited-cube
- now retains snow occlusion and the plant's own render layer. Native TopSnow uses terrain tessellation and separate visual bounds.
- Vanilla `LevelBuilder::tryRebuild` uses a radius-16 X/Z availability check:
- unbound `$vars` in `ignored`/`requires` read as null like `UIEval::evalVariable`.
- The pack confirms chat notification 10s and toast notification 3s defaults. Current OptionRegistry defaults remain unconfirmed. FOV, gamma, sensitivities, FPS limits and added boolean defaults therefore
- selected item is that block (`LevelRendererCamera::render`); those layers are not built.
- `getCloudColor` day/weather/sunrise colour with alpha 0.7, drift 0.02
- boat and horse "client predicted" systems (`SetIsClientPredictedBoatSystem`,
- `SetIsClientPredictedHorseSystem`) plus boat paddle/move/friction systems, and a boat's
- `setIconIfLegacy` call in the 26.30 client's `VanillaItems::initClientData`, plus
- potion icons by aux, each row citing its call site. Still open: cooked foods and
- candles, chains) draw `BlockItem::getIconInfo`'s icon: carried texture, else
- slots and use durations. Item glint (provisional): stacks `Item::isGlint`
- marks (an `ench` list, the glint component, always-glinting vanilla items)
- OptionRegistry numeric defaults remain provisional pending current-client confirmation. Cloud and hand preferences do not modify the JSON-UI engine.
- world-item, or entity-glint parity. Current OptionRegistry defaults/ranges remain open.
- Vanilla selects creative destruction from `Actor::isCreative`, not that
- Spawn/motion vectors and interpolation follow the native StateVector velocity
- Current SkullBlockRenderer reads light at the placed skull's integer

## tools/jsonui-editor/src/lib.rs
- // `SceneFactory::_createSafeZoneSizeVar` with a full safe zone: a zero

## tools/registrygen/block_v2193_light.go
- // DynamicLiquidBlock final registration has
- // no version gate: still and flowing types differ.

## tools/registrygen/block_v2193_light_test.go
- // 1.26.50.26 TopSnowBlock sets dampening to zero; the
- // inherited getter and per-height component override do not change it.

## crates/render-model/src/entity_shadow.rs
- Volume mesh: `PrefabMeshGenerator::buildShadowVolume` (13 segments, rings 0.25 at y −3 and 0.75 at y 0.01; scaled per instance by radius in `_insertVanillaShadows`).
- Colour: `ShadowColor` uniform built in `LevelRendererPlayer::createViewRenderObject` from `DimensionClientUtils::getInterpolatedSkyColor` and `getSunriseColor` (constants 0.5/0.4 tint, Rec. 709 luminance, 0.7 grey, 0.03 span).
- Blend and overlap: `ShadowVolume` back/front stencil passes then the `ShadowOverlay` pass (`shadow_back`, `shadow_front`, `shadow_overlay` materials).

## crates/client-world/src/actor_store/shadow.rs
- Radius: `Actor::getShadowRadius` (AABBShapeComponent width) and overrides on Ghast, HappyGhast, Creaking, Spider (CaveSpider), EvocationFang, Armadillo, Horse, EnderDragon, Tadpole, IronGolem, Shulker, Turtle, Slime (LavaSlime, SulfurCube), TripodCamera, EnderCrystal, Boat (ChestBoat), Parrot, Player, and the zero-radius classes (ArmorStand, AreaEffectCloud, FishingHook, Minecart family, ExperienceOrb, LeashKnot, EyeOfEnder, LightningBolt, PrimedTnt, FallingBlockActor, FireworksRocketActor, Painting).
- Admission: `createViewRenderObject` caster loop: `isAlive`, radius > 0, `!isOnFire`, `!isUnderLiquid(Any)` at attach location 7, `!isInvisible`, ActorType projectile bit, vehicle `isInvisible`.
- Drop: `RelativeShadowOffsetComponent` (Ghast −0.875, HappyGhast −0.5) × ActorDataBoundingBoxComponent height × scale.
- Projectile identifiers: `VanillaActorRegistryAnon` factory table (types with bit 0x400000).

## crates/sim/src/simulator/water.rs
- `sample_liquid_submersion`: `ActorMobilityUtils::isUnderLiquid` with MaterialType Any.

## Primitive shapes: protocol, state and reference rules

Files: `crates/protocol/src/primitive_shapes.rs`,
`crates/render-api/src/primitive_shapes.rs`,
`crates/render-model/src/primitive_shapes/`, `docs/reference/primitive-shapes.md`.

Current Lens function reads corroborated the older owner-organized lookup files under
`~/coding/go/lunar/refs/mcsrc-1.26.50/reference/26.30/src/by-owner/`; the directory label alone
was not used as version evidence.

- `ClientNetworkHandler::handle(PrimitiveShapesPacket)` `0x103541c60`: client dispatch.
- `ClientScriptPrimitiveShapesDataComponent::handlePacket` `0x1022bab10`: ordered id lookup,
  absent-type removal, present-type creation/update, no explicit count cap.
- `PrimitiveShapeDataPayload::constructShape` `0x1065c93e0`: kinds and creation defaults.
- `ScriptPrimitiveShape::applyUpdatedData` `0x10918c750`: optional patches, zero lifetime,
  negative-distance reset, dimension and actor unique id.
- `ScriptSpherePrimitive::applyUpdatedData` `0x109191530` and
  `ScriptCirclePrimitive::applyUpdatedData` `0x109192b50`: byte segment count.
- `ScriptArrowPrimitive` constructor `0x109195740` and updater `0x109195af0`:
  independent optional head and endpoint fields; `0x10d986350` f32 pair `[0.5, 1.0]` gives
  default radius and length.
- `ScriptTextPrimitive` constructor `0x109192c20` and updater `0x109193240`: default options,
  text-object parsing, complete text option replacement and background clearing.
- `ClientScriptPrimitiveShapesSystem::tick` `0x1022bc960`: dimension filtering, missing-actor
  suppression and render-helper construction; no local lifetime decrement.
- Server primitive system tick adapter `0x10538dc30`: monotonic elapsed-time subtraction and
  removal at remaining lifetime `<= 0`.
- `serialize<mce::Color>::read` `0x1066bf090`, `cerealizer<mce::Color>::bind` `0x106e63a70`:
  ARGB channel order in the four-byte wire integer.
- `Scripting::RenderHelper::Renderer::convertStringsToNameTags` `0x104449a80`: literal
  backslash-n replacement, discard-empty line splitting and integer half widths.

## Primitive shape rendering

- `crates/render/src/primitive_shapes/mesh.rs`: current Lens `Scripting::RenderHelper::LinePrimitive::_rebuild` at `0x10443dab0`, `BoxPrimitive::_rebuild` at `0x10443d540`, `DiscPrimitive::_rebuild` at `0x10443dce0`, `AxialSpherePrimitive::_rebuild` at `0x10443df80`, `ArrowPrimitive::_rebuild` at `0x10443e290`, and `generateDiscVerts` at `0x10443ed90`.
- `crates/render/src/primitive_shapes/pipeline.rs`: current Lens `Scripting::RenderHelper::Renderer::onEndRender` at `0x104447e40` submits line-list vertices through the `debug` material. Installed PlayCover `data/resource_packs/vanilla/materials/ui3D.material` lines 454–466 corroborate LessEqual, default depth write and no blending; the installed asset version is 1.26.51.01, not a matched 1.26.50 witness.
- `crates/render/src/primitive_shapes/shapes.wgsl`: `BasePrimitive::getAttachedToPosition` uses interpolated riding position; `Renderer::convertStringsToNameTags` at `0x104449a80` forwards text to `BaseActorRenderer::extractRenderTextObjects` at `0x103dc03c0` and `_extractRenderTextObject` at `0x103dc0840`. `LevelNameTagRenderer::renderText` at `0x103eb9050` applies incoming scale times 1.6 times 1/60 and fixed 0.125-per-extra-line lift; data reads at `0x10d9a15a0`, `0x10db88db4`, `0x10d91dae0` confirmed these constants.
- Local files consulted are the matching owner files under `~/coding/go/lunar/refs/mcsrc-1.26.50/reference/26.30/src/by-owner`; their catalog is labeled 26.30. Current function reads corroborate `Renderer::onEndRender` and `convertStringsToNameTags`; full material/geometry capture parity remains open.

- `crates/render-model/src/primitive_shapes/state.rs`: current `ClientScriptPrimitiveShapesSystem::tick` `0x1022bc960` text quaternion is `Rz * Ry * Rx`; data at `0x10d972210` and `0x10d8ec9d8` are degree-to-radian and half-angle constants.
- `crates/render/src/primitive_shapes/shapes.wgsl`: current `Renderer::onBeginRender` `0x104446fa0` performs strict squared-distance comparison against the optional shape range or context range. `LevelRenderer::renderLevel` `0x10436ad94` passes the range computed by `LevelRendererPlayer::recalculateRenderDistance` `0x10439a330`, already mirrored by the cloud distance helper. Vtable reads at `0x110c96290` and `0x110c96360` select `BasePrimitivePosition::getPosition` `0x10443d500`; box tick construction supplies its lower corner.
- `crates/client-presentation/src/primitive_shapes.rs`: `BasePrimitive::getAttachedToPosition` `0x10443d390` gets interpolated riding position and subtracts `OffsetsComponent` vertical offset; existing actor-store snapshots already normalize feet positions and seat riders before interpolation.

- Primitive draw ordering: `Renderer::onBeginRender` comparator in `__sort3` `0x104479810` compares signed `BasePrimitive + 0xc`; packet helper construction in `ClientScriptPrimitiveShapesSystem::tick` `0x1022bc960` initializes that priority to zero for all supported geometry. The unstable introsort does not order by kind or distance.

- `crates/client-ui/src/ui_runtime/presentation/primitive_shapes.rs`: current `ScriptTextPrimitive::applyUpdatedData` `0x109193240` retains parsed `TextObjectRoot` or literal; `Renderer::onBeginRender` `0x104446fa0` resolves only when the helper dirty flag or player input/interaction mode changes. Domain dynamic-text markers preserve common-patch refresh without rebuilding literal text geometry.

- Equal packet updates: current `ClientScriptPrimitiveShapesDataComponent::handlePacket` `0x1022bab10` unconditionally marks an existing present-type entry dirty after its updater, with no equality check. `generateDiscVerts` `0x10443ed90` zero-segment branch initializes both closing vertices and packed colors to zero, then appends the closing pair unconditionally.
