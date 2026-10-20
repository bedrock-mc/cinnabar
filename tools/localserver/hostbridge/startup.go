package hostbridge

import (
	"bytes"
	"encoding/binary"
	"encoding/json"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/internal/hostcontract"
)

// startupPackets projects actual Dragonfly GameData with the pinned listener's original startup prefix.
func startupPackets(data minecraft.GameData) ([]packet.Packet, error) {
	var packets []packet.Packet
	if len(data.Dimensions) > 0 {
		packets = append(packets, &packet.DimensionData{Definitions: data.Dimensions})
	}
	packets = append(packets, &packet.JigsawStructureData{StructureData: map[string]any{
		"processors": []map[string]any{}, "template_pools": []map[string]any{},
		"jigsaws": []map[string]any{}, "structure_sets": []map[string]any{},
	}}, &packet.VoxelShapes{})
	start := minecraft.StartGameFromGameData(data)
	start.AchievementsDisabled, start.EducationFeaturesEnabled = true, true
	start.Generator, start.MultiPlayerGame, start.CommandsEnabled, start.LANBroadcastEnabled = 1, true, true, true
	start.GameVersion = protocol.CurrentVersion
	if start.MultiPlayerCorrelationID == "" {
		id, err := uuid.NewRandom()
		if err != nil {
			return nil, errProtocol
		}
		start.MultiPlayerCorrelationID = id.String()
	}
	return append(packets, start, &packet.ItemRegistry{Items: data.Items}, &packet.CreativeContent{}), nil
}

// worldBytes encodes generated packets, not a separately maintained JSON GameData mapping.
func worldBytes(data minecraft.GameData, maxRadius uint32, shield int32) ([]byte, error) {
	packets, err := startupPackets(data)
	if err != nil {
		return nil, err
	}
	policy, err := json.Marshal(hostcontract.WorldPolicy{
		MaxRadius: maxRadius, InitialRadius: uint32(data.ChunkRadius), PacketCount: uint32(len(packets)),
	})
	if err != nil || len(policy) > hostcontract.MAX_CONTROL_BYTES || data.ChunkRadius < 0 {
		return nil, errProtocol
	}
	var out bytes.Buffer
	if err := binary.Write(&out, binary.BigEndian, uint32(len(policy))); err != nil {
		return nil, err
	}
	out.Write(policy)
	for _, pk := range packets {
		encoded, err := encodePacket(pk, shield)
		if err != nil || out.Len()+4+len(encoded) > hostcontract.MAX_PACKET_BYTES {
			return nil, errProtocol
		}
		_ = binary.Write(&out, binary.BigEndian, uint32(len(encoded)))
		out.Write(encoded)
	}
	return out.Bytes(), nil
}

// encodePacket uses the selected Go integration's current packet writer and original primary header.
func encodePacket(pk packet.Packet, shield int32) (b []byte, err error) {
	defer func() {
		if recover() != nil {
			b, err = nil, errProtocol
		}
	}()
	var out bytes.Buffer
	header := packet.Header{PacketID: pk.ID()}
	if err := header.Write(&out); err != nil {
		return nil, errProtocol
	}
	pk.Marshal(minecraft.DefaultProtocol.NewWriter(&out, shield))
	if out.Len() > hostcontract.MAX_PACKET_BYTES {
		return nil, errProtocol
	}
	return out.Bytes(), nil
}
