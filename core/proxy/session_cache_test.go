package proxy

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/sessionwire"
	"github.com/hashimthearab/rust-mcbe/core/packcache"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// A cached pack is handed off by path; the next message is a batch rather than archive bytes.
func TestSessionHandoffUsesCachedArchive(t *testing.T) {
	pack := testAdmissionPack(t)
	cache, err := packcache.New(filepath.Join(t.TempDir(), "cache"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = cache.Close() })
	key := minecraft.ResourcePackCacheKey{UUID: pack.UUID(), Version: pack.Version(), Size: uint64(pack.Size())}
	if err := cache.Store(t.Context(), key, pack); err != nil {
		t.Fatal(err)
	}
	upstream := negotiatedOfferUpstream(t, []*resource.Pack{pack}, true)
	stack, err := captureSelectedResourcePackStack(upstream, nil)
	if err != nil {
		t.Fatal(err)
	}
	fake := newFakeUpstream(nil)
	fake.useBatchReads = true
	fake.batchReads <- batchResult{packets: []packet.Packet{&packet.StartGame{EntityRuntimeID: 9}, &packet.SetTime{Time: 123}}}
	dir := t.TempDir()
	newTestSessionServer(t, dir, func(server *sessionServer) {
		server.prepared.resourcePackCache = cache
		server.prepared.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
			return &preparedConnection{upstream: fake, packStack: stack}, nil
		}
	})
	_, frames := dialTestSession(t, dir, testSessionConnect(t))
	frame := <-frames
	if len(frame) < 5 || frame[0] != sessionwire.KindHandoff {
		t.Fatalf("expected handoff, got %x", frame)
	}
	size := binary.BigEndian.Uint32(frame[1:5])
	var handoff struct {
		Packs []struct {
			Cache *struct {
				Path   string
				SHA256 [32]byte
			}
		}
	}
	if err := json.Unmarshal(frame[5:5+size], &handoff); err != nil {
		t.Fatal(err)
	}
	if len(handoff.Packs) != 1 || handoff.Packs[0].Cache == nil {
		t.Fatal("cached archive was not referenced in the handoff")
	}
	ref := handoff.Packs[0].Cache
	if !filepath.IsAbs(ref.Path) || ref.SHA256 != pack.Checksum() {
		t.Fatal("archive reference is not absolute and hash checked")
	}
	if _, err := os.Stat(ref.Path); err != nil {
		t.Fatal(err)
	}
	if next := <-frames; len(next) == 0 || next[0] != sessionwire.KindBatch {
		t.Fatal("cached pack was streamed instead of proceeding to the batch")
	}
}

// A mixed stack streams only cache misses under their original stack indices and keeps metadata.
func TestSessionCacheMissesKeepTheByteStream(t *testing.T) {
	first := testAdmissionPack(t)
	second := admissionPackWithUUID(t, "11223344-5566-7788-99aa-bbccddeeff00")
	for _, mode := range []string{"disabled", "missing", "mixed"} {
		t.Run(mode, func(t *testing.T) {
			var cache minecraft.ResourcePackCache
			if mode != "disabled" {
				disk, err := packcache.New(filepath.Join(t.TempDir(), "cache"))
				if err != nil {
					t.Fatal(err)
				}
				defer disk.Close()
				cache = disk
				if mode == "mixed" {
					key := minecraft.ResourcePackCacheKey{UUID: first.UUID(), Version: first.Version(), Size: uint64(first.Size())}
					if err := disk.Store(t.Context(), key, first); err != nil {
						t.Fatal(err)
					}
				}
			}
			packs := []*resource.Pack{first, second}
			metadata := []sessionwire.Pack{
				{UUID: first.UUID().String(), Version: first.Version(), Size: uint64(first.Size()), SubPack: "high", ContentKey: "secret"},
				{UUID: second.UUID().String(), Version: second.Version(), Size: uint64(second.Size())},
			}
			release := referenceSessionPacks(cache, metadata, packs)
			defer release()
			received := make([][]byte, len(packs))
			err := sessionwire.WritePacks(func(frame []byte) error {
				if len(frame) < 5 || frame[0] != sessionwire.KindPackData {
					t.Fatal("incorrect pack frame")
				}
				index := binary.BigEndian.Uint32(frame[1:5])
				received[index] = append(received[index], frame[5:]...)
				return nil
			}, packs)
			if err != nil {
				t.Fatal(err)
			}
			for index, pack := range []*resource.Pack{first, second} {
				if mode == "mixed" && index == 0 {
					if len(received[index]) != 0 || metadata[index].Cache == nil {
						t.Fatal("cache hit was streamed")
					}
				} else if sha256.Sum256(received[index]) != pack.Checksum() || metadata[index].Cache != nil {
					t.Fatal("cache miss did not preserve archive bytes")
				}
			}
			if metadata[0].SubPack != "high" || metadata[0].ContentKey != "secret" {
				t.Fatal("cache selection changed pack metadata")
			}
		})
	}
}
