package launcher

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
)

type feedFixture struct {
	dir, art, file string
	featured       func() ([]catalog.FeaturedServer, error)
	calls          int
}

func newFeedFixture(t *testing.T) *feedFixture {
	t.Helper()
	dir := t.TempDir()
	f := &feedFixture{dir: dir, art: filepath.Join(dir, "artwork"), file: filepath.Join(dir, "catalog.json")}
	if err := os.MkdirAll(f.art, 0o700); err != nil {
		t.Fatal(err)
	}
	return f
}

func (f *feedFixture) service() *Service {
	return New(Config{
		Account: testAccount(), ArtworkDir: f.art, CacheFile: f.file,
		Featured: func(context.Context, *authcache.Account) ([]catalog.FeaturedServer, error) {
			f.calls++
			return f.featured()
		},
		Home: func(context.Context, *authcache.Account, *catalog.MessagingSession, string) (catalog.Home, error) {
			return catalog.Home{}, errors.New("offline")
		},
		CacheArt: func(context.Context, string, []*catalog.Image) {},
	})
}

// writeCache stores a stale snapshot whose featured logo is image.
func (f *feedFixture) writeCache(t *testing.T, name, image string) {
	t.Helper()
	snap := snapshot{Version: cacheVersion}
	snap.Featured = feed[[]catalog.FeaturedServer]{
		Fetched: time.Now().Add(-time.Hour),
		Value:   []catalog.FeaturedServer{{Name: name, Logo: catalog.Image{URL: "https://a.test/" + image, Path: image}}},
	}
	snap.Home = feed[catalog.Home]{Fetched: time.Now().Add(-time.Hour), Value: catalog.Home{
		RealmInvites: 3,
	}}
	data, _ := json.Marshal(snap)
	if err := os.WriteFile(f.file, data, 0o600); err != nil {
		t.Fatal(err)
	}
}

func (f *feedFixture) cachedFeatured(t *testing.T) string {
	t.Helper()
	var snap snapshot
	data, err := os.ReadFile(f.file)
	if err != nil || json.Unmarshal(data, &snap) != nil || len(snap.Featured.Value) != 1 {
		t.Fatalf("cache file = %s, err = %v", data, err)
	}
	return snap.Featured.Value[0].Name
}

func settle(s *Service) {
	s.mu.Lock()
	flights := s.flights
	s.mu.Unlock()
	for _, f := range flights {
		if f != nil {
			<-f.done
		}
	}
}

// A cold start answers from the cache file while the network fetch is still blocked.
func TestColdStartServesCacheBeforeNetwork(t *testing.T) {
	f := newFeedFixture(t)
	f.writeCache(t, "Cached", "")
	release := make(chan struct{})
	f.featured = func() ([]catalog.FeaturedServer, error) {
		<-release
		return []catalog.FeaturedServer{{Name: "Fresh"}}, nil
	}
	service := f.service()
	servers, err := service.FeaturedServers(context.Background())
	if err != nil || len(servers) != 1 || servers[0].Name != "Cached" {
		t.Fatalf("servers = %+v, err = %v", servers, err)
	}
	if home, err := service.Home(context.Background()); err != nil || home.RealmInvites != 3 {
		t.Fatalf("home = %+v, err = %v", home, err)
	}
	close(release)
	settle(service)
}

// A successful refresh replaces the served copy and rewrites the cache file.
func TestRefreshReplacesSnapshotAndRewritesFile(t *testing.T) {
	f := newFeedFixture(t)
	f.writeCache(t, "Cached", "")
	f.featured = func() ([]catalog.FeaturedServer, error) { return []catalog.FeaturedServer{{Name: "Fresh"}}, nil }
	service := f.service()
	service.Prefetch()
	settle(service)
	servers, err := service.FeaturedServers(context.Background())
	if err != nil || servers[0].Name != "Fresh" || f.cachedFeatured(t) != "Fresh" {
		t.Fatalf("servers = %+v, err = %v", servers, err)
	}
	if home, err := service.Home(context.Background()); err != nil || home.RealmInvites != 3 {
		t.Fatalf("a failed home refresh dropped the cached home: %+v, %v", home, err)
	}
	settle(service)
	if f.calls != 1 {
		t.Fatalf("fresh snapshot refetched: %d calls", f.calls)
	}
}

// A failed refresh keeps both the served copy and the file; with no cache the error surfaces.
func TestFailedRefreshKeepsCache(t *testing.T) {
	f := newFeedFixture(t)
	f.writeCache(t, "Cached", "")
	f.featured = func() ([]catalog.FeaturedServer, error) { return nil, errors.New("offline") }
	service := f.service()
	service.Prefetch()
	settle(service)
	if servers, err := service.FeaturedServers(context.Background()); err != nil || servers[0].Name != "Cached" {
		t.Fatalf("servers = %+v, err = %v", servers, err)
	}
	if f.cachedFeatured(t) != "Cached" {
		t.Fatal("failed refresh rewrote the cache")
	}
	empty := newFeedFixture(t)
	empty.featured = f.featured
	if _, err := empty.service().FeaturedServers(context.Background()); err == nil {
		t.Fatal("uncached failure returned no error")
	}
}

// Pruning removes unreferenced artwork only, sparing the profile's and files still in use.
func TestPruneRemovesOnlyUnreferencedImages(t *testing.T) {
	f := newFeedFixture(t)
	path := func(name string) string { return filepath.Join(f.art, name) }
	old := time.Now().Add(-time.Hour)
	for _, name := range []string{"logo.img", "gamerpic.img", "stale.img", "young.img", "notes.txt"} {
		if err := os.WriteFile(path(name), []byte("x"), 0o600); err != nil {
			t.Fatal(err)
		}
		if name != "young.img" {
			_ = os.Chtimes(path(name), old, old)
		}
	}
	f.featured = func() ([]catalog.FeaturedServer, error) {
		return []catalog.FeaturedServer{{Name: "S", Logo: catalog.Image{Path: path("logo.img")}}}, nil
	}
	service := f.service()
	service.gamerpic = path("gamerpic.img")
	if _, err := service.FeaturedServers(context.Background()); err != nil {
		t.Fatal(err)
	}
	for name, want := range map[string]bool{
		"logo.img": true, "gamerpic.img": true, "stale.img": false, "young.img": true, "notes.txt": true,
	} {
		if _, err := os.Stat(path(name)); (err == nil) != want {
			t.Fatalf("%s exists = %v, want %v", name, err == nil, want)
		}
	}
}

// A cached image whose file vanished loads without a path so the client never gets a dead one.
func TestLoadDropsMissingImagePaths(t *testing.T) {
	f := newFeedFixture(t)
	f.writeCache(t, "Cached", filepath.Join(f.art, "gone.img"))
	f.featured = func() ([]catalog.FeaturedServer, error) { return nil, errors.New("offline") }
	service := f.service()
	servers, _ := service.FeaturedServers(context.Background())
	settle(service)
	if servers[0].Logo.Path != "" || servers[0].Logo.URL == "" {
		t.Fatalf("logo = %+v", servers[0].Logo)
	}
}
