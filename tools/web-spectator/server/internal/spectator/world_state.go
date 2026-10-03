package spectator

import (
	"math"
	"time"
)

const (
	MaxEntities      = 128
	MaxChangedBlocks = 8192
	MaxWorldEvents   = 512
)

type Entity struct {
	ID       string     `json:"id"`
	Kind     string     `json:"kind"`
	Position [3]float64 `json:"position"`
	Yaw      float64    `json:"yaw"`
	Pitch    float64    `json:"pitch"`
	Item     *Item      `json:"item,omitempty"`
}
type BlockChange struct {
	Position [3]int32       `json:"position"`
	Name     string         `json:"name"`
	States   map[string]any `json:"states"`
}
type Event struct {
	ItemName    string             `json:"itemName,omitempty"`
	ItemAux     uint16             `json:"itemAux,omitempty"`
	BlockName   string             `json:"blockName,omitempty"`
	BlockStates map[string]any     `json:"blockStates,omitempty"`
	ID          string             `json:"id"`
	Kind        string             `json:"kind"`
	Position    [3]float64         `json:"position"`
	Name        string             `json:"name"`
	EntityType  string             `json:"entityType,omitempty"`
	Data        map[string]float64 `json:"data"`
	UpdatedAt   time.Time          `json:"updatedAt"`
}

func validWorldState(frame Frame) bool {
	if len(frame.Entities) > MaxEntities || len(frame.Blocks) > MaxChangedBlocks || len(frame.Events) > MaxWorldEvents {
		return false
	}
	if start := frame.MatchStartedAt; start != nil && (start.IsZero() || start.After(frame.UpdatedAt)) {
		return false
	}
	ids := map[string]bool{}
	for _, entity := range frame.Entities {
		if !cleanLabel(entity.ID, 128) || ids[entity.ID] || !cleanLabel(entity.Kind, 128) || !validPosition(entity.Position) || !finite(entity.Yaw) || math.Abs(entity.Yaw) > 360_000 || !finite(entity.Pitch) || math.Abs(entity.Pitch) > 360 || !validItem(entity.Item) {
			return false
		}
		ids[entity.ID] = true
	}
	positions := map[[3]int32]bool{}
	for _, block := range frame.Blocks {
		if positions[block.Position] || !validPaletteEntry(PaletteEntry{Name: block.Name, States: block.States}) {
			return false
		}
		positions[block.Position] = true
		for _, value := range block.Position {
			if value < -1_000_000 || value > 1_000_000 {
				return false
			}
		}
	}
	ids = map[string]bool{}
	for _, event := range frame.Events {
		if event.ItemName != "" && (len(event.ItemName) > 128 || !blockPattern.MatchString(event.ItemName)) {
			return false
		}
		if (event.BlockName != "" || len(event.BlockStates) != 0) && !validPaletteEntry(PaletteEntry{Name: event.BlockName, States: event.BlockStates}) {
			return false
		}
		if !cleanLabel(event.ID, 128) || ids[event.ID] || !cleanLabel(event.Kind, 128) || (event.Name != "" && !cleanLabel(event.Name, 256)) || (event.EntityType != "" && !cleanLabel(event.EntityType, 128)) || !validPosition(event.Position) || len(event.Data) > 32 || event.UpdatedAt.IsZero() || event.UpdatedAt.After(frame.UpdatedAt.Add(time.Second)) {
			return false
		}
		ids[event.ID] = true
		for key, value := range event.Data {
			if !cleanLabel(key, 128) || !finite(value) || math.Abs(value) > 1_000_000_000 {
				return false
			}
		}
	}
	return true
}
func validPosition(position [3]float64) bool {
	for _, value := range position {
		if !finite(value) || math.Abs(value) > 1_000_000 {
			return false
		}
	}
	return true
}
func validPaletteEntry(entry PaletteEntry) bool {
	if len(entry.Name) > 128 || !blockPattern.MatchString(entry.Name) || len(entry.States) > 32 {
		return false
	}
	for key, value := range entry.States {
		if !cleanLabel(key, 128) {
			return false
		}
		switch v := value.(type) {
		case bool:
		case string:
			if !cleanLabel(v, 256) {
				return false
			}
		case float64:
			if !finite(v) || math.Trunc(v) != v || v < math.MinInt32 || v > math.MaxInt32 {
				return false
			}
		default:
			return false
		}
	}
	return true
}
func validCloseReason(reason string) bool {
	return reason == "" || reason == "finished" || reason == "revoked" || reason == "shutdown"
}
