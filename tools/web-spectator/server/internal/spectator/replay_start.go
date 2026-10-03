package spectator

import (
	"errors"
	"time"
)

const (
	MaxReplayStartFrames = 200
	MaxReplayStartBytes  = 512 << 10
	MaxReplayStartSpan   = 10 * time.Second
	MaxReplayStartGap    = 500 * time.Millisecond
)

// ValidateReplayStart reuses live frame validation while allowing only the
// bounded historical window. It never inserts those frames into the live cache.
func (s *Store) ValidateReplayStart(data []byte, now time.Time) (ReplayStart, *Arena, error) {
	var start ReplayStart
	if len(data) > MaxReplayStartBytes || decode(data, &start) != nil || start.Version != Version || !ValidID(start.ID) || !ValidID(start.ArenaID) || len(start.Frames) < 1 || len(start.Frames) > MaxReplayStartFrames {
		return ReplayStart{}, nil, errors.New("invalid replay opening")
	}
	first := start.Frames[0]
	if first.MatchStartedAt == nil || first.MatchStartedAt.IsZero() || first.RoundActive || first.UpdatedAt.Sub(*first.MatchStartedAt) > time.Second || now.Sub(first.UpdatedAt) > MaxReplayStartSpan {
		return ReplayStart{}, nil, errors.New("replay opening was not captured before the round")
	}
	previous := first.UpdatedAt
	for index, frame := range start.Frames {
		if frame.ID != start.ID || frame.ArenaID != start.ArenaID || frame.MatchStartedAt == nil || !frame.MatchStartedAt.Equal(*first.MatchStartedAt) || frame.Mode != first.Mode || frame.Ranked != first.Ranked || frame.Incomplete || !validFrameAge(frame, now, MaxReplayStartSpan) || frame.UpdatedAt.Sub(first.UpdatedAt) > MaxReplayStartSpan || frame.skinIDPresent() || len(frame.Players) != len(first.Players) {
			return ReplayStart{}, nil, errors.New("invalid replay opening frame")
		}
		for playerIndex, player := range frame.Players {
			if player.ID != first.Players[playerIndex].ID || player.Team != first.Players[playerIndex].Team {
				return ReplayStart{}, nil, errors.New("replay fighters changed during opening")
			}
		}
		if index > 0 && (frame.UpdatedAt.Before(previous) || frame.UpdatedAt.Sub(previous) > MaxReplayStartGap) {
			return ReplayStart{}, nil, errors.New("incomplete replay opening timeline")
		}
		previous = frame.UpdatedAt
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.arenas[start.ArenaID] == nil || s.matches[start.ID] != nil || now.Before(s.blockedUntil) {
		return ReplayStart{}, nil, errors.New("replay arena unavailable or match already live")
	}
	if _, closed := s.closed[start.ID]; closed {
		return ReplayStart{}, nil, errors.New("replay match already closed")
	}
	return start, s.arenas[start.ArenaID], nil
}

func (frame Frame) skinIDPresent() bool {
	for _, player := range frame.Players {
		if player.SkinID != "" || player.AppearanceID != "" {
			return true
		}
	}
	return false
}
