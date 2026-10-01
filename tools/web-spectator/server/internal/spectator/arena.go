package spectator

import (
	"errors"
	"reflect"
	"time"
)

type assembly struct {
	arena Arena
	parts [][][4]int32
	seen  []bool
	count int
	last  time.Time
}

func (s *Store) acceptArena(part ArenaPart, now time.Time) error {
	if !validArena(part) {
		return errors.New("invalid spectator arena part")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if complete := s.arenas[part.ID]; complete != nil {
		// Complete IDs are immutable. Repeated exports need no new allocations.
		return nil
	}
	a := s.pending[part.ID]
	if a == nil {
		if len(s.arenas)+len(s.pending) >= MaxArenas && !s.evictArena(now) {
			return errors.New("spectator arena cache full")
		}
		a = &assembly{arena: Arena{ID: part.ID, Name: part.Name, Palette: part.Palette, Bounds: part.Bounds}, parts: make([][][4]int32, part.Parts), seen: make([]bool, part.Parts), last: now}
		s.pending[part.ID] = a
	} else if len(a.parts) != part.Parts || a.arena.Name != part.Name || a.arena.Bounds != part.Bounds || !reflect.DeepEqual(a.arena.Palette, part.Palette) {
		s.discardAssembly(part.ID)
		return errors.New("conflicting spectator arena metadata")
	}
	if a.seen[part.Part] {
		if !reflect.DeepEqual(a.parts[part.Part], part.Blocks) {
			s.discardAssembly(part.ID)
			return errors.New("conflicting spectator arena part")
		}
		return nil
	}
	if a.count+len(part.Blocks) > MaxBlocks || s.blockCount+len(part.Blocks) > MaxCachedBlocks {
		s.discardAssembly(part.ID)
		return errors.New("spectator arena voxel budget exceeded")
	}
	a.parts[part.Part], a.seen[part.Part], a.last = part.Blocks, true, now
	a.count += len(part.Blocks)
	s.blockCount += len(part.Blocks)
	for _, present := range a.seen {
		if !present {
			return nil
		}
	}
	blocks := make([][4]int32, 0, a.count)
	positions := make(map[[3]int32]struct{}, a.count)
	for _, group := range a.parts {
		for _, block := range group {
			position := [3]int32{block[0], block[1], block[2]}
			if _, exists := positions[position]; exists {
				s.discardAssembly(part.ID)
				return errors.New("duplicate spectator voxel")
			}
			positions[position] = struct{}{}
			blocks = append(blocks, block)
		}
	}
	a.arena.Blocks = blocks
	s.arenas[part.ID] = &a.arena
	s.arenaUsed[part.ID] = now
	delete(s.pending, part.ID)
	return nil
}

func (s *Store) discardAssembly(id string) {
	if a := s.pending[id]; a != nil {
		s.blockCount -= a.count
		delete(s.pending, id)
	}
}

// The caller owns the store mutex. Active geometry cannot be evicted.
func (s *Store) evictArena(now time.Time) bool {
	used := make(map[string]bool)
	for _, live := range s.matches {
		used[live.arena.ID] = true
	}
	oldestID := ""
	oldest := now
	for id, stamp := range s.arenaUsed {
		if !used[id] && (oldestID == "" || stamp.Before(oldest)) {
			oldestID, oldest = id, stamp
		}
	}
	for id, a := range s.pending {
		if oldestID == "" || a.last.Before(oldest) {
			oldestID, oldest = id, a.last
		}
	}
	if oldestID == "" {
		return false
	}
	if arena := s.arenas[oldestID]; arena != nil {
		s.blockCount -= len(arena.Blocks)
		delete(s.arenas, oldestID)
		delete(s.arenaUsed, oldestID)
	} else {
		s.discardAssembly(oldestID)
	}
	return true
}
