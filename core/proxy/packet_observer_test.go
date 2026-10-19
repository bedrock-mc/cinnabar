package proxy

import (
	"bytes"
	"context"
	"fmt"
	"log/slog"
	"maps"
	"net"
	"slices"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const observerStreamingPayloadBytes = 64 * 1024

func TestPacketObserverCompositionKeepsIndividualInterests(t *testing.T) {
	var calls []string
	callback := func(name string) func(packet.Header, []byte, net.Addr, net.Addr) {
		return func(packet.Header, []byte, net.Addr, net.Addr) { calls = append(calls, name) }
	}
	dialer := minecraft.Dialer{PacketFunc: callback("legacy")}
	dialer = withPacketObserver(dialer, minecraft.PacketObserver{
		PacketIDs: []uint32{packet.IDResourcePacksInfo, packet.IDResourcePackStack}, Func: callback("previous"),
	})
	dialer = withPacketObserver(dialer, minecraft.PacketObserver{
		PacketIDs: []uint32{packet.IDResourcePacksInfo, packet.IDStartGame}, Func: callback("current"),
	})
	for _, test := range []struct {
		id   uint32
		want []string
	}{
		{packet.IDResourcePacksInfo, []string{"current", "previous"}},
		{packet.IDResourcePackStack, []string{"previous"}},
		{packet.IDStartGame, []string{"current"}},
		{packet.IDLevelChunk, nil},
	} {
		calls = nil
		header := packet.Header{PacketID: test.id}
		dialer.PacketObserver.Func(header, nil, proxyTestAddr("same"), proxyTestAddr("same"))
		if !slices.Equal(calls, test.want) {
			t.Fatalf("packet %d calls = %v, want %v", test.id, calls, test.want)
		}
		dialer.PacketFunc(header, nil, nil, nil)
		if calls[len(calls)-1] != "legacy" {
			t.Fatal("composition replaced the unrestricted callback")
		}
	}
	for _, observer := range []minecraft.PacketObserver{{}, {PacketIDs: []uint32{packet.IDLevelChunk}}, {Func: callback("empty")}} {
		dialer = withPacketObserver(dialer, observer)
	}
	calls = nil
	dialer.PacketObserver.Func(packet.Header{PacketID: packet.IDStartGame}, nil, nil, nil)
	if !slices.Equal(calls, []string{"current"}) {
		t.Fatalf("disabled observer changed composition: %v", calls)
	}
}

func TestJoinStageInterestsAndCompletionPrefixes(t *testing.T) {
	for id, stage := range joinPacketStages {
		t.Run(stage, func(t *testing.T) {
			var logs bytes.Buffer
			dialer := withJoinStages(minecraft.Dialer{}, slog.New(slog.NewTextHandler(&logs, nil)))
			if !slices.Contains(dialer.PacketObserver.PacketIDs, id) || dialer.PacketFunc != nil {
				t.Fatal("stage is missing from selective interests or enables legacy observation")
			}
			for range 2 {
				dialer.PacketObserver.Func(packet.Header{PacketID: id}, []byte{packet.PackResponseCompleted}, nil, nil)
			}
			if got := strings.Count(logs.String(), `msg="`+stage+`"`); got != 1 {
				t.Fatalf("stage count = %d, want 1: %s", got, logs.String())
			}
		})
	}
	for _, test := range []struct {
		name    string
		payload []byte
		logged  bool
	}{
		{name: "empty"},
		{name: "truncated", payload: []byte{0x80}},
		{name: "overflow", payload: bytes.Repeat([]byte{0xff}, 11)},
		{name: "not completed", payload: []byte{packet.PackResponseAllPacksDownloaded}},
		{name: "completed prefix", payload: []byte{packet.PackResponseCompleted}, logged: true},
		{name: "invalid trailing body", payload: []byte{packet.PackResponseCompleted, 0xff}, logged: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			var logs bytes.Buffer
			dialer := withJoinStages(minecraft.Dialer{}, slog.New(slog.NewTextHandler(&logs, nil)))
			dialer.PacketObserver.Func(packet.Header{PacketID: packet.IDResourcePackClientResponse}, test.payload, nil, nil)
			if got := strings.Contains(logs.String(), joinPacketStages[packet.IDResourcePackClientResponse]); got != test.logged {
				t.Fatalf("completed response logged=%t, want %t", got, test.logged)
			}
		})
	}
}

func TestSelectiveBudgetObservesRejectedLateMalformedOffers(t *testing.T) {
	for _, header := range []packet.Header{
		{PacketID: packet.IDResourcePacksInfo, SenderSubClient: 1},
		{PacketID: packet.IDResourcePacksInfo, TargetSubClient: 1},
	} {
		t.Run(fmt.Sprintf("%+v", header), func(t *testing.T) {
			budget, _ := observedBudget(t, packInfos(1))
			budget.finish()
			telemetry := newResourcePackAdmissionTelemetry(1, nil)
			var logs bytes.Buffer
			dialer := newUpstreamDialerForAdmission(dialerTestDownstream{protocol: minecraft.DefaultProtocol}, nil, nil, telemetry, false)
			dialer = withJoinStages(withResourcePackAcquisitionBudget(dialer, budget), slog.New(slog.NewTextHandler(&logs, nil)))
			var seen atomic.Uint32
			dialer.PacketFunc = func(h packet.Header, _ []byte, _, _ net.Addr) {
				if h.PacketID != packet.IDResourcePacksInfo {
					return
				}
				if !strings.Contains(logs.String(), joinPacketStages[h.PacketID]) {
					t.Error("legacy callback ran before stage observation")
				}
				budget.mu.Lock()
				if len(budget.accepted) != 0 || !budget.done {
					t.Error("malformed late offer did not reset the finished budget before the legacy callback")
				}
				budget.mu.Unlock()
				seen.Add(1)
			}
			runPackHeaderFixture(t, dialer, [][]byte{packHeaderFrame(t, header, []byte{0x80})})
			telemetry.observeFailure(context.Background())
			if seen.Load() != 1 || telemetry.snapshot().Acquisition != ResourcePackAcquisitionFailed {
				t.Fatal("rejected offer lost observation or negotiation telemetry")
			}
		})
	}
}

func TestSelectiveBudgetResetsWhenTheProtocolHasNoOfferFactory(t *testing.T) {
	budget, _ := observedBudget(t, packInfos(1))
	budget.proto = observerMissingOfferProtocol{minecraft.DefaultProtocol}
	dialer := withResourcePackAcquisitionBudget(minecraft.Dialer{}, budget)
	dialer.PacketObserver.Func(packet.Header{PacketID: packet.IDResourcePacksInfo}, encodeLatest(t, packInfos(1)), nil, nil)
	if len(budget.accepted) != 0 || len(budget.offered) != 0 {
		t.Fatal("unrecognized offer retained the previous admission")
	}
	nilBudget := withResourcePackAcquisitionBudget(minecraft.Dialer{}, nil)
	nilBudget.PacketObserver.Func(packet.Header{PacketID: packet.IDResourcePacksInfo}, nil, nil, nil)
}

type observerMissingOfferProtocol struct{ minecraft.Protocol }

// Packets returns an independent pool without the offer fixture's packet ID.
func (proto observerMissingOfferProtocol) Packets(listener bool) packet.Pool {
	pool := maps.Clone(proto.Protocol.Packets(listener))
	delete(pool, packet.IDResourcePacksInfo)
	return pool
}

func TestProductionSelectiveObserversSkipStreamingBodies(t *testing.T) {
	for _, legacy := range []bool{false, true} {
		t.Run(fmt.Sprintf("legacy=%t", legacy), func(t *testing.T) {
			network := newMemoryNetwork()
			listener, err := (minecraft.ListenConfig{AuthenticationDisabled: true, ErrorLog: slog.New(slog.DiscardHandler)}).ListenNetwork(network, "")
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = listener.Close() })
			ids := []uint32{packet.IDLevelChunk, packet.IDPlayerSkin, packHeaderUnknownPacketID}
			frames := make(map[uint32][]byte)
			for _, id := range ids {
				frames[id] = packHeaderFrame(t, packet.Header{PacketID: id}, bytes.Repeat([]byte{byte(id)}, observerStreamingPayloadBytes))
			}
			serverDone := make(chan error, 1)
			go func() {
				accepted, err := listener.Accept()
				if err != nil {
					serverDone <- err
					return
				}
				server := accepted.(*minecraft.Conn)
				defer server.Abort()
				if err := server.WritePacketImmediate(relayFixtureStartup()...); err != nil {
					serverDone <- err
					return
				}
				for _, id := range ids {
					if err := server.WritePacketRaw(bytes.Clone(frames[id])); err != nil {
						serverDone <- err
						return
					}
				}
				serverDone <- server.Flush()
				<-server.Context().Done()
			}()
			budget := newResourcePackAcquisitionBudget(minecraft.DefaultProtocol, func(error) {})
			telemetry := newResourcePackAdmissionTelemetry(1, nil)
			dialer := newUpstreamDialerForAdmission(dialerTestDownstream{protocol: minecraft.DefaultProtocol}, nil, nil, telemetry, false)
			var legacyBytes, selectiveBytes, offers atomic.Uint64
			if legacy {
				dialer.PacketFunc = func(header packet.Header, payload []byte, _, _ net.Addr) {
					if slices.Contains(ids, header.PacketID) {
						legacyBytes.Add(uint64(len(payload)))
					}
				}
			}
			dialer = withJoinStages(withResourcePackAcquisitionBudget(dialer, budget), slog.New(slog.DiscardHandler))
			for _, id := range ids {
				if slices.Contains(dialer.PacketObserver.PacketIDs, id) {
					t.Fatalf("unrelated streaming ID %d selected by production observers", id)
				}
			}
			if (dialer.PacketFunc != nil) != legacy {
				t.Fatal("production observers changed legacy callback registration")
			}
			next := dialer.PacketObserver.Func
			dialer.PacketObserver.Func = func(header packet.Header, payload []byte, source, destination net.Addr) {
				if slices.Contains(ids, header.PacketID) {
					selectiveBytes.Add(uint64(len(payload)))
				}
				if header.PacketID == packet.IDResourcePacksInfo {
					offers.Add(1)
				}
				next(header, payload, source, destination)
			}
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			client, err := dialer.DialContextNetwork(ctx, network, "")
			if err != nil {
				t.Fatal(err)
			}
			defer client.Abort()
			deadline, _ := ctx.Deadline()
			_ = client.SetReadDeadline(deadline)
			budget.finish()
			remaining := len(frames)
			seen := make(map[uint32]bool)
			for remaining > 0 {
				batch, err := client.ReadBatchRaw(nil)
				if err != nil {
					t.Fatal(err)
				}
				for _, raw := range batch {
					if want, ok := frames[raw.ID]; ok {
						if seen[raw.ID] {
							t.Fatalf("duplicate streaming frame %d", raw.ID)
						}
						seen[raw.ID] = true
						if !bytes.Equal(raw.Data, want) {
							t.Fatalf("streaming frame %d changed", raw.ID)
						}
						remaining--
					}
				}
			}
			if err := <-serverDone; err != nil {
				t.Fatal(err)
			}
			wantLegacyBytes := uint64(0)
			if legacy {
				wantLegacyBytes = uint64(len(ids) * observerStreamingPayloadBytes)
			}
			if selectiveBytes.Load() != 0 || legacyBytes.Load() != wantLegacyBytes || offers.Load() != 1 {
				t.Fatalf("selective/legacy streaming bytes and offers = %d/%d/%d, want 0/%d/1", selectiveBytes.Load(), legacyBytes.Load(), offers.Load(), wantLegacyBytes)
			}
		})
	}
}
