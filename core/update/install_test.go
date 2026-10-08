package update

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

type fixtureTransport func(*http.Request) (*http.Response, error)

// RoundTrip serves signed release fixtures without opening a network connection.
func (f fixtureTransport) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }

// downloadFixture builds a signed release and an entirely in-memory HTTP transport.
func downloadFixture(t *testing.T, payload []byte) (Config, Manifest, ed25519.PrivateKey) {
	t.Helper()
	pub, private, manifest := fixture(t)
	sum := sha256.Sum256(payload)
	manifest.Artifacts["linux-amd64"] = Artifact{URL: "https://example.test/client.AppImage", SHA256: hex.EncodeToString(sum[:]), Size: int64(len(payload))}
	cfg := Config{ManifestURL: "https://example.test/update.json", Channel: "stable", Platform: "linux-amd64", Current: "0.1.0", Keys: map[string]ed25519.PublicKey{"k1": pub}, Now: func() time.Time { return fixedNow }}
	return cfg, manifest, private
}

// fixtureClient returns fixture bytes for both manifest and artifact requests.
func fixtureClient(envelope, payload []byte) *http.Client {
	return &http.Client{Transport: fixtureTransport(func(r *http.Request) (*http.Response, error) {
		data := payload
		if strings.HasSuffix(r.URL.Path, "update.json") {
			data = envelope
		}
		return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(bytes.NewReader(data)), Header: make(http.Header)}, nil
	})}
}

func TestDownloadRejectsVerificationFailuresAndRetries(t *testing.T) {
	payload := []byte("verified application")
	for _, kind := range []string{"hash", "size", "oversize", "expired", "channel", "valid"} {
		t.Run(kind, func(t *testing.T) {
			cfg, manifest, private := downloadFixture(t, payload)
			artifact := manifest.Artifacts[cfg.Platform]
			switch kind {
			case "hash":
				artifact.SHA256 = strings.Repeat("0", 64)
			case "size":
				artifact.Size++
			case "oversize":
				artifact.Size--
			case "expired":
				manifest.Expires = fixedNow.Add(-time.Second)
			case "channel":
				manifest.Channel = "nightly"
			}
			manifest.Artifacts[cfg.Platform] = artifact
			cfg.Client = fixtureClient(signed(t, private, manifest), payload)
			cache := t.TempDir()
			for attempt := 0; attempt < 2; attempt++ {
				result, err := Download(context.Background(), cfg, cache, nil)
				if kind == "valid" {
					if err != nil || result.State != "ready" {
						t.Fatalf("result=%+v err=%v", result, err)
					}
					if err := verifyFile(filepath.Join(result.Stage, "artifact"), artifact); err != nil {
						t.Fatal(err)
					}
				} else if err == nil {
					t.Fatal("invalid update staged")
				}
			}
			if kind != "valid" {
				files, _ := os.ReadDir(cache)
				if len(files) != 0 {
					t.Fatal("failed download left a stage")
				}
			}
		})
	}
}

func TestApplyWaitsReverifiesAndKeepsBackup(t *testing.T) {
	payload := []byte("new application")
	cfg, manifest, private := downloadFixture(t, payload)
	cfg.Client = fixtureClient(signed(t, private, manifest), payload)
	staged, err := Download(context.Background(), cfg, t.TempDir(), nil)
	if err != nil {
		t.Fatal(err)
	}
	target := filepath.Join(t.TempDir(), "client.AppImage")
	if err := os.WriteFile(target, []byte("old application"), 0o700); err != nil {
		t.Fatal(err)
	}
	install := ApplyConfig{Config: cfg, Stage: staged.Stage, Target: target}
	blocked := errors.New("session still running")
	if err := ApplyAfterExit(context.Background(), install, func(context.Context) error { return blocked }); !errors.Is(err, blocked) {
		t.Fatal(err)
	}
	old, _ := os.ReadFile(target)
	if string(old) != "old application" {
		t.Fatal("installed during session")
	}
	if err := ApplyAfterExit(context.Background(), install, func(context.Context) error { return nil }); err != nil {
		t.Fatal(err)
	}
	newFile, _ := os.ReadFile(target)
	backup, _ := os.ReadFile(target + ".previous")
	if !bytes.Equal(newFile, payload) || string(backup) != "old application" {
		t.Fatal("replacement or backup is wrong")
	}
	if err := os.WriteFile(filepath.Join(staged.Stage, "artifact"), []byte("corrupt"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := ApplyAfterExit(context.Background(), install, func(context.Context) error { return nil }); err == nil {
		t.Fatal("tampered stage installed")
	}
	install.Config.Now = func() time.Time { return manifest.Expires.Add(time.Second) }
	if err := ApplyAfterExit(context.Background(), install, func(context.Context) error { return nil }); err == nil {
		t.Fatal("expired stage installed")
	}
}

func TestReplacementRestoresPreviousOnPromotionFailure(t *testing.T) {
	dir := t.TempDir()
	target, candidate := filepath.Join(dir, "app"), filepath.Join(dir, "new")
	if err := os.WriteFile(target, []byte("old"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(candidate, []byte("new"), 0o600); err != nil {
		t.Fatal(err)
	}
	var calls []string
	err := replaceWithRollback(target, candidate, func(from, to string) error {
		calls = append(calls, from)
		if from == candidate {
			return errors.New("fixture promotion failure")
		}
		return os.Rename(from, to)
	})
	content, _ := os.ReadFile(target)
	if err == nil || string(content) != "old" || len(calls) != 3 {
		t.Fatalf("rollback failed: %s %v %v", content, calls, err)
	}
}

func TestArchiveRejectsTraversalAndEscapingLinks(t *testing.T) {
	for _, header := range []*tar.Header{
		{Name: "../escape", Typeflag: tar.TypeReg, Mode: 0o600},
		{Name: "Cinnabar.app/escape", Typeflag: tar.TypeSymlink, Linkname: "../../escape"},
		{Name: "Cinnabar.app/hard", Typeflag: tar.TypeLink, Linkname: "outside"},
	} {
		var data bytes.Buffer
		gzipWriter := gzip.NewWriter(&data)
		tarWriter := tar.NewWriter(gzipWriter)
		if err := tarWriter.WriteHeader(header); err != nil {
			t.Fatal(err)
		}
		_ = tarWriter.Close()
		_ = gzipWriter.Close()
		dir := t.TempDir()
		archive := filepath.Join(dir, "app.tar.gz")
		if err := os.WriteFile(archive, data.Bytes(), 0o600); err != nil {
			t.Fatal(err)
		}
		if _, err := extractApp(archive, dir); err == nil {
			t.Fatalf("unsafe header accepted: %+v", header)
		}
	}
}

// TestAppArchivePreservesExecutableAndContainedLink covers the release bundle layout.
func TestAppArchivePreservesExecutableAndContainedLink(t *testing.T) {
	var data bytes.Buffer
	gzipWriter := gzip.NewWriter(&data)
	writer := tar.NewWriter(gzipWriter)
	payload := []byte("fixture executable")
	for _, header := range []*tar.Header{
		{Name: "Cinnabar.app/Contents/MacOS/bedrock-client", Typeflag: tar.TypeReg, Mode: 0o755, Size: int64(len(payload))},
		{Name: "Cinnabar.app/Contents/MacOS/client-link", Typeflag: tar.TypeSymlink, Linkname: "bedrock-client"},
	} {
		if err := writer.WriteHeader(header); err != nil {
			t.Fatal(err)
		}
		if header.Typeflag == tar.TypeReg {
			if _, err := writer.Write(payload); err != nil {
				t.Fatal(err)
			}
		}
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	if err := gzipWriter.Close(); err != nil {
		t.Fatal(err)
	}
	dir := t.TempDir()
	archive := filepath.Join(dir, "app.tar.gz")
	if err := os.WriteFile(archive, data.Bytes(), 0o600); err != nil {
		t.Fatal(err)
	}
	bundle, err := extractApp(archive, dir)
	if err != nil {
		t.Fatal(err)
	}
	got, err := os.ReadFile(filepath.Join(bundle, "Contents", "MacOS", "client-link"))
	if err != nil || !bytes.Equal(got, payload) {
		t.Fatalf("link=%q err=%v", got, err)
	}
}

// TestRestartDropsOldAppImageMount prevents loader paths from referencing a deleted image.
func TestRestartDropsOldAppImageMount(t *testing.T) {
	got := cleanAppImageEnvironment([]string{"HOME=/home/user", "APPIMAGE=old", "APPDIR=/old/mount", "OWD=/old", "LD_LIBRARY_PATH=/old/lib", "LD_PRELOAD=/old/lib.so", "PATH=/usr/bin"})
	if strings.Join(got, ";") != "HOME=/home/user;PATH=/usr/bin" {
		t.Fatalf("environment=%v", got)
	}
}

// TestArchiveRejectsChainedTraversal covers links that are safe only before resolution.
func TestArchiveRejectsChainedTraversal(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "outside"), []byte("untouched"), 0o600); err != nil {
		t.Fatal(err)
	}
	var data bytes.Buffer
	compressed := gzip.NewWriter(&data)
	writer := tar.NewWriter(compressed)
	for _, header := range []*tar.Header{
		{Name: "Cinnabar.app/sub/deep/a", Typeflag: tar.TypeSymlink, Linkname: ".."},
		{Name: "Cinnabar.app/sub/b", Typeflag: tar.TypeSymlink, Linkname: "deep/a/../../outside"},
	} {
		if err := writer.WriteHeader(header); err != nil {
			t.Fatal(err)
		}
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	if err := compressed.Close(); err != nil {
		t.Fatal(err)
	}
	archive := filepath.Join(dir, "app.tar.gz")
	if err := os.WriteFile(archive, data.Bytes(), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := extractApp(archive, dir); err == nil || !strings.Contains(err.Error(), "escapes") {
		t.Fatalf("chained traversal: %v", err)
	}
	content, err := os.ReadFile(filepath.Join(dir, "outside"))
	if err != nil || string(content) != "untouched" {
		t.Fatalf("outside=%q err=%v", content, err)
	}
}
