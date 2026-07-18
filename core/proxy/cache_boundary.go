package proxy

import (
	"log/slog"
	"net"
	"sync/atomic"

	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const (
	cacheStatusUnseen uint32 = iota
	cacheStatusDisabled
	cacheStatusEnabled
)

type cacheBoundaryTelemetry struct {
	upstreamStatus      atomic.Uint32
	cachedLevelChunks   atomic.Uint64
	ordinaryLevelChunks atomic.Uint64
	cachedSubChunks     atomic.Uint64
	ordinarySubChunks   atomic.Uint64
}

type cacheBoundarySnapshot struct {
	upstreamStatusSeen    bool
	upstreamStatusEnabled bool
	cachedLevelChunks     uint64
	ordinaryLevelChunks   uint64
	cachedSubChunks       uint64
	ordinarySubChunks     uint64
}

func (telemetry *cacheBoundaryTelemetry) observeUpstreamPacket(
	header packet.Header,
	payload []byte,
	_, _ net.Addr,
) {
	if header.PacketID != packet.IDClientCacheStatus || len(payload) != 1 || payload[0] > 1 {
		return
	}
	status := uint32(cacheStatusDisabled)
	if payload[0] == 1 {
		status = cacheStatusEnabled
	}
	telemetry.upstreamStatus.CompareAndSwap(cacheStatusUnseen, status)
}

func (telemetry *cacheBoundaryTelemetry) observeRelayPacket(value packet.Packet) {
	switch value := value.(type) {
	case *packet.LevelChunk:
		if value.CacheEnabled {
			atomicSaturatingIncrement(&telemetry.cachedLevelChunks)
		} else {
			atomicSaturatingIncrement(&telemetry.ordinaryLevelChunks)
		}
	case *packet.SubChunk:
		if value.CacheEnabled {
			atomicSaturatingIncrement(&telemetry.cachedSubChunks)
		} else {
			atomicSaturatingIncrement(&telemetry.ordinarySubChunks)
		}
	}
}

func (telemetry *cacheBoundaryTelemetry) snapshot() cacheBoundarySnapshot {
	status := telemetry.upstreamStatus.Load()
	return cacheBoundarySnapshot{
		upstreamStatusSeen:    status != cacheStatusUnseen,
		upstreamStatusEnabled: status == cacheStatusEnabled,
		cachedLevelChunks:     telemetry.cachedLevelChunks.Load(),
		ordinaryLevelChunks:   telemetry.ordinaryLevelChunks.Load(),
		cachedSubChunks:       telemetry.cachedSubChunks.Load(),
		ordinarySubChunks:     telemetry.ordinarySubChunks.Load(),
	}
}

func (telemetry *cacheBoundaryTelemetry) report(logger *slog.Logger) {
	snapshot := telemetry.snapshot()
	logger.Info(
		"PHASE2_CACHE_BOUNDARY",
		"upstream_status_seen", snapshot.upstreamStatusSeen,
		"upstream_status_enabled", snapshot.upstreamStatusEnabled,
		"cached_level_chunks", snapshot.cachedLevelChunks,
		"ordinary_level_chunks", snapshot.ordinaryLevelChunks,
		"cached_sub_chunks", snapshot.cachedSubChunks,
		"ordinary_sub_chunks", snapshot.ordinarySubChunks,
	)
}

func atomicSaturatingIncrement(counter *atomic.Uint64) {
	for {
		current := counter.Load()
		if current == ^uint64(0) || counter.CompareAndSwap(current, current+1) {
			return
		}
	}
}
