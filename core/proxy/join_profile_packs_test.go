package proxy

import (
	"bytes"
	"math"
	"net"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const profilePackLatencyBin = 100 * time.Millisecond

type profilePackTransfer struct {
	ordinal                  int
	size                     uint64
	chunkSize                uint32
	chunkCount               uint32
	requests, received       uint64
	bytes                    uint64
	maxOutstanding           int
	pending                  map[int32]time.Time
	firstRequest, firstData  time.Time
	lastData                 time.Time
	maximumGap               time.Duration
	latencyTotal, latencyMax time.Duration
	latencyCount             uint64
	latencies                [129]uint64
	dropped                  uint64
}

type profilePackTransfers struct {
	mu          sync.Mutex
	protocol    minecraft.Protocol
	byID        map[string]*profilePackTransfer
	started     time.Time
	concurrency int
}

func newProfilePackTransfers(protocol minecraft.Protocol) *profilePackTransfers {
	return &profilePackTransfers{protocol: protocol, byID: make(map[string]*profilePackTransfer), started: time.Now()}
}

func (transfers *profilePackTransfers) wrap(observe func(packet.Header, []byte, net.Addr, net.Addr)) func(packet.Header, []byte, net.Addr, net.Addr) {
	return func(header packet.Header, payload []byte, source, destination net.Addr) {
		transfers.observe(header.PacketID, payload)
		observe(header, payload, source, destination)
	}
}

func decodeProfilePacket(protocol minecraft.Protocol, value packet.Packet, payload []byte) (ok bool) {
	defer func() {
		if recover() != nil {
			ok = false
		}
	}()
	buffer := bytes.NewBuffer(payload)
	value.Marshal(protocol.NewReader(buffer, 0, true))
	return buffer.Len() == 0
}

func (transfers *profilePackTransfers) observe(id uint32, payload []byte) {
	switch id {
	case packet.IDResourcePackDataInfo:
		info := new(packet.ResourcePackDataInfo)
		if !decodeProfilePacket(transfers.protocol, info, payload) {
			return
		}
		identity := strings.SplitN(info.UUID, "_", 2)[0]
		transfers.mu.Lock()
		defer transfers.mu.Unlock()
		if len(transfers.byID) == maxSelectedResourcePacks || transfers.byID[identity] != nil {
			return
		}
		transfers.byID[identity] = &profilePackTransfer{
			ordinal: len(transfers.byID), size: info.Size, chunkSize: info.DataChunkSize,
			chunkCount: info.ChunkCount, pending: make(map[int32]time.Time),
		}
	case packet.IDResourcePackChunkRequest:
		request := new(packet.ResourcePackChunkRequest)
		if !decodeProfilePacket(transfers.protocol, request, payload) {
			return
		}
		identity := strings.SplitN(request.UUID, "_", 2)[0]
		transfers.mu.Lock()
		defer transfers.mu.Unlock()
		transfer := transfers.byID[identity]
		if transfer == nil {
			return
		}
		transfer.requests++
		if len(transfer.pending) == 4096 {
			transfer.dropped++
			return
		}
		now := time.Now()
		if transfer.firstRequest.IsZero() {
			transfer.firstRequest = now
		}
		transfer.pending[request.ChunkIndex] = now
		transfer.maxOutstanding = max(transfer.maxOutstanding, len(transfer.pending))
		active := 0
		for _, other := range transfers.byID {
			if len(other.pending) > 0 {
				active++
			}
		}
		transfers.concurrency = max(transfers.concurrency, active)
	case packet.IDResourcePackChunkData:
		data := new(packet.ResourcePackChunkData)
		if !decodeProfilePacket(transfers.protocol, data, payload) {
			return
		}
		identity := strings.SplitN(data.UUID, "_", 2)[0]
		transfers.mu.Lock()
		defer transfers.mu.Unlock()
		transfer := transfers.byID[identity]
		if transfer == nil {
			return
		}
		now := time.Now()
		if sent, found := transfer.pending[int32(data.ChunkIndex)]; found {
			latency := now.Sub(sent)
			transfer.latencyTotal += latency
			transfer.latencyMax = max(transfer.latencyMax, latency)
			transfer.latencyCount++
			bin := min(int(latency/profilePackLatencyBin), len(transfer.latencies)-1)
			transfer.latencies[bin]++
			delete(transfer.pending, int32(data.ChunkIndex))
		}
		if transfer.firstData.IsZero() {
			transfer.firstData = now
		} else {
			transfer.maximumGap = max(transfer.maximumGap, now.Sub(transfer.lastData))
		}
		transfer.lastData = now
		transfer.received++
		transfer.bytes += uint64(len(data.Data))
	}
}

func (transfers *profilePackTransfers) report(t *testing.T, attempt int) {
	t.Helper()
	transfers.mu.Lock()
	defer transfers.mu.Unlock()
	if len(transfers.byID) == 0 {
		return
	}
	t.Logf("JOIN_PROFILE_PACKS attempt=%d transfers=%d max_concurrent=%d", attempt, len(transfers.byID), transfers.concurrency)
	for _, transfer := range transfers.byID {
		duration := transfer.lastData.Sub(transfer.firstRequest)
		var rate, mean float64
		if duration > 0 {
			rate = float64(transfer.bytes) / duration.Seconds()
		}
		if transfer.latencyCount > 0 {
			mean = transfer.latencyTotal.Seconds() * 1000 / float64(transfer.latencyCount)
		}
		t.Logf("JOIN_PROFILE_PACK attempt=%d ordinal=%d offered_bytes=%d chunk_bytes=%d advertised_chunks=%d requests=%d received=%d bytes=%d max_in_flight=%d duration_ms=%.3f bytes_per_second=%.0f first_request_ms=%.3f first_response_wait_ms=%.3f max_response_gap_ms=%.3f request_latency_mean_ms=%.3f request_latency_p50_upper_ms=%d request_latency_p99_upper_ms=%d request_latency_max_ms=%.3f dropped_samples=%d",
			attempt, transfer.ordinal, transfer.size, transfer.chunkSize, transfer.chunkCount, transfer.requests, transfer.received, transfer.bytes, transfer.maxOutstanding,
			duration.Seconds()*1000, rate, transfer.firstRequest.Sub(transfers.started).Seconds()*1000, transfer.firstData.Sub(transfer.firstRequest).Seconds()*1000,
			transfer.maximumGap.Seconds()*1000, mean, profilePackLatencyQuantile(transfer, 0.5), profilePackLatencyQuantile(transfer, 0.99), transfer.latencyMax.Seconds()*1000, transfer.dropped)
	}
}

func profilePackLatencyQuantile(transfer *profilePackTransfer, fraction float64) int {
	if transfer.latencyCount == 0 {
		return 0
	}
	threshold := uint64(math.Ceil(float64(transfer.latencyCount) * fraction))
	var cumulative uint64
	for bin, count := range transfer.latencies {
		cumulative += count
		if cumulative >= threshold {
			if bin == len(transfer.latencies)-1 {
				return int(math.Ceil(transfer.latencyMax.Seconds() * 1000))
			}
			return (bin + 1) * int(profilePackLatencyBin/time.Millisecond)
		}
	}
	return 0
}
