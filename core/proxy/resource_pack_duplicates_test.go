package proxy

import (
	"context"
	"net/http"
	"net/http/httptest"
	"slices"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

func TestAcquisitionBudgetChargesRepeatedOffersOnce(t *testing.T) {
	for _, test := range []struct {
		name      string
		repeats   int
		offerSize uint64
	}{
		{name: "count", repeats: 40},
		{name: "bytes", repeats: 2, offerSize: 300 * 1024 * 1024},
	} {
		t.Run(test.name, func(t *testing.T) {
			info := packInfos(0, 0)
			packs := make([]*resource.Pack, len(info.TexturePacks))
			for index := range info.TexturePacks {
				packs[index] = admissionPackWithUUID(t, info.TexturePacks[index].UUID.String())
				info.TexturePacks[index].Size = uint64(packs[index].Size())
				if test.offerSize != 0 {
					info.TexturePacks[index].Size = test.offerSize
				}
			}
			first, second := info.TexturePacks[0], info.TexturePacks[1]
			info.TexturePacks = slices.Repeat([]protocol.TexturePackInfo{first}, test.repeats)
			info.TexturePacks = append(info.TexturePacks, second)
			budget, causes := observedBudget(t, info)
			want := make([]bool, len(info.TexturePacks))
			want[0], want[len(want)-1] = true, true
			if got := admitted(budget, len(want)); !slices.Equal(got, want) {
				t.Fatalf("admitted = %v, want %v", got, want)
			}
			// A corrected transfer must still fit beside the other unique pack.
			for _, pack := range packs {
				budget.event(started(minecraft.ResourcePackSourceChunks, pack.UUID(), uint64(pack.Size())))
				if budget.excludes(pack) {
					t.Fatal("repeated offers excluded a bounded unique archive")
				}
			}
			if len(*causes) != 0 {
				t.Fatalf("repeated offers cancelled acquisition: %v", *causes)
			}
		})
	}
}

func TestAcquisitionBudgetKeepsFirstRepeatedOfferSize(t *testing.T) {
	info := packInfos(300*1024*1024, 300*1024*1024)
	first, second := info.TexturePacks[0], info.TexturePacks[1]
	repeated := first
	repeated.Size = 1
	info.TexturePacks = []protocol.TexturePackInfo{first, repeated, second}
	budget, _ := observedBudget(t, info)
	if got, want := admitted(budget, len(info.TexturePacks)), []bool{true, false, true}; !slices.Equal(got, want) {
		t.Fatalf("admitted = %v, want %v", got, want)
	}
	budget.event(started(minecraft.ResourcePackSourceChunks, first.UUID, first.Size))
	if budget.excludes(admissionPackWithUUID(t, first.UUID.String())) {
		t.Fatal("a repeated estimate replaced the first offer's reserved size")
	}

	// A repeated smaller estimate cannot replace an unrepresentable first offer either.
	first.Size = ^uint64(0)
	info.TexturePacks = []protocol.TexturePackInfo{first, repeated, second}
	budget, _ = observedBudget(t, info)
	if got, want := admitted(budget, len(info.TexturePacks)), []bool{false, false, true}; !slices.Equal(got, want) {
		t.Fatalf("unrepresentable first offer admitted = %v, want %v", got, want)
	}
}

func TestRequiredRepeatedURLOfferRetainsLaterChunkPack(t *testing.T) {
	archive := testAdmissionPackArchive(t)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write(archive)
	}))
	t.Cleanup(server.Close)
	first, err := resource.ReadURL(server.URL)
	if err != nil {
		t.Fatal(err)
	}
	second := admissionPackWithUUID(t, "11223344-5566-7788-99aa-bbccddeeff00")
	selection, ok := negotiatedAdmissionPacks(t, []*resource.Pack{first, second}, true).ResourcePackStack()
	if !ok {
		t.Fatal("fixture did not negotiate a stack")
	}
	packs := append(slices.Repeat([]*resource.Pack{first}, 40), second)
	offer, ok := negotiatedAdmissionPacks(t, packs, true).ResourcePackOffer()
	if !ok {
		t.Fatal("fixture did not negotiate an offer")
	}
	listener, network := newAdmissionTestListener(t, func(_ context.Context, conn *minecraft.Conn) error {
		if err := conn.ConfigureResourcePackOfferSnapshot(offer, true); err != nil {
			return err
		}
		return conn.ConfigureResourcePackStack(selection, true)
	})
	done := make(chan error, 1)
	go func() {
		conn, err := listener.Accept()
		if err == nil {
			err = conn.(*minecraft.Conn).WritePacketImmediate(&packet.StartGame{EntityRuntimeID: 9})
		}
		done <- err
	}()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	dialCtx, cancelDial := context.WithCancelCause(ctx)
	defer cancelDial(nil)
	budget := newResourcePackAcquisitionBudget(minecraft.DefaultProtocol, cancelDial)
	conn, err := withResourcePackAcquisitionBudget(minecraft.Dialer{
		Handoff: minecraft.HandoffAtStartGame, IdentityData: login.IdentityData{DisplayName: "RepeatedOffer"},
	}, budget).DialContextNetwork(dialCtx, network, "")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = conn.Close() })
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	receivedOffer, ok := conn.ResourcePackOffer()
	if !ok || !receivedOffer.TexturePackRequired() || len(receivedOffer.TexturePacks()) != len(packs) {
		t.Fatal("client did not receive the full required offer with repeated entries")
	}
	for index, entry := range receivedOffer.TexturePacks() {
		info, pack := entry.Info(), packs[index]
		if info.UUID != pack.UUID() || info.Version != pack.Version() || info.Size != uint64(pack.Size()) || info.DownloadURL != pack.DownloadURL() {
			t.Fatalf("received offer entry %d lost its identity, size, source or order", index)
		}
	}
	stack, err := captureSelectedResourcePackStack(conn, budget.excludes)
	if err != nil {
		t.Fatalf("required offer with two unique packs: %v", err)
	}
	selected, archives, err := selectSessionPacks(stack, nil)
	if err != nil {
		t.Fatal(err)
	}
	if !stack.required || len(selected) != 2 || len(archives) != 2 {
		t.Fatalf("handoff selected=%d archives=%d required=%t", len(selected), len(archives), stack.required)
	}
	for index, pack := range []*resource.Pack{first, second} {
		if selected[index].UUID != pack.UUID().String() || archives[index].UUID() != pack.UUID() {
			t.Fatalf("handoff pack %d lost its identity or offer order", index)
		}
	}
}
