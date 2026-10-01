package spectator

import (
	"encoding/json"
	"fmt"
	"sync"
	"testing"
	"time"
)

func arenaPart(id string, part, parts int, x int32) ArenaPart {
	return ArenaPart{Version: Version, Arena: Arena{ID: id, Name: "Test arena", Palette: []PaletteEntry{{Name: "minecraft:air", States: map[string]any{}}, {Name: "minecraft:stone", States: map[string]any{}}}, Bounds: [6]int32{0, 0, 0, 31, 15, 31}, Blocks: [][4]int32{{x, 0, 0, 1}}}, Part: part, Parts: parts}
}

func frameAt(id, arena string, now time.Time) Frame {
	return Frame{Version: Version, ID: id, ArenaID: arena, Mode: "boxing", RoundActive: true, UpdatedAt: now, Players: []Player{{ID: "one", Name: "One", Position: [3]float64{1, 1, 1}, Health: 20, MaxHealth: 20}, {ID: "two", Name: "Two", Team: 1, Position: [3]float64{2, 1, 1}, Health: 20, MaxHealth: 20}}, TeamWins: []int{0, 0}}
}

func accept(t *testing.T, s *Store, subject string, value any, now time.Time) error {
	t.Helper()
	data, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	return s.Accept(subject, data, now)
}

func liveStore(t *testing.T, now time.Time) *Store {
	t.Helper()
	s := NewStore()
	if err := accept(t, s, ArenaSubject, arenaPart("arena", 0, 1, 0), now); err != nil {
		t.Fatal(err)
	}
	if err := accept(t, s, FrameSubject, frameAt("match", "arena", now), now); err != nil {
		t.Fatal(err)
	}
	return s
}

func TestGeometryMustBeCompleteBeforePublication(t *testing.T) {
	now := time.Now()
	s := NewStore()
	if err := accept(t, s, ArenaSubject, arenaPart("arena", 1, 2, 1), now); err != nil {
		t.Fatal(err)
	}
	if err := accept(t, s, FrameSubject, frameAt("match", "arena", now), now); err == nil {
		t.Fatal("incomplete geometry admitted a match")
	}
	if len(s.List(now)) != 0 {
		t.Fatal("incomplete match exposed")
	}
	if err := accept(t, s, ArenaSubject, arenaPart("arena", 0, 2, 0), now); err != nil {
		t.Fatal(err)
	}
	if err := accept(t, s, FrameSubject, frameAt("match", "arena", now), now); err != nil {
		t.Fatal(err)
	}
	if len(s.List(now)) != 1 || len(s.arenas["arena"].Blocks) != 2 {
		t.Fatal("complete geometry did not become available")
	}
}

func TestConflictingPartsAndDuplicateVoxelsAreRejected(t *testing.T) {
	now := time.Now()
	for _, conflict := range []string{"part", "metadata", "voxel"} {
		t.Run(conflict, func(t *testing.T) {
			s := NewStore()
			if err := accept(t, s, ArenaSubject, arenaPart("arena", 0, 2, 0), now); err != nil {
				t.Fatal(err)
			}
			second := arenaPart("arena", 1, 2, 1)
			switch conflict {
			case "part":
				second.Part = 0
			case "metadata":
				second.Name = "Other"
			case "voxel":
				second.Blocks[0][0] = 0
			}
			if err := accept(t, s, ArenaSubject, second, now); err == nil {
				t.Fatal("conflicting terrain was accepted")
			}
			if len(s.arenas) != 0 || len(s.pending) != 0 || s.blockCount != 0 {
				t.Fatal("rejected assembly retained state")
			}
		})
	}
}

func TestInclusiveBoundsAndMalformedExports(t *testing.T) {
	now := time.Now()
	s := NewStore()
	part := arenaPart("edge", 0, 1, 31)
	if err := accept(t, s, ArenaSubject, part, now); err != nil {
		t.Fatalf("inclusive maximum rejected: %v", err)
	}
	for _, change := range []func(*ArenaPart){func(p *ArenaPart) { p.Blocks[0][0] = 32 }, func(p *ArenaPart) { p.Blocks[0][3] = 0 }, func(p *ArenaPart) { p.Blocks[0][3] = 10 }, func(p *ArenaPart) { p.Parts = MaxParts + 1 }, func(p *ArenaPart) { p.Palette[1].States["wrong"] = []int{1} }, func(p *ArenaPart) { p.Bounds[3] = 1024 }} {
		bad := arenaPart("bad", 0, 1, 0)
		change(&bad)
		if err := accept(t, s, ArenaSubject, bad, now); err == nil {
			t.Fatal("malformed arena accepted")
		}
	}
	if err := s.Accept(ArenaSubject, make([]byte, MaxMessage+1), now); err == nil {
		t.Fatal("oversized message admitted")
	}
	if err := s.Accept(FrameSubject, []byte(`{"version":1`), now); err == nil {
		t.Fatal("truncated frame admitted")
	}
}

func TestClosedStaleAndReorderedFramesStayUnavailable(t *testing.T) {
	now := time.Now()
	s := liveStore(t, now)
	live := s.Lookup("match")
	sub := live.Subscribe(now)
	if sub == nil {
		t.Fatal("subscription refused")
	}
	s.Close("match", now)
	select {
	case <-sub.Closed:
	default:
		t.Fatal("closure did not notify viewer")
	}
	if _, ok := live.Snapshot(now); ok {
		t.Fatal("closed match still visible through existing handle")
	}
	if err := accept(t, s, FrameSubject, frameAt("match", "arena", now), now); err == nil {
		t.Fatal("late frame reopened a closed match")
	}
	sub.Cancel()
	s = liveStore(t, now)
	if _, ok := s.Lookup("match").Snapshot(now.Add(Freshness + time.Nanosecond)); ok {
		t.Fatal("stale frame visible")
	}
	s.Sweep(now.Add(Freshness + time.Second))
	if s.Lookup("match") != nil || len(s.List(now.Add(Freshness+time.Second))) != 0 {
		t.Fatal("stale match retained")
	}
	if err := accept(t, s, FrameSubject, frameAt("old", "arena", now.Add(-Freshness-time.Nanosecond)), now); err == nil {
		t.Fatal("stale incoming frame admitted")
	}
	if err := accept(t, s, FrameSubject, frameAt("future", "arena", now.Add(2*time.Second)), now); err == nil {
		t.Fatal("future frame admitted")
	}
}

func TestCacheBoundsAndActiveArenaRetention(t *testing.T) {
	now := time.Now()
	s := liveStore(t, now)
	for i := 1; i < MaxArenas+4; i++ {
		id := fmt.Sprintf("arena-%d", i)
		if err := accept(t, s, ArenaSubject, arenaPart(id, 0, 1, 0), now.Add(time.Duration(i)*time.Millisecond)); err != nil {
			t.Fatal(err)
		}
	}
	if len(s.arenas) != MaxArenas || s.arenas["arena"] == nil {
		t.Fatal("active geometry evicted or arena bound exceeded")
	}
	for i := 1; i < MaxMatches; i++ {
		if err := accept(t, s, FrameSubject, frameAt(fmt.Sprintf("match-%d", i), "arena", now), now); err != nil {
			t.Fatal(err)
		}
	}
	if err := accept(t, s, FrameSubject, frameAt("overflow", "arena", now), now); err == nil {
		t.Fatal("match bound exceeded")
	}
	p := arenaPart("unfinished", 0, 2, 0)
	if err := accept(t, s, ArenaSubject, p, now); err != nil {
		t.Fatal(err)
	}
	s.Sweep(now.Add(31 * time.Second))
	if len(s.pending) != 0 {
		t.Fatal("incomplete geometry failed to expire")
	}
}

func TestConcurrentClosePreventsSubsequentWrites(t *testing.T) {
	now := time.Now()
	s := liveStore(t, now)
	live := s.Lookup("match")
	entered, unblock, closed := make(chan struct{}), make(chan struct{}), make(chan struct{})
	go func() {
		_, _ = live.WithCurrent(now, func(Frame, *Arena) error { close(entered); <-unblock; return nil })
	}()
	<-entered
	go func() { s.Close("match", now); close(closed) }()
	close(unblock)
	<-closed
	if ok, _ := live.WithCurrent(now, func(Frame, *Arena) error { t.Error("wrote after match closed"); return nil }); ok {
		t.Fatal("closed write accepted")
	}
	var group sync.WaitGroup
	for i := 0; i < 8; i++ {
		group.Add(1)
		go func() {
			defer group.Done()
			for j := 0; j < 100; j++ {
				_ = accept(t, s, FrameSubject, frameAt("match", "arena", now), now)
				s.List(now)
				s.Sweep(now)
				s.Close("match", now)
			}
		}()
	}
	group.Wait()
}

func TestExpiryRechecksFreshnessAndMatchIdentity(t *testing.T) {
	now := time.Now()
	s := liveStore(t, now)
	previous := s.Lookup("match")
	later := now.Add(Freshness + time.Second)
	// A fresh export arriving after the sweep captures its candidate must survive.
	if err := accept(t, s, FrameSubject, frameAt("match", "arena", later), later); err != nil {
		t.Fatal(err)
	}
	s.expire("match", previous, later)
	if s.Lookup("match") != previous {
		t.Fatal("fresh match expired from an earlier candidate")
	}
	s.Sweep(later.Add(Freshness + time.Second))
	if s.Lookup("match") != nil {
		t.Fatal("stale match did not expire")
	}
	reopenedAt := later.Add(Freshness + time.Second)
	if err := accept(t, s, FrameSubject, frameAt("match", "arena", reopenedAt), reopenedAt); err != nil {
		t.Fatal(err)
	}
	s.expire("match", previous, reopenedAt)
	if s.Lookup("match") == nil {
		t.Fatal("old handle expired a replacement match")
	}
	if _, ok := previous.Snapshot(reopenedAt); ok {
		t.Fatal("old subscription reopened")
	}
}
