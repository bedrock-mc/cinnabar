package proxy

import (
	"context"
	"errors"
	"log/slog"
	"math"
	"sync"
	"sync/atomic"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/redact"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

type ResourcePackOffer string

const (
	ResourcePackOfferNone     ResourcePackOffer = bridgecontract.PackOfferNone
	ResourcePackOfferOptional ResourcePackOffer = bridgecontract.PackOfferOptional
	ResourcePackOfferRequired ResourcePackOffer = bridgecontract.PackOfferRequired
)

type ResourcePackAcquisition string

const (
	ResourcePackAcquisitionNone      ResourcePackAcquisition = bridgecontract.PackAcquisitionNone
	ResourcePackAcquisitionComplete  ResourcePackAcquisition = bridgecontract.PackAcquisitionComplete
	ResourcePackAcquisitionIgnored   ResourcePackAcquisition = bridgecontract.PackAcquisitionIgnored
	ResourcePackAcquisitionPartial   ResourcePackAcquisition = bridgecontract.PackAcquisitionPartial
	ResourcePackAcquisitionFailed    ResourcePackAcquisition = bridgecontract.PackAcquisitionFailed
	ResourcePackAcquisitionCancelled ResourcePackAcquisition = bridgecontract.PackAcquisitionCancelled
)

type ResourcePackDownstreamOutcome string

const (
	ResourcePackDownstreamNone              ResourcePackDownstreamOutcome = bridgecontract.PackDownstreamNone
	ResourcePackDownstreamOfferedOptional   ResourcePackDownstreamOutcome = bridgecontract.PackDownstreamOfferedOptional
	ResourcePackDownstreamHandedOffOptional ResourcePackDownstreamOutcome = bridgecontract.PackDownstreamHandedOffOptional
	ResourcePackDownstreamOfferedRequired   ResourcePackDownstreamOutcome = bridgecontract.PackDownstreamOfferedRequired
	ResourcePackDownstreamHandedOffRequired ResourcePackDownstreamOutcome = bridgecontract.PackDownstreamHandedOffRequired
	ResourcePackDownstreamRejectedRequired  ResourcePackDownstreamOutcome = bridgecontract.PackDownstreamRejectedRequired
	ResourcePackDownstreamStrippedIgnored   ResourcePackDownstreamOutcome = bridgecontract.PackDownstreamStrippedIgnored
)

const (
	ResourcePackApplicationUnavailable = bridgecontract.PackApplicationUnavailable
	// ResourcePackApplicationApplied is reported once the client confirms it applied the handed-off packs.
	ResourcePackApplicationApplied = bridgecontract.PackApplicationApplied
)

// ResourcePackAdmissionSnapshot deliberately contains no pack identity,
// version, source, content key, digest, or filesystem location.
type ResourcePackAdmissionSnapshot struct {
	AttemptID         uint64                        `json:"attempt_id"`
	Offer             ResourcePackOffer             `json:"offer"`
	PackCount         uint32                        `json:"pack_count"`
	TotalBytes        uint64                        `json:"total_bytes"`
	Acquisition       ResourcePackAcquisition       `json:"acquisition"`
	CacheLoads        uint64                        `json:"cache_loads"`
	CacheHits         uint64                        `json:"cache_hits"`
	CacheMisses       uint64                        `json:"cache_misses"`
	CacheStores       uint64                        `json:"cache_stores"`
	CacheErrors       uint64                        `json:"cache_errors"`
	DownstreamOutcome ResourcePackDownstreamOutcome `json:"downstream_outcome"`
	Application       string                        `json:"application"`
}

type resourcePackAdmissionTelemetry struct {
	attemptID uint64
	callback  func(ResourcePackAdmissionSnapshot)
	update    func(ResourcePackAdmissionSnapshot)
	report    sync.Once

	mu          sync.Mutex
	offer       ResourcePackOffer
	packCount   uint32
	totalBytes  uint64
	acquisition ResourcePackAcquisition
	downstream  ResourcePackDownstreamOutcome
	negotiation bool
	loads       atomic.Uint64
	hits        atomic.Uint64
	misses      atomic.Uint64
	stores      atomic.Uint64
	errors      atomic.Uint64
}

func (telemetry *resourcePackAdmissionTelemetry) setUpdateCallback(callback func(ResourcePackAdmissionSnapshot)) {
	telemetry.update = callback
	telemetry.publishUpdate()
}

func (telemetry *resourcePackAdmissionTelemetry) publishUpdate() {
	if telemetry == nil || telemetry.update == nil {
		return
	}
	defer func() { _ = recover() }()
	telemetry.update(telemetry.snapshot())
}

func newResourcePackAdmissionTelemetry(id uint64, callback func(ResourcePackAdmissionSnapshot)) *resourcePackAdmissionTelemetry {
	return &resourcePackAdmissionTelemetry{
		attemptID: id, callback: callback, offer: ResourcePackOfferNone,
		acquisition: ResourcePackAcquisitionNone, downstream: ResourcePackDownstreamNone,
	}
}

func (telemetry *resourcePackAdmissionTelemetry) observeOffer(upstream upstreamSession) {
	packs := upstream.ResourcePacks()
	acquired := 0
	for _, pack := range packs {
		if pack != nil {
			acquired++
		}
	}
	count := acquired
	required := upstream.TexturePacksRequired()
	var total uint64
	if source, ok := upstream.(resourcePackStackSource); ok {
		if offer, available := source.ResourcePackOffer(); available {
			entries := offer.TexturePacks()
			count = len(entries)
			required = required || offer.TexturePackRequired()
			for _, entry := range entries {
				total = saturatingAdd(total, entry.Info().Size)
			}
		}
	}
	if total == 0 {
		for _, pack := range packs {
			if pack != nil {
				total = saturatingAdd(total, uint64(pack.Size()))
			}
		}
	}
	offer := ResourcePackOfferNone
	acquisition := ResourcePackAcquisitionNone
	switch {
	case count == 0:
	case acquired >= count:
		acquisition = ResourcePackAcquisitionComplete
	case acquired == 0:
		acquisition = ResourcePackAcquisitionIgnored
	default:
		acquisition = ResourcePackAcquisitionPartial
	}
	if count != 0 {
		offer = ResourcePackOfferOptional
		if required {
			offer = ResourcePackOfferRequired
		}
	}
	telemetry.mu.Lock()
	telemetry.offer, telemetry.packCount, telemetry.totalBytes = offer, uint32(min(count, math.MaxUint32)), total
	telemetry.acquisition = acquisition
	telemetry.mu.Unlock()
}

func saturatingAdd(total, size uint64) uint64 {
	if size > math.MaxUint64-total {
		return math.MaxUint64
	}
	return total + size
}

func (telemetry *resourcePackAdmissionTelemetry) observeNegotiation() {
	telemetry.mu.Lock()
	telemetry.negotiation = true
	telemetry.mu.Unlock()
}

func (telemetry *resourcePackAdmissionTelemetry) observeFailure(ctx context.Context) {
	telemetry.mu.Lock()
	if !telemetry.negotiation {
		telemetry.mu.Unlock()
		return
	}
	if ctx.Err() != nil {
		telemetry.acquisition = ResourcePackAcquisitionCancelled
	} else {
		telemetry.acquisition = ResourcePackAcquisitionFailed
	}
	telemetry.mu.Unlock()
}

func (telemetry *resourcePackAdmissionTelemetry) observeRejectedRequired() {
	telemetry.mu.Lock()
	telemetry.downstream = ResourcePackDownstreamRejectedRequired
	telemetry.mu.Unlock()
}

func (telemetry *resourcePackAdmissionTelemetry) observePolicyOutcome(stack *selectedResourcePackStack, configured bool) {
	if telemetry == nil {
		return
	}
	telemetry.mu.Lock()
	switch {
	case stack == nil || !configured || telemetry.offer == ResourcePackOfferNone:
		telemetry.downstream = ResourcePackDownstreamNone
	case len(stack.packs) == 0:
		telemetry.downstream = ResourcePackDownstreamStrippedIgnored
	case stack.required:
		telemetry.downstream = ResourcePackDownstreamOfferedRequired
	default:
		telemetry.downstream = ResourcePackDownstreamOfferedOptional
	}
	telemetry.mu.Unlock()
	telemetry.publishUpdate()
}

func (telemetry *resourcePackAdmissionTelemetry) observeLocalHandoff(stack *selectedResourcePackStack) {
	if telemetry == nil || stack == nil || len(stack.packs) == 0 {
		return
	}
	telemetry.mu.Lock()
	switch telemetry.downstream {
	case ResourcePackDownstreamOfferedOptional:
		telemetry.downstream = ResourcePackDownstreamHandedOffOptional
	case ResourcePackDownstreamOfferedRequired:
		telemetry.downstream = ResourcePackDownstreamHandedOffRequired
	}
	telemetry.mu.Unlock()
	telemetry.publishUpdate()
}

func (telemetry *resourcePackAdmissionTelemetry) snapshot() ResourcePackAdmissionSnapshot {
	telemetry.mu.Lock()
	snapshot := ResourcePackAdmissionSnapshot{
		AttemptID: telemetry.attemptID, Offer: telemetry.offer, PackCount: telemetry.packCount,
		TotalBytes: telemetry.totalBytes, Acquisition: telemetry.acquisition,
		DownstreamOutcome: telemetry.downstream, Application: ResourcePackApplicationUnavailable,
	}
	telemetry.mu.Unlock()
	snapshot.CacheLoads = telemetry.loads.Load()
	snapshot.CacheHits = telemetry.hits.Load()
	snapshot.CacheMisses = telemetry.misses.Load()
	snapshot.CacheStores = telemetry.stores.Load()
	snapshot.CacheErrors = telemetry.errors.Load()
	return snapshot
}

func (telemetry *resourcePackAdmissionTelemetry) reportFinal() {
	if telemetry == nil || telemetry.callback == nil && telemetry.update == nil {
		return
	}
	telemetry.report.Do(func() {
		telemetry.publishUpdate()
		if telemetry.callback == nil {
			return
		}
		defer func() { _ = recover() }()
		telemetry.callback(telemetry.snapshot())
	})
}

type observedResourcePackCache struct {
	cache     minecraft.ResourcePackCache
	telemetry *resourcePackAdmissionTelemetry
}

var errResourcePackCacheUnavailable = errors.New("resource pack cache unavailable")

func (cache observedResourcePackCache) Load(ctx context.Context, key minecraft.ResourcePackCacheKey) (*resource.Pack, error) {
	atomicSaturatingIncrement(&cache.telemetry.loads)
	pack, err := cache.cache.Load(ctx, key)
	if err != nil {
		atomicSaturatingIncrement(&cache.telemetry.errors)
		return nil, errResourcePackCacheUnavailable
	}
	if pack == nil {
		atomicSaturatingIncrement(&cache.telemetry.misses)
	} else {
		atomicSaturatingIncrement(&cache.telemetry.hits)
	}
	return pack, nil
}

func (cache observedResourcePackCache) Store(ctx context.Context, key minecraft.ResourcePackCacheKey, pack *resource.Pack) error {
	atomicSaturatingIncrement(&cache.telemetry.stores)
	if err := cache.cache.Store(ctx, key, pack); err != nil {
		atomicSaturatingIncrement(&cache.telemetry.errors)
		return errResourcePackCacheUnavailable
	}
	return nil
}

// secretSafeResourcePackLogger routes third-party dialer and discovery logs through logger's
// handler, so they share its format, level and attributes while dropping pack identities and
// passing all text through the shared redaction rules.
func secretSafeResourcePackLogger(logger *slog.Logger) *slog.Logger {
	return slog.New(secretSafeResourcePackHandler{next: logger.Handler()}).With("component", "upstream-dialer")
}

type secretSafeResourcePackHandler struct{ next slog.Handler }

func (handler secretSafeResourcePackHandler) Enabled(ctx context.Context, level slog.Level) bool {
	return handler.next.Enabled(ctx, level)
}

func (handler secretSafeResourcePackHandler) Handle(ctx context.Context, record slog.Record) error {
	clean := slog.NewRecord(record.Time, record.Level, redact.Text(record.Message), record.PC)
	record.Attrs(func(attr slog.Attr) bool {
		if attr, ok := redactResourcePackAttr(attr); ok {
			clean.AddAttrs(attr)
		}
		return true
	})
	return handler.next.Handle(ctx, clean)
}

func (handler secretSafeResourcePackHandler) WithAttrs(attrs []slog.Attr) slog.Handler {
	clean := make([]slog.Attr, 0, len(attrs))
	for _, attr := range attrs {
		if attr, ok := redactResourcePackAttr(attr); ok {
			clean = append(clean, attr)
		}
	}
	return secretSafeResourcePackHandler{next: handler.next.WithAttrs(clean)}
}

// redactResourcePackAttr drops pack identity attributes at any group depth and redacts the text
// of the rest, including errors, whose URLs may be signed CDN links.
func redactResourcePackAttr(attr slog.Attr) (slog.Attr, bool) {
	if redact.PackIdentityKey(attr.Key) {
		return slog.Attr{}, false
	}
	value := attr.Value.Resolve()
	switch value.Kind() {
	case slog.KindGroup:
		var members []any
		for _, member := range value.Group() {
			if member, ok := redactResourcePackAttr(member); ok {
				members = append(members, member)
			}
		}
		return slog.Group(attr.Key, members...), true
	case slog.KindString, slog.KindAny:
		return slog.String(attr.Key, redact.Text(value.String())), true
	}
	return slog.Attr{Key: attr.Key, Value: value}, true
}

func (handler secretSafeResourcePackHandler) WithGroup(name string) slog.Handler {
	return secretSafeResourcePackHandler{next: handler.next.WithGroup(name)}
}

// atomicSaturatingIncrement keeps diagnostic counters bounded without wrapping.
func atomicSaturatingIncrement(counter *atomic.Uint64) {
	for {
		value := counter.Load()
		if value == math.MaxUint64 || counter.CompareAndSwap(value, value+1) {
			return
		}
	}
}
