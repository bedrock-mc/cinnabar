package spectator

import "time"

const (
	Version            = 1
	ArenaSubject       = "practice.spectator.v1.arena"
	FrameSubject       = "practice.spectator.v1.frame"
	ReplayStartSubject = "practice.spectator.v1.replay-start"
	ClosedSubject      = "practice.spectator.v1.closed"
	SkinSubject        = "practice.spectator.v1.skin"
	Freshness          = 5 * time.Second
	MaxMessage         = 1 << 20
	MaxPartBlocks      = 4096
	MaxParts           = 256
	MaxBlocks          = 1_000_000
	MaxCachedBlocks    = 2_000_000
	MaxArenas          = 16
	MaxMatches         = 32
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
	ID           string     `json:"id"`
	Name         string     `json:"name"`
	Bot          bool       `json:"bot"`
	Team         int        `json:"team"`
	Position     [3]float64 `json:"position"`
	Yaw          float64    `json:"yaw"`
	Pitch        float64    `json:"pitch"`
	Health       float64    `json:"health"`
	MaxHealth    float64    `json:"maxHealth"`
	Hits         int        `json:"hits"`
	Dead         bool       `json:"dead"`
	Equipment    *Equipment `json:"equipment,omitempty"`
	AppearanceID string     `json:"appearanceId,omitempty"`
	SkinID       string     `json:"skinId,omitempty"`
	SkinModel    string     `json:"skinModel,omitempty"`
	Sneaking     bool       `json:"sneaking,omitempty"`
	Sprinting    bool       `json:"sprinting,omitempty"`
	UsingItem    bool       `json:"usingItem,omitempty"`
	OnGround     bool       `json:"onGround,omitempty"`
	Swimming     bool       `json:"swimming,omitempty"`
	SwingAt      *time.Time `json:"swingAt,omitempty"`
	HurtAt       *time.Time `json:"hurtAt,omitempty"`
	POV          *POV       `json:"pov,omitempty"`
}

type Frame struct {
	Version        int           `json:"version"`
	ID             string        `json:"id"`
	ArenaID        string        `json:"arenaId"`
	Mode           string        `json:"mode"`
	Ranked         bool          `json:"ranked"`
	RoundActive    bool          `json:"roundActive"`
	UpdatedAt      time.Time     `json:"updatedAt"`
	MatchStartedAt *time.Time    `json:"matchStartedAt,omitempty"`
	Entities       []Entity      `json:"entities,omitempty"`
	Blocks         []BlockChange `json:"blocks,omitempty"`
	Events         []Event       `json:"events,omitempty"`
	Incomplete     bool          `json:"incomplete,omitempty"`
	Players        []Player      `json:"players"`
	TeamWins       []int         `json:"teamWins"`
}

// ReplayStart carries complete, real snapshots sampled before the terrain
// export became available. It is admitted only to the recorder, never Live.
type ReplayStart struct {
	Version int     `json:"version"`
	ID      string  `json:"id"`
	ArenaID string  `json:"arenaId"`
	Frames  []Frame `json:"frames"`
}

type Closed struct {
	Reason           string    `json:"reason,omitempty"`
	ReplayIncomplete bool      `json:"replayIncomplete,omitempty"`
	FinalFrame       *Frame    `json:"finalFrame,omitempty"`
	Version          int       `json:"version"`
	ID               string    `json:"id"`
	UpdatedAt        time.Time `json:"updatedAt"`
}
