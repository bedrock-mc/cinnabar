package replay_test

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/replay"
)

func openStore(t *testing.T, dir string, limit int64) *replay.Store {
	t.Helper()
	s, err := replay.Open(replay.Config{Directory: dir, MaxBytes: limit})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(s.Close)
	return s
}

func record(t *testing.T, s *replay.Store, id string, started time.Time, assets []string) replay.Manifest {
	t.Helper()
	r, err := s.Begin(replay.Metadata{ID: id, StartedAt: started, Detail: json.RawMessage(`{"mode":"NoDebuff"}`)}, assets)
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 80; i++ {
		payload := json.RawMessage(fmt.Sprintf(`{"players":[{"name":"First","position":[%.3f,80,4],"health":20,"yaw":%d,"skinId":"stable","inventory":[1,2,3,4,5,6,7,8,9]},{"name":"Second","position":[4,80,%.3f],"health":19}],"events":[]}`, float64(i)*.12, i, float64(i)*.09))
		if err = r.Append(int64(i)*50, payload); err != nil {
			t.Fatal(err)
		}
	}
	m, err := r.Finish()
	if err != nil {
		t.Fatal(err)
	}
	return m
}

func TestSeekAndRestartKeepCompleteSnapshots(t *testing.T) {
	dir := t.TempDir()
	s := openStore(t, dir, 0)
	asset, err := s.PutAsset("arena", []byte("immutable arena"))
	if err != nil {
		t.Fatal(err)
	}
	m := record(t, s, "match-one", time.Now(), []string{asset})
	if len(m.Chunks) != 4 || m.DurationMS != 3950 {
		t.Fatalf("unexpected index: %+v", m.Chunks)
	}
	frames, err := s.ReadChunk(m.Metadata.ID, m.ChunkAt(2500))
	if err != nil {
		t.Fatal(err)
	}
	if frames[0].TimeMS != 2000 || !bytes.Contains(frames[0].Payload, []byte("inventory")) {
		t.Fatal("seek did not include a complete snapshot")
	}
	partial, err := s.Begin(replay.Metadata{ID: "interrupted"}, nil)
	if err != nil {
		t.Fatal(err)
	}
	if err = partial.Append(0, json.RawMessage(`{"unused":true}`)); err != nil {
		t.Fatal(err)
	}
	if err = partial.Flush(); err != nil {
		t.Fatal(err)
	}
	s.Close()
	// Simulate a crash remnant rather than relying only on graceful Close.
	if err = os.MkdirAll(filepath.Join(dir, "active", "crash"), 0700); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(dir, "active", "crash", "partial.zst"), []byte("partial"), 0600); err != nil {
		t.Fatal(err)
	}
	s = openStore(t, dir, 0)
	if len(s.List()) != 1 {
		t.Fatal("partial recording became visible")
	}
	if _, err = s.ReadChunk(m.Metadata.ID, 2); err != nil {
		t.Fatal(err)
	}
	actual, err := s.ReadAsset(m.Metadata.ID, asset)
	if err != nil || string(actual) != "immutable arena" {
		t.Fatalf("asset recovery: %q %v", actual, err)
	}
	used, _ := s.Stats()
	assetInfo, err := os.Stat(filepath.Join(dir, "assets", asset))
	if err != nil {
		t.Fatal(err)
	}
	if used != m.FileBytes+assetInfo.Size() {
		t.Fatalf("quota accounting %d != %d", used, m.FileBytes+assetInfo.Size())
	}
}

func TestOldestEvictionPreservesSharedAssets(t *testing.T) {
	dir := t.TempDir()
	s := openStore(t, dir, 0)
	asset, err := s.PutAsset("arena", []byte("shared terrain"))
	if err != nil {
		t.Fatal(err)
	}
	old := record(t, s, "old", time.Unix(1, 0), []string{asset})
	assetInfo, err := os.Stat(filepath.Join(dir, "assets", asset))
	if err != nil {
		t.Fatal(err)
	}
	s.Close()
	// Enough for two recordings, not three; reserve variance in ID/timestamps.
	limit := old.FileBytes*2 + assetInfo.Size() + 100
	s = openStore(t, dir, limit)
	record(t, s, "middle", time.Unix(2, 0), []string{asset})
	record(t, s, "newest", time.Unix(3, 0), []string{asset})
	if _, err = s.Get("old"); !errors.Is(err, replay.ErrNotFound) {
		t.Fatalf("old replay retained: %v", err)
	}
	if _, err = s.ReadAsset("newest", asset); err != nil {
		t.Fatal("shared arena was removed", err)
	}
	used, _ := s.Stats()
	if used > limit {
		t.Fatal("quota exceeded", used, limit)
	}
}

func TestConcurrentActiveWritesCannotExceedQuota(t *testing.T) {
	s := openStore(t, t.TempDir(), 1400)
	var wg sync.WaitGroup
	for i := 0; i < 8; i++ {
		r, err := s.Begin(replay.Metadata{ID: fmt.Sprintf("active-%d", i)}, nil)
		if err != nil {
			t.Fatal(err)
		}
		wg.Add(1)
		go func(r *replay.Recording) {
			defer wg.Done()
			for j := 0; j < 20; j++ {
				if err := r.Append(int64(j)*1000, json.RawMessage(fmt.Sprintf(`{"value":%d,"payload":"nonempty snapshot"}`, j))); err != nil {
					return
				}
				if err := r.Flush(); err != nil {
					if !errors.Is(err, replay.ErrQuota) {
						t.Error(err)
					}
					return
				}
			}
			r.Abort()
		}(r)
	}
	wg.Wait()
	used, limit := s.Stats()
	if used > limit {
		t.Fatalf("oversubscribed %d > %d", used, limit)
	}
}

func TestLateSkinReservationAndExclusiveOwner(t *testing.T) {
	dir := t.TempDir()
	s := openStore(t, dir, 0)
	if other, err := replay.Open(replay.Config{Directory: dir}); err == nil {
		other.Close()
		t.Fatal("two repositories acquired the same directory")
	}
	r, err := s.Begin(replay.Metadata{ID: "late-skin"}, nil)
	if err != nil {
		t.Fatal(err)
	}
	hash, err := s.PutAsset("skin", []byte("skin png"))
	if err != nil {
		t.Fatal(err)
	}
	if err = r.AddAsset(hash); err != nil {
		t.Fatal(err)
	}
	if err = r.Append(0, json.RawMessage(`{"player":"with skin"}`)); err != nil {
		t.Fatal(err)
	}
	if _, err = r.Finish(); err != nil {
		t.Fatal(err)
	}
	if _, err = s.ReadAsset("late-skin", hash); err != nil {
		t.Fatal(err)
	}
}

func TestMovingSnapshotsCompressAndSeekWithoutWholeReplay(t *testing.T) {
	s := openStore(t, t.TempDir(), 0)
	m := record(t, s, "moving", time.Now(), nil)
	var raw, compressed int64
	for _, chunk := range m.Chunks {
		frames, err := s.ReadChunk(m.Metadata.ID, chunk.Index)
		if err != nil {
			t.Fatal(err)
		}
		for _, frame := range frames {
			data, _ := json.Marshal(frame)
			raw += int64(len(data) + 1)
		}
		compressed += chunk.Bytes
	}
	// This measures a changing match fixture, not identical repeated bytes.
	if compressed*3 >= raw {
		t.Fatalf("weak compression: %d compressed / %d raw", compressed, raw)
	}
	t.Logf("moving snapshot fixture: %d raw, %d compressed, %.1fx reduction", raw, compressed, float64(raw)/float64(compressed))
}

func TestArenaCompressionPreservesOriginalIdentityAndDeduplicates(t *testing.T) {
	dir := t.TempDir()
	s := openStore(t, dir, 0)
	var arena bytes.Buffer
	arena.WriteString(`{"blocks":[`)
	for i := 0; i < 4096; i++ {
		if i > 0 {
			arena.WriteByte(',')
		}
		fmt.Fprintf(&arena, `{"x":%d,"y":64,"z":%d,"runtimeId":1}`, i%64, i/64)
	}
	arena.WriteString(`]}`)
	hash, err := s.PutAsset("arena", arena.Bytes())
	if err != nil {
		t.Fatal(err)
	}
	used, _ := s.Stats()
	same, err := s.PutAsset("arena", arena.Bytes())
	if err != nil || same != hash {
		t.Fatal("identity changed", err)
	}
	again, _ := s.Stats()
	if used != again {
		t.Fatal("asset was duplicated")
	}
	if used*3 >= int64(arena.Len()) {
		t.Fatalf("weak arena compression: %d / %d", used, arena.Len())
	}
	record(t, s, "arena", time.Now(), []string{hash, same})
	original, err := s.ReadAsset("arena", hash)
	if err != nil || !bytes.Equal(original, arena.Bytes()) {
		t.Fatal("arena bytes changed", err)
	}
	s.Close()
	s = openStore(t, dir, 0)
	original, err = s.ReadAsset("arena", hash)
	if err != nil || !bytes.Equal(original, arena.Bytes()) {
		t.Fatal("compressed asset recovery failed", err)
	}
	t.Logf("terrain fixture: %d original, %d stored, %.1fx reduction", arena.Len(), used, float64(arena.Len())/float64(used))
}
