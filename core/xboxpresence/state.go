// Package xboxpresence publishes the signed-in account's Minecraft activity.
package xboxpresence

import (
	"fmt"

	"github.com/df-mc/go-xsapi/v2/presence"
	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
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
		return bridgecontract.PresenceActivityMenus
	}
	if s.Experience {
		return bridgecontract.PresenceActivityExperience
	}
	activity := bridgecontract.PresenceActivitySurvival
	switch s.GameMode {
	case bridgecontract.PresenceGameModeCreative:
		activity = bridgecontract.PresenceActivityCreative
	case bridgecontract.PresenceGameModeAdventure:
		activity = bridgecontract.PresenceActivityAdventure
	}
	if s.Realm {
		return bridgecontract.PresenceActivityRealmPrefix + activity
	}
	return activity
}

// request uses the same title identity as the account's authentication configuration.
func (s State) request() presence.TitleRequest {
	title := bridgecontract.PresenceTitleID
	scid := uuid.MustParse(fmt.Sprintf("00000000-0000-0000-0000-%012x", title))
	return presence.TitleRequest{ID: uint32(title), State: presence.StateActive,
		Activity: &presence.ActivityRequest{RichPresence: &presence.RichPresenceRequest{
			ID: s.ID(), ServiceConfigID: scid,
		}},
	}
}
