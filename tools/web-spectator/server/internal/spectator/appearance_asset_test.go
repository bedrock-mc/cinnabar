package spectator

import (
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"strings"
	"testing"
)

func TestAppearanceContentAndBounds(t *testing.T) {
	body := []byte(`{"geometry":{"format_version":"1.12.0"},"resourcePatch":{"geometry":{"default":"geometry.test"}}}`)
	sum := sha256.Sum256(body)
	asset := SkinAsset{AppearanceID: hex.EncodeToString(sum[:]), Appearance: base64.StdEncoding.EncodeToString(body)}
	if data, err := decodeAppearance(asset); err != nil || string(data) != string(body) {
		t.Fatalf("valid appearance rejected: %v", err)
	}
	asset.AppearanceID = strings.Repeat("0", 64)
	if _, err := decodeAppearance(asset); err == nil {
		t.Fatal("tampered appearance accepted")
	}
	asset.AppearanceID = hex.EncodeToString(sum[:])
	asset.Appearance = base64.StdEncoding.EncodeToString([]byte(strings.Repeat("x", 385<<10)))
	if _, err := decodeAppearance(asset); err == nil {
		t.Fatal("oversized appearance accepted")
	}
}
