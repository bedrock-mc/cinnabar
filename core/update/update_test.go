package update

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"strings"
	"testing"
	"time"
)

var fixedNow = time.Date(2026, 9, 1, 0, 0, 0, 0, time.UTC)

func signed(t *testing.T, priv ed25519.PrivateKey, manifest Manifest) []byte {
	t.Helper()
	payload, err := json.Marshal(manifest)
	if err != nil {
		t.Fatal(err)
	}
	out, err := json.Marshal(Envelope{
		KeyID:     "k1",
		Signature: base64.StdEncoding.EncodeToString(ed25519.Sign(priv, payload)),
		Manifest:  base64.StdEncoding.EncodeToString(payload),
	})
	if err != nil {
		t.Fatal(err)
	}
	return out
}

func fixture(t *testing.T) (ed25519.PublicKey, ed25519.PrivateKey, Manifest) {
	t.Helper()
	pub, priv, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	return pub, priv, Manifest{
		Schema: 1, Channel: "stable", Version: "0.2.0", Expires: fixedNow.Add(time.Hour),
		Artifacts: map[string]Artifact{"macos-arm64": {
			URL: "https://example.test/c.dmg", SHA256: strings.Repeat("a", 64), Size: 10,
		}},
	}
}

func TestCheckReportsNewerVerifiedBuild(t *testing.T) {
	pub, priv, manifest := fixture(t)
	body := signed(t, priv, manifest)
	result, err := Check(context.Background(), Config{
		ManifestURL: "https://example.test/update.json", Client: fixtureClient(body, nil), Channel: "stable", Platform: "macos-arm64", Current: "0.1.9",
		Keys: map[string]ed25519.PublicKey{"k1": pub}, Now: func() time.Time { return fixedNow }, AllowHTTP: true,
	})
	if err != nil || !result.Available || result.Artifact == nil || result.Latest != "0.2.0" {
		t.Fatalf("result=%+v err=%v", result, err)
	}
}

func TestVerifyRejectsTamperUnknownKeyAndExpiry(t *testing.T) {
	pub, priv, manifest := fixture(t)
	keys := map[string]ed25519.PublicKey{"k1": pub}
	good := signed(t, priv, manifest)
	if _, err := Verify(good, keys, fixedNow); err != nil {
		t.Fatal(err)
	}
	var env Envelope
	_ = json.Unmarshal(good, &env)
	payload, _ := base64.StdEncoding.DecodeString(env.Manifest)
	env.Manifest = base64.StdEncoding.EncodeToString([]byte(strings.Replace(string(payload), "0.2.0", "9.9.9", 1)))
	tampered, _ := json.Marshal(env)
	if _, err := Verify(tampered, keys, fixedNow); err == nil {
		t.Fatal("tampered manifest accepted")
	}
	if _, err := Verify(good, map[string]ed25519.PublicKey{}, fixedNow); err == nil {
		t.Fatal("unknown key accepted")
	}
	if _, err := Verify(good, keys, fixedNow.Add(2*time.Hour)); err == nil {
		t.Fatal("expired manifest accepted")
	}
}

func TestNewer(t *testing.T) {
	cases := []struct {
		candidate, current string
		want               bool
	}{
		{"0.2.0", "0.1.9", true}, {"0.1.9", "0.2.0", false}, {"1.0.0", "1.0.0", false},
		{"1.0.0", "1.0.0-beta", true}, {"1.0.0-beta", "1.0.0", false}, {"v1.10.0", "1.9.9", true},
	}
	for _, c := range cases {
		got, err := Newer(c.candidate, c.current)
		if err != nil || got != c.want {
			t.Errorf("Newer(%s,%s)=%v,%v want %v", c.candidate, c.current, got, err, c.want)
		}
	}
	if _, err := Newer("1.0", "1.0.0"); err == nil {
		t.Error("malformed version accepted")
	}
}

func TestCheckRejectsNonHTTPSManifestURL(t *testing.T) {
	if _, err := Check(context.Background(), Config{ManifestURL: "http://example.test/m"}); err == nil {
		t.Fatal("plain HTTP accepted")
	}
}

func TestParseKeys(t *testing.T) {
	pub, _, _ := ed25519.GenerateKey(rand.Reader)
	keys, err := ParseKeys("k1:" + base64.StdEncoding.EncodeToString(pub))
	if err != nil || !keys["k1"].Equal(pub) {
		t.Fatalf("keys=%v err=%v", keys, err)
	}
	if _, err := ParseKeys("k1:bad"); err == nil {
		t.Fatal("bad key accepted")
	}
}

func TestSignRoundTripsThroughVerify(t *testing.T) {
	pub, priv, manifest := fixture(t)
	body, err := Sign(manifest, "k1", priv)
	if err != nil {
		t.Fatal(err)
	}
	got, err := Verify(body, map[string]ed25519.PublicKey{"k1": pub}, fixedNow)
	if err != nil || got.Version != manifest.Version {
		t.Fatalf("got=%+v err=%v", got, err)
	}
	if _, err := Sign(manifest, "k1", nil); err == nil {
		t.Fatal("empty key accepted")
	}
}
