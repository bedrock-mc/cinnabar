//go:build joinprofile

package proxy

import (
	"context"
	"encoding/binary"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"log/slog"
	"net"
	"sync"
	"sync/atomic"
	"time"
)

// withDiagnosticJoinStages observes each additional milestone once without retaining packet bodies.
func withDiagnosticJoinStages(d minecraft.Dialer, logger *slog.Logger) minecraft.Dialer {
	if !logger.Enabled(context.Background(), slog.LevelDebug) {
		return d
	}
	stages := map[uint32]string{
		packet.IDPlayStatus:                  "play_status",
		packet.IDResourcePackDataInfo:        "pack_data_info",
		packet.IDResourcePackChunkData:       "first_pack_chunk",
		packet.IDItemRegistry:                "item_registry",
		packet.IDRequestChunkRadius:          "chunk_radius_request",
		packet.IDChunkRadiusUpdated:          "chunk_radius_reply",
		packet.IDNetworkChunkPublisherUpdate: "first_publisher_update",
		packet.IDLevelChunk:                  "first_level_chunk",
		packet.IDSubChunkRequest:             "first_subchunk_request",
		packet.IDSubChunk:                    "first_subchunk",
		packet.IDSetLocalPlayerAsInitialised: "player_initialized",
	}
	ids := make([]uint32, 0, len(stages))
	for id := range stages {
		ids = append(ids, id)
	}
	started := time.Now()
	var mu sync.Mutex
	seen := make(map[string]bool)
	var done atomic.Bool
	return withPacketObserver(d, minecraft.PacketObserver{PacketIDs: ids, Func: func(h packet.Header, body []byte, _, _ net.Addr) {
		if done.Load() {
			return
		}
		stage := stages[h.PacketID]
		if h.PacketID == packet.IDPlayStatus {
			if len(body) < 4 {
				return
			}
			switch int32(binary.BigEndian.Uint32(body[:4])) {
			case packet.PlayStatusLoginSuccess:
				stage = "login_success"
			case packet.PlayStatusPlayerSpawn:
				stage = "player_spawn"
			default:
				return
			}
		}
		mu.Lock()
		defer mu.Unlock()
		if done.Load() {
			return
		}
		if !seen[stage] {
			seen[stage] = true
			if h.PacketID == packet.IDSetLocalPlayerAsInitialised {
				done.Store(true)
			}
			logger.Debug("join packet milestone", "stage", stage, "dial_elapsed_ms", float64(time.Since(started).Microseconds())/1000)
		}
	}})
}
