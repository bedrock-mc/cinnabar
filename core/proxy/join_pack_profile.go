//go:build joinprofile

package proxy

import (
	"context"
	"log/slog"
	"net"
	"sync"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/joinprofile"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// joinPackSource aggregates transfer events; durations may overlap between packs.
type joinPackSource struct {
	started, finished, failed, declared, received uint64
	duration                                      time.Duration
}

// joinPackTransfer distinguishes concurrent URL and chunk attempts for one identity.
type joinPackTransfer struct {
	source   minecraft.ResourcePackSource
	identity string
}

// joinPackProfile retains identities only as temporary correlation keys, never log fields.
type joinPackProfile struct {
	logger                                                        *slog.Logger
	mu                                                            sync.Mutex
	sources                                                       map[minecraft.ResourcePackSource]*joinPackSource
	active                                                        map[joinPackTransfer]time.Time
	failedURLs                                                    map[string]bool
	fallbacks                                                     uint64
	loads, hits, misses, invalid, loadErrors, stores, storeErrors uint64
	loadTime, storeTime                                           time.Duration
}

// withJoinPackProfile adds aggregate pack diagnostics only when developer debug logging is active.
// Existing admission, packet and cache callbacks retain their results and cancellation contexts.
func withJoinPackProfile(ctx context.Context, dialer minecraft.Dialer, logger *slog.Logger) (minecraft.Dialer, func()) {
	if !logger.Enabled(ctx, slog.LevelDebug) {
		return dialer, func() {}
	}
	profile := &joinPackProfile{logger: joinprofile.Logger(ctx, logger), sources: make(map[minecraft.ResourcePackSource]*joinPackSource), active: make(map[joinPackTransfer]time.Time), failedURLs: make(map[string]bool)}
	dialer.HTTPClient = joinprofile.HTTPClient(ctx, dialer.HTTPClient)
	previous := dialer.ResourcePackProgress
	dialer.ResourcePackProgress = func(event minecraft.ResourcePackEvent) {
		profile.record(event, time.Now())
		if previous != nil {
			previous(event)
		}
	}
	proto := dialer.Protocol
	dialer = withPacketObserver(dialer, minecraft.PacketObserver{PacketIDs: []uint32{packet.IDResourcePacksInfo}, Func: func(header packet.Header, payload []byte, _, _ net.Addr) {
		info, ok := decodeInboundPacket[*packet.ResourcePacksInfo](proto, header.PacketID, payload)
		if !ok {
			return
		}
		var urls, chunks, urlBytes, chunkBytes uint64
		for _, pack := range info.TexturePacks {
			if pack.DownloadURL != "" {
				urls++
				urlBytes = saturatingAdd(urlBytes, pack.Size)
			} else {
				chunks++
				chunkBytes = saturatingAdd(chunkBytes, pack.Size)
			}
		}
		profile.logger.Debug("join pack offer", "url_packs", urls, "chunk_packs", chunks, "url_bytes", urlBytes, "chunk_bytes", chunkBytes)
	}})
	if dialer.ResourcePackCache != nil {
		dialer.ResourcePackCache = joinPackProfileCache{ResourcePackCache: dialer.ResourcePackCache, profile: profile}
	}
	return dialer, profile.report
}

// record counts source events without logging per-chunk progress, identifiers or errors.
func (profile *joinPackProfile) record(event minecraft.ResourcePackEvent, now time.Time) {
	profile.mu.Lock()
	defer profile.mu.Unlock()
	source := profile.sources[event.Source]
	if source == nil {
		source = &joinPackSource{}
		profile.sources[event.Source] = source
	}
	identity := resourcePackIdentity(event.UUID.String(), event.Version)
	// A source prefix keeps a failed URL attempt distinct from its chunk fallback.
	key := joinPackTransfer{source: event.Source, identity: identity}
	switch event.Kind {
	case minecraft.ResourcePackStarted:
		source.started++
		source.declared = saturatingAdd(source.declared, event.Size)
		profile.active[key] = now
		if event.Source == minecraft.ResourcePackSourceChunks && profile.failedURLs[identity] {
			profile.fallbacks++
		}
	case minecraft.ResourcePackReceived:
		source.received = saturatingAdd(source.received, event.Size)
	case minecraft.ResourcePackFinished, minecraft.ResourcePackFailed:
		if event.Kind == minecraft.ResourcePackFinished {
			source.finished++
		} else {
			source.failed++
			if event.Source == minecraft.ResourcePackSourceURL {
				profile.failedURLs[identity] = true
			}
		}
		if started, ok := profile.active[key]; ok {
			source.duration += now.Sub(started)
			delete(profile.active, key)
		}
	}
}

// report writes numeric summaries; summed work durations are not sequential join latency.
func (profile *joinPackProfile) report() {
	profile.mu.Lock()
	defer profile.mu.Unlock()
	profile.logger.Debug("join pack cache", "loads", profile.loads, "hits", profile.hits, "misses", profile.misses, "invalid", profile.invalid, "load_errors", profile.loadErrors, "stores", profile.stores, "store_errors", profile.storeErrors,
		"load_sum_ms", float64(profile.loadTime)/float64(time.Millisecond), "store_sum_ms", float64(profile.storeTime)/float64(time.Millisecond))
	for source, totals := range profile.sources {
		profile.logger.Debug("join pack acquisition", "source", uint8(source), "started", totals.started, "finished", totals.finished, "failed", totals.failed,
			"declared_bytes", totals.declared, "received_bytes", totals.received, "duration_sum_ms", float64(totals.duration)/float64(time.Millisecond))
	}
	profile.logger.Debug("join pack fallback", "url_to_chunks", profile.fallbacks, "unfinished", len(profile.active))
}

// joinPackProfileCache measures cache operations without retaining their keys or archive data.
type joinPackProfileCache struct {
	minecraft.ResourcePackCache
	profile *joinPackProfile
}

// Load returns the original cache result and records only its timing and hit or miss.
func (cache joinPackProfileCache) Load(ctx context.Context, key minecraft.ResourcePackCacheKey) (*resource.Pack, error) {
	started := time.Now()
	pack, err := cache.ResourcePackCache.Load(ctx, key)
	elapsed := time.Since(started)
	cache.profile.mu.Lock()
	defer cache.profile.mu.Unlock()
	cache.profile.loads++
	cache.profile.loadTime += elapsed
	if err != nil {
		cache.profile.loadErrors++
	} else if pack == nil {
		cache.profile.misses++
	} else if !key.Matches(pack) {
		cache.profile.invalid++
	} else {
		cache.profile.hits++
	}
	return pack, err
}

// Store preserves the original cache write, including cancellation and error semantics.
func (cache joinPackProfileCache) Store(ctx context.Context, key minecraft.ResourcePackCacheKey, pack *resource.Pack) error {
	started := time.Now()
	err := cache.ResourcePackCache.Store(ctx, key, pack)
	elapsed := time.Since(started)
	cache.profile.mu.Lock()
	defer cache.profile.mu.Unlock()
	cache.profile.stores++
	cache.profile.storeTime += elapsed
	if err != nil {
		cache.profile.storeErrors++
	}
	return err
}
