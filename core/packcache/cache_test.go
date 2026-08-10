package packcache

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

func TestCacheHitMissAndContentKeyOmission(t *testing.T) {
	c := newTestCache(t, 1<<20)
	pack, key, archive := testPack(t, uuid.New(), "1.2.3", "one")
	pack = pack.WithContentKey("must-not-persist")
	if got, err := c.Load(context.Background(), key); err != nil || got != nil {
		t.Fatalf("initial Load = %v, %v; want miss", got, err)
	}
	if err := c.Store(context.Background(), key, pack); err != nil {
		t.Fatal(err)
	}
	got, err := c.Load(context.Background(), key)
	if err != nil || got == nil {
		t.Fatalf("Load = %v, %v; want hit", got, err)
	}
	if got.ContentKey() != "" {
		t.Fatal("content key survived persistence")
	}
	entries, _ := os.ReadDir(c.root)
	var objects []os.DirEntry
	for _, item := range entries {
		if validObjectName(item.Name()) {
			objects = append(objects, item)
		}
	}
	if len(objects) != 1 {
		t.Fatalf("objects = %d, want 1", len(objects))
	}
	onDisk, err := os.ReadFile(filepath.Join(c.root, objects[0].Name()))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(onDisk, archive) || bytes.Contains(onDisk, []byte("must-not-persist")) {
		t.Fatal("cache did not contain only the original archive bytes")
	}
}

func TestCacheRejectsWrongPackIdentity(t *testing.T) {
	c := newTestCache(t, 1<<20)
	pack, key, _ := testPack(t, uuid.New(), "1.0.0", "one")
	cases := []minecraft.ResourcePackCacheKey{key}
	cases[0].Version = "2.0.0"
	wrongSize := key
	wrongSize.Size++
	cases = append(cases, wrongSize)
	wrongHash := key
	wrongHash.SHA256[0] ^= 1
	cases = append(cases, wrongHash)
	wrongID := key
	wrongID.UUID = uuid.New()
	cases = append(cases, wrongID)
	for _, bad := range cases {
		if err := c.Store(context.Background(), bad, pack); err == nil {
			t.Fatalf("Store accepted mismatched key %+v", bad)
		}
	}
}

func TestCacheLoadRejectsValidArchiveWithWrongIdentity(t *testing.T) {
	c := newTestCache(t, 1<<20)
	_, _, archive := testPack(t, uuid.New(), "1.0.0", "other identity")
	key := minecraft.ResourcePackCacheKey{UUID: uuid.New(), Version: "1.0.0", Size: uint64(len(archive)), SHA256: sha256.Sum256(archive)}
	name, err := objectName(key)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(c.root, name), archive, 0o600); err != nil {
		t.Fatal(err)
	}
	got, err := c.Load(context.Background(), key)
	if err != nil || got != nil {
		t.Fatalf("Load = %v, %v; want identity miss", got, err)
	}
}

func TestCacheTamperWrongSizeAndHashAreMisses(t *testing.T) {
	for _, tc := range []struct {
		name string
		edit func([]byte) []byte
	}{
		{"size", func(p []byte) []byte { return p[:len(p)-1] }},
		{"hash", func(p []byte) []byte { q := append([]byte(nil), p...); q[len(q)-1] ^= 1; return q }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			c := newTestCache(t, 1<<20)
			pack, key, _ := testPack(t, uuid.New(), "1.0.0", tc.name)
			if err := c.Store(context.Background(), key, pack); err != nil {
				t.Fatal(err)
			}
			name, _ := objectName(key)
			path := filepath.Join(c.root, name)
			data, _ := os.ReadFile(path)
			if err := os.WriteFile(path, tc.edit(data), 0o600); err != nil {
				t.Fatal(err)
			}
			got, err := c.Load(context.Background(), key)
			if err != nil || got != nil {
				t.Fatalf("Load = %v, %v; want miss", got, err)
			}
			if _, err := os.Stat(path); !errors.Is(err, fs.ErrNotExist) {
				t.Fatalf("corrupt object remains: %v", err)
			}
		})
	}
}

func TestConcurrentSameKeyWritersAndExistingDestination(t *testing.T) {
	c := newTestCache(t, 1<<20)
	pack, key, archive := testPack(t, uuid.New(), "1.0.0", "race")
	const writers = 16
	var wg sync.WaitGroup
	errs := make(chan error, writers)
	for range writers {
		wg.Add(1)
		go func() { defer wg.Done(); errs <- c.Store(context.Background(), key, pack) }()
	}
	wg.Wait()
	close(errs)
	for err := range errs {
		if err != nil {
			t.Fatal(err)
		}
	}
	name, _ := objectName(key)
	got, err := os.ReadFile(filepath.Join(c.root, name))
	if err != nil || !bytes.Equal(got, archive) {
		t.Fatalf("published object invalid: %v", err)
	}
	// A valid existing destination is a successful no-op.
	if err := c.Store(context.Background(), key, pack); err != nil {
		t.Fatal(err)
	}
	temp := filepath.Join(c.root, tempPrefix+"publish")
	if err := os.WriteFile(temp, []byte("loser"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := publishNoReplace(temp, filepath.Join(c.root, name)); !errors.Is(err, fs.ErrExist) {
		t.Fatalf("publish error = %v, want exists", err)
	}
	got, err = os.ReadFile(filepath.Join(c.root, name))
	if err != nil || !bytes.Equal(got, archive) {
		t.Fatal("existing destination was replaced")
	}
}

func TestInterruptedTempIgnoredAndRemovedOnRestart(t *testing.T) {
	parent := secureTempDir(t)
	root := filepath.Join(parent, "objects")
	if err := os.Mkdir(root, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := secureCreatedPath(root, true); err != nil {
		t.Fatal(err)
	}
	temp := filepath.Join(root, tempPrefix+"interrupted")
	if err := os.WriteFile(temp, []byte("partial"), 0o600); err != nil {
		t.Fatal(err)
	}
	old := time.Now().Add(-25 * time.Hour)
	if err := os.Chtimes(temp, old, old); err != nil {
		t.Fatal(err)
	}
	c, err := New(root, WithQuota(1<<20))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = c.Close() })
	if _, err := os.Stat(temp); !errors.Is(err, fs.ErrNotExist) {
		t.Fatalf("temp remains: %v", err)
	}
}

func TestQuotaBoundaryEvictionPinsAndRestartScan(t *testing.T) {
	parent := secureTempDir(t)
	root := filepath.Join(parent, "objects")
	p1, k1, b1 := testPack(t, uuid.New(), "1.0.0", "first")
	p2, k2, b2 := testPack(t, uuid.New(), "1.0.0", "second payload")
	quota := uint64(len(b1) + len(b2))
	c, err := New(root, WithQuota(quota))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = c.Close() })
	now := time.Unix(100, 0)
	c.clock = func() time.Time { now = now.Add(time.Second); return now }
	if err := c.Store(context.Background(), k1, p1); err != nil {
		t.Fatal(err)
	}
	release, err := c.Pin(k1)
	if err != nil {
		t.Fatal(err)
	}
	if err := c.Store(context.Background(), k2, p2); err != nil {
		t.Fatalf("exact quota: %v", err)
	}
	p3, k3, _ := testPack(t, uuid.New(), "1.0.0", "third payload plus one")
	if err := c.Store(context.Background(), k3, p3); err == nil {
		t.Fatal("admission succeeded while older object pinned")
	}
	release()
	if err := c.Store(context.Background(), k3, p3); err != nil {
		t.Fatal(err)
	}
	if got, _ := c.Load(context.Background(), k1); got != nil {
		t.Fatal("oldest object was not evicted")
	}
	// Re-open to prove existing bytes are counted and remain readable.
	if err := c.Close(); err != nil {
		t.Fatal(err)
	}
	c2, err := New(root, WithQuota(quota))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = c2.Close() })
	if got, err := c2.Load(context.Background(), k3); err != nil || got == nil {
		t.Fatalf("restart hit = %v, %v", got, err)
	}
	if err := c2.Close(); err != nil {
		t.Fatal(err)
	}
	tooSmall, err := New(root, WithQuota(uint64(len(b2))-1))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = tooSmall.Close() })
	if tooSmall.used > tooSmall.quota {
		t.Fatalf("restart quota not enforced: %d > %d", tooSmall.used, tooSmall.quota)
	}
}

func TestImpossibleAdmissionAndContextCancellation(t *testing.T) {
	pack, key, archive := testPack(t, uuid.New(), "1.0.0", "large")
	c := newTestCache(t, uint64(len(archive)-1))
	if err := c.Store(context.Background(), key, pack); err == nil {
		t.Fatal("oversized object admitted")
	}
	c = newTestCache(t, 1<<20)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := c.Store(ctx, key, pack); !errors.Is(err, context.Canceled) {
		t.Fatalf("Store error = %v", err)
	}
}

func TestLinkedParentAndInsecurePermissionsRejected(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Unix permission and symlink test")
	}
	base := secureTempDir(t)
	realParent := filepath.Join(base, "real")
	if err := os.Mkdir(realParent, 0o700); err != nil {
		t.Fatal(err)
	}
	linked := filepath.Join(base, "linked")
	if err := os.Symlink(realParent, linked); err != nil {
		t.Fatal(err)
	}
	if _, err := New(filepath.Join(linked, "objects")); err == nil {
		t.Fatal("linked parent accepted")
	}
	insecure := filepath.Join(base, "insecure")
	if err := os.Mkdir(insecure, 0o755); err != nil {
		t.Fatal(err)
	}
	if _, err := New(filepath.Join(insecure, "objects")); err == nil {
		t.Fatal("insecure parent accepted")
	}
	root := filepath.Join(base, "root")
	if err := os.Mkdir(root, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(root, 0o755); err != nil {
		t.Fatal(err)
	}
	if _, err := New(root); err == nil {
		t.Fatal("insecure root accepted")
	}
	c := newTestCache(t, 1<<20)
	pack, key, _ := testPack(t, uuid.New(), "1.0.0", "permissions")
	if err := c.Store(context.Background(), key, pack); err != nil {
		t.Fatal(err)
	}
	name, _ := objectName(key)
	if err := os.Chmod(filepath.Join(c.root, name), 0o644); err != nil {
		t.Fatal(err)
	}
	if got, err := c.Load(context.Background(), key); err != nil || got != nil {
		t.Fatalf("insecure object Load = %v, %v; want miss", got, err)
	}
}

func TestKeyValidationIsBounded(t *testing.T) {
	_, key, _ := testPack(t, uuid.New(), "1.0.0", "key")
	key.Version = string(bytes.Repeat([]byte{'x'}, maxVersionBytes+1))
	if _, err := objectName(key); err == nil {
		t.Fatal("oversized version accepted")
	}
}

func TestSameRootLifecycleAndUseAfterClose(t *testing.T) {
	root := filepath.Join(secureTempDir(t), "objects")
	c, err := New(root, WithQuota(1<<20))
	if err != nil {
		t.Fatal(err)
	}
	alias := filepath.Join(root, "..", filepath.Base(root))
	if _, err := New(alias, WithQuota(1<<20)); !errors.Is(err, ErrInUse) {
		t.Fatalf("second New = %v, want ErrInUse", err)
	}
	pack, key, _ := testPack(t, uuid.New(), "1.0.0", "lifecycle")
	if err := c.Store(context.Background(), key, pack); err != nil {
		t.Fatal(err)
	}
	if err := c.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := c.Load(context.Background(), key); !errors.Is(err, ErrClosed) {
		t.Fatalf("Load after Close = %v", err)
	}
	if err := c.Store(context.Background(), key, pack); !errors.Is(err, ErrClosed) {
		t.Fatalf("Store after Close = %v", err)
	}
	if _, err := c.Pin(key); !errors.Is(err, ErrClosed) {
		t.Fatalf("Pin after Close = %v", err)
	}
	reopened, err := New(root, WithQuota(1<<20))
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	if got, err := reopened.Load(context.Background(), key); err != nil || got == nil {
		t.Fatalf("reopen Load = %v, %v", got, err)
	}
}

func TestConcurrentSameRootOpeners(t *testing.T) {
	root := filepath.Join(secureTempDir(t), "objects")
	const count = 12
	start := make(chan struct{})
	results := make(chan struct {
		c   *Cache
		err error
	}, count)
	for range count {
		go func() {
			<-start
			c, err := New(root)
			results <- struct {
				c   *Cache
				err error
			}{c, err}
		}()
	}
	close(start)
	var winner *Cache
	for range count {
		result := <-results
		if result.err == nil {
			if winner != nil {
				t.Fatal("multiple same-root openers succeeded")
			}
			winner = result.c
			continue
		}
		if !errors.Is(result.err, ErrInUse) {
			t.Fatalf("New error = %v", result.err)
		}
	}
	if winner == nil {
		t.Fatal("no opener succeeded")
	}
	if err := winner.Close(); err != nil {
		t.Fatal(err)
	}
}

func TestExclusiveLeaseAcrossProcess(t *testing.T) {
	if os.Getenv("PACKCACHE_LEASE_HELPER") == "1" {
		root, ready, stop := os.Getenv("PACKCACHE_ROOT"), os.Getenv("PACKCACHE_READY"), os.Getenv("PACKCACHE_STOP")
		c, err := New(root)
		if err != nil {
			os.Exit(2)
		}
		if err := os.WriteFile(ready, []byte("ready"), 0o600); err != nil {
			os.Exit(3)
		}
		for i := 0; i < 500; i++ {
			if _, err := os.Stat(stop); err == nil {
				_ = c.Close()
				return
			}
			time.Sleep(10 * time.Millisecond)
		}
		os.Exit(4)
	}
	base := secureTempDir(t)
	root := filepath.Join(base, "objects")
	seed, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	if err := seed.Close(); err != nil {
		t.Fatal(err)
	}
	ready, stop := filepath.Join(base, "ready"), filepath.Join(base, "stop")
	cmd := exec.Command(os.Args[0], "-test.run=^TestExclusiveLeaseAcrossProcess$")
	cmd.Env = append(os.Environ(), "PACKCACHE_LEASE_HELPER=1", "PACKCACHE_ROOT="+root, "PACKCACHE_READY="+ready, "PACKCACHE_STOP="+stop)
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = os.WriteFile(stop, nil, 0o600); _ = cmd.Wait() }()
	for i := 0; i < 500; i++ {
		if _, err := os.Stat(ready); err == nil {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	if _, err := os.Stat(ready); err != nil {
		t.Fatal("helper did not acquire lease")
	}
	if _, err := New(root); !errors.Is(err, ErrInUse) {
		t.Fatalf("cross-process New = %v, want ErrInUse", err)
	}
}

func TestStartupCleansAllPrivateTempsBeforeEntryLimit(t *testing.T) {
	if testing.Short() {
		t.Skip("creates more than 100,000 temporary directory entries")
	}
	root := filepath.Join(secureTempDir(t), "objects")
	if err := os.Mkdir(root, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := secureCreatedPath(root, true); err != nil {
		t.Fatal(err)
	}
	for i := 0; i <= maxIndexEntries; i++ {
		name := filepath.Join(root, fmt.Sprintf("%s%06d", tempPrefix, i))
		if err := os.WriteFile(name, nil, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	c, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close()
	entries, err := os.ReadDir(root)
	if err != nil {
		t.Fatal(err)
	}
	for _, item := range entries {
		if strings.HasPrefix(item.Name(), tempPrefix) {
			t.Fatalf("temporary object remains: %s", item.Name())
		}
	}
}

func TestStartupRemovesFreshAndHardlinkedTemps(t *testing.T) {
	root := filepath.Join(secureTempDir(t), "objects")
	c, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	pack, key, _ := testPack(t, uuid.New(), "1.0.0", "hardlink temp")
	if err := c.Store(context.Background(), key, pack); err != nil {
		t.Fatal(err)
	}
	name, _ := objectName(key)
	object := filepath.Join(root, name)
	if err := c.Close(); err != nil {
		t.Fatal(err)
	}
	for _, suffix := range []string{"fresh-a", "fresh-b"} {
		if err := os.WriteFile(filepath.Join(root, tempPrefix+suffix), []byte("partial"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	linked := filepath.Join(root, tempPrefix+"published-link")
	if err := os.Link(object, linked); err != nil {
		t.Fatal(err)
	}
	reopened, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	if _, err := os.Stat(linked); !errors.Is(err, fs.ErrNotExist) {
		t.Fatalf("hardlinked temp remains: %v", err)
	}
	if got, err := reopened.Load(context.Background(), key); err != nil || got == nil {
		t.Fatalf("published object lost: %v, %v", got, err)
	}
}

func newTestCache(t *testing.T, quota uint64) *Cache {
	t.Helper()
	c, err := New(filepath.Join(secureTempDir(t), "objects"), WithQuota(quota))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = c.Close() })
	return c
}

func secureTempDir(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	if err := secureCreatedPath(dir, true); err != nil {
		t.Fatal(err)
	}
	return dir
}

func testPack(t *testing.T, id uuid.UUID, version, payload string) (*resource.Pack, minecraft.ResourcePackCacheKey, []byte) {
	t.Helper()
	var b bytes.Buffer
	zw := zip.NewWriter(&b)
	manifest, err := zw.Create("manifest.json")
	if err != nil {
		t.Fatal(err)
	}
	_, err = fmt.Fprintf(manifest, `{"format_version":2,"header":{"name":"test","description":"test","uuid":"%s","version":[%s],"min_engine_version":[1,0,0]},"modules":[{"type":"resources","uuid":"%s","version":[%s]}]}`, id, csvVersion(version), uuid.New(), csvVersion(version))
	if err != nil {
		t.Fatal(err)
	}
	f, err := zw.Create("payload.txt")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := f.Write([]byte(payload)); err != nil {
		t.Fatal(err)
	}
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	archive := b.Bytes()
	pack, err := resource.ReadBytes(archive)
	if err != nil {
		t.Fatal(err)
	}
	key := minecraft.ResourcePackCacheKey{UUID: id, Version: version, Size: uint64(len(archive)), SHA256: sha256.Sum256(archive)}
	return pack, key, append([]byte(nil), archive...)
}

func csvVersion(version string) string {
	var a, b, c int
	fmt.Sscanf(version, "%d.%d.%d", &a, &b, &c)
	return fmt.Sprintf("%d,%d,%d", a, b, c)
}
