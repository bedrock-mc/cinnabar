package launcher

import (
	"context"
	"encoding/json"
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
)

const (
	cacheVersion       = 2
	legacyCacheVersion = 1
	listTTL            = 5 * time.Minute
	homeTTL            = 15 * time.Minute
	refreshTimeout     = 40 * time.Second
)

// snapshot is the last good catalog, persisted as CacheFile.
type snapshot struct {
	Version  int                            `json:"version"`
	Featured feed[[]catalog.FeaturedServer] `json:"featured"`
	Home     feed[catalog.Home]             `json:"home"`
}

type feed[T any] struct {
	Fetched time.Time `json:"fetched"` // zero until a fetch succeeds
	Value   T         `json:"value"`
}

func (snap *snapshot) images() []*catalog.Image {
	images := catalog.FeaturedImages(snap.Featured.Value)
	return append(images, catalog.HomeImages(&snap.Home.Value)...)
}

type flight struct {
	done chan struct{}
	err  error
}

// feedSpec binds one cached feed; fetch caches the artwork and gets the cached value, if any.
type feedSpec[T any] struct {
	name  string
	index int
	ttl   time.Duration
	slot  func(*snapshot) *feed[T]
	fetch func(s *Service, ctx context.Context, src *authcache.Account, previous *T) (T, error)
}

var featuredFeed = feedSpec[[]catalog.FeaturedServer]{
	name: "featured", index: 0, ttl: listTTL,
	slot: func(snap *snapshot) *feed[[]catalog.FeaturedServer] { return &snap.Featured },
	fetch: func(s *Service, ctx context.Context, src *authcache.Account, _ *[]catalog.FeaturedServer) ([]catalog.FeaturedServer, error) {
		servers, err := s.cfg.Featured(ctx, src)
		if err == nil {
			s.cacheArt(ctx, catalog.FeaturedImages(servers))
		}
		return servers, err
	},
}

var homeFeed = feedSpec[catalog.Home]{
	name: "home", index: 1, ttl: homeTTL,
	slot: func(snap *snapshot) *feed[catalog.Home] { return &snap.Home },
	fetch: func(s *Service, ctx context.Context, src *authcache.Account, previous *catalog.Home) (catalog.Home, error) {
		home, err := s.cfg.Home(ctx, src, s.messaging, s.cfg.ArtworkDir)
		if err != nil {
			return home, err
		}
		if len(home.Errors) > 0 {
			partial := errors.New(strings.Join(home.Errors, "; "))
			if home.Failed() {
				return home, partial
			}
			s.logger.Warn("launcher feed partly failed", "feed", "home", "error", control.RedactError(partial))
		}
		s.cacheArt(ctx, catalog.HomeImages(&home))
		if previous != nil {
			home = home.Refill(*previous)
		}
		return home, nil
	},
}

// cached serves the feed's cached value, refreshing it in the background once it and the last
// attempt are older than its TTL; with nothing cached it waits for a fetch and returns its error.
func cached[T any](ctx context.Context, s *Service, spec feedSpec[T]) (T, error) {
	var zero T
	src, err := s.source()
	if err != nil {
		return zero, err
	}
	s.mu.Lock()
	current, attempted := *spec.slot(&s.snap), s.attempted[spec.index]
	s.mu.Unlock()
	if !current.Fetched.IsZero() {
		if time.Since(current.Fetched) >= spec.ttl && time.Since(attempted) >= spec.ttl {
			refresh(s, src, spec)
		}
		return current.Value, nil
	}
	f := refresh(s, src, spec)
	select {
	case <-f.done:
	case <-ctx.Done():
		return zero, ctx.Err()
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if current = *spec.slot(&s.snap); current.Fetched.IsZero() {
		return zero, f.err
	}
	return current.Value, nil
}

// refresh starts a background fetch of the feed unless one is already in flight.
func refresh[T any](s *Service, src *authcache.Account, spec feedSpec[T]) *flight {
	s.mu.Lock()
	defer s.mu.Unlock()
	if f := s.flights[spec.index]; f != nil {
		return f
	}
	f := &flight{done: make(chan struct{})}
	s.flights[spec.index] = f
	s.attempted[spec.index] = time.Now()
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), refreshTimeout)
		defer cancel()
		s.mu.Lock()
		var previous *T
		if slot := spec.slot(&s.snap); !slot.Fetched.IsZero() {
			value := slot.Value
			previous = &value
		}
		s.mu.Unlock()
		value, err := spec.fetch(s, ctx, src, previous)
		s.mu.Lock()
		if err == nil && s.signedOut.Load() {
			err = control.ErrSignedOut
		}
		if err == nil {
			*spec.slot(&s.snap) = feed[T]{Fetched: time.Now(), Value: value}
		}
		s.mu.Unlock()
		if err != nil {
			s.logger.Warn("launcher feed refresh failed", "feed", spec.name, "error", control.RedactError(err))
		} else {
			s.persist()
			s.prune()
		}
		s.mu.Lock()
		f.err = err
		s.flights[spec.index] = nil
		s.mu.Unlock()
		close(f.done)
	}()
	return f
}

// Prefetch refreshes every cached feed in the background.
func (s *Service) Prefetch() {
	src, err := s.source()
	if err != nil {
		return
	}
	refresh(s, src, featuredFeed)
	refresh(s, src, homeFeed)
}

// load restores the cached snapshot, dropping image paths whose files are gone.
func (s *Service) load() {
	if s.cfg.CacheFile == "" {
		return
	}
	data, err := os.ReadFile(s.cfg.CacheFile)
	if err != nil {
		if !errors.Is(err, fs.ErrNotExist) {
			s.logger.Warn("launcher catalog cache unreadable", "error", control.RedactError(err))
		}
		return
	}
	var snap snapshot
	if err := json.Unmarshal(data, &snap); err != nil || (snap.Version != cacheVersion && snap.Version != legacyCacheVersion) {
		s.logger.Warn("launcher catalog cache discarded", "version", snap.Version)
		return
	}
	legacy := snap.Version == legacyCacheVersion
	if legacy {
		for i := range snap.Featured.Value {
			if snap.Featured.Value[i].Group == "" {
				snap.Featured.Value[i].Group = "featured"
			}
		}
	}
	for _, image := range snap.images() {
		if image.Path != "" {
			if _, err := os.Stat(image.Path); err != nil {
				image.Path = ""
			}
		}
	}
	s.snap = snap
	if legacy {
		s.persist()
	}
}

// persist atomically rewrites CacheFile with the current snapshot.
func (s *Service) persist() {
	if s.cfg.CacheFile == "" {
		return
	}
	s.disk.Lock()
	defer s.disk.Unlock()
	s.mu.Lock()
	if s.signedOut.Load() {
		s.mu.Unlock()
		return
	}
	s.snap.Version = cacheVersion
	data, err := json.Marshal(&s.snap)
	s.mu.Unlock()
	if err == nil {
		err = writeAtomic(s.cfg.CacheFile, data)
	}
	if err != nil {
		s.logger.Warn("launcher catalog cache not written", "error", control.RedactError(err))
	}
}

func writeAtomic(path string, data []byte) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return err
	}
	temporary, err := os.CreateTemp(filepath.Dir(path), ".catalog-*.json")
	if err != nil {
		return err
	}
	defer os.Remove(temporary.Name())
	_, err = temporary.Write(data)
	if closeErr := temporary.Close(); err == nil {
		err = closeErr
	}
	if err != nil {
		return err
	}
	return os.Rename(temporary.Name(), path)
}

// prune deletes cached artwork that neither the snapshot nor the profile references. Files used
// within refreshTimeout are spared: an in-flight fetch may not have stored its paths yet.
func (s *Service) prune() {
	if s.cfg.ArtworkDir == "" {
		return
	}
	entries, err := os.ReadDir(s.cfg.ArtworkDir)
	if err != nil {
		return
	}
	keep := map[string]bool{}
	s.mu.Lock()
	for _, image := range s.snap.images() {
		keep[image.Path] = true
	}
	keep[s.gamerpic] = true
	for _, path := range s.profileArt {
		keep[path] = true
	}
	s.mu.Unlock()
	for _, entry := range entries {
		path := filepath.Join(s.cfg.ArtworkDir, entry.Name())
		if !entry.Type().IsRegular() || !strings.HasSuffix(entry.Name(), ".img") || keep[path] {
			continue
		}
		if info, err := entry.Info(); err == nil && time.Since(info.ModTime()) >= refreshTimeout {
			_ = os.Remove(path)
		}
	}
}
