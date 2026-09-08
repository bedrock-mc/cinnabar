package proxy

import (
	"bytes"
	"encoding/binary"
	"fmt"
	"log/slog"
	"math"
	"net"
	"strings"
	"sync"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestLoadingOrderTraceRecordsBoundaryOrderAndLevelChunksBeforePublisher(t *testing.T) {
	telemetry := new(cacheBoundaryTelemetry)
	payload := publisherUpdatePayload(protocol.BlockPos{-512, 42, -511}, 128, 3)

	telemetry.observeUpstreamPacket(packet.Header{PacketID: packet.IDLevelChunk}, nil, nil, nil)
	telemetry.observeUpstreamPacket(packet.Header{PacketID: packet.IDText}, nil, nil, nil)
	telemetry.observeUpstreamPacket(packet.Header{PacketID: packet.IDNetworkChunkPublisherUpdate}, payload, nil, nil)
	telemetry.observeRelayPacket(&packet.LevelChunk{})
	telemetry.observeRelayPacket(&packet.NetworkChunkPublisherUpdate{
		Position:    protocol.BlockPos{-512, 42, -511},
		Radius:      128,
		SavedChunks: make([]protocol.ChunkPos, 3),
	})

	snapshot := telemetry.snapshot().loadingOrder
	want := publisherOrderEvidence{
		Sequence:          3,
		CenterX:           -512,
		CenterY:           42,
		CenterZ:           -511,
		RadiusBlocks:      128,
		SavedChunkCount:   3,
		LevelChunksBefore: 1,
	}
	if got := snapshot.UpstreamCallback.Updates[0]; got != want {
		t.Fatalf("upstream callback update = %#v, want %#v", got, want)
	}
	want.Sequence = 2
	if got := snapshot.Relay.Updates[0]; got != want {
		t.Fatalf("relay update = %#v, want %#v", got, want)
	}
	if snapshot.UpstreamCallback.UpdateCount != 1 || snapshot.Relay.UpdateCount != 1 {
		t.Fatalf("update counts = upstream %d relay %d, want 1/1", snapshot.UpstreamCallback.UpdateCount, snapshot.Relay.UpdateCount)
	}
}

func TestLoadingOrderTraceKeepsTwoBoundariesIndependent(t *testing.T) {
	telemetry := new(cacheBoundaryTelemetry)
	first := &packet.NetworkChunkPublisherUpdate{Position: protocol.BlockPos{1, 2, 3}, Radius: 32}
	second := &packet.NetworkChunkPublisherUpdate{Position: protocol.BlockPos{4, 5, 6}, Radius: 128}

	telemetry.observeUpstreamPacket(packet.Header{PacketID: packet.IDNetworkChunkPublisherUpdate}, publisherUpdatePayload(first.Position, first.Radius, 0), nil, nil)
	telemetry.observeRelayPacket(first)
	telemetry.observeUpstreamPacket(packet.Header{PacketID: packet.IDLevelChunk}, nil, nil, nil)
	telemetry.observeUpstreamPacket(packet.Header{PacketID: packet.IDNetworkChunkPublisherUpdate}, publisherUpdatePayload(second.Position, second.Radius, 7), nil, nil)

	snapshot := telemetry.snapshot().loadingOrder
	if got := snapshot.UpstreamCallback.Updates[0]; got.CenterX != 1 || got.RadiusBlocks != 32 {
		t.Fatalf("first upstream callback update = %#v", got)
	}
	if got := snapshot.Relay.Updates[0]; got.CenterX != 1 || got.RadiusBlocks != 32 {
		t.Fatalf("first relay update = %#v", got)
	}
	if got := snapshot.UpstreamCallback.Updates[1]; got.CenterX != 4 || got.RadiusBlocks != 128 || got.SavedChunkCount != 7 || got.LevelChunksBefore != 1 {
		t.Fatalf("second upstream callback update = %#v", got)
	}
	if snapshot.Relay.UpdateCount != 1 {
		t.Fatalf("relay update count = %d, want 1", snapshot.Relay.UpdateCount)
	}
}

func TestLoadingOrderTraceBoundsPublisherEvidence(t *testing.T) {
	telemetry := new(cacheBoundaryTelemetry)
	for index := 0; index < loadingOrderTraceLimit+5; index++ {
		update := &packet.NetworkChunkPublisherUpdate{Position: protocol.BlockPos{int32(index), 0, 0}}
		telemetry.observeUpstreamPacket(packet.Header{PacketID: packet.IDNetworkChunkPublisherUpdate}, publisherUpdatePayload(update.Position, 0, 0), nil, nil)
		telemetry.observeRelayPacket(update)
	}

	snapshot := telemetry.snapshot().loadingOrder
	for name, boundary := range map[string]publisherBoundarySnapshot{
		"upstream_callback": snapshot.UpstreamCallback,
		"relay":             snapshot.Relay,
	} {
		if boundary.UpdateCount != loadingOrderTraceLimit+5 || boundary.OverflowCount != 5 {
			t.Fatalf("%s counts = updates %d overflow %d", name, boundary.UpdateCount, boundary.OverflowCount)
		}
		for index, update := range boundary.Updates {
			if update.CenterX != int32(index) {
				t.Fatalf("%s update %d center X = %d", name, index, update.CenterX)
			}
		}
	}
}

func TestLoadingOrderTraceRejectsMalformedPublisherPayloadWithoutMutation(t *testing.T) {
	oversized := publisherUpdatePayload(protocol.BlockPos{}, 0, maxPublisherSavedChunks+1)
	cases := map[string][]byte{
		"empty":                 nil,
		"unterminated position": {0x80, 0x80, 0x80, 0x80, 0x80},
		"overflowing radius":    {0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0x10, 0, 0, 0, 0},
		"truncated count":       publisherUpdatePrefix(protocol.BlockPos{}, 0),
		"oversized list":        oversized,
		"truncated list":        publisherUpdatePayload(protocol.BlockPos{}, 0, 1)[:9],
		"trailing bytes":        append(publisherUpdatePayload(protocol.BlockPos{}, 0, 0), 0xff),
	}

	telemetry := new(cacheBoundaryTelemetry)
	for name, payload := range cases {
		t.Run(name, func(t *testing.T) {
			before := bytes.Clone(payload)
			telemetry.observeUpstreamPacket(packet.Header{PacketID: packet.IDNetworkChunkPublisherUpdate}, payload, nil, nil)
			if !bytes.Equal(payload, before) {
				t.Fatalf("observer mutated payload: got %v want %v", payload, before)
			}
		})
	}

	boundary := telemetry.snapshot().loadingOrder.UpstreamCallback
	if boundary.UpdateCount != 0 || boundary.InvalidCount != uint64(len(cases)) {
		t.Fatalf("malformed snapshot = %#v, want zero updates and %d invalid", boundary, len(cases))
	}
}

func TestPublisherOrderParserMatchesCurrentPacketMarshaller(t *testing.T) {
	savedChunks := make([]protocol.ChunkPos, maxPublisherSavedChunks)
	for index := range savedChunks {
		savedChunks[index] = protocol.ChunkPos{int32(index), -int32(index)}
	}
	value := &packet.NetworkChunkPublisherUpdate{
		Position:    protocol.BlockPos{math.MinInt32, math.MaxInt32, -1},
		Radius:      math.MaxUint32,
		SavedChunks: savedChunks,
	}
	var payload bytes.Buffer
	value.Marshal(minecraft.DefaultProtocol.NewWriter(&payload, 0))

	got, ok := parsePublisherOrderEvidence(payload.Bytes())
	want := publisherOrderEvidence{
		CenterX:         math.MinInt32,
		CenterY:         math.MaxInt32,
		CenterZ:         -1,
		RadiusBlocks:    math.MaxUint32,
		SavedChunkCount: maxPublisherSavedChunks,
	}
	if !ok || got != want {
		t.Fatalf("marshaller evidence = %#v, %t; want %#v, true", got, ok, want)
	}
}

func TestLoadingOrderTraceCountersSaturate(t *testing.T) {
	trace := loadingOrderTrace{}
	for _, boundary := range []*publisherBoundaryTrace{&trace.upstreamCallback, &trace.relay} {
		boundary.sequence.Store(math.MaxUint64)
		boundary.levelChunkCount.Store(math.MaxUint64)
		boundary.updateCount = math.MaxUint64
		boundary.overflowCount = math.MaxUint64
		boundary.invalidCount = math.MaxUint64
	}

	trace.observeUpstreamCallback(packet.Header{PacketID: packet.IDNetworkChunkPublisherUpdate}, nil)
	trace.observeRelay(&packet.NetworkChunkPublisherUpdate{})
	snapshot := trace.snapshot()
	for name, got := range map[string]publisherBoundarySnapshot{
		"upstream_callback": snapshot.UpstreamCallback,
		"relay":             snapshot.Relay,
	} {
		if got.UpdateCount != math.MaxUint64 || got.OverflowCount != math.MaxUint64 || got.InvalidCount != math.MaxUint64 {
			t.Fatalf("%s counters did not saturate: %#v", name, got)
		}
	}
}

func TestLoadingOrderTraceSnapshotIsSafeDuringObservation(t *testing.T) {
	telemetry := new(cacheBoundaryTelemetry)
	payload := publisherUpdatePayload(protocol.BlockPos{1, 2, 3}, 32, 0)
	const observerCount = 4
	const updatesPerObserver = 100
	var observers sync.WaitGroup
	observers.Add(observerCount)
	for range observerCount {
		go func() {
			defer observers.Done()
			for range updatesPerObserver {
				telemetry.observeUpstreamPacket(packet.Header{PacketID: packet.IDNetworkChunkPublisherUpdate}, payload, nil, nil)
				telemetry.observeRelayPacket(&packet.NetworkChunkPublisherUpdate{Position: protocol.BlockPos{1, 2, 3}, Radius: 32})
			}
		}()
	}
	for index := 0; index < updatesPerObserver; index++ {
		_ = telemetry.snapshot()
	}
	observers.Wait()

	snapshot := telemetry.snapshot().loadingOrder
	want := uint64(observerCount * updatesPerObserver)
	if snapshot.UpstreamCallback.UpdateCount != want || snapshot.Relay.UpdateCount != want {
		t.Fatalf("concurrent update counts = upstream %d relay %d, want %d", snapshot.UpstreamCallback.UpdateCount, snapshot.Relay.UpdateCount, want)
	}
}

func TestCacheBoundaryReportContainsOnlyBoundedPublisherNumericEvidence(t *testing.T) {
	telemetry := new(cacheBoundaryTelemetry)
	secretAddr := testSecretAddr("sensitive-value-must-not-appear")
	payload := publisherUpdatePayload(protocol.BlockPos{-7, 64, 9}, 32, 2)
	telemetry.observeUpstreamPacket(packet.Header{PacketID: packet.IDNetworkChunkPublisherUpdate}, payload, secretAddr, secretAddr)
	telemetry.observeRelayPacket(&packet.NetworkChunkPublisherUpdate{Position: protocol.BlockPos{-7, 64, 9}, Radius: 32, SavedChunks: make([]protocol.ChunkPos, 2)})

	var output bytes.Buffer
	telemetry.report(slog.New(slog.NewTextHandler(&output, nil)))
	report := output.String()
	for _, want := range []string{
		"PHASE2_CACHE_BOUNDARY",
		"upstream_callback_publisher_updates",
		"relay_publisher_updates",
		"publisher_trace_limit=16",
		"CenterX:-7",
		"RadiusBlocks:32",
		"SavedChunkCount:2",
	} {
		if !strings.Contains(report, want) {
			t.Fatalf("report %q does not contain %q", report, want)
		}
	}
	if strings.Contains(report, secretAddr.String()) || strings.Contains(report, fmt.Sprint(payload)) {
		t.Fatalf("report retained unsafe input: %q", report)
	}
}

func publisherUpdatePayload(position protocol.BlockPos, radius, savedChunkCount uint32) []byte {
	payload := publisherUpdatePrefix(position, radius)
	var count [4]byte
	binary.LittleEndian.PutUint32(count[:], savedChunkCount)
	payload = append(payload, count[:]...)
	for index := uint32(0); index < savedChunkCount && index <= maxPublisherSavedChunks; index++ {
		payload = appendZigZag32(payload, int32(index))
		payload = appendZigZag32(payload, -int32(index))
	}
	return payload
}

func publisherUpdatePrefix(position protocol.BlockPos, radius uint32) []byte {
	payload := appendZigZag32(nil, position.X())
	payload = appendZigZag32(payload, position.Y())
	payload = appendZigZag32(payload, position.Z())
	return appendVaruint32(payload, radius)
}

func appendZigZag32(destination []byte, value int32) []byte {
	encoded := uint32(value) << 1
	if value < 0 {
		encoded = ^encoded
	}
	return appendVaruint32(destination, encoded)
}

func appendVaruint32(destination []byte, value uint32) []byte {
	for value >= 0x80 {
		destination = append(destination, byte(value)|0x80)
		value >>= 7
	}
	return append(destination, byte(value))
}

type testSecretAddr string

func (address testSecretAddr) Network() string { return "test" }
func (address testSecretAddr) String() string  { return string(address) }

var _ net.Addr = testSecretAddr("")
