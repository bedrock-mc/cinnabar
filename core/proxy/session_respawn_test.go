package proxy

import (
	"bytes"
	"net"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func newTestSessionConn(t *testing.T) (*sessionConn, *streamnet.FramedConn) {
	c1, c2 := net.Pipe()
	framed1 := streamnet.NewFramedConn(c1)
	framed2 := streamnet.NewFramedConn(c2)
	session := newSessionConn(framed1)
	session.runtimeID.Store(12345)
	t.Cleanup(func() {
		_ = session.Close()
		_ = framed2.Close()
	})
	return session, framed2
}

func sendBatchFromClient(t *testing.T, peer *streamnet.FramedConn, pks ...packet.Packet) {
	var batch []byte
	batch = append(batch, bridgecontract.SessionKindBatch)
	for _, pk := range pks {
		buf := new(bytes.Buffer)
		(&packet.Header{PacketID: pk.ID()}).Write(buf)
		pk.Marshal(minecraft.DefaultProtocol.NewWriter(buf, 0))
		batch = appendSessionBatchPacket(batch, buf.Bytes())
	}
	_, err := peer.Write(batch)
	if err != nil {
		t.Fatalf("failed to write client batch: %v", err)
	}
}

func TestSessionFramesBuffered(t *testing.T) {
	session, _ := newTestSessionConn(t)
	if cap(session.frames) < 256 {
		t.Fatalf("expected frames channel capacity to be at least 256 to prevent client drop choking, got %d", cap(session.frames))
	}
}

func TestClientRespawnPacketInjectsPlayerActionRespawn(t *testing.T) {
	session, peer := newTestSessionConn(t)

	// Client sends only Respawn packet (e.g. ClientReadyToSpawn)
	clientPk := &packet.Respawn{
		EntityRuntimeID: 12345,
		State:           packet.RespawnStateClientReadyToSpawn,
	}
	go sendBatchFromClient(t, peer, clientPk)

	rawPackets, err := session.ReadBatchRaw(func(uint32) bool { return true })
	if err != nil {
		t.Fatalf("ReadBatchRaw failed: %v", err)
	}

	if len(rawPackets) < 2 {
		t.Fatalf("expected at least 2 packets (Respawn + PlayerActionRespawn), got %d", len(rawPackets))
	}

	hasActionRespawn := false
	for _, raw := range rawPackets {
		for _, decoded := range raw.Decoded {
			if action, ok := decoded.(*packet.PlayerAction); ok {
				if action.ActionType == protocol.PlayerActionRespawn && action.EntityRuntimeID == 12345 {
					hasActionRespawn = true
				}
			}
		}
	}

	if !hasActionRespawn {
		t.Fatalf("expected injected PlayerActionRespawn packet in batch, but not found")
	}
}

func TestClientPlayerActionRespawnInjectsRespawnPacket(t *testing.T) {
	session, peer := newTestSessionConn(t)

	// Client sends PlayerAction with PlayerActionRespawn
	clientPk := &packet.PlayerAction{
		EntityRuntimeID: 12345,
		ActionType:      protocol.PlayerActionRespawn,
		BlockFace:       -1,
	}
	go sendBatchFromClient(t, peer, clientPk)

	rawPackets, err := session.ReadBatchRaw(func(uint32) bool { return true })
	if err != nil {
		t.Fatalf("ReadBatchRaw failed: %v", err)
	}

	if len(rawPackets) < 2 {
		t.Fatalf("expected at least 2 packets (PlayerActionRespawn + Respawn), got %d", len(rawPackets))
	}

	hasRespawnState := false
	for _, raw := range rawPackets {
		for _, decoded := range raw.Decoded {
			if respawn, ok := decoded.(*packet.Respawn); ok {
				if respawn.State == packet.RespawnStateClientReadyToSpawn && respawn.EntityRuntimeID == 12345 {
					hasRespawnState = true
				}
			}
		}
	}

	if !hasRespawnState {
		t.Fatalf("expected injected Respawn packet with ClientReadyToSpawn, but not found")
	}
}

func TestImmediateRespawnTriggeredOnHealthZero(t *testing.T) {
	session, peer := newTestSessionConn(t)

	// Server enables doimmediaterespawn
	rulesPk := &packet.GameRulesChanged{
		GameRules: []protocol.GameRule{
			{Name: "doimmediaterespawn", Value: true},
		},
	}
	buf := new(bytes.Buffer)
	(&packet.Header{PacketID: rulesPk.ID()}).Write(buf)
	rulesPk.Marshal(minecraft.DefaultProtocol.NewWriter(buf, 0))
	if err := session.WritePacketRaw(buf.Bytes()); err != nil {
		t.Fatalf("WritePacketRaw failed: %v", err)
	}

	if !session.immediateRespawn.Load() {
		t.Fatalf("expected immediateRespawn to be true")
	}

	// Server sends UpdateAttributes with health <= 0
	attrPk := &packet.UpdateAttributes{
		EntityRuntimeID: 12345,
		Attributes: []protocol.Attribute{
			{AttributeValue: protocol.AttributeValue{Name: "minecraft:health", Value: 0.0, Max: 20.0, Min: 0.0}},
		},
	}
	buf.Reset()
	(&packet.Header{PacketID: attrPk.ID()}).Write(buf)
	attrPk.Marshal(minecraft.DefaultProtocol.NewWriter(buf, 0))
	if err := session.WritePacketRaw(buf.Bytes()); err != nil {
		t.Fatalf("WritePacketRaw failed: %v", err)
	}

	// Now client sends an ordinary packet
	dummyPk := &packet.RequestChunkRadius{
		ChunkRadius: 8,
	}
	go sendBatchFromClient(t, peer, dummyPk)

	rawPackets, err := session.ReadBatchRaw(func(uint32) bool { return true })
	if err != nil {
		t.Fatalf("ReadBatchRaw failed: %v", err)
	}

	// Should contain the injected auto-respawn packets
	hasActionRespawn := false
	hasRespawnState := false
	for _, raw := range rawPackets {
		for _, decoded := range raw.Decoded {
			if action, ok := decoded.(*packet.PlayerAction); ok && action.ActionType == protocol.PlayerActionRespawn {
				hasActionRespawn = true
			}
			if respawn, ok := decoded.(*packet.Respawn); ok && respawn.State == packet.RespawnStateClientReadyToSpawn {
				hasRespawnState = true
			}
		}
	}

	if !hasActionRespawn || !hasRespawnState {
		t.Fatalf("expected both auto-respawn packets injected, got hasActionRespawn=%v, hasRespawnState=%v", hasActionRespawn, hasRespawnState)
	}
}

func TestServerReadyToSpawnTriggersCompletion(t *testing.T) {
	session, peer := newTestSessionConn(t)

	// Server sends Respawn with ReadyToSpawn
	serverPk := &packet.Respawn{
		EntityRuntimeID: 12345,
		State:           packet.RespawnStateReadyToSpawn,
	}
	buf := new(bytes.Buffer)
	(&packet.Header{PacketID: serverPk.ID()}).Write(buf)
	serverPk.Marshal(minecraft.DefaultProtocol.NewWriter(buf, 0))
	if err := session.WritePacketRaw(buf.Bytes()); err != nil {
		t.Fatalf("WritePacketRaw failed: %v", err)
	}

	// Client sends a frame
	dummyPk := &packet.RequestChunkRadius{ChunkRadius: 8}
	go sendBatchFromClient(t, peer, dummyPk)

	rawPackets, err := session.ReadBatchRaw(func(uint32) bool { return true })
	if err != nil {
		t.Fatalf("ReadBatchRaw failed: %v", err)
	}

	hasActionRespawn := false
	hasRespawnState := false
	for _, raw := range rawPackets {
		for _, decoded := range raw.Decoded {
			if action, ok := decoded.(*packet.PlayerAction); ok && action.ActionType == protocol.PlayerActionRespawn {
				hasActionRespawn = true
			}
			if respawn, ok := decoded.(*packet.Respawn); ok && respawn.State == packet.RespawnStateClientReadyToSpawn {
				hasRespawnState = true
			}
		}
	}

	if !hasActionRespawn || !hasRespawnState {
		t.Fatalf("expected both respawn completion packets injected on ReadyToSpawn, got hasAction=%v, hasRespawn=%v", hasActionRespawn, hasRespawnState)
	}
}
