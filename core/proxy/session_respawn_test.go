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

// newTestSessionConn connects a session to a framed peer and joins its reader on cleanup.
func newTestSessionConn(t *testing.T) (*sessionConn, *streamnet.FramedConn) {
	t.Helper()
	local, remote := net.Pipe()
	session := newSessionConn(streamnet.NewFramedConn(local))
	peer := streamnet.NewFramedConn(remote)
	session.runtimeID.Store(12345)
	t.Cleanup(func() {
		_ = session.Close()
		_ = peer.Close()
		<-session.readDone
	})
	return session, peer
}

// sessionTestBatch encodes the supplied packets as one client batch.
func sessionTestBatch(packets ...packet.Packet) []byte {
	batch := []byte{bridgecontract.SessionKindBatch}
	for _, pk := range packets {
		batch = appendSessionBatchPacket(batch, encodeTestPacket(pk))
	}
	return batch
}

func TestSessionRespawnPreservesClientHandshake(t *testing.T) {
	request := &packet.Respawn{EntityRuntimeID: 12345, State: packet.RespawnStateClientReadyToSpawn}
	completion := &packet.PlayerAction{EntityRuntimeID: 12345, ActionType: protocol.PlayerActionRespawn, BlockFace: -1}
	for name, packets := range map[string][]packet.Packet{
		"request waits for server readiness":  {request},
		"completion does not restart respawn": {completion},
		"complete pair stays unchanged":       {request, completion},
	} {
		t.Run(name, func(t *testing.T) {
			session, peer := newTestSessionConn(t)
			if _, err := peer.Write(sessionTestBatch(packets...)); err != nil {
				t.Fatal(err)
			}
			got, err := session.ReadBatchRaw(nil)
			if err != nil {
				t.Fatal(err)
			}
			if len(got) != len(packets) {
				t.Fatalf("forwarded %d packets, want %d", len(got), len(packets))
			}
			for i, pk := range packets {
				if !bytes.Equal(got[i].Data, encodeTestPacket(pk)) {
					t.Fatalf("packet %d changed", i)
				}
			}
		})
	}
}

func TestSessionRespawnServerControlsLeaveCompletionToClient(t *testing.T) {
	for _, immediate := range []bool{false, true} {
		t.Run(map[bool]string{false: "ordinary", true: "immediate"}[immediate], func(t *testing.T) {
			session, peer := newTestSessionConn(t)
			for _, pk := range []packet.Packet{
				&packet.GameRulesChanged{GameRules: []protocol.GameRule{{Name: "doimmediaterespawn", Value: immediate}}},
				&packet.UpdateAttributes{EntityRuntimeID: 12345, Attributes: []protocol.Attribute{{AttributeValue: protocol.AttributeValue{Name: "minecraft:health", Value: 0, Max: 20}}}},
				&packet.Respawn{EntityRuntimeID: 12345, State: packet.RespawnStateReadyToSpawn},
			} {
				if err := session.WritePacketRaw(encodeTestPacket(pk)); err != nil {
					t.Fatal(err)
				}
			}
			// Unrelated traffic must not acknowledge a ready control the client has not applied.
			latency := &packet.NetworkStackLatency{Timestamp: 77}
			if _, err := peer.Write(sessionTestBatch(latency)); err != nil {
				t.Fatal(err)
			}
			got, err := session.ReadBatchRaw(nil)
			if err != nil {
				t.Fatal(err)
			}
			if len(got) != 1 || !bytes.Equal(got[0].Data, encodeTestPacket(latency)) {
				t.Fatalf("server controls altered unrelated client traffic: %d packets", len(got))
			}
		})
	}
}

func TestUntimedMovePlayerStampedWithLastClientTick(t *testing.T) {
	session, peer := newTestSessionConn(t)

	// Client sends PlayerAuthInput with Tick = 777
	inputPk := &packet.PlayerAuthInput{
		Tick: 777,
	}
	if _, err := peer.Write(sessionTestBatch(inputPk)); err != nil {
		t.Fatal(err)
	}

	_, err := session.ReadBatchRaw(func(uint32) bool { return true })
	if err != nil {
		t.Fatalf("ReadBatchRaw failed: %v", err)
	}

	if session.lastClientTick.Load() != 777 {
		t.Fatalf("expected lastClientTick to be 777, got %d", session.lastClientTick.Load())
	}

	// Server sends MovePlayer with Tick = 0
	movePk := &packet.MovePlayer{
		EntityRuntimeID: 12345,
		Tick:            0,
	}
	buf := new(bytes.Buffer)
	(&packet.Header{PacketID: movePk.ID()}).Write(buf)
	movePk.Marshal(minecraft.DefaultProtocol.NewWriter(buf, 0))

	if err := session.WritePacketRaw(buf.Bytes()); err != nil {
		t.Fatalf("WritePacketRaw failed: %v", err)
	}

	// Verify forming batch pending data has rewritten Tick = 777
	packets, err := splitSessionBatch(session.pending[1:])
	if err != nil {
		t.Fatalf("splitSessionBatch failed: %v", err)
	}
	raws, err := rawSessionPackets(packets, func(uint32) bool { return true }, session.serverPool, 0)
	if err != nil {
		t.Fatalf("rawSessionPackets failed: %v", err)
	}
	if len(raws) != 1 {
		t.Fatalf("expected 1 packet, got %d", len(raws))
	}
	decodedMove, ok := raws[0].Decoded[0].(*packet.MovePlayer)
	if !ok {
		t.Fatalf("expected MovePlayer packet, got %T", raws[0].Decoded[0])
	}
	if decodedMove.Tick != 777 {
		t.Fatalf("expected rewritten MovePlayer Tick to be 777, got %d", decodedMove.Tick)
	}
}

func TestUntimedSetActorMotionStampedWithLastClientTick(t *testing.T) {
	session, _ := newTestSessionConn(t)
	session.lastClientTick.Store(888)

	motionPk := &packet.SetActorMotion{
		EntityRuntimeID: 12345,
		Tick:            0,
	}
	buf := new(bytes.Buffer)
	(&packet.Header{PacketID: motionPk.ID()}).Write(buf)
	motionPk.Marshal(minecraft.DefaultProtocol.NewWriter(buf, 0))

	if err := session.WritePacketRaw(buf.Bytes()); err != nil {
		t.Fatalf("WritePacketRaw failed: %v", err)
	}

	packets, err := splitSessionBatch(session.pending[1:])
	if err != nil {
		t.Fatalf("splitSessionBatch failed: %v", err)
	}
	raws, err := rawSessionPackets(packets, func(uint32) bool { return true }, session.serverPool, 0)
	if err != nil {
		t.Fatalf("rawSessionPackets failed: %v", err)
	}
	decodedMotion, ok := raws[0].Decoded[0].(*packet.SetActorMotion)
	if !ok {
		t.Fatalf("expected SetActorMotion packet, got %T", raws[0].Decoded[0])
	}
	if decodedMotion.Tick != 888 {
		t.Fatalf("expected rewritten SetActorMotion Tick to be 888, got %d", decodedMotion.Tick)
	}
}

func TestUntimedCorrectPlayerMovePredictionStampedWithLastClientTick(t *testing.T) {
	session, _ := newTestSessionConn(t)
	session.lastClientTick.Store(999)

	correctionPk := &packet.CorrectPlayerMovePrediction{
		Tick: 0,
	}
	buf := new(bytes.Buffer)
	(&packet.Header{PacketID: correctionPk.ID()}).Write(buf)
	correctionPk.Marshal(minecraft.DefaultProtocol.NewWriter(buf, 0))

	if err := session.WritePacketRaw(buf.Bytes()); err != nil {
		t.Fatalf("WritePacketRaw failed: %v", err)
	}

	packets, err := splitSessionBatch(session.pending[1:])
	if err != nil {
		t.Fatalf("splitSessionBatch failed: %v", err)
	}
	raws, err := rawSessionPackets(packets, func(uint32) bool { return true }, session.serverPool, 0)
	if err != nil {
		t.Fatalf("rawSessionPackets failed: %v", err)
	}
	decodedCorrection, ok := raws[0].Decoded[0].(*packet.CorrectPlayerMovePrediction)
	if !ok {
		t.Fatalf("expected CorrectPlayerMovePrediction packet, got %T", raws[0].Decoded[0])
	}
	if decodedCorrection.Tick != 999 {
		t.Fatalf("expected rewritten CorrectPlayerMovePrediction Tick to be 999, got %d", decodedCorrection.Tick)
	}
}

func TestTimedMovePlayerPreserved(t *testing.T) {
	session, _ := newTestSessionConn(t)
	session.lastClientTick.Store(888)

	movePk := &packet.MovePlayer{
		EntityRuntimeID: 12345,
		Tick:            450,
	}
	buf := new(bytes.Buffer)
	(&packet.Header{PacketID: movePk.ID()}).Write(buf)
	movePk.Marshal(minecraft.DefaultProtocol.NewWriter(buf, 0))

	if err := session.WritePacketRaw(buf.Bytes()); err != nil {
		t.Fatalf("WritePacketRaw failed: %v", err)
	}

	packets, err := splitSessionBatch(session.pending[1:])
	if err != nil {
		t.Fatalf("splitSessionBatch failed: %v", err)
	}
	raws, err := rawSessionPackets(packets, func(uint32) bool { return true }, session.serverPool, 0)
	if err != nil {
		t.Fatalf("rawSessionPackets failed: %v", err)
	}
	decodedMove, ok := raws[0].Decoded[0].(*packet.MovePlayer)
	if !ok {
		t.Fatalf("expected MovePlayer packet, got %T", raws[0].Decoded[0])
	}
	if decodedMove.Tick != 450 {
		t.Fatalf("expected preserved MovePlayer Tick to be 450, got %d", decodedMove.Tick)
	}
}
