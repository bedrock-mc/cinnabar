package targetpin

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// targetDirectory writes an isolated target manifest above a nested working directory.
func targetDirectory(t *testing.T, hashes map[string]string) string {
	t.Helper()
	root := t.TempDir()
	if err := os.Mkdir(filepath.Join(root, "assets"), 0755); err != nil {
		t.Fatal(err)
	}
	payload, err := json.Marshal(map[string]any{"hashes": hashes})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "assets", "bedrock-target.json"), payload, 0644); err != nil {
		t.Fatal(err)
	}
	nested := filepath.Join(root, "tools", "registrygen")
	if err := os.MkdirAll(nested, 0755); err != nil {
		t.Fatal(err)
	}
	return nested
}

func TestCarrierHashesFollowTheTargetManifest(t *testing.T) {
	block, light := strings.Repeat("a", 64), strings.Repeat("b", 64)
	t.Chdir(targetDirectory(t, map[string]string{"block_registry": block, "light_registry": light}))
	if got, err := BlockHash(); err != nil || got != block {
		t.Fatalf("block hash = %q, %v", got, err)
	}
	if got, err := LightHash(); err != nil || got != light {
		t.Fatalf("light hash = %q, %v", got, err)
	}
	if _, err := Hash("missing"); err == nil {
		t.Fatal("accepted missing carrier pin")
	}
}

func TestCarrierHashesRejectMalformedPins(t *testing.T) {
	for _, hash := range []string{"", strings.Repeat("a", 63), strings.Repeat("g", 64), strings.Repeat("A", 64)} {
		t.Run(hash, func(t *testing.T) {
			t.Chdir(targetDirectory(t, map[string]string{"block_registry": hash}))
			if _, err := BlockHash(); err == nil {
				t.Fatal("accepted malformed block pin")
			}
		})
	}
}
