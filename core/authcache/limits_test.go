package authcache

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestNativeOAuthShapeAtSharedLimit checks the real Go reader against the native publication shape.
func TestNativeOAuthShapeAtSharedLimit(t *testing.T) {
	path := filepath.Join(t.TempDir(), "synthetic-oauth.json")
	prefix := `{"access_token":"`
	suffix := `","refresh_token":"rotated-refresh","expiry":"2099-01-01T00:00:00Z","token_type":"Bearer","cinnabar_sign_in_generation":"boundary","future_metadata":{"kept":true}}`
	serialized := []byte(prefix + strings.Repeat("a", maxCacheSize-len(prefix)-len(suffix)) + suffix)
	if err := savePrivate(path, serialized); err != nil {
		t.Fatal(err)
	}
	loaded, err := load(path)
	if err != nil || loaded.RefreshToken != "rotated-refresh" || loaded.Generation != "boundary" {
		t.Fatalf("native-shaped boundary cache was not accepted: %v", err)
	}
	if err := os.WriteFile(path, append(serialized, ' '), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := load(path); err == nil {
		t.Fatal("Go reader accepted a cache beyond the shared serialized limit")
	}
}
