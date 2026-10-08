package catalog

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/imagecache"
	"github.com/sandertv/gophertunnel/minecraft/service/persona"
)

// TestProfileAvatarUsesDiscoveredService caches authored image bytes through a synthetic MC token.
func TestProfileAvatarUsesDiscoveredService(t *testing.T) {
	directory := t.TempDir()
	imageData := []byte("\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR")
	env := new(persona.Environment)
	if err := json.Unmarshal([]byte(`{"serviceUri":"https://persona.fixture.test"}`), env); err != nil {
		t.Fatal(err)
	}
	env.HTTPClient = &http.Client{Transport: roundTripFunc(func(request *http.Request) (*http.Response, error) {
		if request.Method != http.MethodGet || request.URL.Host != "persona.fixture.test" || request.URL.Path != "/api/v1.0/profile/xuid/123/image/avatar" || request.Header.Get("Accept") != "image/*" {
			t.Fatalf("unexpected avatar request: %s %s", request.Method, request.URL)
		}
		if request.Header.Get("Authorization") != "MCToken synthetic" {
			t.Fatal("avatar did not use the Minecraft service token")
		}
		return &http.Response{StatusCode: http.StatusOK, Header: http.Header{}, Body: io.NopCloser(bytes.NewReader(imageData))}, nil
	})}
	image, err := cacheProfileAvatar(context.Background(), env, fixedTokens{}, "123", directory)
	if err != nil {
		t.Fatal(err)
	}
	if filepath.Dir(image.Path) != directory || image.URL != "" {
		t.Fatalf("cached avatar = %+v", image)
	}
	data, err := os.ReadFile(image.Path)
	if err != nil || !bytes.Equal(data, imageData) {
		t.Fatalf("cached data mismatch: %v", err)
	}
	repeated, err := cacheProfileAvatar(context.Background(), env, fixedTokens{}, "123", directory)
	if err != nil || repeated.Path != image.Path {
		t.Fatalf("content-addressed cache changed: %+v %v", repeated, err)
	}
}

// TestProfileAvatarCacheHitSurvivesEviction keeps a reused avatar ahead of older artwork.
func TestProfileAvatarCacheHitSurvivesEviction(t *testing.T) {
	directory := t.TempDir()
	env := new(persona.Environment)
	if err := json.Unmarshal([]byte(`{"serviceUri":"https://persona.fixture.test"}`), env); err != nil {
		t.Fatal(err)
	}
	env.HTTPClient = &http.Client{Transport: roundTripFunc(func(*http.Request) (*http.Response, error) {
		data := []byte("\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR")
		return &http.Response{StatusCode: http.StatusOK, Header: http.Header{}, Body: io.NopCloser(bytes.NewReader(data))}, nil
	})}
	avatar, err := cacheProfileAvatar(context.Background(), env, fixedTokens{}, "123", directory)
	if err != nil {
		t.Fatal(err)
	}
	unusedPath := filepath.Join(directory, "unused-artwork.img")
	if err := os.WriteFile(unusedPath, []byte("unused artwork"), 0o600); err != nil {
		t.Fatal(err)
	}
	now := time.Now()
	for path, stamp := range map[string]time.Time{
		avatar.Path: now.Add(-2 * time.Hour),
		unusedPath:  now.Add(-time.Hour),
	} {
		if err := os.Chtimes(path, stamp, stamp); err != nil {
			t.Fatal(err)
		}
	}
	reused, err := cacheProfileAvatar(context.Background(), env, fixedTokens{}, "123", directory)
	if err != nil || reused.Path != avatar.Path {
		t.Fatalf("reused avatar = %+v, error = %v", reused, err)
	}
	imagecache.New(directory, imagecache.Config{MaxFiles: 1}).Prune()
	if _, err := os.Stat(avatar.Path); err != nil {
		t.Fatalf("recently reused avatar was evicted: %v", err)
	}
	if _, err := os.Stat(unusedPath); !os.IsNotExist(err) {
		t.Fatalf("older unused artwork survived eviction: %v", err)
	}
}

// TestProfileAvatarRejectsNonImage never publishes malformed service responses as artwork.
func TestProfileAvatarRejectsNonImage(t *testing.T) {
	directory := t.TempDir()
	env := new(persona.Environment)
	if err := json.Unmarshal([]byte(`{"serviceUri":"https://persona.fixture.test"}`), env); err != nil {
		t.Fatal(err)
	}
	env.HTTPClient = &http.Client{Transport: roundTripFunc(func(*http.Request) (*http.Response, error) {
		return &http.Response{StatusCode: http.StatusOK, Header: http.Header{}, Body: io.NopCloser(bytes.NewBufferString(`{"error":"bad image"}`))}, nil
	})}
	if _, err := cacheProfileAvatar(context.Background(), env, fixedTokens{}, "123", directory); err == nil {
		t.Fatal("JSON avatar accepted")
	}
	entries, err := os.ReadDir(directory)
	if err != nil || len(entries) != 0 {
		t.Fatalf("invalid avatar wrote cache files: %v", err)
	}
}
