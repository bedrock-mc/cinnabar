package core_test

import (
	"context"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

type resourcePackCacheStub struct{}

func (*resourcePackCacheStub) Load(context.Context, minecraft.ResourcePackCacheKey) (*resource.Pack, error) {
	return nil, nil
}

func (*resourcePackCacheStub) Store(context.Context, minecraft.ResourcePackCacheKey, *resource.Pack) error {
	return nil
}

func TestGophertunnelResourcePackTransportAPI(t *testing.T) {
	var _ minecraft.ResourcePackCache = (*resourcePackCacheStub)(nil)
	var _ minecraft.ResourcePackCache = minecraft.DirResourcePackCache{}
	var _ func(*minecraft.Conn, []*resource.Pack, bool) error = (*minecraft.Conn).ConfigureResourcePackOffer
	var _ func(*minecraft.Conn) []*resource.Pack = (*minecraft.Conn).ResourcePacks
	var _ func(*minecraft.Conn) bool = (*minecraft.Conn).TexturePacksRequired
	var _ func(*minecraft.Conn) (minecraft.ResourcePackStackSnapshot, bool) = (*minecraft.Conn).ResourcePackStack
	var _ func(minecraft.ResourcePackStackSnapshot) []*resource.Pack = minecraft.ResourcePackStackSnapshot.Packs
	var _ func(minecraft.ResourcePackStackSnapshot) bool = minecraft.ResourcePackStackSnapshot.Required
	var _ func(*resource.Pack) *resource.Pack = (*resource.Pack).Clone

	download := minecraft.ResourcePackDownloadConfig{
		MaxInFlightChunks:  7,
		MaxPacks:           8,
		MaxPackBytes:       9,
		MaxTotalBytes:      10,
		MaxChunkBytes:      11,
		MaxChunks:          12,
		ResponseTimeout:    time.Second,
		AllowHTTPDownloads: true,
	}
	cache := &resourcePackCacheStub{}
	dialer := minecraft.Dialer{ResourcePackDownload: download, ResourcePackCache: cache}
	listener := minecraft.ListenConfig{
		TexturePacksRequired: true,
		ResourcePackDelivery: minecraft.ResourcePackDeliveryConfig{ChunkSize: 13, ChunkSendDelay: time.Millisecond},
		PrepareResourcePackOffer: func(_ context.Context, conn *minecraft.Conn) error {
			return conn.ConfigureResourcePackOffer(nil, true)
		},
	}
	key := minecraft.ResourcePackCacheKey{
		UUID: uuid.Nil, Version: "1.0.0", Size: 14,
		SHA256: [32]byte{0: 1, 31: 2},
	}

	if dialer.ResourcePackDownload != download || dialer.ResourcePackCache != cache {
		t.Fatal("Dialer did not retain resource-pack transport configuration")
	}
	if !listener.TexturePacksRequired || listener.PrepareResourcePackOffer == nil || key.Version != "1.0.0" ||
		key.SHA256[0] != 1 || key.SHA256[31] != 2 {
		t.Fatal("resource-pack offer and cache-key API drifted")
	}
	if minecraft.DefaultResourcePackMaxPacks <= 0 || minecraft.DefaultResourcePackMaxPackBytes == 0 ||
		minecraft.DefaultResourcePackMaxTotalBytes == 0 || minecraft.DefaultResourcePackMaxChunkBytes == 0 ||
		minecraft.DefaultResourcePackMaxChunks == 0 || minecraft.DefaultResourcePackMaxInFlightChunks <= 0 ||
		minecraft.DefaultResourcePackResponseTimeout <= 0 || minecraft.DefaultResourcePackChunkSize == 0 ||
		minecraft.DefaultResourcePackChunkSendDelay <= 0 {
		t.Fatal("resource-pack transport defaults must remain bounded and non-zero")
	}
}
