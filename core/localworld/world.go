// Package localworld manages saved single-player worlds and the local server
// process that hosts the open one.
package localworld

import (
	"errors"
	"fmt"
	"strings"
	"unicode"
	"unicode/utf8"
)

var (
	ErrNotFound = errors.New("world not found")
	ErrInvalid  = errors.New("invalid world settings")
	ErrBusy     = errors.New("another world is starting, running or stopping")
	ErrInUse    = errors.New("world is open")
	ErrNotOpen  = errors.New("no world is open")

	ErrEULARequired       = errors.New("the Minecraft EULA must be accepted before the server is downloaded")
	ErrBackendUnavailable = errors.New("world backend is not available on this platform")
	ErrVanillaNeedsBDS    = errors.New("default worlds need Bedrock Dedicated Server; create a superflat world instead")
	ErrDockerNotRunning   = errors.New("Docker is not running")
	ErrRuntimePending     = errors.New("still checking whether Docker is running; try again")
)

const (
	maxNameRunes = 64

	GameModeSurvival  = "survival"
	GameModeCreative  = "creative"
	GameModeAdventure = "adventure"

	GeneratorNormal = "normal" // vanilla terrain; BDS only
	GeneratorFlat   = "flat"

	BackendBDS       = "bds"       // Bedrock Dedicated Server: vanilla worldgen and mobs
	BackendDragonfly = "dragonfly" // superflat only, where BDS does not run

	DifficultyPeaceful = "peaceful"
	DifficultyEasy     = "easy"
	DifficultyNormal   = "normal"
	DifficultyHard     = "hard"
)

// World is the persisted metadata of one saved world.
type World struct {
	ID             string `json:"id"`
	Name           string `json:"name"`
	GameMode       string `json:"game_mode"`
	Generator      string `json:"generator"`
	Difficulty     string `json:"difficulty"`
	Backend        string `json:"backend"`
	Seed           int64  `json:"seed"`
	CreatedUnix    int64  `json:"created_unix"`
	LastPlayedUnix int64  `json:"last_played_unix"`
	SizeBytes      int64  `json:"size_bytes,omitempty"` // on disk; filled by List, never persisted
}

// Update changes a saved world's settings; nil fields are left alone.
type Update struct {
	Name       *string `json:"name,omitempty"`
	GameMode   *string `json:"game_mode,omitempty"`
	Difficulty *string `json:"difficulty,omitempty"`
}

// Spec is the user-chosen settings of a new world; empty fields take defaults and a nil Seed is random.
type Spec struct {
	Name       string `json:"name"`
	GameMode   string `json:"game_mode,omitempty"`
	Generator  string `json:"generator,omitempty"`
	Difficulty string `json:"difficulty,omitempty"`
	Backend    string `json:"backend,omitempty"` // empty takes BDS for normal worlds, else the store default
	Seed       *int64 `json:"seed,omitempty"`
}

func oneOf(value, fallback string, allowed ...string) (string, error) {
	value = strings.ToLower(strings.TrimSpace(value))
	if value == "" {
		return fallback, nil
	}
	for _, candidate := range allowed {
		if value == candidate {
			return value, nil
		}
	}
	return "", fmt.Errorf("%w: unknown value %q", ErrInvalid, value)
}

// ValidateName returns the trimmed world name or ErrInvalid.
func ValidateName(name string) (string, error) {
	name = strings.TrimSpace(name)
	if name == "" {
		return "", fmt.Errorf("%w: name is empty", ErrInvalid)
	}
	if !utf8.ValidString(name) || utf8.RuneCountInString(name) > maxNameRunes {
		return "", fmt.Errorf("%w: name must be valid UTF-8 of at most %d characters", ErrInvalid, maxNameRunes)
	}
	for _, r := range name {
		if unicode.IsControl(r) {
			return "", fmt.Errorf("%w: name contains control characters", ErrInvalid)
		}
	}
	return name, nil
}

func (spec Spec) normalize() (Spec, error) {
	var err error
	if spec.Name, err = ValidateName(spec.Name); err != nil {
		return Spec{}, err
	}
	if spec.GameMode, err = oneOf(spec.GameMode, GameModeSurvival, GameModeSurvival, GameModeCreative, GameModeAdventure); err != nil {
		return Spec{}, err
	}
	if spec.Generator, err = oneOf(spec.Generator, GeneratorNormal, GeneratorNormal, GeneratorFlat); err != nil {
		return Spec{}, err
	}
	if spec.Difficulty, err = oneOf(spec.Difficulty, DifficultyNormal, DifficultyPeaceful, DifficultyEasy, DifficultyNormal, DifficultyHard); err != nil {
		return Spec{}, err
	}
	if spec.Backend, err = oneOf(spec.Backend, "", BackendBDS, BackendDragonfly); err != nil {
		return Spec{}, err
	}
	if spec.Generator == GeneratorNormal {
		if spec.Backend == BackendDragonfly {
			return Spec{}, ErrVanillaNeedsBDS
		}
		spec.Backend = BackendBDS
	}
	return spec, nil
}

// OpenOptions are per-open client settings; zero values take the backend default.
type OpenOptions struct {
	ViewDistance int // chunks
}
