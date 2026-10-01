package spectator

import (
	"errors"
	"sort"
	"sync"
	"time"
)

// Store owns bounded, ephemeral game exports; it never persists identities or geometry.
type Store struct {
	mu           sync.Mutex
	arenas       map[string]*Arena
	arenaUsed    map[string]time.Time
	pending      map[string]*assembly
	matches      map[string]*Live
	closed       map[string]time.Time
	blockCount   int
	blockedUntil time.Time
}

func NewStore() *Store {
	return &Store{arenas: make(map[string]*Arena), arenaUsed: make(map[string]time.Time), pending: make(map[string]*assembly), matches: make(map[string]*Live), closed: make(map[string]time.Time)}
}

func (s *Store) Accept(subject string, data []byte, now time.Time) error {
	switch subject {
	case ArenaSubject:
		var part ArenaPart
		if err := decode(data, &part); err != nil {
			return err
		}
		return s.acceptArena(part, now)
	case FrameSubject:
		var frame Frame
		if err := decode(data, &frame); err != nil {
			return err
		}
		if !validFrame(frame, now) {
			return errors.New("invalid or stale spectator frame")
		}
		return s.acceptFrame(frame, now)
	case ClosedSubject:
		var closed Closed
		if err := decode(data, &closed); err != nil {
			return err
		}
		if closed.Version != Version || !ValidID(closed.ID) || !fresh(closed.UpdatedAt, now) {
			return errors.New("invalid spectator close")
		}
		s.Close(closed.ID, now)
		return nil
	default:
		return errors.New("unknown spectator subject")
	}
}

func (s *Store) acceptFrame(frame Frame, now time.Time) error {
	s.mu.Lock()
	if _, closed := s.closed[frame.ID]; closed {
		s.mu.Unlock()
		return errors.New("spectator match already closed")
	}
	arena := s.arenas[frame.ArenaID]
	if arena == nil {
		s.mu.Unlock()
		return errors.New("spectator arena incomplete")
	}
	live := s.matches[frame.ID]
	if live == nil {
		if len(s.matches) >= MaxMatches {
			s.mu.Unlock()
			return errors.New("spectator match cache full")
		}
		if now.Before(s.blockedUntil) {
			s.mu.Unlock()
			return errors.New("spectator closure ledger saturated")
		}
		live = &Live{id: frame.ID, arena: arena, frame: frame, active: true, watchers: make(map[*Subscription]struct{})}
		s.matches[frame.ID] = live
	} else if live.arena.ID != frame.ArenaID {
		s.mu.Unlock()
		return errors.New("spectator arena changed within match")
	}
	s.arenaUsed[arena.ID] = now
	s.mu.Unlock()
	live.update(frame)
	return nil
}

func (s *Store) Lookup(id string) *Live {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.matches[id]
}

func (s *Store) List(now time.Time) []Frame {
	var frames []Frame
	_ = s.WithList(now, func(current []Frame) error { frames = current; return nil })
	return frames
}

// WithList keeps catalog consent and freshness valid through a bounded write.
// The callback must not call Store methods and must enforce a short write deadline.
func (s *Store) WithList(now time.Time, write func([]Frame) error) error {
	s.mu.Lock()
	live := make([]*Live, 0, len(s.matches))
	for _, match := range s.matches {
		live = append(live, match)
	}
	s.mu.Unlock()
	// Stable ordering avoids deadlocks between simultaneous catalog requests.
	sort.Slice(live, func(i, j int) bool { return live[i].id < live[j].id })
	for _, match := range live {
		match.mu.Lock()
	}
	defer func() {
		for i := len(live) - 1; i >= 0; i-- {
			live[i].mu.Unlock()
		}
	}()
	frames := make([]Frame, 0, len(live))
	for _, match := range live {
		if match.active && fresh(match.frame.UpdatedAt, now) {
			frames = append(frames, match.frame)
		}
	}
	sort.Slice(frames, func(i, j int) bool { return frames[i].ID < frames[j].ID })
	return write(frames)
}

func (s *Store) Close(id string, now time.Time) {
	s.mu.Lock()
	live := s.matches[id]
	delete(s.matches, id)
	// A short tombstone is enough to reject every still-fresh reordered frame.
	for closedID, stamp := range s.closed {
		if now.Sub(stamp) > 2*Freshness {
			delete(s.closed, closedID)
		}
	}
	if len(s.closed) < 1024 {
		s.closed[id] = now
	} else {
		s.blockedUntil = now.Add(2 * Freshness)
	}
	s.mu.Unlock()
	if live != nil {
		live.close()
	}
}

func (s *Store) Sweep(now time.Time) {
	s.mu.Lock()
	live := make(map[string]*Live, len(s.matches))
	for id, match := range s.matches {
		live[id] = match
	}
	for id, stamp := range s.closed {
		if now.Sub(stamp) > 2*Freshness {
			delete(s.closed, id)
		}
	}
	for id, a := range s.pending {
		if now.Sub(a.last) > 30*time.Second {
			s.discardAssembly(id)
		}
	}
	s.mu.Unlock()
	for id, match := range live {
		s.expire(id, match, now)
	}
}

func (s *Store) expire(id string, match *Live, now time.Time) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.matches[id] != match {
		return
	}
	match.mu.Lock()
	defer match.mu.Unlock()
	if match.active && fresh(match.frame.UpdatedAt, now) {
		return
	}
	match.deactivate()
	delete(s.matches, id)
	// Expiry is temporary unavailability, not revoked consent: a new fresh frame
	// may reopen the match, while old subscribers retain their closed handle.
}

func (s *Store) CloseAll(now time.Time) {
	s.mu.Lock()
	ids := make([]string, 0, len(s.matches))
	for id := range s.matches {
		ids = append(ids, id)
	}
	s.mu.Unlock()
	for _, id := range ids {
		s.Close(id, now)
	}
}
