package proxy

import (
	"encoding/binary"
	"sync"
	"sync/atomic"

	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const (
	loadingOrderTraceLimit    = 16
	maxPublisherSavedChunks   = 9216
	maxPublisherVaruint32Size = 5
)

// publisherOrderEvidence contains only the bounded numeric fields needed to
// compare publisher ordering at the upstream callback and relay boundaries.
type publisherOrderEvidence struct {
	Sequence          uint64
	CenterX           int32
	CenterY           int32
	CenterZ           int32
	RadiusBlocks      uint32
	SavedChunkCount   uint32
	LevelChunksBefore uint64
}

type publisherBoundarySnapshot struct {
	Updates       [loadingOrderTraceLimit]publisherOrderEvidence
	UpdateCount   uint64
	OverflowCount uint64
	InvalidCount  uint64
}

type loadingOrderTraceSnapshot struct {
	UpstreamCallback publisherBoundarySnapshot
	Relay            publisherBoundarySnapshot
}

type loadingOrderTrace struct {
	upstreamCallback publisherBoundaryTrace
	relay            publisherBoundaryTrace
}

type publisherBoundaryTrace struct {
	mu sync.Mutex

	sequence        atomic.Uint64
	levelChunkCount atomic.Uint64
	updates         [loadingOrderTraceLimit]publisherOrderEvidence
	updateCount     uint64
	overflowCount   uint64
	invalidCount    uint64
}

func (trace *loadingOrderTrace) observeUpstreamCallback(header packet.Header, payload []byte) {
	boundary := &trace.upstreamCallback
	sequence := atomicSaturatingIncrementValue(&boundary.sequence)
	switch header.PacketID {
	case packet.IDLevelChunk:
		atomicSaturatingIncrement(&boundary.levelChunkCount)
	case packet.IDNetworkChunkPublisherUpdate:
		update, ok := parsePublisherOrderEvidence(payload)
		if !ok {
			boundary.mu.Lock()
			saturatingIncrement(&boundary.invalidCount)
			boundary.mu.Unlock()
			return
		}
		boundary.record(update, sequence, boundary.levelChunkCount.Load())
	}
}

func (trace *loadingOrderTrace) observeRelay(value packet.Packet) {
	boundary := &trace.relay
	sequence := atomicSaturatingIncrementValue(&boundary.sequence)
	switch value := value.(type) {
	case *packet.LevelChunk:
		atomicSaturatingIncrement(&boundary.levelChunkCount)
	case *packet.NetworkChunkPublisherUpdate:
		boundary.record(publisherOrderEvidence{
			CenterX:         value.Position.X(),
			CenterY:         value.Position.Y(),
			CenterZ:         value.Position.Z(),
			RadiusBlocks:    value.Radius,
			SavedChunkCount: boundedUint32Len(len(value.SavedChunks)),
		}, sequence, boundary.levelChunkCount.Load())
	}
}

func (boundary *publisherBoundaryTrace) record(update publisherOrderEvidence, sequence, levelChunksBefore uint64) {
	boundary.mu.Lock()
	defer boundary.mu.Unlock()
	update.Sequence = sequence
	update.LevelChunksBefore = levelChunksBefore
	if boundary.updateCount < loadingOrderTraceLimit {
		boundary.updates[boundary.updateCount] = update
	} else {
		saturatingIncrement(&boundary.overflowCount)
	}
	saturatingIncrement(&boundary.updateCount)
}

func (trace *loadingOrderTrace) snapshot() loadingOrderTraceSnapshot {
	return loadingOrderTraceSnapshot{
		UpstreamCallback: trace.upstreamCallback.snapshot(),
		Relay:            trace.relay.snapshot(),
	}
}

func (boundary *publisherBoundaryTrace) snapshot() publisherBoundarySnapshot {
	boundary.mu.Lock()
	defer boundary.mu.Unlock()
	return publisherBoundarySnapshot{
		Updates:       boundary.updates,
		UpdateCount:   boundary.updateCount,
		OverflowCount: boundary.overflowCount,
		InvalidCount:  boundary.invalidCount,
	}
}

// parsePublisherOrderEvidence follows the current NetworkChunkPublisherUpdate
// marshaller: three zig-zag varint32 block coordinates, a varuint32 block
// radius, a little-endian uint32 saved/server-built chunk-list count, then that
// many pairs of zig-zag varint32 chunk coordinates. It validates the entire
// payload but retains only the list count. Both varints and the list are capped;
// the parser never allocates from wire-provided sizes and never calls protocol
// readers whose malformed-data path may panic.
func parsePublisherOrderEvidence(payload []byte) (publisherOrderEvidence, bool) {
	reader := publisherEvidenceReader{payload: payload}
	x, ok := reader.varint32()
	if !ok {
		return publisherOrderEvidence{}, false
	}
	y, ok := reader.varint32()
	if !ok {
		return publisherOrderEvidence{}, false
	}
	z, ok := reader.varint32()
	if !ok {
		return publisherOrderEvidence{}, false
	}
	radius, ok := reader.varuint32()
	if !ok {
		return publisherOrderEvidence{}, false
	}
	count, ok := reader.uint32()
	if !ok || count > maxPublisherSavedChunks {
		return publisherOrderEvidence{}, false
	}
	for index := uint32(0); index < count; index++ {
		if _, ok := reader.varint32(); !ok {
			return publisherOrderEvidence{}, false
		}
		if _, ok := reader.varint32(); !ok {
			return publisherOrderEvidence{}, false
		}
	}
	if reader.offset != len(reader.payload) {
		return publisherOrderEvidence{}, false
	}
	return publisherOrderEvidence{
		CenterX:         x,
		CenterY:         y,
		CenterZ:         z,
		RadiusBlocks:    radius,
		SavedChunkCount: count,
	}, true
}

type publisherEvidenceReader struct {
	payload []byte
	offset  int
}

func (reader *publisherEvidenceReader) varint32() (int32, bool) {
	value, ok := reader.varuint32()
	if !ok {
		return 0, false
	}
	decoded := int32(value >> 1)
	if value&1 != 0 {
		decoded = ^decoded
	}
	return decoded, true
}

func (reader *publisherEvidenceReader) varuint32() (uint32, bool) {
	var value uint32
	for index := uint(0); index < maxPublisherVaruint32Size; index++ {
		if reader.offset >= len(reader.payload) {
			return 0, false
		}
		current := reader.payload[reader.offset]
		reader.offset++
		if index == maxPublisherVaruint32Size-1 && current&0xf0 != 0 {
			return 0, false
		}
		value |= uint32(current&0x7f) << (index * 7)
		if current&0x80 == 0 {
			return value, true
		}
	}
	return 0, false
}

func (reader *publisherEvidenceReader) uint32() (uint32, bool) {
	if len(reader.payload)-reader.offset < 4 {
		return 0, false
	}
	value := binary.LittleEndian.Uint32(reader.payload[reader.offset : reader.offset+4])
	reader.offset += 4
	return value, true
}

func boundedUint32Len(length int) uint32 {
	if uint64(length) > uint64(^uint32(0)) {
		return ^uint32(0)
	}
	return uint32(length)
}

func saturatingIncrement(value *uint64) {
	if *value != ^uint64(0) {
		*value++
	}
}

func atomicSaturatingIncrementValue(counter *atomic.Uint64) uint64 {
	for {
		current := counter.Load()
		if current == ^uint64(0) {
			return current
		}
		if counter.CompareAndSwap(current, current+1) {
			return current + 1
		}
	}
}
