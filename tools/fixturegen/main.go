package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const (
	gameVersion     = "1.26.50"
	protocolID      = 2193
	senderSubClient = 1
	targetSubClient = 2
)

type fixture struct {
	name          string
	file          string
	pk            packet.Packet
	wireAuthority string
	wireCommit    string
}

type manifestEntry struct {
	Name          string `json:"name"`
	File          string `json:"file"`
	ID            uint32 `json:"id"`
	ByteLength    int    `json:"byte_length"`
	SHA256        string `json:"sha256"`
	WireAuthority string `json:"wire_authority,omitempty"`
	WireCommit    string `json:"wire_commit,omitempty"`
}

func main() {
	out := flag.String("out", "", "directory to write protocol fixtures")
	idsOut := flag.String("ids-out", "", "file to write protocol ID constants")
	flag.Parse()
	if *idsOut != "" {
		if err := writeProtocolIDs(*idsOut); err != nil {
			fmt.Fprintf(os.Stderr, "fixturegen: %v\n", err)
			os.Exit(1)
		}
		if *out == "" {
			return
		}
	}
	if *out == "" {
		fmt.Fprintln(os.Stderr, "fixturegen: -out is required")
		os.Exit(2)
	}
	if err := generate(*out); err != nil {
		fmt.Fprintf(os.Stderr, "fixturegen: %v\n", err)
		os.Exit(1)
	}
}

func generate(out string) error {
	if minecraft.DefaultProtocol.ID() != protocolID || minecraft.DefaultProtocol.Ver() != gameVersion {
		return fmt.Errorf(
			"gophertunnel protocol drift: got %d/%s, want %d/%s",
			minecraft.DefaultProtocol.ID(), minecraft.DefaultProtocol.Ver(), protocolID, gameVersion,
		)
	}
	if out == "" {
		return errors.New("output directory is empty")
	}
	if err := os.MkdirAll(out, 0o755); err != nil {
		return fmt.Errorf("create output directory: %w", err)
	}

	manifest := make([]manifestEntry, 0, len(fixtures()))
	for _, fixture := range fixtures() {
		encoded, err := encode(fixture.pk)
		if err != nil {
			return fmt.Errorf("encode %s: %w", fixture.name, err)
		}
		path := filepath.Join(out, fixture.file)
		if err := os.WriteFile(path, encoded, 0o644); err != nil {
			return fmt.Errorf("write %s: %w", path, err)
		}
		digest := sha256.Sum256(encoded)
		manifest = append(manifest, manifestEntry{
			Name:          fixture.name,
			File:          fixture.file,
			ID:            fixture.pk.ID(),
			ByteLength:    len(encoded),
			SHA256:        hex.EncodeToString(digest[:]),
			WireAuthority: fixture.wireAuthority,
			WireCommit:    fixture.wireCommit,
		})
	}

	encodedManifest, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return fmt.Errorf("encode manifest: %w", err)
	}
	encodedManifest = append(encodedManifest, '\n')
	manifestPath := filepath.Join(out, "manifest.json")
	if err := os.WriteFile(manifestPath, encodedManifest, 0o644); err != nil {
		return fmt.Errorf("write %s: %w", manifestPath, err)
	}
	return nil
}

func fixtures() []fixture {
	return append([]fixture{
		{
			name: "NetworkSettings",
			file: "network_settings.bin",
			pk: &packet.NetworkSettings{
				CompressionThreshold:    512,
				CompressionAlgorithm:    packet.CompressionAlgorithmFlate,
				ClientThrottle:          true,
				ClientThrottleThreshold: 8,
				ClientThrottleScalar:    0.5,
			},
		},
		{
			name: "StartGame",
			file: "start_game.bin",
			pk: &packet.StartGame{
				EntityUniqueID:        1,
				EntityRuntimeID:       2,
				PlayerGameMode:        1,
				PlayerPosition:        mgl32.Vec3{1.25, 64, -2.5},
				Pitch:                 10.5,
				Yaw:                   20.25,
				WorldSeed:             12345,
				SpawnBiomeType:        packet.SpawnBiomeTypeDefault,
				UserDefinedBiomeName:  "plains",
				Dimension:             0,
				Generator:             1,
				WorldGameMode:         0,
				Hardcore:              false,
				Difficulty:            1,
				WorldSpawn:            protocol.BlockPos{8, 64, -8},
				AchievementsDisabled:  false,
				EditorWorldType:       packet.EditorWorldTypeNotEditor,
				MultiPlayerGame:       true,
				LANBroadcastEnabled:   true,
				CommandsEnabled:       true,
				PlayerPermissions:     1,
				ServerChunkTickRadius: 4,
				BaseGameVersion:       gameVersion,
				NewNether:             true,
				ChatRestrictionLevel:  packet.ChatRestrictionLevelNone,
				LevelID:               "fixture-level",
				WorldName:             "Fixture World",
				PlayerMovementSettings: protocol.PlayerMovementSettings{
					RewindHistorySize:                20,
					ServerAuthoritativeBlockBreaking: true,
				},
				Time:                         123456789,
				EnchantmentSeed:              12345,
				MultiPlayerCorrelationID:     "00000000-0000-0000-0000-000000000001",
				ServerAuthoritativeInventory: true,
				GameVersion:                  gameVersion,
				PropertyData: map[string]any{
					"gophertunnel:test": int32(1),
				},
				UseBlockNetworkIDHashes: true,
			},
		},
		{
			name: "LevelChunk",
			file: "level_chunk.bin",
			pk: &packet.LevelChunk{
				Position:      protocol.ChunkPos{3, -4},
				Dimension:     0,
				SubChunkCount: 0,
				// 1.26.40 replaced the SubChunkRequestMode sentinels stored in
				// SubChunkCount (and the uint16 HighestSubChunk that followed
				// the "limited" sentinel) with a plain count plus an optional
				// varint32 sub-chunk limit. The blob hash slice is now written
				// unconditionally rather than only when CacheEnabled is set.
				SubChunkLimit: protocol.Option(int32(24)),
				CacheEnabled:  false,
				RawPayload:    []byte{0xde, 0xad, 0xbe, 0xef},
			},
		},
		{
			name: "MovePlayer",
			file: "move_player.bin",
			pk: &packet.MovePlayer{
				EntityRuntimeID:       42,
				Position:              mgl32.Vec3{1.25, 64, -2.5},
				Pitch:                 10.5,
				Yaw:                   20.25,
				HeadYaw:               30.75,
				Mode:                  packet.MoveModeTeleport,
				OnGround:              true,
				RiddenEntityRuntimeID: 0,
				// 1.26.40 moved the two teleport int32s behind an optional
				// TeleportData block instead of gating them on Mode.
				TeleportData: protocol.Option(protocol.TeleportData{
					TeleportCause:            packet.TeleportCauseCommand,
					TeleportSourceEntityType: 87,
				}),
				Tick: 1234,
			},
		},
		{
			name: "PlayerAuthInput",
			file: "player_auth_input.bin",
			pk:   playerAuthInputFixture(),
		},
		{
			name: "PlayerAuthInputBlockActions",
			file: "player_auth_input_block_actions.bin",
			pk:   playerAuthInputBlockActionsFixture(),
		},
		{
			name: "PlayerAuthInputBreakBlock",
			file: "player_auth_input_break_block.bin",
			pk:   playerAuthInputBreakBlockFixture(),
		},
		{
			name: "PlayerAuthInputUseBlock",
			file: "player_auth_input_use_block.bin",
			pk:   playerAuthInputUseBlockFixture(),
		},
		{
			name: "PlayerAuthInputBlockActionsAndBreakBlock",
			file: "player_auth_input_block_actions_and_break_block.bin",
			pk:   playerAuthInputBlockActionsAndBreakBlockFixture(),
		},
		{
			name: "PlayerAuthInputMineBlock",
			file: "player_auth_input_mine_block.bin",
			pk:   playerAuthInputMineBlockFixture(false),
		},
		{
			name: "PlayerAuthInputMineBlockAndPredict",
			file: "player_auth_input_mine_block_and_predict.bin",
			pk:   playerAuthInputMineBlockFixture(true),
		},
		{
			name: "AddActor",
			file: "add_actor.bin",
			pk: &packet.AddActor{
				EntityUniqueID:  -77,
				EntityRuntimeID: 77,
				EntityType:      "minecraft:pig",
				Position:        mgl32.Vec3{2, 65, -3},
				Velocity:        mgl32.Vec3{0.1, 0.2, 0.3},
				Pitch:           1,
				Yaw:             2,
				HeadYaw:         3,
				BodyYaw:         4,
				EntityMetadata:  protocol.EntityMetadata{},
			},
		},
		{
			name: "Text",
			file: "text.bin",
			pk: &packet.Text{
				TextType:         packet.TextTypeRaw,
				Message:          "§aFixture message",
				XUID:             "",
				PlatformChatID:   "",
				NeedsTranslation: false,
			},
		},
		{
			name: "TextObjectRawText",
			file: "text_object_rawtext.bin",
			pk: &packet.Text{
				TextType:         packet.TextTypeObject,
				Message:          `{"rawtext":[{"text":"\u00a7aLBSG "},{"rawtext":[{"text":"human chat"}]}]}`,
				XUID:             "",
				PlatformChatID:   "",
				NeedsTranslation: false,
			},
		},
		{
			name: "TextObjectWhisperRawText",
			file: "text_object_whisper_rawtext.bin",
			pk: &packet.Text{
				TextType:         packet.TextTypeObjectWhisper,
				Message:          `{"rawtext":[{"text":"private "},{"translate":"chat.type.text","with":["Alice",{"rawtext":[{"text":"hello"}]}]}]}`,
				XUID:             "",
				PlatformChatID:   "",
				NeedsTranslation: false,
			},
		},
		{
			name: "TextObjectAnnouncementRawText",
			file: "text_object_announcement_rawtext.bin",
			pk: &packet.Text{
				TextType:         packet.TextTypeObjectAnnouncement,
				Message:          `{"rawtext":[{"text":"Announcement"}]}`,
				XUID:             "",
				PlatformChatID:   "",
				NeedsTranslation: false,
			},
		},
		{
			name: "SetTitle",
			file: "set_title.bin",
			pk: &packet.SetTitle{
				ActionType:       packet.TitleActionSetTitle,
				Text:             "Fixture title",
				FadeInDuration:   5,
				RemainDuration:   40,
				FadeOutDuration:  10,
				XUID:             "",
				PlatformOnlineID: "",
				FilteredMessage:  "",
			},
		},
		{
			name: "BossEvent",
			file: "boss_event.bin",
			pk: &packet.BossEvent{
				BossEntityUniqueID: 77,
				EventType:          packet.BossEventShow,
				BossBarTitle:       "Fixture boss",
				HealthPercentage:   0.75,
				Colour:             packet.BossEventColourRebeccaPurple,
				Overlay:            packet.BossEventOverlayNotched10,
			},
		},
		{
			name: "ModalFormRequest",
			file: "modal_form_request.bin",
			pk: &packet.ModalFormRequest{
				FormID:   91,
				FormData: []byte(`{"type":"form","title":"Fixture"}`),
			},
		},
		{
			name: "ModalFormTextMenu",
			file: "modal_form_text_menu.bin",
			pk: &packet.ModalFormRequest{
				FormID:   92,
				FormData: []byte(`{"type":"form","title":"Choose 世界","content":"Pick one\nα β","buttons":[{"text":"First ✓"},{"text":"第二"}]}`),
			},
		},
		{
			name: "ModalFormResponseButton",
			file: "modal_form_response_button.bin",
			pk:   &packet.ModalFormResponse{FormID: 92, ResponseData: protocol.Option([]byte("1\n"))},
		},
		{
			name: "ModalFormResponseClosed",
			file: "modal_form_response_closed.bin",
			pk:   &packet.ModalFormResponse{FormID: 92, CancelReason: protocol.Option(uint8(packet.ModalFormCancelReasonUserClosed))},
		},
		{
			name: "ModalFormResponseBusy",
			file: "modal_form_response_busy.bin",
			pk:   &packet.ModalFormResponse{FormID: 92, CancelReason: protocol.Option(uint8(packet.ModalFormCancelReasonUserBusy))},
		},
		{
			name: "AvailableCommands",
			file: "available_commands.bin",
			pk:   availableCommandsFixture(),
		},
		{
			name: "AvailableCommandsLive356513",
			file: "available_commands_live_356513.bin",
			pk:   availableCommandsLiveRegression(),
		},
		{
			name: "BiomeDefinitionListChunkGeneration",
			file: "biome_definition_list_chunk_generation.bin",
			pk: &packet.BiomeDefinitionList{
				BiomeDefinitions: []protocol.BiomeDefinition{
					{
						ChunkGeneration: protocol.Option(protocol.BiomeChunkGeneration{}),
					},
				},
			},
			wireAuthority: "hashimthearab/gophertunnel",
			wireCommit:    "9f42f3679a573fc4b51104569cc4f422036e28ec",
		},
		{
			name: "InventoryContent",
			file: "inventory_content.bin",
			pk: &packet.InventoryContent{
				WindowID: 0,
				Content:  []protocol.ItemInstance{inventoryItem(5, 2, 11)},
				Container: protocol.FullContainerName{
					ContainerID:        12,
					DynamicContainerID: protocol.Option(uint32(7)),
				},
			},
		},
		{
			name: "InventorySlot",
			file: "inventory_slot.bin",
			pk: &packet.InventorySlot{
				WindowID: 0,
				Slot:     4,
				Container: protocol.Option(protocol.FullContainerName{
					ContainerID: 29,
				}),
				NewItem: inventoryItem(6, 3, 12),
			},
		},
		{
			name: "PlayerHotBar",
			file: "player_hotbar.bin",
			pk: &packet.PlayerHotBar{
				SelectedHotBarSlot: 4,
				WindowID:           0,
				SelectHotBarSlot:   true,
			},
		},
		{
			name: "ItemStackResponse",
			file: "item_stack_response.bin",
			pk: &packet.ItemStackResponse{
				Responses: []protocol.ItemStackResponse{
					{
						Status:    0,
						RequestID: 44,
						ContainerInfo: []protocol.StackResponseContainerInfo{
							{
								Container: protocol.FullContainerName{ContainerID: 28},
								SlotInfo: []protocol.StackResponseSlotInfo{
									{
										Slot:                 2,
										HotbarSlot:           2,
										Count:                5,
										StackNetworkID:       13,
										CustomName:           "Fixture item",
										FilteredCustomName:   protocol.Option("Fixture item"),
										DurabilityCorrection: -3,
									},
								},
							},
						},
					},
				},
			},
		},
		{
			name: "InventoryTransactionClickBlock",
			file: "inventory_transaction_click_block.bin",
			pk: &packet.InventoryTransaction{
				TransactionData: &protocol.UseItemTransactionData{
					ActionType:          protocol.UseItemActionClickBlock,
					TriggerType:         protocol.TriggerTypePlayerInput,
					BlockPosition:       protocol.BlockPos{13, 71, -29},
					BlockFace:           5,
					HotBarSlot:          7,
					HeldItem:            inventoryItem(5, 2, 11),
					Position:            mgl32.Vec3{13.25, 72.625, -28.75},
					ClickedPosition:     mgl32.Vec3{0.125, 0.875, 0.625},
					BlockRuntimeID:      123_456,
					ClientPrediction:    protocol.ClientPredictionFailure,
					ClientCooldownState: protocol.ClientCooldownStateOff,
				},
			},
		},
		{
			name: "InventoryTransactionClickBlockEmptyHand",
			file: "inventory_transaction_click_block_empty_hand.bin",
			pk: &packet.InventoryTransaction{
				TransactionData: &protocol.UseItemTransactionData{
					ActionType:          protocol.UseItemActionClickBlock,
					TriggerType:         protocol.TriggerTypePlayerInput,
					BlockPosition:       protocol.BlockPos{-8, 63, 21},
					BlockFace:           0,
					HotBarSlot:          0,
					HeldItem:            protocol.ItemInstance{},
					Position:            mgl32.Vec3{-7.75, 64.5, 21.875},
					ClickedPosition:     mgl32.Vec3{0.75, 0.25, 0.5},
					BlockRuntimeID:      ^uint32(0),
					ClientPrediction:    protocol.ClientPredictionFailure,
					ClientCooldownState: protocol.ClientCooldownStateOff,
				},
			},
		},
		{
			name: "InventoryTransactionDestroyBlock",
			file: "inventory_transaction_destroy_block.bin",
			pk: &packet.InventoryTransaction{
				TransactionData: &protocol.UseItemTransactionData{
					ActionType:          protocol.UseItemActionBreakBlock,
					TriggerType:         protocol.TriggerTypePlayerInput,
					BlockPosition:       protocol.BlockPos{24, 68, -41},
					BlockFace:           3,
					HotBarSlot:          5,
					HeldItem:            inventoryItem(9, 3, 15),
					Position:            mgl32.Vec3{24.625, 69.5, -40.125},
					ClickedPosition:     mgl32.Vec3{0.625, 0.375, 0.875},
					BlockRuntimeID:      654_321,
					ClientPrediction:    protocol.ClientPredictionFailure,
					ClientCooldownState: protocol.ClientCooldownStateOff,
				},
			},
			wireAuthority: "hashimthearab/gophertunnel",
			wireCommit:    "b725d82563e93308fd1f92d27da5e97301ad5040",
		},
		{
			name: "InventoryTransactionDestroyBlockEmptyHand",
			file: "inventory_transaction_destroy_block_empty_hand.bin",
			pk: &packet.InventoryTransaction{
				TransactionData: &protocol.UseItemTransactionData{
					ActionType:          protocol.UseItemActionBreakBlock,
					TriggerType:         protocol.TriggerTypePlayerInput,
					BlockPosition:       protocol.BlockPos{-17, 92, 6},
					BlockFace:           1,
					HotBarSlot:          0,
					HeldItem:            protocol.ItemInstance{},
					Position:            mgl32.Vec3{-16.5, 93.625, 6.25},
					ClickedPosition:     mgl32.Vec3{0.5, 1, 0.25},
					BlockRuntimeID:      ^uint32(0),
					ClientPrediction:    protocol.ClientPredictionFailure,
					ClientCooldownState: protocol.ClientCooldownStateOff,
				},
			},
			wireAuthority: "hashimthearab/gophertunnel",
			wireCommit:    "b725d82563e93308fd1f92d27da5e97301ad5040",
		},
		{
			name: "InventoryTransactionAttackActor",
			file: "inventory_transaction_attack_actor.bin",
			pk: &packet.InventoryTransaction{
				TransactionData: &protocol.UseItemOnEntityTransactionData{
					TargetEntityRuntimeID: 0x0102_0304_0506_0708,
					ActionType:            protocol.UseItemOnEntityActionAttack,
					HotBarSlot:            8,
					HeldItem:              inventoryItem(7, 1, 13),
					Position:              mgl32.Vec3{10.25, 65.625, -4.75},
					ClickedPosition:       mgl32.Vec3{0.375, 1.25, -0.125},
				},
			},
		},
		{
			name: "InventoryTransactionAttackActorEmptyHand",
			file: "inventory_transaction_attack_actor_empty_hand.bin",
			pk: &packet.InventoryTransaction{
				TransactionData: &protocol.UseItemOnEntityTransactionData{
					TargetEntityRuntimeID: ^uint64(0),
					ActionType:            protocol.UseItemOnEntityActionAttack,
					HotBarSlot:            0,
					HeldItem:              protocol.ItemInstance{},
					Position:              mgl32.Vec3{-12.5, 70, 31.75},
					ClickedPosition:       mgl32.Vec3{-0.5, 0.625, 1.5},
				},
			},
		},
		{
			name: "InventoryTransactionInteractActor",
			file: "inventory_transaction_interact_actor.bin",
			pk: &packet.InventoryTransaction{
				TransactionData: &protocol.UseItemOnEntityTransactionData{
					TargetEntityRuntimeID: 123_456_789,
					ActionType:            protocol.UseItemOnEntityActionInteract,
					HotBarSlot:            3,
					HeldItem:              inventoryItem(8, 4, 14),
					Position:              mgl32.Vec3{2.5, 63.875, 9.125},
					ClickedPosition:       mgl32.Vec3{0.25, 0.75, 0.5},
				},
			},
		},
		{
			name: "InventoryTransactionInteractActorEmptyHand",
			file: "inventory_transaction_interact_actor_empty_hand.bin",
			pk: &packet.InventoryTransaction{
				TransactionData: &protocol.UseItemOnEntityTransactionData{
					TargetEntityRuntimeID: 1,
					ActionType:            protocol.UseItemOnEntityActionInteract,
					HotBarSlot:            6,
					HeldItem:              protocol.ItemInstance{},
					Position:              mgl32.Vec3{-1.25, 80.5, -16.75},
					ClickedPosition:       mgl32.Vec3{1.125, -0.25, 0.875},
				},
			},
		},
		{
			name: "ContainerClose",
			file: "container_close.bin",
			pk: &packet.ContainerClose{
				WindowID:      5,
				ContainerType: 0,
				ServerSide:    false,
			},
		},
		{
			name: "PlaySound",
			file: "play_sound.bin",
			pk: &packet.PlaySound{
				SoundName:                "custom:odd.sound",
				Position:                 mgl32.Vec3{1.25, -2.5, 3.875},
				Volume:                   -0.25,
				Pitch:                    3.5,
				LoopCount:                -7,
				BypassListenerRangeCheck: true,
				Handle:                   protocol.Option(uint64(0x0123_4567_89ab_cdef)),
				PlaybackPositionSeconds:  protocol.Option(float32(1.5)),
			},
			wireAuthority: "hashimthearab/gophertunnel",
			wireCommit:    "b725d82563e93308fd1f92d27da5e97301ad5040",
		},
		{
			name:          "StopSound",
			file:          "stop_sound.bin",
			pk:            &packet.StopSound{StopAll: true, StopMusicLegacy: true},
			wireAuthority: "hashimthearab/gophertunnel",
			wireCommit:    "0f3bd7e6f748ca972da664130af63244d625a6b8",
		},
		{
			name: "LevelSoundEvent",
			file: "level_sound_event.bin",
			pk: &packet.LevelSoundEvent{
				SoundType:             "custom:unmapped.event",
				Position:              mgl32.Vec3{-1.25, 64.5, 2.75},
				ExtraData:             -12345,
				BabyMob:               true,
				DisableRelativeVolume: true,
				EntityUniqueID:        -42,
				FireAtPosition:        protocol.Option(mgl32.Vec3{9.25, -4.5, 0.125}),
			},
			wireAuthority: "hashimthearab/gophertunnel",
			wireCommit:    "0f3bd7e6f748ca972da664130af63244d625a6b8",
		},
		{
			name: "DisconnectVisible",
			file: "disconnect_visible.bin",
			pk:   &packet.Disconnect{Reason: packet.DisconnectReasonKicked, Message: "Server closing"},
		},
		{
			name: "DisconnectFiltered",
			file: "disconnect_filtered.bin",
			pk:   &packet.Disconnect{Reason: -7, Message: "Original message", FilteredMessage: "Filtered message"},
		},
		{
			name: "DisconnectHidden",
			file: "disconnect_hidden.bin",
			pk:   &packet.Disconnect{Reason: packet.DisconnectReasonKicked, HideDisconnectionScreen: true},
		},
	}, manualCraftFixtures()...)
}

// Independently authored codec fixtures, not captured client requests.
func manualCraftFixtures() []fixture {
	shape := func(id uint32, width, height int32) protocol.ShapedRecipe {
		inputs := make([]protocol.ItemDescriptorCount, width*height)
		for i := range inputs {
			inputs[i] = protocol.ItemDescriptorCount{Descriptor: &protocol.DefaultItemDescriptor{Name: "minecraft:oak_log"}, Count: 1}
		}
		return protocol.ShapedRecipe{RecipeID: "test:manual", Width: width, Height: height, Input: inputs,
			Output: []protocol.ItemStack{{ItemType: protocol.ItemType{NetworkID: 7}, Count: 4}},
			Block:  "crafting_table", RecipeNetworkID: id}
	}
	source := protocol.StackRequestSlotInfo{Container: protocol.FullContainerName{ContainerID: 13}, Slot: 28, StackNetworkID: 101}
	consume := &protocol.ConsumeStackRequestAction{}
	consume.Count, consume.Source = 1, source
	take := &protocol.TakeStackRequestAction{}
	take.Count = 4
	take.Source = protocol.StackRequestSlotInfo{Container: protocol.FullContainerName{ContainerID: 60}, Slot: 50, StackNetworkID: -3}
	take.Destination = protocol.StackRequestSlotInfo{Container: protocol.FullContainerName{ContainerID: 59}, Slot: 0, StackNetworkID: 0}
	request := &packet.ItemStackRequest{Requests: []protocol.ItemStackRequest{{RequestID: -3, FilterCause: -1,
		Actions: []protocol.StackRequestAction{
			&protocol.CraftRecipeStackRequestAction{RecipeNetworkID: 17, NumberOfCrafts: 1},
			&protocol.CraftResultsDeprecatedStackRequestAction{ResultItems: []protocol.StackRequestItem{{Identifier: "minecraft:oak_planks", Count: 4}}, TimesCrafted: 1},
			consume, take,
		}}}}
	response := &packet.ItemStackResponse{Responses: []protocol.ItemStackResponse{
		{RequestID: -3, Status: 0, ContainerInfo: []protocol.StackResponseContainerInfo{
			{Container: protocol.FullContainerName{ContainerID: 13}, SlotInfo: []protocol.StackResponseSlotInfo{{Slot: 28, HotbarSlot: 28, Count: 0, StackNetworkID: 0}}},
			{Container: protocol.FullContainerName{ContainerID: 59}, SlotInfo: []protocol.StackResponseSlotInfo{{Slot: 0, Count: 4, StackNetworkID: 201}}},
		}},
		{RequestID: -5, Status: 1},
	}}
	return []fixture{
		{name: "CraftingDataManualNamed1x1", file: "crafting_data_manual_named_1x1.bin", pk: &packet.CraftingData{ShapedRecipes: []protocol.ShapedRecipe{shape(17, 1, 1)}, ClearRecipes: true}},
		{name: "CraftingDataManualNamed1x2", file: "crafting_data_manual_named_1x2.bin", pk: &packet.CraftingData{ShapedRecipes: []protocol.ShapedRecipe{shape(18, 1, 2)}}},
		{name: "CraftingDataManualUnsupportedReplacement", file: "crafting_data_manual_unsupported_replacement.bin", pk: &packet.CraftingData{ShapedRecipes: []protocol.ShapedRecipe{shape(17, 3, 1)}}},
		{name: "CraftingDataManualClearEmpty", file: "crafting_data_manual_clear_empty.bin", pk: &packet.CraftingData{ClearRecipes: true}},
		{name: "ItemStackRequestManualCraft", file: "item_stack_request_manual_craft.bin", pk: request},
		{name: "ItemStackResponseManualCraft", file: "item_stack_response_manual_craft.bin", pk: response},
	}
}

func inventoryItem(networkID int32, count uint16, stackNetworkID int32) protocol.ItemInstance {
	return protocol.ItemInstance{
		StackNetworkID: stackNetworkID,
		Stack: protocol.ItemStack{
			ItemType: protocol.ItemType{
				NetworkID:     networkID,
				MetadataValue: 3,
			},
			BlockRuntimeID: 91,
			Count:          count,
			NBTData:        map[string]any{"fixture": int32(1)},
		},
	}
}

func playerAuthInputFixture() *packet.PlayerAuthInput {
	// 1.26.40 sends the input flags as a list of set flag IDs rather than a
	// std::bitset, so the fixture builds a protocol.InputFlags instead of a
	// protocol.Bitset.
	flags := protocol.NewInputFlags(packet.InputFlagCount)
	for _, flag := range []int{
		packet.InputFlagJumping,
		packet.InputFlagUp,
		packet.InputFlagLeft,
		packet.InputFlagSprinting,
	} {
		flags.Set(flag)
	}
	return &packet.PlayerAuthInput{
		Pitch:              10.5,
		Yaw:                20.25,
		Position:           mgl32.Vec3{1.25, 64, -2.5},
		MoveVector:         mgl32.Vec2{-1, 1},
		HeadYaw:            30.75,
		InputData:          flags,
		InputMode:          packet.InputModeMouse,
		PlayMode:           packet.PlayModeNormal,
		InteractionModel:   packet.InteractionModelCrosshair,
		InteractPitch:      10.5,
		InteractYaw:        20.25,
		Tick:               1234,
		Delta:              mgl32.Vec3{0.25, 0, -0.5},
		AnalogueMoveVector: mgl32.Vec2{-1, 1},
		CameraOrientation:  mgl32.Vec3{0.25, -0.5, -0.75},
		RawMoveVector:      mgl32.Vec2{-1, 1},
	}
}

// Independently authored codec coverage; no completion applicability is implied.
func playerAuthInputMineBlockFixture(predict bool) *packet.PlayerAuthInput {
	pk := playerAuthInputFixture()
	pk.InputData.Set(packet.InputFlagPerformItemStackRequest)
	pk.ItemStackRequest = protocol.Option(protocol.ItemStackRequest{
		RequestID: -3,
		Actions: []protocol.StackRequestAction{&protocol.MineBlockStackRequestAction{
			HotbarSlot: 2, PredictedDurability: 7, StackNetworkID: 12345,
		}},
		FilterCause: -1,
	})
	if predict {
		pk.InputData.Set(packet.InputFlagPerformBlockActions)
		pk.BlockActions = protocol.Option([]protocol.PlayerBlockAction{{
			Action:   protocol.PlayerActionPredictDestroyBlock,
			BlockPos: protocol.BlockPos{13, 71, -29}, Face: 5,
		}})
	}
	return pk
}

// playerAuthInputBlockActionsFixture is the movement fixture plus the
// PerformBlockActions flag and two block actions: the survival start of a
// destroy and the local prediction that finishes one. Every block action
// writes its action, block position, and face unconditionally
// (protocol/player.go PlayerBlockAction.Marshal), which is the exact shape the
// Rust encoder must reproduce.
func playerAuthInputBlockActionsFixture() *packet.PlayerAuthInput {
	pk := playerAuthInputFixture()
	pk.InputData.Set(packet.InputFlagPerformBlockActions)
	pk.BlockActions = protocol.Option([]protocol.PlayerBlockAction{
		{
			Action:   protocol.PlayerActionStartBreak,
			BlockPos: protocol.BlockPos{13, 71, -29},
			Face:     5,
		},
		{
			Action:   protocol.PlayerActionPredictDestroyBlock,
			BlockPos: protocol.BlockPos{-8, 63, 21},
			Face:     1,
		},
	})
	return pk
}

// playerAuthInputBreakBlockFixture is the movement fixture plus the
// PerformItemInteraction flag and an embedded break-block item-use
// transaction, the creative instant-destroy carrier. Its field values mirror
// the standalone InventoryTransactionDestroyBlock fixture so both carriers are
// cross-checkable byte for byte.
func playerAuthInputBreakBlockFixture() *packet.PlayerAuthInput {
	pk := playerAuthInputFixture()
	pk.InputData.Set(packet.InputFlagPerformItemInteraction)
	pk.ItemInteractionData = protocol.Option(protocol.UseItemTransactionData{
		ActionType:          protocol.UseItemActionBreakBlock,
		TriggerType:         protocol.TriggerTypePlayerInput,
		BlockPosition:       protocol.BlockPos{24, 68, -41},
		BlockFace:           3,
		HotBarSlot:          5,
		HeldItem:            inventoryItem(9, 3, 15),
		Position:            mgl32.Vec3{24.625, 69.5, -40.125},
		ClickedPosition:     mgl32.Vec3{0.625, 0.375, 0.875},
		BlockRuntimeID:      654_321,
		ClientPrediction:    protocol.ClientPredictionFailure,
		ClientCooldownState: protocol.ClientCooldownStateOff,
	})
	return pk
}

// playerAuthInputUseBlockFixture pins one filled-stack click in the movement
// envelope, with a sign-bit-set runtime identity and no predicted inventory actions.
func playerAuthInputUseBlockFixture() *packet.PlayerAuthInput {
	pk := playerAuthInputFixture()
	pk.InputData.Set(packet.InputFlagPerformItemInteraction)
	held := inventoryItem(5, 37, 41)
	heldRuntimeID := uint32(0x87654321)
	held.Stack.BlockRuntimeID = int32(heldRuntimeID)
	pk.ItemInteractionData = protocol.Option(protocol.UseItemTransactionData{
		ActionType:          protocol.UseItemActionClickBlock,
		TriggerType:         protocol.TriggerTypePlayerInput,
		BlockPosition:       protocol.BlockPos{13, 71, -29},
		BlockFace:           5,
		HotBarSlot:          7,
		HeldItem:            held,
		Position:            mgl32.Vec3{13.25, 72.625, -28.75},
		ClickedPosition:     mgl32.Vec3{0.125, 0.875, 0.625},
		BlockRuntimeID:      123456,
		ClientPrediction:    protocol.ClientPredictionFailure,
		ClientCooldownState: protocol.ClientCooldownStateOff,
	})
	return pk
}

// playerAuthInputBlockActionsAndBreakBlockFixture carries both optional
// interaction payloads in one tick so the flag order (PerformItemInteraction
// before PerformBlockActions) and the field order (item-use transaction
// before block actions) are pinned byte for byte.
func playerAuthInputBlockActionsAndBreakBlockFixture() *packet.PlayerAuthInput {
	pk := playerAuthInputBreakBlockFixture()
	pk.InputData.Set(packet.InputFlagPerformBlockActions)
	pk.BlockActions = protocol.Option([]protocol.PlayerBlockAction{
		{
			Action:   protocol.PlayerActionStartBreak,
			BlockPos: protocol.BlockPos{13, 71, -29},
			Face:     5,
		},
		{
			Action:   protocol.PlayerActionPredictDestroyBlock,
			BlockPos: protocol.BlockPos{-8, 63, 21},
			Face:     1,
		},
	})
	return pk
}

func availableCommandsFixture() *packet.AvailableCommands {
	return &packet.AvailableCommands{
		EnumValues:              []string{"alpha", "beta"},
		ChainedSubcommandValues: []string{"chain"},
		Suffixes:                []string{"suffix"},
		Enums: []protocol.CommandEnum{
			{Type: "fixture_enum", ValueIndices: []uint32{0, 1}},
		},
		ChainedSubcommands: []protocol.ChainedSubcommand{
			{
				Name: "fixture_chain",
				Values: []protocol.ChainedSubcommandValue{
					{Index: 0, Value: protocol.CommandArgTypeString},
				},
			},
		},
		Commands: []protocol.Command{
			{
				Name:                     "fixture",
				Description:              "fixture command",
				Flags:                    1,
				PermissionLevel:          protocol.CommandPermissionLevelAny,
				AliasesOffset:            0,
				ChainedSubcommandOffsets: []uint32{0},
				Overloads: []protocol.CommandOverload{
					{
						Chaining: true,
						Parameters: []protocol.CommandParameter{
							{
								Name:     "value",
								Type:     protocol.CommandArgTypeString | protocol.CommandArgValid | protocol.CommandArgEnum,
								Optional: false,
								Options:  protocol.ParamOptionCollapseEnum,
							},
						},
					},
				},
			},
		},
		DynamicEnums: []protocol.DynamicEnum{
			{Type: "fixture_dynamic", Values: []string{"one", "two"}},
		},
		Constraints: []protocol.CommandEnumConstraint{
			{
				EnumValueIndex: 0,
				EnumIndex:      0,
				Constraints:    []byte{protocol.CommandEnumConstraintCheatsEnabled},
			},
		},
	}
}

func availableCommandsLiveRegression() *packet.AvailableCommands {
	const observedLiveBodyLength = 356_513

	fixture := availableCommandsFixture()
	fixture.EnumValues = append(fixture.EnumValues, "")
	paddingIndex := len(fixture.EnumValues) - 1
	paddingLength := observedLiveBodyLength - availableCommandsBodyLength(fixture)
	if paddingLength < 0 {
		panic("AvailableCommands fixture exceeds observed live body length")
	}
	fixture.EnumValues[paddingIndex] = strings.Repeat("x", paddingLength)
	for availableCommandsBodyLength(fixture) != observedLiveBodyLength {
		delta := observedLiveBodyLength - availableCommandsBodyLength(fixture)
		paddingLength += delta
		if paddingLength < 0 {
			panic("cannot size AvailableCommands live regression fixture")
		}
		fixture.EnumValues[paddingIndex] = strings.Repeat("x", paddingLength)
	}
	return fixture
}

func availableCommandsBodyLength(fixture *packet.AvailableCommands) int {
	var body bytes.Buffer
	fixture.Marshal(protocol.NewWriter(&body, 0))
	return body.Len()
}

func encode(pk packet.Packet) ([]byte, error) {
	var entry bytes.Buffer
	if err := (&packet.Header{
		PacketID:        pk.ID(),
		SenderSubClient: senderSubClient,
		TargetSubClient: targetSubClient,
	}).Write(&entry); err != nil {
		return nil, err
	}
	pk.Marshal(minecraft.DefaultProtocol.NewWriter(&entry, 0))

	var batch bytes.Buffer
	if err := packet.NewEncoder(&batch).Encode([][]byte{entry.Bytes()}); err != nil {
		return nil, err
	}
	return append([]byte(nil), batch.Bytes()...), nil
}
