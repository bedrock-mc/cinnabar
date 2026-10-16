package control

import (
	"encoding/json"
	"net"

	"github.com/hashimthearab/rust-mcbe/core/cinnabarapi"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

// serveCinnabarEmotes exchanges coalesced playback state without waiting for an API reply.
func (server *Server) serveCinnabarEmotes(conn net.Conn, id uint64, raw json.RawMessage) error {
	reply := responseWriter{server: server, conn: conn, id: id}
	var state cinnabarapi.EmoteState
	if !decodeStrict(raw, &state) || !cinnabarapi.ValidEmoteState(state) {
		return reply.invalid()
	}
	var emotes []cinnabarapi.RemoteEmote
	if api, ok := server.cinnabarAPI().(interface {
		SyncEmotes(cinnabarapi.EmoteState) []cinnabarapi.RemoteEmote
	}); ok {
		emotes = api.SyncEmotes(state)
	}
	if emotes == nil {
		emotes = []cinnabarapi.RemoteEmote{}
	}
	return reply.ok(struct {
		SchemaVersion uint32                    `json:"schema_version"`
		Emotes        []cinnabarapi.RemoteEmote `json:"emotes"`
	}{bridgecontract.ControlSchemaVersion, emotes})
}
