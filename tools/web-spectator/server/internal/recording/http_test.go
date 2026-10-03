package recording

import (
	"bytes"
	"encoding/json"
	"errors"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/replay"
)

func TestFramesEvictedAfterManifestLookupReturns404(t *testing.T) {
	directory := t.TempDir()
	store, err := replay.Open(replay.Config{Directory: directory})
	if err != nil {
		t.Fatal(err)
	}
	recording, err := store.Begin(replay.Metadata{ID: "old", StartedAt: time.Unix(1, 0)}, nil)
	if err != nil {
		t.Fatal(err)
	}
	for _, instant := range []int64{0, 1000} {
		if err := recording.Append(instant, json.RawMessage(`{"version":1,"players":[]}`)); err != nil {
			t.Fatal(err)
		}
	}
	manifest, err := recording.Finish()
	if err != nil {
		t.Fatal(err)
	}
	store.Close()

	store, err = replay.Open(replay.Config{Directory: directory, MaxBytes: manifest.FileBytes + 1})
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	stale, err := store.Get("old")
	if err != nil {
		t.Fatal(err)
	}
	// A quota write can remove the replay between ServeHTTP's metadata lookup
	// and its first chunk read. The preloaded manifest remains in the handler.
	if manifest.FileBytes < 64 {
		t.Fatal("fixture replay is unexpectedly small")
	}
	if _, err := store.PutAsset("skin", bytes.Repeat([]byte{'x'}, int(manifest.FileBytes/2))); err != nil {
		t.Fatal(err)
	}
	if _, err := store.Get("old"); !errors.Is(err, replay.ErrNotFound) {
		t.Fatalf("quota did not evict old replay: %v", err)
	}

	response := httptest.NewRecorder()
	request := httptest.NewRequest("GET", "/api/replays/old/frames?fromMs=0&toMs=1000", nil)
	NewHTTP(store).frames(response, request, stale, view(stale), 0, 1000)
	if response.Code != 404 {
		t.Fatalf("evicted replay returned %d: %s", response.Code, response.Body.String())
	}
	var failure struct {
		Error string `json:"error"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &failure); err != nil || failure.Error == "" {
		t.Fatalf("evicted replay response is not a complete error: %s (%v)", response.Body.String(), err)
	}
}
