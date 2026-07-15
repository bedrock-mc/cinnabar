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
	gameVersion     = "1.26.30"
	protocolID      = 1001
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
	flag.Parse()
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
	return []fixture{
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
				BaseGameVersion:       protocol.CurrentVersion,
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
				GameVersion:                  protocol.CurrentVersion,
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
				Position:        protocol.ChunkPos{3, -4},
				Dimension:       0,
				SubChunkCount:   protocol.SubChunkRequestModeLimited,
				HighestSubChunk: 24,
				CacheEnabled:    false,
				RawPayload:      []byte{0xde, 0xad, 0xbe, 0xef},
			},
		},
		{
			name: "MovePlayer",
			file: "move_player.bin",
			pk: &packet.MovePlayer{
				EntityRuntimeID:          42,
				Position:                 mgl32.Vec3{1.25, 64, -2.5},
				Pitch:                    10.5,
				Yaw:                      20.25,
				HeadYaw:                  30.75,
				Mode:                     packet.MoveModeTeleport,
				OnGround:                 true,
				RiddenEntityRuntimeID:    0,
				TeleportCause:            packet.TeleportCauseCommand,
				TeleportSourceEntityType: 87,
				Tick:                     1234,
			},
		},
		{
			name: "PlayerAuthInput",
			file: "player_auth_input.bin",
			pk:   playerAuthInputFixture(),
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
			name: "CraftingDataMaterialReducer",
			file: "material_reducer.bin",
			pk: &packet.CraftingData{
				MaterialReducers: []protocol.MaterialReducer{
					{
						InputItem: protocol.ItemType{NetworkID: 42, MetadataValue: 3},
						Outputs: []protocol.MaterialReducerOutput{
							{NetworkID: 7, Count: 2},
							{NetworkID: -9, Count: 4},
						},
					},
				},
				ClearRecipes: true,
			},
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
			wireCommit:    "9948b1729395d2e819fce28e079d4a7bfc67716c",
		},
	}
}

func playerAuthInputFixture() *packet.PlayerAuthInput {
	flags := protocol.NewBitset(packet.PlayerAuthInputBitsetSize)
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
	pk.Marshal(protocol.NewWriter(&entry, 0))

	var batch bytes.Buffer
	if err := packet.NewEncoder(&batch).Encode([][]byte{entry.Bytes()}); err != nil {
		return nil, err
	}
	return append([]byte(nil), batch.Bytes()...), nil
}
