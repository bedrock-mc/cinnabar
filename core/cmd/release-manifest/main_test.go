package main

import (
	"bytes"
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/update"
)

func TestSignProducesVerifiableManifestWithArtifactDigest(t *testing.T) {
	pub, priv, _ := ed25519.GenerateKey(rand.Reader)
	t.Setenv(keyEnv, base64.StdEncoding.EncodeToString(priv.Seed()))
	file := filepath.Join(t.TempDir(), "c.dmg")
	if err := os.WriteFile(file, []byte("payload"), 0o600); err != nil {
		t.Fatal(err)
	}
	var out bytes.Buffer
	err := signWithKeys([]string{"-version", "1.2.3", "-artifact", "macos-arm64=https://example.test/c.dmg=" + file}, &out, map[string]ed25519.PublicKey{"k1": pub})
	if err != nil {
		t.Fatal(err)
	}
	var envelope update.Envelope
	if err := json.Unmarshal(out.Bytes(), &envelope); err != nil {
		t.Fatal(err)
	}
	manifest, err := update.Verify(out.Bytes(), map[string]ed25519.PublicKey{"k1": pub}, time.Now())
	if err != nil {
		t.Fatal(err)
	}
	artifact := manifest.Artifacts["macos-arm64"]
	if manifest.Version != "1.2.3" || artifact.Size != 7 || len(artifact.SHA256) != 64 {
		t.Fatalf("manifest=%+v", manifest)
	}
}

func TestSignRequiresKeyAndArtifact(t *testing.T) {
	t.Setenv(keyEnv, "")
	if err := run([]string{"sign", "-version", "1.0.0"}, &bytes.Buffer{}); err == nil {
		t.Fatal("missing key accepted")
	}
}

// TestSignRejectsUntrustedSeed prevents publishing releases that clients cannot verify.
func TestSignRejectsUntrustedSeed(t *testing.T) {
	_, private, _ := ed25519.GenerateKey(rand.Reader)
	public, _, _ := ed25519.GenerateKey(rand.Reader)
	t.Setenv(keyEnv, base64.StdEncoding.EncodeToString(private.Seed()))
	if err := signWithKeys([]string{"-version", "1.2.3"}, &bytes.Buffer{}, map[string]ed25519.PublicKey{"k1": public}); err == nil {
		t.Fatal("mismatched signing key accepted")
	}
}
