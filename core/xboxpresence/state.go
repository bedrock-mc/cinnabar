// Package xboxpresence publishes the signed-in account's Minecraft activity.
package xboxpresence

import (
	"fmt"

	"github.com/df-mc/go-xsapi/v2/presence"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/auth"
)

// State describes the client's committed world state, independent of Xbox credentials.
type State struct {
	InWorld    bool `json:"in_world"`
	GameMode   int  `json:"game_mode"`
	Realm      bool `json:"realm"`
	Experience bool `json:"experience,omitempty"`
}

// ID selects a base world activity or the generic featured/experience activity.
func (s State) ID() string {
	if !s.InWorld {
		return "Menus"
	}
	if s.Experience {
		return "COM_Experience"
	}
	activity := "Survival"
	switch s.GameMode {
	case 1:
		activity = "Creative"
	case 2:
		activity = "Adventure"
	}
	if s.Realm {
		return "Realm_" + activity
	}
	return activity
}

// request uses the same title identity as the account's authentication configuration.
func (s State) request() presence.TitleRequest {
	title := auth.AndroidConfig.TitleID
	scid := uuid.MustParse(fmt.Sprintf("00000000-0000-0000-0000-%012x", title))
	return presence.TitleRequest{ID: uint32(title), State: presence.StateActive,
		Activity: &presence.ActivityRequest{RichPresence: &presence.RichPresenceRequest{
			ID: s.ID(), ServiceConfigID: scid,
		}},
	}
}
