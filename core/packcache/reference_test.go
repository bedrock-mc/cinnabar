package packcache

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"

	"github.com/google/uuid"
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
	if _, _, err := cache.Reference(pack); err == nil {
		t.Fatal("missing archive was referenced")
	}
	if err := cache.Store(t.Context(), key, pack); err != nil {
		t.Fatal(err)
	}
	path, release, err := cache.Reference(pack)
	if err != nil {
		t.Fatal(err)
	}
	defer release()
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
	if _, _, err := cache.Reference(pack); err == nil {
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
			_, release, err := cache.Reference(pack)
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
	if _, _, err := cache.Reference(pack); err != ErrClosed {
		t.Fatalf("closed cache: %v", err)
	}
}
