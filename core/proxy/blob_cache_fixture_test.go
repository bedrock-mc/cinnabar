package proxy

import (
	"bytes"
	"testing"

	"github.com/cespare/xxhash/v2"
	bedrock "github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// TestClientBlobCachePinnedFixture keeps the Rust resolver's construction fixtures aligned with
// the exact xxHash and packet shapes used by the pinned gophertunnel dependency. Go is not part of
// the client runtime cache path.
func TestClientBlobCachePinnedFixture(t *testing.T) {
	subChunkA := []byte("subchunk-a")
	subChunkB := []byte("subchunk-b")
	biome := []byte("biome-data")
	const (
		subChunkAHash = uint64(0x283c6a98a9b9fd25)
		subChunkBHash = uint64(0x9e95225692d718f4)
		biomeHash     = uint64(0xdd633fd0a10121df)
	)
	for _, fixture := range []struct {
		payload []byte
		hash    uint64
	}{
		{subChunkA, subChunkAHash},
		{subChunkB, subChunkBHash},
		{biome, biomeHash},
	} {
		if got := xxhash.Sum64(fixture.payload); got != fixture.hash {
			t.Fatalf("pinned xxHash mismatch: got %#x, want %#x", got, fixture.hash)
		}
	}

	column := packet.LevelChunk{
		SubChunkCount: 2,
		CacheEnabled:  true,
		BlobHashes:    []uint64{subChunkAHash, subChunkBHash, subChunkAHash},
		RawPayload:    []byte("tail"),
	}
	cache := map[uint64][]byte{
		subChunkAHash: subChunkA,
		subChunkBHash: subChunkB,
	}
	var reconstructed []byte
	for _, hash := range column.BlobHashes {
		reconstructed = append(reconstructed, cache[hash]...)
	}
	reconstructed = append(reconstructed, column.RawPayload...)
	if want := []byte("subchunk-asubchunk-bsubchunk-atail"); !bytes.Equal(reconstructed, want) {
		t.Fatalf("repeated LevelChunk reconstruction = %q, want %q", reconstructed, want)
	}

	subChunks := packet.SubChunk{
		CacheEnabled: true,
		SubChunkEntries: []bedrock.SubChunkEntry{
			{
				Result: bedrock.SubChunkResultSuccess,
				// 1.26.40 carries the blob hash and the raw payload as
				// optionals rather than a bare uint64 and byte slice, so an
				// all-air entry simply omits the hash instead of sending the
				// ^0 sentinel.
				BlobHash:   bedrock.Option(subChunkAHash),
				RawPayload: bedrock.Option([]byte("block-entity-nbt")),
			},
			{
				Result: bedrock.SubChunkResultSuccessAllAir,
			},
		},
	}
	firstHash, ok := subChunks.SubChunkEntries[0].BlobHash.Value()
	if !ok {
		t.Fatal("cached SubChunk entry must carry a blob hash")
	}
	firstPayload, ok := subChunks.SubChunkEntries[0].RawPayload.Value()
	if !ok {
		t.Fatal("cached SubChunk entry must carry a block-entity payload")
	}
	first := append(append([]byte{}, cache[firstHash]...), firstPayload...)
	if want := []byte("subchunk-ablock-entity-nbt"); !bytes.Equal(first, want) {
		t.Fatalf("SubChunk block-entity tail = %q, want %q", first, want)
	}
	if subChunks.SubChunkEntries[1].Result != bedrock.SubChunkResultSuccessAllAir {
		t.Fatal("all-air entry must not request its sentinel blob hash")
	}
	if _, ok := subChunks.SubChunkEntries[1].BlobHash.Value(); ok {
		t.Fatal("all-air entry must omit its blob hash")
	}

	invalid := bedrock.CacheBlob{Hash: subChunkAHash, Payload: []byte("poison")}
	if xxhash.Sum64(invalid.Payload) == invalid.Hash {
		t.Fatal("invalid miss fixture unexpectedly matched its claimed hash")
	}
}
