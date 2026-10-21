package proxy

import (
	"encoding/binary"
	"log/slog"
	"net"
	"sync"
	"sync/atomic"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

var nextJoinID atomic.Uint64

var joinPacketStages = map[uint32]string{
	packet.IDRequestNetworkSettings:     "upstream transport connected",
	packet.IDNetworkSettings:            "upstream network settings",
	packet.IDLogin:                      "upstream login sent",
	packet.IDServerToClientHandshake:    "upstream server-to-client handshake",
	packet.IDClientToServerHandshake:    "upstream encryption enabled",
	packet.IDResourcePacksInfo:          "upstream resource packs info",
	packet.IDResourcePackStack:          "upstream resource packs stack",
	packet.IDResourcePackClientResponse: "upstream resource packs done",
	packet.IDStartGame:                  "upstream StartGame received",
}

// withJoinStages preserves packet observers and logs each login milestone once per dial attempt.
func withJoinStages(dialer minecraft.Dialer, logger *slog.Logger) minecraft.Dialer {
	dialer = withDiagnosticJoinStages(dialer, logger)
	started, id := time.Now(), nextJoinID.Add(1)
	var mu sync.Mutex
	seen := make(map[string]bool)
	ids := make([]uint32, 0, len(joinPacketStages))
	for id := range joinPacketStages {
		ids = append(ids, id)
	}
	return withPacketObserver(dialer, minecraft.PacketObserver{
		PacketIDs: ids,
		Func: func(header packet.Header, payload []byte, _, _ net.Addr) {
			stage := joinPacketStages[header.PacketID]
			if header.PacketID == packet.IDResourcePackClientResponse {
				response, length := binary.Uvarint(payload)
				if length <= 0 || response != packet.PackResponseCompleted {
					stage = ""
				}
			}
			if stage != "" {
				mu.Lock()
				if !seen[stage] {
					seen[stage] = true
					logger.Info(stage, "join_id", id, "elapsed_ms", time.Since(started).Milliseconds())
				}
				mu.Unlock()
			}
		},
	})
}
