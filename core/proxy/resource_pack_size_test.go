package proxy

import (
	"context"
	"log/slog"
	"net"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

func TestRequiredChunkPackWithUnderstatedWireOfferReachesHandoff(t *testing.T) {
	pack := testAdmissionPack(t)
	network := streamnet.New(filepath.Join(t.TempDir(), "socket"))
	var rewritten atomic.Bool
	listener, err := (minecraft.ListenConfig{
		AuthenticationDisabled: true,
		ErrorLog:               slog.New(slog.DiscardHandler),
		PrepareResourcePackOffer: func(_ context.Context, conn *minecraft.Conn) error {
			return conn.ConfigureResourcePackOffer([]*resource.Pack{pack}, true)
		},
		PacketFunc: func(header packet.Header, payload []byte, _, _ net.Addr) {
			if header.PacketID != packet.IDResourcePacksInfo {
				return
			}
			info, ok := decodeInboundPacket[*packet.ResourcePacksInfo](minecraft.DefaultProtocol, header.PacketID, payload)
			if !ok || len(info.TexturePacks) != 1 {
				t.Error("fixture did not send its resource-pack offer")
				return
			}
			info.TexturePacks[0].Size = 1
			encoded := encodeLatest(t, info)
			if len(encoded) != len(payload) {
				t.Error("fixture changed the packet payload length")
				return
			}
			copy(payload, encoded)
			rewritten.Store(true)
		},
	}).ListenNetwork(network, "")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	serverDone := make(chan error, 1)
	go func() {
		conn, err := listener.Accept()
		if err == nil {
			err = conn.(*minecraft.Conn).WritePacketImmediate(&packet.StartGame{EntityRuntimeID: 1, EntityUniqueID: 1})
		}
		serverDone <- err
	}()
	connections := newPreparedConnections("unused.invalid:19132", nil, slog.New(slog.DiscardHandler))
	connections.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{network: network}, nil
	}
	connections.dialTarget = func(ctx context.Context, target *resolvedUpstreamTarget, dialer minecraft.Dialer) (upstreamSession, error) {
		return dialer.DialContextNetwork(ctx, target.network, "")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	prepared, err := connections.connect(ctx, dialerTestDownstream{
		protocol: minecraft.DefaultProtocol, identity: login.IdentityData{DisplayName: "PackSize"},
	})
	if err != nil {
		t.Fatalf("required pack with corrected chunk size: %v", err)
	}
	defer prepared.close()
	if err := <-serverDone; err != nil {
		t.Fatal(err)
	}
	selected, archives, err := selectSessionPacks(prepared.packStack, nil)
	if err != nil {
		t.Fatal(err)
	}
	if !rewritten.Load() || !prepared.packStack.required || len(selected) != 1 || len(archives) != 1 {
		t.Fatal("required wire offer did not retain its acquired pack")
	}
	if selected[0].Size != uint64(pack.Size()) || archives[0].UUID() != pack.UUID() {
		t.Fatal("handoff did not use the corrected archive size and identity")
	}
}

func TestAcquisitionBudgetRetainsRequiredPackWithUnderstatedOffer(t *testing.T) {
	pack := testAdmissionPack(t)
	upstream := negotiatedAdmissionPacks(t, []*resource.Pack{pack}, true)
	budget, causes := observedBudget(t, &packet.ResourcePacksInfo{
		TexturePackRequired: true,
		TexturePacks: []protocol.TexturePackInfo{{
			UUID: pack.UUID(), Version: pack.Version(), Size: uint64(pack.Size() - 1),
		}},
	})
	budget.event(started(minecraft.ResourcePackSourceChunks, pack.UUID(), uint64(pack.Size())))
	budget.event(minecraft.ResourcePackEvent{
		Kind: minecraft.ResourcePackFinished, Source: minecraft.ResourcePackSourceChunks,
		UUID: pack.UUID(), Version: pack.Version(), Size: uint64(pack.Size()),
	})
	stack, err := captureSelectedResourcePackStack(upstream, budget.excludes)
	if err != nil {
		t.Fatalf("required acquired pack rejected after transfer size correction: %v", err)
	}
	selected, archives, err := selectSessionPacks(stack, nil)
	if err != nil {
		t.Fatal(err)
	}
	if !stack.required || len(selected) != 1 || len(archives) != 1 || len(*causes) != 0 {
		t.Fatalf("corrected required handoff: selected=%d archives=%d required=%t causes=%v",
			len(selected), len(archives), stack.required, *causes)
	}
	if selected[0].Size != uint64(pack.Size()) || archives[0].UUID() != pack.UUID() {
		t.Fatal("handoff did not retain the acquired archive and its actual size")
	}
}

func TestAcquisitionBudgetCorrectedSizesKeepByteBounds(t *testing.T) {
	tests := []struct {
		name     string
		sizes    []uint64
		excluded []bool
	}{
		{name: "archive boundary", sizes: []uint64{maxResourcePackArchiveBytes}, excluded: []bool{false}},
		{name: "oversized archive", sizes: []uint64{maxResourcePackArchiveBytes + 1}, excluded: []bool{true}},
		{
			name:     "selection boundary",
			sizes:    []uint64{maxResourcePackArchiveBytes, maxSelectedResourcePackTotalBytes - maxResourcePackArchiveBytes},
			excluded: []bool{false, false},
		},
		{
			name:     "selection overflow",
			sizes:    []uint64{maxResourcePackArchiveBytes, maxSelectedResourcePackTotalBytes - maxResourcePackArchiveBytes - 1, 2},
			excluded: []bool{false, false, true},
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			info := packInfos(make([]uint64, len(test.sizes))...)
			budget, causes := observedBudget(t, info)
			for index, size := range test.sizes {
				id := info.TexturePacks[index].UUID
				budget.event(started(minecraft.ResourcePackSourceChunks, id, size))
				pack := admissionPackWithUUID(t, id.String())
				if got := budget.excludes(pack); got != test.excluded[index] {
					t.Fatalf("transfer %d excluded=%t, want %t", index, got, test.excluded[index])
				}
			}
			if len(*causes) != 0 {
				t.Fatalf("bounded transfers cancelled the join: %v", *causes)
			}
		})
	}
}

func TestAcquisitionBudgetReplacementDoesNotReserveSizeTwice(t *testing.T) {
	const firstSize = maxResourcePackArchiveBytes
	const secondSize = maxResourcePackArchiveBytes / 4
	const thirdSize = maxSelectedResourcePackTotalBytes - firstSize - secondSize
	info := packInfos(0, 0, 0)
	budget, causes := observedBudget(t, info)
	first, second, third := info.TexturePacks[0].UUID, info.TexturePacks[1].UUID, info.TexturePacks[2].UUID
	budget.event(started(minecraft.ResourcePackSourceURL, first, firstSize))
	budget.event(started(minecraft.ResourcePackSourceChunks, second, secondSize))
	budget.event(minecraft.ResourcePackEvent{
		Kind: minecraft.ResourcePackFailed, Source: minecraft.ResourcePackSourceURL,
		UUID: first, Version: "1.0.0",
	})
	budget.event(started(minecraft.ResourcePackSourceChunks, first, firstSize))
	budget.event(started(minecraft.ResourcePackSourceChunks, third, thirdSize))
	for _, id := range []uuid.UUID{first, second, third} {
		if budget.excludes(admissionPackWithUUID(t, id.String())) {
			t.Fatal("fallback transfer charged its reservation twice")
		}
	}
	if len(*causes) != 0 {
		t.Fatalf("fallback transfers cancelled the join: %v", *causes)
	}
}
