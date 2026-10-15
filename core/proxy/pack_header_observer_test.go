package proxy

import (
	"bytes"
	"context"
	"encoding/binary"
	"fmt"
	"log/slog"
	"math"
	"net"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const packHeaderUnknownPacketID = 0x3ff

func TestPackHeaderObserverPreservesSubclientFilter(t *testing.T) {
	for _, enabled := range []bool{false, true} {
		for sender := byte(0); sender < 4; sender++ {
			for target := byte(0); target < 4; target++ {
				t.Run(fmt.Sprintf("observer=%t/sender=%d/target=%d", enabled, sender, target), func(t *testing.T) {
					var telemetry *resourcePackAdmissionTelemetry
					if enabled {
						telemetry = newResourcePackAdmissionTelemetry(1, nil)
					}
					dialer := newUpstreamDialerForAdmission(dialerTestDownstream{protocol: minecraft.DefaultProtocol}, nil, nil, telemetry, false)
					if dialer.PacketFunc != nil {
						t.Fatal("header-only telemetry installed a payload observer")
					}
					for _, id := range []uint32{packet.IDResourcePacksInfo, packet.IDLevelChunk, packHeaderUnknownPacketID} {
						header := packet.Header{PacketID: id, SenderSubClient: sender, TargetSubClient: target}
						if got, want := dialer.AcceptPacketHeader(header), sender == 0 && target == 0; got != want {
							t.Fatalf("header %+v accepted=%t, want %t", header, got, want)
						}
					}
					if telemetry != nil {
						telemetry.observeFailure(context.Background())
						if got := telemetry.snapshot().Acquisition; got != ResourcePackAcquisitionFailed {
							t.Fatalf("negotiation failure = %q, want %q", got, ResourcePackAcquisitionFailed)
						}
					}
				})
			}
		}
	}
}

func TestPackHeaderObserverAllocatesNothingPerHeader(t *testing.T) {
	telemetry := newResourcePackAdmissionTelemetry(1, nil)
	dialer := newUpstreamDialerForAdmission(dialerTestDownstream{protocol: minecraft.DefaultProtocol}, nil, nil, telemetry, false)
	if dialer.PacketFunc != nil {
		t.Fatal("header-only telemetry enables incoming payload cloning")
	}
	headers := []packet.Header{
		{PacketID: packet.IDLevelChunk},
		{PacketID: packet.IDResourcePacksInfo},
		{PacketID: packet.IDResourcePacksInfo, TargetSubClient: 1},
	}
	allocations := testing.AllocsPerRun(100, func() {
		for _, header := range headers {
			dialer.AcceptPacketHeader(header)
		}
	})
	if allocations != 0 {
		t.Fatalf("header observation allocations = %g, want 0", allocations)
	}
}

func TestPackHeaderObserverReceivesBeforeFilteringAndDecoding(t *testing.T) {
	for _, test := range []struct {
		name        string
		headers     []packet.Header
		malformed   bool
		negotiation bool
		accepted    uint32
	}{
		{name: "unrelated login packets"},
		{name: "accepted invalid pack body", headers: []packet.Header{{PacketID: packet.IDResourcePacksInfo}}, negotiation: true, accepted: 1},
		{name: "rejected sender", headers: []packet.Header{{PacketID: packet.IDResourcePacksInfo, SenderSubClient: 1}}, negotiation: true},
		{name: "rejected target twice", headers: []packet.Header{{PacketID: packet.IDResourcePacksInfo, TargetSubClient: 1}, {PacketID: packet.IDResourcePacksInfo, TargetSubClient: 1}}, negotiation: true},
		{name: "unknown accepted", headers: []packet.Header{{PacketID: packHeaderUnknownPacketID}}, accepted: 1},
		{name: "unknown rejected", headers: []packet.Header{{PacketID: packHeaderUnknownPacketID, TargetSubClient: 1}}},
		{name: "malformed header", malformed: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			telemetry := newResourcePackAdmissionTelemetry(1, nil)
			dialer := newUpstreamDialerForAdmission(dialerTestDownstream{protocol: minecraft.DefaultProtocol}, nil, nil, telemetry, false)
			if dialer.PacketFunc != nil {
				t.Fatal("header-only telemetry installed a payload observer")
			}
			var seen, accepted, observed atomic.Uint32
			next := dialer.AcceptPacketHeader
			dialer.AcceptPacketHeader = func(header packet.Header) bool {
				allow := next(header)
				if header.PacketID == packet.IDResourcePacksInfo || header.PacketID == packHeaderUnknownPacketID {
					seen.Add(1)
					if allow {
						accepted.Add(1)
					}
					telemetry.mu.Lock()
					if telemetry.negotiation {
						observed.Add(1)
					}
					telemetry.mu.Unlock()
				}
				return allow
			}
			var frames [][]byte
			for _, header := range test.headers {
				// A truncated body proves observation does not depend on successful decoding.
				frames = append(frames, packHeaderFrame(t, header, []byte{0x80}))
			}
			if test.malformed {
				frames = append(frames, []byte{0x80})
			}
			runPackHeaderFixture(t, dialer, frames)
			if seen.Load() != uint32(len(test.headers)) || accepted.Load() != test.accepted {
				t.Fatalf("seen/accepted = %d/%d, want %d/%d", seen.Load(), accepted.Load(), len(test.headers), test.accepted)
			}
			wantObserved := uint32(0)
			if test.negotiation {
				wantObserved = uint32(len(test.headers))
			}
			if observed.Load() != wantObserved {
				t.Fatalf("negotiations observed at filter = %d, want %d", observed.Load(), wantObserved)
			}
			telemetry.observeFailure(context.Background())
			want := ResourcePackAcquisitionNone
			if test.negotiation {
				want = ResourcePackAcquisitionFailed
			}
			if got := telemetry.snapshot().Acquisition; got != want {
				t.Fatalf("failure after fixture = %q, want %q", got, want)
			}
		})
	}
}

func TestPackHeaderObserverPreservesPayloadObserverOwnership(t *testing.T) {
	telemetry := newResourcePackAdmissionTelemetry(1, nil)
	dialer := newUpstreamDialerForAdmission(dialerTestDownstream{protocol: minecraft.DefaultProtocol}, nil, nil, telemetry, false)
	var loginSuccessSeen atomic.Bool
	var unknownSeen atomic.Uint32
	var payloads [][]byte
	dialer.PacketFunc = func(header packet.Header, payload []byte, _, _ net.Addr) {
		switch header.PacketID {
		case packet.IDPlayStatus:
			// The real decoder must still see success and send cache status.
			binary.BigEndian.PutUint32(payload, uint32(packet.PlayStatusLoginFailedClient))
			loginSuccessSeen.Store(true)
		case packHeaderUnknownPacketID:
			payloads = append(payloads, payload)
			unknownSeen.Add(1)
		}
	}
	runPackHeaderFixture(t, dialer, [][]byte{
		packHeaderFrame(t, packet.Header{PacketID: packHeaderUnknownPacketID}, []byte{1, 2, 3}),
		packHeaderFrame(t, packet.Header{PacketID: packHeaderUnknownPacketID}, []byte{4, 5, 6}),
	})
	if !loginSuccessSeen.Load() || unknownSeen.Load() != 2 {
		t.Fatal("payload observer mutation prevented login or packet observation")
	}
	if !bytes.Equal(payloads[0], []byte{1, 2, 3}) || !bytes.Equal(payloads[1], []byte{4, 5, 6}) {
		t.Fatalf("retained payloads changed: %v", payloads)
	}
	payloads[0][0] = 9
	if payloads[1][0] != 4 {
		t.Fatal("payload observers received shared mutable storage")
	}
}

// packHeaderFrame encodes a header with an arbitrary body for receive-path fixtures.
func packHeaderFrame(t *testing.T, header packet.Header, payload []byte) []byte {
	t.Helper()
	var buffer bytes.Buffer
	if err := header.Write(&buffer); err != nil {
		t.Fatal(err)
	}
	buffer.Write(payload)
	return buffer.Bytes()
}

// runPackHeaderFixture sends frames at pack negotiation, then ends the incomplete
// login. Reading the client's cache status first fixes the observation phase.
func runPackHeaderFixture(t *testing.T, dialer minecraft.Dialer, frames [][]byte) {
	t.Helper()
	var terminal bytes.Buffer
	(&packet.Disconnect{HideDisconnectionScreen: true}).Marshal(minecraft.DefaultProtocol.NewWriter(&terminal, 0))
	frames = append(frames, packHeaderFrame(t, packet.Header{PacketID: packet.IDDisconnect}, terminal.Bytes()))
	network := newCacheStatusScriptedNetwork(func(conn net.Conn) error {
		decoder, encoder := packet.NewDecoder(conn), packet.NewEncoder(conn)
		if _, err := decoder.Decode(); err != nil {
			return fmt.Errorf("read network request: %w", err)
		}
		if err := encodeCacheStatusScriptedPackets(encoder, &packet.NetworkSettings{
			CompressionThreshold: math.MaxUint16,
			CompressionAlgorithm: packet.CompressionAlgorithmFlate,
		}); err != nil {
			return err
		}
		decoder.EnableCompression(packet.FlateCompression, math.MaxInt)
		encoder.EnableCompression(packet.FlateCompression, math.MaxUint16)
		if _, err := decoder.Decode(); err != nil {
			return fmt.Errorf("read login: %w", err)
		}
		if err := encodeCacheStatusScriptedPackets(encoder, &packet.PlayStatus{Status: packet.PlayStatusLoginSuccess}); err != nil {
			return err
		}
		if _, err := decoder.Decode(); err != nil {
			return fmt.Errorf("read cache status: %w", err)
		}
		// One batch ensures a peer close cannot race the fixture's final packets.
		if err := encoder.Encode(frames); err != nil {
			return err
		}
		if _, err := decoder.Decode(); err == nil {
			return fmt.Errorf("client continued after terminal fixture")
		}
		return nil
	})
	dialer.ErrorLog = slog.New(slog.DiscardHandler)
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	conn, err := dialer.DialContextNetwork(ctx, network, "pack-header.invalid:19132")
	if conn != nil {
		_ = conn.Close()
	}
	if err == nil {
		t.Fatal("incomplete login succeeded")
	}
	if scriptErr := <-network.done; scriptErr != nil {
		t.Fatalf("pack header fixture: %v (dial: %v)", scriptErr, err)
	}
	if ctx.Err() != nil {
		t.Fatalf("pack header fixture did not terminate before its deadline: %v", ctx.Err())
	}
}
