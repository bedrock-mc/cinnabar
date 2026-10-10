package packcache

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// References prevent eviction until the client session releases them, without rewriting the archive.
func TestReferencePinsArchiveUntilRelease(t *testing.T) {
	pack, key, data := testPack(t, uuid.New(), "1.0.0", "same payload")
	other, otherKey, otherData := testPack(t, uuid.New(), "1.0.0", "same payload")
	cache, err := New(filepath.Join(t.TempDir(), "objects"), WithQuota(uint64(max(len(data), len(otherData)))))
	if err != nil {
		t.Fatal(err)
	}
	defer cache.Close()
	if _, _, _, err := cache.Reference(pack); err == nil {
		t.Fatal("missing archive was referenced")
	}
	if err := cache.Store(t.Context(), key, pack); err != nil {
		t.Fatal(err)
	}
	path, checksum, release, err := cache.Reference(pack)
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	if checksum != pack.Checksum() {
		t.Fatal("reference checksum differs from stored archive")
	}
	if !filepath.IsAbs(path) || filepath.Dir(path) != cache.root {
		t.Fatal("reference escaped the canonical cache root")
	}
	got, err := os.ReadFile(path)
	if err != nil || !bytes.Equal(got, data) {
		t.Fatal("reference changed archive bytes")
	}
	if err := cache.Store(t.Context(), otherKey, other); err == nil {
		t.Fatal("a referenced archive was evicted")
	}
	release()
	release()
	if err := cache.Store(t.Context(), otherKey, other); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := cache.Reference(pack); err == nil {
		t.Fatal("evicted archive was referenced")
	}
}

// Cache references reject missing, truncated and non-file entries without retaining pins.
func TestReferenceRejectsInvalidFiles(t *testing.T) {
	pack, key, data := testPack(t, uuid.New(), "1.0.0", "payload")
	cache, err := New(filepath.Join(t.TempDir(), "objects"))
	if err != nil {
		t.Fatal(err)
	}
	defer cache.Close()
	name, err := objectName(key)
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(cache.root, name)
	for _, invalid := range []string{"missing", "truncated", "directory"} {
		t.Run(invalid, func(t *testing.T) {
			switch invalid {
			case "truncated":
				if err := os.WriteFile(path, data[:len(data)-1], 0o600); err != nil {
					t.Fatal(err)
				}
			case "directory":
				if err := os.Mkdir(path, 0o700); err != nil {
					t.Fatal(err)
				}
			}
			_, _, release, err := cache.Reference(pack)
			if err == nil || release != nil {
				t.Fatal("invalid cache file was referenced")
			}
			if len(cache.pins) != 0 {
				t.Fatal("failed reference retained a pin")
			}
			_ = os.Remove(path)
		})
	}
	if err := cache.Close(); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := cache.Reference(pack); err != ErrClosed {
		t.Fatalf("closed cache: %v", err)
	}
}

// Equal identity and size do not permit returning an older archive's bytes.
func TestStoreReplacesChangedBytesOnlyAfterReferenceRelease(t *testing.T) {
	cache := newTestCache(t, 1<<20)
	first, key, data := testPack(t, uuid.New(), "1.0.0", "payload")
	// A ZIP comment changes the archive checksum without changing its identity or size.
	data = append(data, 'a')
	data[len(data)-3] = 1
	first, err := resource.ReadBytes(data)
	if err != nil {
		t.Fatal(err)
	}
	key.Size = uint64(len(data))
	changed := bytes.Clone(data)
	changed[len(changed)-1] = 'b'
	second, err := resource.ReadBytes(changed)
	if err != nil {
		t.Fatal(err)
	}
	if !key.Matches(second) || first.Checksum() == second.Checksum() {
		t.Fatal("invalid same-key fixture")
	}
	if err := cache.Store(t.Context(), key, first); err != nil {
		t.Fatal(err)
	}
	path, checksum, release, err := cache.Reference(first)
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	if checksum != first.Checksum() {
		t.Fatal("reference checksum differs from stored archive")
	}
	if err := cache.Store(t.Context(), key, second); err == nil {
		t.Error("replaced or accepted a changed pinned archive")
	}
	if got, err := os.ReadFile(path); err != nil || !bytes.Equal(got, data) {
		t.Fatal("pinned archive changed")
	}
	if _, _, done, err := cache.Reference(second); err == nil {
		done()
		t.Error("referenced a stale archive for the new pack")
	}
	release()
	if err := cache.Store(t.Context(), key, second); err != nil {
		t.Fatal(err)
	}
	got, err := cache.Load(t.Context(), key)
	if err != nil || got == nil || got.Checksum() != second.Checksum() {
		t.Fatal("store retained older same-key bytes")
	}
}

// A changed file cannot replace the checksum recorded at admission or disappear while pinned.
func TestReferenceRetainsRecordedHashAfterDiskCorruption(t *testing.T) {
	cache := newTestCache(t, 1<<20)
	pack, key, data := testPack(t, uuid.New(), "1.0.0", "payload")
	if err := cache.Store(t.Context(), key, pack); err != nil {
		t.Fatal(err)
	}
	path, checksum, release, err := cache.Reference(pack)
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	data[len(data)-1] ^= 1
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatal(err)
	}
	_, recorded, done, err := cache.Reference(pack)
	if err != nil {
		t.Fatal(err)
	}
	done()
	if recorded != checksum {
		t.Fatal("reference accepted the corrupted file's hash")
	}
	if got, err := cache.Load(t.Context(), key); err != nil || got != nil {
		t.Fatal("corrupted object loaded")
	}
	if _, err := os.Stat(path); err != nil {
		t.Fatal("load removed a pinned archive")
	}
	if err := cache.Store(t.Context(), key, pack); err == nil {
		t.Fatal("store replaced a pinned corrupt archive")
	}
	release()
	if err := cache.Store(t.Context(), key, pack); err != nil {
		t.Fatal(err)
	}
}

// The first verified read after a restart records the hash for a later session reference.
func TestReferenceAfterReopenUsesLoadedHash(t *testing.T) {
	cache := newTestCache(t, 1<<20)
	pack, key, _ := testPack(t, uuid.New(), "1.0.0", "payload")
	if err := cache.Store(t.Context(), key, pack); err != nil {
		t.Fatal(err)
	}
	if err := cache.Close(); err != nil {
		t.Fatal(err)
	}
	cache, err := New(cache.root)
	if err != nil {
		t.Fatal(err)
	}
	defer cache.Close()
	loaded, err := cache.Load(t.Context(), key)
	if err != nil || loaded == nil {
		t.Fatalf("load: %v", err)
	}
	_, checksum, release, err := cache.Reference(loaded)
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	if checksum != pack.Checksum() {
		t.Fatal("reopened reference lost archive hash")
	}
}
