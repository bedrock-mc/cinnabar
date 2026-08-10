package proxy

import (
	"archive/zip"
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"strings"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

type scriptedResourcePackCache struct {
	loadPack *resource.Pack
	loadErr  error
	storeErr error
}

func (cache *scriptedResourcePackCache) Load(context.Context, minecraft.ResourcePackCacheKey) (*resource.Pack, error) {
	return cache.loadPack, cache.loadErr
}

func (cache *scriptedResourcePackCache) Store(context.Context, minecraft.ResourcePackCacheKey, *resource.Pack) error {
	return cache.storeErr
}

func TestObservedResourcePackCacheCountsHitMissAndSanitizesErrors(t *testing.T) {
	telemetry := newResourcePackAdmissionTelemetry(7, nil)
	backend := &scriptedResourcePackCache{}
	cache := observedResourcePackCache{cache: backend, telemetry: telemetry}

	if pack, err := cache.Load(context.Background(), minecraft.ResourcePackCacheKey{}); err != nil || pack != nil {
		t.Fatalf("miss = (%v, %v), want (nil, nil)", pack, err)
	}
	backend.loadPack = new(resource.Pack)
	if pack, err := cache.Load(context.Background(), minecraft.ResourcePackCacheKey{}); err != nil || pack == nil {
		t.Fatalf("hit = (%v, %v), want non-nil, nil", pack, err)
	}
	backend.loadPack = nil
	backend.loadErr = errors.New("private path and key sentinel")
	if _, err := cache.Load(context.Background(), minecraft.ResourcePackCacheKey{}); !errors.Is(err, errResourcePackCacheUnavailable) || strings.Contains(err.Error(), "sentinel") {
		t.Fatalf("load error = %v, want sanitized cache failure", err)
	}
	backend.storeErr = errors.New("private path sentinel")
	if err := cache.Store(context.Background(), minecraft.ResourcePackCacheKey{}, nil); !errors.Is(err, errResourcePackCacheUnavailable) || strings.Contains(err.Error(), "sentinel") {
		t.Fatalf("store error = %v, want sanitized cache failure", err)
	}

	snapshot := telemetry.snapshot()
	if snapshot.CacheLoads != 3 || snapshot.CacheHits != 1 || snapshot.CacheMisses != 1 || snapshot.CacheStores != 1 || snapshot.CacheErrors != 2 {
		t.Fatalf("cache snapshot = %#v", snapshot)
	}
}

func TestUpstreamDialerReceivesOnlyResourcePackCacheInterface(t *testing.T) {
	backend := &scriptedResourcePackCache{}
	dialer := newUpstreamDialerForAdmission(
		dialerTestDownstream{protocol: minecraft.DefaultProtocol}, nil, nil, backend, nil,
	)
	if dialer.ResourcePackCache != backend {
		t.Fatal("dialer did not preserve the process-owned cache interface")
	}
}

func TestResourcePackAdmissionSnapshotsCoverOfferPolicyAndOneShotReporting(t *testing.T) {
	pack := testAdmissionPack(t)
	for _, test := range []struct {
		name             string
		packs            []*resource.Pack
		required         bool
		selected         []*resource.Pack
		selectedRequired bool
		wantOffer        ResourcePackOffer
		wantResult       ResourcePackDownstreamOutcome
	}{
		{name: "none", wantOffer: ResourcePackOfferNone, wantResult: ResourcePackDownstreamNone},
		{name: "optional", packs: []*resource.Pack{pack}, selected: []*resource.Pack{pack}, wantOffer: ResourcePackOfferOptional, wantResult: ResourcePackDownstreamStrippedOptional},
		{name: "required", packs: []*resource.Pack{pack}, required: true, selected: []*resource.Pack{pack}, selectedRequired: true, wantOffer: ResourcePackOfferRequired, wantResult: ResourcePackDownstreamRejectedRequired},
		{name: "required offer with empty selected stack", packs: []*resource.Pack{pack}, required: true, selectedRequired: true, wantOffer: ResourcePackOfferRequired, wantResult: ResourcePackDownstreamNone},
	} {
		t.Run(test.name, func(t *testing.T) {
			var snapshots []ResourcePackAdmissionSnapshot
			telemetry := newResourcePackAdmissionTelemetry(42, func(snapshot ResourcePackAdmissionSnapshot) {
				snapshots = append(snapshots, snapshot)
			})
			upstream := newFakeUpstream(nil)
			upstream.packs, upstream.required = test.packs, test.required
			telemetry.observeOffer(upstream)
			telemetry.observePolicyOutcome(&selectedResourcePackStack{packs: test.selected, required: test.selectedRequired}, true)
			telemetry.reportFinal()
			telemetry.reportFinal()
			if len(snapshots) != 1 {
				t.Fatalf("callback count = %d, want 1", len(snapshots))
			}
			got := snapshots[0]
			if got.AttemptID != 42 || got.Offer != test.wantOffer || got.DownstreamOutcome != test.wantResult || got.Application != ResourcePackApplicationUnavailable {
				t.Fatalf("snapshot = %#v", got)
			}
			if len(test.packs) != 0 && (got.PackCount != 1 || got.TotalBytes != uint64(pack.Size())) {
				t.Fatalf("offer bounds = (%d, %d), want (1, %d)", got.PackCount, got.TotalBytes, pack.Size())
			}
		})
	}
}

func TestResourcePackAdmissionFailureStartsOnlyAfterNegotiation(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	beforePack := newResourcePackAdmissionTelemetry(1, nil)
	beforePack.observeFailure(ctx)
	if got := beforePack.snapshot().Acquisition; got != ResourcePackAcquisitionNone {
		t.Fatalf("pre-pack cancellation acquisition = %q", got)
	}
	cancelled := newResourcePackAdmissionTelemetry(2, nil)
	cancelled.observeNegotiation()
	cancelled.observeFailure(ctx)
	if got := cancelled.snapshot().Acquisition; got != ResourcePackAcquisitionCancelled {
		t.Fatalf("cancelled acquisition = %q", got)
	}
	failed := newResourcePackAdmissionTelemetry(3, nil)
	failed.observeNegotiation()
	failed.observeFailure(context.Background())
	if got := failed.snapshot().Acquisition; got != ResourcePackAcquisitionFailed {
		t.Fatalf("failed acquisition = %q", got)
	}
}

func TestResolutionFailureAndCancellationKeepAcquisitionNone(t *testing.T) {
	for _, test := range []struct {
		name   string
		ctx    func() context.Context
		result error
	}{
		{name: "failure", ctx: context.Background, result: errors.New("resolution failed")},
		{name: "cancellation", ctx: func() context.Context { ctx, cancel := context.WithCancel(context.Background()); cancel(); return ctx }, result: context.Canceled},
	} {
		t.Run(test.name, func(t *testing.T) {
			var snapshots []ResourcePackAdmissionSnapshot
			connections := newPreparedConnections("unused.invalid:19132", nil, slog.New(slog.NewTextHandler(io.Discard, nil)))
			defer connections.shutdown()
			connections.resourcePackAdmission = func(snapshot ResourcePackAdmissionSnapshot) { snapshots = append(snapshots, snapshot) }
			connections.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) { return nil, test.result }
			if _, err := connections.connect(test.ctx(), dialerTestDownstream{protocol: minecraft.DefaultProtocol}); err == nil {
				t.Fatal("connect() succeeded")
			}
			if len(snapshots) != 1 || snapshots[0].Acquisition != ResourcePackAcquisitionNone || snapshots[0].Offer != ResourcePackOfferNone {
				t.Fatalf("pre-pack snapshot = %#v", snapshots)
			}
		})
	}
}

func TestSecretSafeResourcePackHandlerDropsIdentityFields(t *testing.T) {
	var output bytes.Buffer
	handler := secretSafeResourcePackHandler{next: slog.NewTextHandler(&output, nil)}
	record := slog.NewRecord(time.Time{}, slog.LevelWarn, "cache fallback", 0)
	record.AddAttrs(
		slog.String("UUID", "uuid-sentinel"), slog.String("version", "version-sentinel"),
		slog.String("URL", "url-sentinel"), slog.String("content_key", "key-sentinel"),
		slog.String("digest", "digest-sentinel"), slog.String("path", "path-sentinel"),
		slog.String("err", "resource pack cache unavailable"),
	)
	if err := handler.Handle(context.Background(), record); err != nil {
		t.Fatalf("Handle() error = %v", err)
	}
	got := output.String()
	for _, forbidden := range []string{"uuid-sentinel", "version-sentinel", "url-sentinel", "key-sentinel", "digest-sentinel", "path-sentinel"} {
		if strings.Contains(got, forbidden) {
			t.Fatalf("log exposed %q: %s", forbidden, got)
		}
	}
	if !strings.Contains(got, "resource pack cache unavailable") {
		t.Fatalf("log lost safe failure class: %s", got)
	}
}

func TestResourcePackAdmissionCallbackPanicDoesNotEscapeCleanup(t *testing.T) {
	telemetry := newResourcePackAdmissionTelemetry(1, func(ResourcePackAdmissionSnapshot) { panic("sentinel") })
	telemetry.reportFinal()
	telemetry.reportFinal()
}

func TestResourcePackAdmissionUpdatePublishesResetThenFinal(t *testing.T) {
	var updates []ResourcePackAdmissionSnapshot
	telemetry := newResourcePackAdmissionTelemetry(11, nil)
	telemetry.setUpdateCallback(func(snapshot ResourcePackAdmissionSnapshot) {
		updates = append(updates, snapshot)
	})
	telemetry.mu.Lock()
	telemetry.offer = ResourcePackOfferOptional
	telemetry.packCount = 2
	telemetry.acquisition = ResourcePackAcquisitionComplete
	telemetry.downstream = ResourcePackDownstreamStrippedOptional
	telemetry.mu.Unlock()
	telemetry.reportFinal()

	if len(updates) != 2 {
		t.Fatalf("updates = %d, want reset and final", len(updates))
	}
	if reset := updates[0]; reset.AttemptID != 11 || reset.Offer != ResourcePackOfferNone || reset.Application != ResourcePackApplicationUnavailable {
		t.Fatalf("reset = %+v", reset)
	}
	if final := updates[1]; final.AttemptID != 11 || final.Offer != ResourcePackOfferOptional || final.PackCount != 2 || final.DownstreamOutcome != ResourcePackDownstreamStrippedOptional {
		t.Fatalf("final = %+v", final)
	}
}

func testAdmissionPack(t *testing.T) *resource.Pack {
	t.Helper()
	var archive bytes.Buffer
	writer := zip.NewWriter(&archive)
	manifest, err := writer.Create("manifest.json")
	if err != nil {
		t.Fatal(err)
	}
	_, err = fmt.Fprint(manifest, `{"format_version":2,"header":{"name":"test","description":"test","uuid":"00112233-4455-6677-8899-aabbccddeeff","version":[1,0,0],"min_engine_version":[1,0,0]},"modules":[{"type":"resources","uuid":"ffeeddcc-bbaa-9988-7766-554433221100","version":[1,0,0]}]}`)
	if err != nil {
		t.Fatal(err)
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	pack, err := resource.ReadBytes(archive.Bytes())
	if err != nil {
		t.Fatal(err)
	}
	return pack
}
