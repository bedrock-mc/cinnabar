package control

import (
	"encoding/json"
	"net"

	"github.com/hashimthearab/rust-mcbe/core/cinnabarapi"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

// serveCinnabarPlayers exchanges coalesced playback state without waiting for an API reply.
func (server *Server) serveCinnabarPlayers(conn net.Conn, id uint64, raw json.RawMessage) error {
	reply := responseWriter{server: server, conn: conn, id: id}
	var state cinnabarapi.PlayerState
	if !decodeStrict(raw, &state) || !cinnabarapi.ValidPlayerState(state) {
		return reply.invalid()
	}
	var updates cinnabarapi.PlayerUpdates
	if api, ok := server.cinnabarAPI().(interface {
		SyncPlayers(cinnabarapi.PlayerState) cinnabarapi.PlayerUpdates
	}); ok {
		updates = api.SyncPlayers(state)
	}
	if updates.Emotes == nil {
		updates.Emotes = []bridgecontract.CinnabarRemoteEmote{}
	}
	if updates.OwnedCosmetics == nil {
		updates.OwnedCosmetics = []string{}
	}
	for i := range updates.Loadouts {
		if updates.Loadouts[i].Slots == nil {
			updates.Loadouts[i].Slots = []bridgecontract.CinnabarCosmeticSlot{}
		}
	}
	if updates.Loadouts == nil {
		updates.Loadouts = []bridgecontract.CinnabarRemoteLoadout{}
	}
	return reply.ok(struct {
		SchemaVersion uint32 `json:"schema_version"`
		cinnabarapi.PlayerUpdates
	}{bridgecontract.ControlSchemaVersion, updates})
}
