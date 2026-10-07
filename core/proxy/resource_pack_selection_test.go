package proxy

import (
	"context"
	"errors"
	"fmt"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// selectedOfferSource keeps acquisition evidence while substituting the server's selection.
type selectedOfferSource struct {
	*minecraft.Conn
	selection minecraft.ResourcePackStackSnapshot
}

// ResourcePackStack returns the selection independently of the acquired offer.
func (s selectedOfferSource) ResourcePackStack() (minecraft.ResourcePackStackSnapshot, bool) {
	return s.selection, true
}

func TestRequiredOfferAllowsAcquiredUnselectedPacks(t *testing.T) {
	second, err := resource.ReadBytes(admissionPackArchiveWithID(t, "11223344-5566-7788-99aa-bbccddeeff00"))
	if err != nil {
		t.Fatal(err)
	}
	packs := []*resource.Pack{testAdmissionPack(t), second}
	listener, network := newAdmissionTestListener(t, func(_ context.Context, conn *minecraft.Conn) error {
		return conn.ConfigureResourcePackOffer(packs, true)
	})
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	clientDone := make(chan admissionDialResult, 1)
	go func() {
		conn, err := (minecraft.Dialer{IdentityData: login.IdentityData{DisplayName: "Selection"}}).DialContextNetwork(ctx, network, "")
		clientDone <- admissionDialResult{conn: conn, err: err}
	}()
	accepted, err := listener.Accept()
	if err != nil {
		t.Fatal(err)
	}
	defer accepted.Close()
	if err := accepted.(*minecraft.Conn).StartGameContext(ctx, minecraft.GameData{EntityRuntimeID: 9}); err != nil {
		t.Fatal(err)
	}
	result := <-clientDone
	if result.err != nil {
		t.Fatal(result.err)
	}
	defer result.conn.Close()
	offer, _ := result.conn.ResourcePackOffer()
	stack, _ := result.conn.ResourcePackStack()
	if len(offer.Packs()) != len(packs) {
		t.Fatal("fixture did not acquire the whole offer")
	}
	for _, offerRequired := range []bool{false, true} {
		for _, stackRequired := range []bool{false, true} {
			for _, empty := range []bool{false, true} {
				t.Run(fmt.Sprintf("offer=%t/stack=%t/empty=%t", offerRequired, stackRequired, empty), func(t *testing.T) {
					selected := stack
					if empty {
						_, selected = minecraft.ProjectResourcePacks(offer, stack, func(*resource.Pack) bool { return false })
					}
					conn := negotiatedPackSelection(t, offer, selected, offerRequired, stackRequired)
					captured, err := captureSelectedResourcePackStack(conn, nil)
					if err != nil {
						t.Fatal(err)
					}
					if captured.offer.TexturePackRequired() != offerRequired || captured.snapshot.Required() != stackRequired {
						t.Fatal("required bits changed in admission")
					}
					downstream := new(offerTestDownstream)
					if err := configureResourcePackOffer(downstream, captured); err != nil {
						t.Fatal(err)
					}
					if downstream.offerRequired != offerRequired || downstream.required != stackRequired {
						t.Fatal("required bits changed in local handoff")
					}
					downstream.err = errors.New("configure failure")
					if err := configureResourcePackOffer(downstream, captured); !errors.Is(err, downstream.err) {
						t.Fatal("configuration error lost")
					}
				})
			}
		}
	}

	for _, count := range []int{0, 1, 2} {
		_, selected := minecraft.ProjectResourcePacks(offer, stack, func(pack *resource.Pack) bool {
			return count == 2 || count == 1 && pack.UUID() == packs[0].UUID()
		})
		source := selectedOfferSource{Conn: result.conn, selection: selected}
		captured, err := captureSelectedResourcePackStack(source, nil)
		if err != nil {
			t.Fatalf("selection %d: %v", count, err)
		}
		if len(captured.packs) != count || !captured.offer.TexturePackRequired() {
			t.Fatalf("selection %d: got %d packs, required=%t", count, len(captured.packs), captured.offer.TexturePackRequired())
		}
		_, err = captureSelectedResourcePackStack(source, func(*resource.Pack) bool { return true })
		var admission *PackAdmissionError
		if errors.As(err, &admission) != (count != 0) {
			t.Fatalf("excluded selection %d: %v", count, err)
		}
	}
}

// negotiatedPackSelection obtains real snapshots with independent offer and stack required bits.
func negotiatedPackSelection(t *testing.T, offer minecraft.ResourcePackOfferSnapshot, stack minecraft.ResourcePackStackSnapshot, offerRequired, stackRequired bool) *minecraft.Conn {
	t.Helper()
	listener, network := newAdmissionTestListener(t, func(_ context.Context, conn *minecraft.Conn) error {
		if err := conn.ConfigureResourcePackOfferSnapshot(offer, offerRequired); err != nil {
			return err
		}
		return conn.ConfigureResourcePackStack(stack, stackRequired)
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
	conn, err := (minecraft.Dialer{Handoff: minecraft.HandoffAtStartGame, IdentityData: login.IdentityData{DisplayName: "Matrix"}}).DialContextNetwork(ctx, network, "")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = conn.Close() })
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	return conn
}
