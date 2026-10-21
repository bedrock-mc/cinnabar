package proxy

import (
	"bytes"
	"net"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
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
