package experience

import (
	"encoding/json"
	"errors"
	"math"
	"os"
)

// worldGenerationFile persists the next opening generation before any handle is published.
const worldGenerationFile = "_worlds.json"

// worldGenerationState reserves monotonically increasing generations across server restarts.
type worldGenerationState struct {
	Next uint64 `json:"next"`
}

// loadWorldGenerations restores the reservation counter without accepting exhausted or zero state.
func (s *Store) loadWorldGenerations(path string) error {
	raw, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	var state worldGenerationState
	if err := json.Unmarshal(raw, &state); err != nil {
		return err
	}
	if state.Next == 0 {
		return errors.New("invalid world generation counter")
	}
	s.nextWorldGeneration = state.Next
	return nil
}

// reserveWorldGeneration saves a consumed generation before its world can be bound.
func (s *Store) reserveWorldGeneration() (uint64, error) {
	s.flushMu.Lock()
	defer s.flushMu.Unlock()
	if s.nextWorldGeneration == 0 {
		s.nextWorldGeneration = 1
	}
	if s.nextWorldGeneration == math.MaxUint64 {
		return 0, errors.New("world generations exhausted")
	}
	generation := s.nextWorldGeneration
	if err := s.writeJSON(worldGenerationFile, worldGenerationState{Next: generation + 1}); err != nil {
		return 0, err
	}
	s.nextWorldGeneration++
	return generation, nil
}
