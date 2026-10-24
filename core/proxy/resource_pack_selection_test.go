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

// Download requirements do not make an optional selection require unavailable entries.
func TestRequiredOfferRetainsAvailablePacksFromOptionalStack(t *testing.T) {
	first := testAdmissionPack(t)
	second, err := resource.ReadBytes(admissionPackArchiveWithID(t, "11223344-5566-7788-99aa-bbccddeeff00"))
	if err != nil {
		t.Fatal(err)
	}
	offer, _ := negotiatedOfferUpstream(t, []*resource.Pack{first}, true).ResourcePackOffer()
	snapshot, _ := negotiatedOfferUpstream(t, []*resource.Pack{first, second}, false).ResourcePackStack()
	stack := &selectedResourcePackStack{offer: offer, snapshot: snapshot, required: true}
	selected, archives, err := selectSessionPacks(stack, nil)
	if err != nil {
		t.Fatalf("optional stack rejected after required acquisition: %v", err)
	}
	if !stack.required || !stack.offer.TexturePackRequired() || stack.snapshot.Required() {
		t.Fatal("selection changed the server's independent required bits")
	}
	if len(selected) != 1 || len(archives) != 1 || selected[0].UUID != first.UUID().String() ||
		archives[0].UUID() != first.UUID() {
		t.Fatal("optional selection did not retain exactly its available offered archive")
	}
}

// A repeated entry in an optional stack uses its archive once after required acquisition.
func TestRequiredOfferDeduplicatesOptionalStack(t *testing.T) {
	pack := testAdmissionPack(t)
	offer, _ := negotiatedOfferUpstream(t, []*resource.Pack{pack}, true).ResourcePackOffer()
	snapshot, _ := negotiatedOfferUpstream(t, []*resource.Pack{pack, pack}, false).ResourcePackStack()
	selected, archives, err := selectSessionPacks(&selectedResourcePackStack{
		offer: offer, snapshot: snapshot, required: true,
	}, nil)
	if err != nil {
		t.Fatalf("optional repeated stack rejected after required acquisition: %v", err)
	}
	if len(selected) != 1 || len(archives) != 1 || selected[0].UUID != pack.UUID().String() ||
		archives[0].UUID() != pack.UUID() {
		t.Fatal("optional repeated selection did not retain exactly one offered archive")
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

func TestChooseSessionPacksDoesNotSubstituteAnotherVersion(t *testing.T) {
	pack := testAdmissionPack(t)
	offers := []sessionOffer{testSessionOffer(pack, "", "")}
	entries := []sessionStackEntry{{uuid: pack.UUID().String(), version: "9.0.0"}}
	for _, required := range []bool{false, true} {
		t.Run(fmt.Sprintf("required=%t", required), func(t *testing.T) {
			selected, packs, err := chooseSessionPacks(offers, entries, required, nil)
			var admission *PackAdmissionError
			if errors.As(err, &admission) != required {
				t.Fatalf("selection error = %v, required=%t", err, required)
			}
			if len(selected) != 0 || len(packs) != 0 {
				t.Fatalf("unacquired version substituted: selected=%+v packs=%d", selected, len(packs))
			}
		})
	}
}

func TestSelectedStackRequiresEverySelectedVersionOfOneUUID(t *testing.T) {
	first := testAdmissionPack(t)
	second := admissionPackWithVersion(t, first.UUID().String(), [3]uint32{2, 0, 0})
	for _, required := range []bool{false, true} {
		t.Run(fmt.Sprintf("required=%t", required), func(t *testing.T) {
			cache := minecraft.DirResourcePackCache{Dir: t.TempDir()}
			for _, pack := range []*resource.Pack{first, second} {
				key := minecraft.ResourcePackCacheKey{UUID: pack.UUID(), Version: pack.Version(), Size: uint64(pack.Size())}
				if err := cache.Store(context.Background(), key, pack); err != nil {
					t.Fatal(err)
				}
			}
			conn := negotiatedAdmissionPacksWithCache(t, []*resource.Pack{first, second}, required, cache)
			for _, excluded := range []*resource.Pack{first, second} {
				captured, err := captureSelectedResourcePackStack(conn, func(pack *resource.Pack) bool {
					return packIdentity(pack) == packIdentity(excluded)
				})
				var admission *PackAdmissionError
				if required {
					if !errors.As(err, &admission) || captured != nil {
						t.Fatalf("required version %s was dropped: stack=%+v err=%v", excluded.Version(), captured, err)
					}
					continue
				}
				if err != nil {
					t.Fatal(err)
				}
				selected, packs, err := selectSessionPacks(captured, nil)
				if err != nil || len(selected) != 1 || len(packs) != 1 || selected[0].Version == excluded.Version() {
					t.Fatalf("optional version projection: selected=%+v packs=%d err=%v", selected, len(packs), err)
				}
			}
		})
	}
}
