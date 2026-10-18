//go:build joinprofile

package proxy

import (
	"bytes"
	"encoding/json"
	"errors"
	"log/slog"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/joinprofile"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestJoinPackProfileSourcesFallbackPrivacyAndCacheResults(t *testing.T) {
	var logs bytes.Buffer
	logger := slog.New(slog.NewJSONHandler(&logs, &slog.HandlerOptions{Level: slog.LevelDebug}))
	ctx := joinprofile.Start(t.Context(), 91)
	backend := &scriptedResourcePackCache{}
	observed := 0
	dialer, finish := withJoinPackProfile(ctx, minecraft.Dialer{
		Protocol: minecraft.DefaultProtocol, ResourcePackCache: backend,
		ResourcePackProgress: func(minecraft.ResourcePackEvent) { observed++ },
	}, logger)
	info := packInfos(11, 17)
	info.TexturePacks[0].DownloadURL = "https://private-host.invalid/signed-secret"
	info.TexturePacks[0].ContentKey = "content-key-secret"
	dialer.PacketObserver.Func(packet.Header{PacketID: packet.IDResourcePacksInfo}, encodeLatest(t, info), nil, nil)
	key := minecraft.ResourcePackCacheKey{UUID: info.TexturePacks[0].UUID, Version: info.TexturePacks[0].Version, Size: 11}
	cached := admissionPackWithUUID(t, key.UUID.String())
	cacheKey := key
	cacheKey.Size = uint64(cached.Size())
	if pack, err := dialer.ResourcePackCache.Load(ctx, cacheKey); pack != nil || err != nil {
		t.Fatal("cache miss changed")
	}
	backend.loadPack = cached
	if pack, err := dialer.ResourcePackCache.Load(ctx, cacheKey); pack != backend.loadPack || err != nil {
		t.Fatal("cache hit changed")
	}
	invalidKey := cacheKey
	invalidKey.Size++
	if pack, err := dialer.ResourcePackCache.Load(ctx, invalidKey); pack != cached || err != nil {
		t.Fatal("mismatched cache entry changed")
	}
	secretErr := errors.New("cache-path-and-error-secret")
	backend.loadErr, backend.storeErr = secretErr, secretErr
	if _, err := dialer.ResourcePackCache.Load(ctx, key); err != secretErr {
		t.Fatal("cache load error changed")
	}
	if err := dialer.ResourcePackCache.Store(ctx, key, nil); err != secretErr {
		t.Fatal("cache store error changed")
	}
	event := minecraft.ResourcePackEvent{UUID: key.UUID, Version: key.Version, Source: minecraft.ResourcePackSourceURL, Kind: minecraft.ResourcePackStarted, Size: 11}
	dialer.ResourcePackProgress(event)
	event.Kind, event.Size = minecraft.ResourcePackReceived, 3
	dialer.ResourcePackProgress(event)
	event.Kind, event.Err = minecraft.ResourcePackFailed, secretErr
	dialer.ResourcePackProgress(event)
	event.Source, event.Kind, event.Size, event.Err = minecraft.ResourcePackSourceChunks, minecraft.ResourcePackStarted, 11, nil
	dialer.ResourcePackProgress(event)
	event.Kind, event.Size = minecraft.ResourcePackReceived, 1
	for range 11 {
		dialer.ResourcePackProgress(event)
	}
	event.Kind = minecraft.ResourcePackFinished
	dialer.ResourcePackProgress(event)
	finish()
	if observed != 16 {
		t.Fatalf("forwarded events = %d, want16", observed)
	}
	records := make(map[string][]map[string]any)
	for _, line := range bytes.Split(bytes.TrimSpace(logs.Bytes()), []byte{'\n'}) {
		var entry map[string]any
		if err := json.Unmarshal(line, &entry); err != nil {
			t.Fatal(err)
		}
		if entry["join_profile_id"] != float64(91) {
			t.Fatal("pack timing lost its join correlation")
		}
		records[entry["msg"].(string)] = append(records[entry["msg"].(string)], entry)
	}
	if len(records["join pack offer"]) != 1 || len(records["join pack cache"]) != 1 || len(records["join pack acquisition"]) != 2 || len(records["join pack fallback"]) != 1 {
		t.Fatal("profile omitted a phase or logged per-chunk progress")
	}
	offer, cache := records["join pack offer"][0], records["join pack cache"][0]
	if offer["url_packs"] != float64(1) || offer["chunk_packs"] != float64(1) || offer["url_bytes"] != float64(11) || offer["chunk_bytes"] != float64(17) {
		t.Fatal("source offer counters changed")
	}
	if cache["loads"] != float64(4) || cache["hits"] != float64(1) || cache["misses"] != float64(1) || cache["invalid"] != float64(1) || cache["load_errors"] != float64(1) || cache["store_errors"] != float64(1) || cache["stores"] != float64(1) {
		t.Fatal("cache counters changed")
	}
	if _, ok := cache["load_sum_ms"]; !ok {
		t.Fatal("cache read timing missing")
	}
	if records["join pack fallback"][0]["url_to_chunks"] != float64(1) || records["join pack fallback"][0]["unfinished"] != float64(0) {
		t.Fatal("fallback or completion counts changed")
	}
	for _, secret := range []string{key.UUID.String(), key.Version, "private-host.invalid", "signed-secret", "content-key-secret", secretErr.Error()} {
		if strings.Contains(logs.String(), secret) {
			t.Fatalf("pack diagnostics leaked %q", secret)
		}
	}
}

func TestJoinPackProfileMeasuresOverlappingSourceWork(t *testing.T) {
	profile := &joinPackProfile{sources: make(map[minecraft.ResourcePackSource]*joinPackSource), active: make(map[joinPackTransfer]time.Time), failedURLs: make(map[string]bool)}
	info := packInfos(1, 1)
	first := minecraft.ResourcePackEvent{UUID: info.TexturePacks[0].UUID, Source: minecraft.ResourcePackSourceURL, Kind: minecraft.ResourcePackStarted}
	second := first
	second.UUID = info.TexturePacks[1].UUID
	now := time.Unix(0, 0)
	profile.record(first, now)
	profile.record(second, now.Add(time.Second))
	first.Kind, second.Kind = minecraft.ResourcePackFinished, minecraft.ResourcePackFinished
	profile.record(second, now.Add(2*time.Second))
	profile.record(first, now.Add(3*time.Second))
	source := profile.sources[minecraft.ResourcePackSourceURL]
	if source.started != 2 || source.finished != 2 || source.duration != 4*time.Second || len(profile.active) != 0 {
		t.Fatal("overlapping transfers were measured as serial latency")
	}
}
