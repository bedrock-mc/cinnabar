package spectator

import (
	"math"
	"time"
	"unicode"
)

// Item carries public rendering state, never custom names, lore or private NBT.
type Item struct {
	Name          string        `json:"name"`
	Meta          int16         `json:"meta"`
	Count         int           `json:"count"`
	Enchanted     bool          `json:"enchanted"`
	Durability    int           `json:"durability"`
	MaxDurability int           `json:"maxDurability"`
	Color         string        `json:"color,omitempty"`
	Block         *PaletteEntry `json:"block,omitempty"`
}
type Equipment struct {
	MainHand *Item    `json:"mainHand"`
	OffHand  *Item    `json:"offHand"`
	Armour   [4]*Item `json:"armour"`
}
type POV struct {
	Hotbar             [9]*Item `json:"hotbar"`
	SelectedSlot       int      `json:"selectedSlot"`
	EyeHeight          float64  `json:"eyeHeight"`
	Food               int      `json:"food"`
	Absorption         float64  `json:"absorption"`
	ArmourPoints       float64  `json:"armourPoints"`
	ExperienceLevel    int      `json:"experienceLevel"`
	ExperienceProgress float64  `json:"experienceProgress"`
	Effects            []Effect `json:"effects"`
	AirTicks           *int     `json:"airTicks,omitempty"`
	MaxAirTicks        *int     `json:"maxAirTicks,omitempty"`
	Breathing          *bool    `json:"breathing,omitempty"`
	HUD                HUD      `json:"hud"`
}
type Effect struct {
	ID              int  `json:"id"`
	Level           int  `json:"level"`
	DurationTicks   int  `json:"durationTicks"`
	Infinite        bool `json:"infinite"`
	ParticlesHidden bool `json:"particlesHidden"`
}
type HUD struct {
	Scoreboard *Scoreboard `json:"scoreboard"`
	Popup      *HUDText    `json:"popup"`
	Title      *HUDTitle   `json:"title"`
	ActionBar  *HUDText    `json:"actionBar"`
}
type Scoreboard struct {
	Title string   `json:"title"`
	Lines []string `json:"lines"`
}
type HUDText struct {
	Text      string    `json:"text"`
	UpdatedAt time.Time `json:"updatedAt"`
}
type HUDTitle struct {
	Text         string    `json:"text"`
	Subtitle     string    `json:"subtitle"`
	FadeInTicks  int32     `json:"fadeInTicks"`
	StayTicks    int32     `json:"stayTicks"`
	FadeOutTicks int32     `json:"fadeOutTicks"`
	UpdatedAt    time.Time `json:"updatedAt"`
}

func validItem(v *Item) bool {
	if v == nil {
		return true
	}
	if v.Block != nil && !validPaletteEntry(*v.Block) {
		return false
	}
	if len(v.Name) > 128 || !blockPattern.MatchString(v.Name) || v.Count < 1 || v.Count > 255 || v.Durability < -1 || v.MaxDurability < -1 || v.Durability > 10_000_000 || v.MaxDurability > 10_000_000 {
		return false
	}
	if v.Color != "" {
		if len(v.Color) != 7 || v.Color[0] != '#' {
			return false
		}
		for _, r := range v.Color[1:] {
			if !(r >= '0' && r <= '9' || r >= 'a' && r <= 'f' || r >= 'A' && r <= 'F') {
				return false
			}
		}
	}
	return true
}
func validAppearance(p Player, now time.Time) bool {
	if p.AppearanceID != "" && !validSkinID(p.AppearanceID) {
		return false
	}
	if p.SkinID != "" && !validSkinID(p.SkinID) {
		return false
	}
	if p.SkinModel != "" && !validSkinModel(p.SkinModel) {
		return false
	}
	for _, stamp := range []*time.Time{p.SwingAt, p.HurtAt} {
		if stamp != nil && (stamp.IsZero() || stamp.Sub(now) > time.Second) {
			return false
		}
	}
	if e := p.Equipment; e != nil {
		if !validItem(e.MainHand) || !validItem(e.OffHand) {
			return false
		}
		for _, v := range e.Armour {
			if !validItem(v) {
				return false
			}
		}
	}
	if v := p.POV; v != nil {
		if v.SelectedSlot < 0 || v.SelectedSlot > 8 || !inRange(v.EyeHeight, 0, 4) || v.Food < 0 || v.Food > 20 || !inRange(v.Absorption, 0, 4096) || !inRange(v.ArmourPoints, 0, 1024) || v.ExperienceLevel < 0 || v.ExperienceLevel > math.MaxInt32 || !inRange(v.ExperienceProgress, 0, 1) || len(v.Effects) > 32 {
			return false
		}
		for _, item := range v.Hotbar {
			if !validItem(item) {
				return false
			}
		}
		seen := map[int]bool{}
		for _, e := range v.Effects {
			if e.ID < 0 || e.ID > 4096 || e.Level < 0 || e.Level > 255 || e.DurationTicks < 0 || e.DurationTicks > math.MaxInt32 || seen[e.ID] {
				return false
			}
			seen[e.ID] = true
		}
		if v.AirTicks != nil && (*v.AirTicks < -19 || *v.AirTicks > 1_000_000) {
			return false
		}
		if v.MaxAirTicks != nil && (*v.MaxAirTicks < 1 || *v.MaxAirTicks > 1_000_000) {
			return false
		}
		if !validHUD(v.HUD, now) {
			return false
		}
	}
	return true
}
func inRange(v, min, max float64) bool { return finite(v) && v >= min && v <= max }
func hudText(v string, max int) bool {
	if len(v) > max {
		return false
	}
	for _, r := range v {
		if unicode.IsControl(r) && r != '\n' && r != '\t' {
			return false
		}
	}
	return true
}
func validHUD(h HUD, now time.Time) bool {
	if s := h.Scoreboard; s != nil {
		if !hudText(s.Title, 1024) || len(s.Lines) > 15 {
			return false
		}
		for _, l := range s.Lines {
			if !hudText(l, 1024) {
				return false
			}
		}
	}
	for _, t := range []*HUDText{h.Popup, h.ActionBar} {
		if t != nil && (!hudText(t.Text, 1024) || t.UpdatedAt.IsZero() || t.UpdatedAt.Sub(now) > time.Second) {
			return false
		}
	}
	if t := h.Title; t != nil {
		if !hudText(t.Text, 1024) || !hudText(t.Subtitle, 1024) || t.UpdatedAt.IsZero() || t.UpdatedAt.Sub(now) > time.Second || t.FadeInTicks < 0 || t.StayTicks < 0 || t.FadeOutTicks < 0 || t.FadeInTicks > 72000 || t.StayTicks > 72000 || t.FadeOutTicks > 72000 {
			return false
		}
	}
	return true
}
