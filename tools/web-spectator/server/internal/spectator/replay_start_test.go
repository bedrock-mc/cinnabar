package spectator

import (
	"encoding/json"
	"testing"
	"time"
)

func TestReplayOpeningValidatesHistoryWithoutPublishingItLive(t *testing.T) {
	now := time.Now().UTC()
	store := NewStore()
	if err := accept(t, store, ArenaSubject, arenaPart("arena", 0, 1, 0), now); err != nil {
		t.Fatal(err)
	}
	started := now.Add(-2 * time.Second)
	first := frameAt("match", "arena", started)
	first.RoundActive = false
	first.MatchStartedAt = &started
	second := first
	second.RoundActive = true
	second.UpdatedAt = started.Add(450 * time.Millisecond)
	opening := ReplayStart{Version: Version, ID: first.ID, ArenaID: first.ArenaID, Frames: []Frame{first, second}}
	validate := func(value ReplayStart) error {
		data, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		_, _, err = store.ValidateReplayStart(data, now)
		return err
	}
	if err := validate(opening); err != nil {
		t.Fatal(err)
	}
	if len(store.List(now)) != 0 || store.Lookup("match") != nil {
		t.Fatal("historical opening entered the live spectator cache")
	}
	gap := opening
	gap.Frames = append([]Frame(nil), opening.Frames...)
	gap.Frames[1].UpdatedAt = started.Add(MaxReplayStartGap + time.Millisecond)
	if validate(gap) == nil {
		t.Fatal("missing opening samples were accepted")
	}
	incomplete := opening
	incomplete.Frames = append([]Frame(nil), opening.Frames...)
	incomplete.Frames[1].Incomplete = true
	if validate(incomplete) == nil {
		t.Fatal("incomplete opening was accepted")
	}
	stale := opening
	stale.Frames = append([]Frame(nil), opening.Frames...)
	old := now.Add(-MaxReplayStartSpan - time.Millisecond)
	stale.Frames[0].UpdatedAt = old
	stale.Frames[0].MatchStartedAt = &old
	if validate(stale) == nil {
		t.Fatal("opening older than the bounded capture window was accepted")
	}
	if err := accept(t, store, FrameSubject, frameAt("match", "arena", now), now); err != nil {
		t.Fatal(err)
	}
	if validate(opening) == nil {
		t.Fatal("late opening was accepted after the match went live")
	}
}
