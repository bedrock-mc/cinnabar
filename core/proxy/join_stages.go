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

// withJoinStages preserves packet observers and logs each login milestone once per dial attempt.
func withJoinStages(dialer minecraft.Dialer, logger *slog.Logger) minecraft.Dialer {
	started, id := time.Now(), nextJoinID.Add(1)
	var mu sync.Mutex
	seen := make(map[string]bool)
	next := dialer.PacketFunc
	dialer.PacketFunc = func(header packet.Header, payload []byte, source, destination net.Addr) {
		stage := ""
		switch header.PacketID {
		case packet.IDRequestNetworkSettings:
			stage = "upstream transport connected"
		case packet.IDNetworkSettings:
			stage = "upstream network settings"
		case packet.IDLogin:
			stage = "upstream login sent"
		case packet.IDServerToClientHandshake:
			stage = "upstream server-to-client handshake"
		case packet.IDClientToServerHandshake:
			stage = "upstream encryption enabled"
		case packet.IDResourcePacksInfo:
			stage = "upstream resource packs info"
		case packet.IDResourcePackStack:
			stage = "upstream resource packs stack"
		case packet.IDResourcePackClientResponse:
			response, length := binary.Uvarint(payload)
			if length > 0 && response == packet.PackResponseCompleted {
				stage = "upstream resource packs done"
			}
		case packet.IDStartGame:
			stage = "upstream StartGame received"
		}
		if stage != "" {
			mu.Lock()
			if !seen[stage] {
				seen[stage] = true
				logger.Info(stage, "join_id", id, "elapsed_ms", time.Since(started).Milliseconds())
			}
			mu.Unlock()
		}
		if next != nil {
			next(header, payload, source, destination)
		}
	}
	return dialer
}
