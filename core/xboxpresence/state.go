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
	InWorld    bool   `json:"in_world"`
	GameMode   int    `json:"game_mode"`
	Realm      bool   `json:"realm"`
	Experience string `json:"experience,omitempty"`
}

// Valid bounds local control input without rejecting unknown world game modes.
func (s State) Valid() bool {
	if s.Experience == "" {
		return true
	}
	_, err := uuid.Parse(s.Experience)
	return err == nil
}

// ID selects the world default's activity, or the experience's configured activity.
func (s State) ID() string {
	if !s.InWorld {
		return "Menus"
	}
	if s.Experience != "" {
		id, err := uuid.Parse(s.Experience)
		if err == nil {
			if activity, ok := experiences[id]; ok {
				return activity
			}
		}
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

// experiences binds service experience identities to their configured activity IDs.
var experiences = map[uuid.UUID]string{
	uuid.MustParse("adc666b2-1eb1-49c7-9ae7-7536da1dfb50"): "COM_Experience_CTF",
	uuid.MustParse("e0176267-4d03-4f0f-9784-ddddf8a113f3"): "COM_Experience_CTF",
	uuid.MustParse("06af81d0-c0d3-4f5e-8aa5-df22a395af65"): "COM_Experience_CTF",
	uuid.MustParse("f8f91ca8-c233-4ae8-9667-1846ff9d1f91"): "COM_Experience_CTF",
	uuid.MustParse("f22e6d68-93c7-44b2-9cc5-716b2e6031cf"): "COM_Experience_MobMaze",
	uuid.MustParse("e3da296b-8be0-4d79-8608-e53d60531b7b"): "COM_Experience_SoulSteel",
	uuid.MustParse("c35f0f8f-857a-49cd-8696-07bb93863166"): "COM_Experience_GenWars",
	uuid.MustParse("81ac183c-1d09-44a2-b0b5-78abaf8c9877"): "COM_Experience_TreasureHunt",
	uuid.MustParse("6620f751-ca90-40d0-8e8f-15808ba76158"): "COM_Experience_ColosseumNova",
	uuid.MustParse("d2d8b426-51f1-48bd-b294-6159b205027f"): "COM_Experience_ColosseumNova",
	uuid.MustParse("1d16dd1c-036e-42a4-809d-cc40110404d7"): "COM_Experience_ColosseumNova",
	uuid.MustParse("b7f5596c-e811-49ec-b318-80ff3c435d1d"): "COM_Experience_OneBlock",
	uuid.MustParse("bc42bb5f-b034-47fb-99ef-f1a4696c899f"): "COM_Experience_VotingMap",
	uuid.MustParse("558648b7-76c6-4611-9b7b-1789513430af"): "COM_Experience_SkyDimensions",
	uuid.MustParse("e6c02c82-3e51-4253-98ff-7650840260fe"): "COM_Experience_PatientCraft",
	uuid.MustParse("36943f1b-5a71-494a-a812-44b5dc33e27a"): "COM_Experience_TheHive",
	uuid.MustParse("19b73946-f31a-4062-b39c-5f372c11d907"): "COM_Experience_SkyblockHorizons",
	uuid.MustParse("4162b588-7e36-4a97-a681-5e182ef06254"): "COM_Experience_Cubecraft",
	uuid.MustParse("7db90356-35a9-4172-9f1f-b5bf7ea46ceb"): "COM_Experience_Lifeboat",
	uuid.MustParse("afc1ad93-53f0-4b02-bb8f-b665a3cee638"): "COM_Experience_Enchanted",
	uuid.MustParse("bbb868e4-84a9-4229-a700-4b426c7d920b"): "COM_Experience_Galaxite",
	uuid.MustParse("a26af7a2-ed3b-473d-8d1c-a5b4733b85bd"): "COM_Experience_MegaSMP",
	uuid.MustParse("99ba16bb-2576-44b9-b9d6-50a85f43e6ad"): "COM_Experience_Mineville",
}
