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
