package extension

import (
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// testStart is the test servers' clock when they start.
var testStart = time.Unix(1_700_000_000, 0)

// testClock is a clock that tests move.
type testClock struct {
	mu  sync.Mutex
	now time.Time
}

func (c *testClock) Now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.now
}

func (c *testClock) Advance(d time.Duration) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.now = c.now.Add(d)
}

// countingReader is deterministic randomness: the bytes 0, 1, 2, … in turn.
type countingReader struct{ next byte }

func (r *countingReader) Read(p []byte) (int, error) {
	for i := range p {
		p[i] = r.next
		r.next++
	}
	return len(p), nil
}

// goldenBundle is the bundle of the golden manifest, with the digest and size that the golden
// offer gives the real .cxb that `cinnabar-cxb write-fixtures` builds.
func goldenBundle(t *testing.T) Bundle {
	t.Helper()
	var manifest Manifest
	decodeGolden(t, "manifest_payload.json", &manifest)
	var offer Offer
	decodeGolden(t, "offer_payload.json", &offer)
	return Bundle{Manifest: manifest, Digest: offer.Packages[0].Digest, Bytes: offer.Packages[0].Bytes}
}

// testConfig offers the golden bundle under the fixture server seed and the golden audience and
// revision, on a test clock with counting randomness.
func testConfig(t *testing.T) (Config, *testClock) {
	t.Helper()
	_, key, _ := testKeys(t)
	var golden Offer
	decodeGolden(t, "offer_payload.json", &golden)
	clock := &testClock{now: testStart}
	return Config{
		Key:      key,
		Audience: golden.Audience,
		Revision: golden.Revision,
		Bundles:  []Bundle{goldenBundle(t)},
		Now:      clock.Now,
		Rand:     &countingReader{},
	}, clock
}

// testServer is a Server for testConfig changed by change.
func testServer(t *testing.T, change func(*Config)) (*Server, *testClock) {
	t.Helper()
	cfg, clock := testConfig(t)
	if change != nil {
		change(&cfg)
	}
	s, err := NewServer(cfg)
	if err != nil {
		t.Fatal(err)
	}
	return s, clock
}

// markerOffer verifies the server's marker as the client does and returns its offer and digest.
func markerOffer(t *testing.T, marker Marker) (Offer, string) {
	t.Helper()
	var offer Offer
	digest, err := marker.Offer.Verify(marker.ServerKey, OfferDomain, MaxMarkerBytes/2, &offer)
	if err != nil {
		t.Fatalf("the marker does not verify: %v", err)
	}
	if offer.ServerKey != marker.ServerKey {
		t.Fatalf("offer key %s, marker key %s", offer.ServerKey, marker.ServerKey)
	}
	return offer, digest
}

// The signed offer names every bundle by id, publisher, digest and size at a URL of the reserved
// delivery origin, asks for what the bundles' manifests need and expires within the client's
// offer lifetime, with room for a client clock that runs behind.
func TestNewServerOffersItsBundles(t *testing.T) {
	cfg, clock := testConfig(t)
	another := goldenBundle(t)
	renamed("another")(&another.Manifest)
	another.Manifest.Permissions = NewPermissions(PermissionUI)
	another.Digest = strings.Repeat("ab", 32)
	cfg.Bundles = append(cfg.Bundles, another)
	s, err := NewServer(cfg)
	if err != nil {
		t.Fatal(err)
	}
	marker := s.Marker()
	if encoded, err := Encode(marker); err != nil || len(encoded) > MaxMarkerBytes {
		t.Fatalf("marker of %d bytes, %v", len(encoded), err)
	}
	offer, _ := markerOffer(t, marker)
	_, key, _ := testKeys(t)
	now := uint64(clock.Now().Unix())
	switch {
	case offer.ServerKey != PublicKey(key):
		t.Errorf("server key %s", offer.ServerKey)
	case offer.Version != WireVersion || offer.Carrier != Carrier:
		t.Errorf("version %d, carrier %q", offer.Version, offer.Carrier)
	case offer.Audience != cfg.Audience || offer.Revision != cfg.Revision:
		t.Errorf("audience %q, revision %d", offer.Audience, offer.Revision)
	case offer.ExpiresUnix <= now || offer.ExpiresUnix-now >= MaxOfferLifetimeSecs:
		t.Errorf("expires %d at %d: not within the client's offer lifetime", offer.ExpiresUnix, now)
	case offer.Fallback == "" || len(offer.Fallback) > MaxFallbackBytes:
		t.Errorf("fallback %q", offer.Fallback)
	}
	scope := offer.Scope
	union := cfg.Bundles[0].Manifest.Permissions | another.Manifest.Permissions
	if scope.Permissions != union || !slices.Equal(scope.Origins, []string{DeliveryOrigin}) || scope.GPUBytes != 0 {
		t.Errorf("scope %+v, want permissions %v and origins [%s]", scope, union.List(), DeliveryOrigin)
	}
	if guest := min(scope.MemoryBytes/uint64(len(cfg.Bundles)), MaxGuestMemory); guest == 0 || scope.MemoryBytes > MaxSessionMemory {
		t.Errorf("memory %d: each guest gets %d", scope.MemoryBytes, guest)
	}
	var want []PackageOffer
	for _, b := range cfg.Bundles {
		want = append(want, PackageOffer{
			ID:           b.Manifest.ID,
			PublisherKey: b.Manifest.PublisherKey,
			Digest:       b.Digest,
			Bytes:        b.Bytes,
			URL:          DeliveryOrigin + "/" + b.Digest + ".cxb",
		})
	}
	if !slices.Equal(offer.Packages, want) {
		t.Errorf("packages %+v, want %+v", offer.Packages, want)
	}
}

// Media origins join the delivery origin, and a scene or media bundle asks for surface memory.
func TestNewServerOffersMediaOriginsAndSurfaceMemory(t *testing.T) {
	const media = "https://127.0.0.1:19443"
	s, _ := testServer(t, func(cfg *Config) {
		cfg.MediaOrigins = []string{media, DeliveryOrigin}
		cfg.Bundles[0].Manifest.Permissions |= NewPermissions(PermissionMedia)
	})
	offer, _ := markerOffer(t, s.Marker())
	if !slices.Equal(offer.Scope.Origins, []string{media, DeliveryOrigin}) || offer.Scope.GPUBytes != MediaGPUBytes {
		t.Errorf("scope %+v, want origins [%s %s] and %d GPU bytes", offer.Scope, media, DeliveryOrigin, MediaGPUBytes)
	}
}

// Startup refuses an offer the client would refuse or could not reach.
func TestNewServerRejects(t *testing.T) {
	tooMany := func(cfg *Config) {
		for i := range MaxBundles {
			b := goldenBundle(t)
			renamed("b" + string(rune('a'+i)))(&b.Manifest)
			cfg.Bundles = append(cfg.Bundles, b)
		}
	}
	for _, c := range []struct {
		name   string
		change func(*Config)
	}{
		{"an audience without a port", func(cfg *Config) { cfg.Audience = "127.0.0.1" }},
		{"an audience on port 0", func(cfg *Config) { cfg.Audience = "127.0.0.1:0" }},
		{"an audience in upper case", func(cfg *Config) { cfg.Audience = "LOCALHOST:19132" }},
		{"an audience with a padded port", func(cfg *Config) { cfg.Audience = "127.0.0.1:019132" }},
		{"no bundles", func(cfg *Config) { cfg.Bundles = nil }},
		{"too many bundles", tooMany},
		{"a bundle twice", func(cfg *Config) { cfg.Bundles = append(cfg.Bundles, cfg.Bundles[0]) }},
		{"no key", func(cfg *Config) { cfg.Key = nil }},
	} {
		t.Run(c.name, func(t *testing.T) {
			cfg, _ := testConfig(t)
			c.change(&cfg)
			if _, err := NewServer(cfg); err == nil {
				t.Fatal("accepted")
			}
		})
	}
}

// The revision goes up by one per start and is never reset: a file that does not hold a
// revision fails startup instead of offering a lower one.
func TestNextRevisionNeverDecreases(t *testing.T) {
	path := filepath.Join(t.TempDir(), "world", RevisionFile)
	for want := uint64(1); want <= 2; want++ {
		if got, err := NextRevision(path); err != nil || got != want {
			t.Fatalf("revision %d, %v; want %d", got, err, want)
		}
	}
	if data, err := os.ReadFile(path); err != nil || strings.TrimSpace(string(data)) != "2" {
		t.Fatalf("stored %q, %v", data, err)
	}
	for _, bad := range []string{"garbage", "-1", "18446744073709551615"} {
		if err := os.WriteFile(path, []byte(bad), 0o644); err != nil {
			t.Fatal(err)
		}
		if got, err := NextRevision(path); err == nil {
			t.Fatalf("after %q: revision %d", bad, got)
		}
		if data, _ := os.ReadFile(path); string(data) != bad {
			t.Fatalf("after %q the file holds %q", bad, data)
		}
	}
}

// The marker pack is a resource pack Dragonfly loads that holds exactly the marker. Its version is
// the revision, so a client never takes an older offer from its pack cache; rewriting it leaves
// nothing of the previous one.
func TestWriteMarkerPackHoldsTheMarker(t *testing.T) {
	resources := t.TempDir()
	read := func(s *Server) *resource.Pack {
		t.Helper()
		if err := s.WriteMarkerPack(resources); err != nil {
			t.Fatal(err)
		}
		pack, err := resource.ReadPath(filepath.Join(resources, MarkerPackDir))
		if err != nil {
			t.Fatal(err)
		}
		data, err := pack.ReadFile(MarkerPath)
		if err != nil {
			t.Fatal(err)
		}
		if want, _ := Encode(s.Marker()); string(data) != string(want) {
			t.Fatalf("marker file %s, want %s", data, want)
		}
		return pack
	}
	first, _ := testServer(t, nil)
	pack := read(first)
	if v := pack.Manifest().Header.Version; v[2] != int(first.offer.Revision) {
		t.Fatalf("pack version %v for revision %d", v, first.offer.Revision)
	}
	stale := filepath.Join(resources, MarkerPackDir, "stale.json")
	if err := os.WriteFile(stale, []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}
	second, _ := testServer(t, func(cfg *Config) { cfg.Revision++ })
	next := read(second)
	if next.UUID() != pack.UUID() || next.Version() == pack.Version() {
		t.Fatalf("pack %s %s after %s %s", next.UUID(), next.Version(), pack.UUID(), pack.Version())
	}
	if _, err := os.Stat(stale); err == nil {
		t.Fatal("a file of the previous marker pack survived")
	}
	for range 2 {
		if err := RemoveMarkerPack(resources); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := os.Stat(filepath.Join(resources, MarkerPackDir)); err == nil {
		t.Fatal("the marker pack survived RemoveMarkerPack")
	}
}

// TestAudienceMatchesClientURLHost rejects spellings the Rust URL parser would normalize or refuse.
func TestAudienceMatchesClientURLHost(t *testing.T) {
	for _, audience := range []string{
		":19132", "[0:0:0:0:0:0:0:1]:19132", "127.000.000.001:19132", "[::ffff:127.0.0.1]:19132",
		"127.1:19132", "2130706433:19132", "0x7f000001:19132", "example.com/path:19132",
		"user@example.com:19132", "éxample.com:19132", "[fe80::1%en0]:19132",
	} {
		if err := checkAudience(audience); err == nil {
			t.Errorf("accepted noncanonical audience %q", audience)
		}
	}
	for _, audience := range []string{"127.0.0.1:19132", "[::1]:19132", "[2001:db8::1]:19132", "[::ffff:7f00:1]:19132", "localhost:19132", "xn--xample-9ua.com:19132"} {
		if err := checkAudience(audience); err != nil {
			t.Errorf("rejected canonical audience %q: %v", audience, err)
		}
	}
}
