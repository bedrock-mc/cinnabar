package spectator

import (
	"encoding/json"
	"errors"
	"math"
	"regexp"
	"strings"
	"time"
	"unicode"
)

var idPattern = regexp.MustCompile(`^[a-zA-Z0-9_-]{1,80}$`)
var blockPattern = regexp.MustCompile(`^[a-z0-9_]+:[a-z0-9_./-]+$`)

func ValidID(id string) bool { return idPattern.MatchString(id) }

func decode(data []byte, target any) error {
	if len(data) == 0 || len(data) > MaxMessage {
		return errors.New("spectator message exceeds size limit")
	}
	return json.Unmarshal(data, target)
}

func cleanLabel(value string, max int) bool {
	if len(value) == 0 || len(value) > max || strings.TrimSpace(value) != value {
		return false
	}
	for _, r := range value {
		if unicode.IsControl(r) || r == '\u00a7' {
			return false
		}
	}
	return true
}

func fresh(stamp, now time.Time) bool {
	return !stamp.IsZero() && now.Sub(stamp) <= Freshness && stamp.Sub(now) <= time.Second
}

func validArena(part ArenaPart) bool {
	if part.Version != Version || !ValidID(part.ID) || !cleanLabel(part.Name, 128) || part.Parts < 1 || part.Parts > MaxParts || part.Part < 0 || part.Part >= part.Parts || len(part.Blocks) > MaxPartBlocks || len(part.Palette) == 0 || len(part.Palette) > 2048 || part.Palette[0].Name != "minecraft:air" {
		return false
	}
	for axis := 0; axis < 3; axis++ {
		if part.Bounds[axis] < -1_000_000 || part.Bounds[axis+3] > 1_000_000 || part.Bounds[axis+3] < part.Bounds[axis] || part.Bounds[axis+3]-part.Bounds[axis]+1 > 1024 {
			return false
		}
	}
	for _, entry := range part.Palette {
		if !validPaletteEntry(entry) {
			return false
		}
	}
	for _, block := range part.Blocks {
		if block[3] <= 0 || int(block[3]) >= len(part.Palette) {
			return false
		}
		for axis := 0; axis < 3; axis++ {
			if block[axis] < part.Bounds[axis] || block[axis] > part.Bounds[axis+3] {
				return false
			}
		}
	}
	return true
}

func validFrame(frame Frame, now time.Time) bool {
	return validFrameAge(frame, now, Freshness)
}

func validFrameAge(frame Frame, now time.Time, maxAge time.Duration) bool {
	if !validWorldState(frame) || frame.Version != Version || !ValidID(frame.ID) || !ValidID(frame.ArenaID) || !cleanLabel(frame.Mode, 64) || frame.UpdatedAt.IsZero() || now.Sub(frame.UpdatedAt) > maxAge || frame.UpdatedAt.Sub(now) > time.Second || len(frame.Players) < 2 || len(frame.Players) > 32 || len(frame.TeamWins) > 16 {
		return false
	}
	ids := make(map[string]struct{}, len(frame.Players))
	for _, p := range frame.Players {
		if !validAppearance(p, now) {
			return false
		}
		if !cleanLabel(p.ID, 128) || !cleanLabel(p.Name, 64) || p.Team < 0 || p.Team > 15 || p.Hits < 0 || p.Hits > 1_000_000 || !finite(p.Yaw) || !finite(p.Pitch) || math.Abs(p.Yaw) > 360_000 || math.Abs(p.Pitch) > 360 || !finite(p.Health) || !finite(p.MaxHealth) || p.Health < 0 || p.MaxHealth <= 0 || p.MaxHealth > 4096 || p.Health > p.MaxHealth {
			return false
		}
		if _, duplicate := ids[p.ID]; duplicate {
			return false
		}
		ids[p.ID] = struct{}{}
		for _, v := range p.Position {
			if !finite(v) || math.Abs(v) > 1_000_000 {
				return false
			}
		}
	}
	for _, wins := range frame.TeamWins {
		if wins < 0 || wins > 1_000_000 {
			return false
		}
	}
	return true
}

func finite(v float64) bool { return !math.IsNaN(v) && !math.IsInf(v, 0) }
