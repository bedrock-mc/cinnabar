package spectator

import "time"

const (
	Version         = 1
	ArenaSubject    = "practice.spectator.v1.arena"
	FrameSubject    = "practice.spectator.v1.frame"
	ClosedSubject   = "practice.spectator.v1.closed"
	Freshness       = 5 * time.Second
	MaxMessage      = 1 << 20
	MaxPartBlocks   = 4096
	MaxParts        = 256
	MaxBlocks       = 1_000_000
	MaxCachedBlocks = 2_000_000
	MaxArenas       = 16
	MaxMatches      = 32
)

type PaletteEntry struct {
	Name   string         `json:"name"`
	States map[string]any `json:"states"`
}

// Bounds uses inclusive minima and maxima: minX/minY/minZ/maxX/maxY/maxZ.
type Arena struct {
	ID      string         `json:"id"`
	Name    string         `json:"name"`
	Palette []PaletteEntry `json:"palette"`
	Bounds  [6]int32       `json:"bounds"`
	Blocks  [][4]int32     `json:"blocks"`
}

type ArenaPart struct {
	Version int `json:"version"`
	Arena
	Part  int `json:"part"`
	Parts int `json:"parts"`
}

type Player struct {
	ID        string     `json:"id"`
	Name      string     `json:"name"`
	Bot       bool       `json:"bot"`
	Team      int        `json:"team"`
	Position  [3]float64 `json:"position"`
	Yaw       float64    `json:"yaw"`
	Pitch     float64    `json:"pitch"`
	Health    float64    `json:"health"`
	MaxHealth float64    `json:"maxHealth"`
	Hits      int        `json:"hits"`
	Dead      bool       `json:"dead"`
}

type Frame struct {
	Version     int       `json:"version"`
	ID          string    `json:"id"`
	ArenaID     string    `json:"arenaId"`
	Mode        string    `json:"mode"`
	Ranked      bool      `json:"ranked"`
	RoundActive bool      `json:"roundActive"`
	UpdatedAt   time.Time `json:"updatedAt"`
	Players     []Player  `json:"players"`
	TeamWins    []int     `json:"teamWins"`
}

type Closed struct {
	Version   int       `json:"version"`
	ID        string    `json:"id"`
	UpdatedAt time.Time `json:"updatedAt"`
}
