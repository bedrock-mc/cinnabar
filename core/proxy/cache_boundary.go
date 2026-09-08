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
	loadingOrder        loadingOrderTrace
}

type cacheBoundarySnapshot struct {
	upstreamStatusSeen    bool
	upstreamStatusEnabled bool
	cachedLevelChunks     uint64
	ordinaryLevelChunks   uint64
	cachedSubChunks       uint64
	ordinarySubChunks     uint64
	loadingOrder          loadingOrderTraceSnapshot
}

func (telemetry *cacheBoundaryTelemetry) observeUpstreamPacket(
	header packet.Header,
	payload []byte,
	_, _ net.Addr,
) {
	telemetry.loadingOrder.observeUpstreamCallback(header, payload)
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
	telemetry.loadingOrder.observeRelay(value)
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
		loadingOrder:          telemetry.loadingOrder.snapshot(),
	}
}

func (telemetry *cacheBoundaryTelemetry) report(logger *slog.Logger) {
	snapshot := telemetry.snapshot()
	upstreamCaptured := min(snapshot.loadingOrder.UpstreamCallback.UpdateCount, loadingOrderTraceLimit)
	relayCaptured := min(snapshot.loadingOrder.Relay.UpdateCount, loadingOrderTraceLimit)
	logger.Info(
		"PHASE2_CACHE_BOUNDARY",
		"upstream_status_seen", snapshot.upstreamStatusSeen,
		"upstream_status_enabled", snapshot.upstreamStatusEnabled,
		"cached_level_chunks", snapshot.cachedLevelChunks,
		"ordinary_level_chunks", snapshot.ordinaryLevelChunks,
		"cached_sub_chunks", snapshot.cachedSubChunks,
		"ordinary_sub_chunks", snapshot.ordinarySubChunks,
		"publisher_trace_limit", loadingOrderTraceLimit,
		"upstream_callback_publisher_update_count", snapshot.loadingOrder.UpstreamCallback.UpdateCount,
		"upstream_callback_publisher_overflow_count", snapshot.loadingOrder.UpstreamCallback.OverflowCount,
		"upstream_callback_publisher_invalid_count", snapshot.loadingOrder.UpstreamCallback.InvalidCount,
		"upstream_callback_publisher_updates", snapshot.loadingOrder.UpstreamCallback.Updates[:upstreamCaptured],
		"relay_publisher_update_count", snapshot.loadingOrder.Relay.UpdateCount,
		"relay_publisher_overflow_count", snapshot.loadingOrder.Relay.OverflowCount,
		"relay_publisher_invalid_count", snapshot.loadingOrder.Relay.InvalidCount,
		"relay_publisher_updates", snapshot.loadingOrder.Relay.Updates[:relayCaptured],
	)
}

// flipUpstreamClientCacheStatus rewrites the single enabled byte of an
// outbound upstream ClientCacheStatus payload so the real Bedrock server
// streams blob-referencing cached chunks. On the pinned gophertunnel module
// the write path hands the PacketFunc callback the live encode buffer and
// copies it afterwards, so writing payload[0] changes the wire byte; the
// read path passes bytes.Clone, so an unexpected inbound copy stays inert.
// Callers must flip before telemetry observation so PHASE2_CACHE_BOUNDARY
// records the effective value.
func flipUpstreamClientCacheStatus(payload []byte) {
	payload[0] = 1
}

func atomicSaturatingIncrement(counter *atomic.Uint64) {
	_ = atomicSaturatingIncrementValue(counter)
}
