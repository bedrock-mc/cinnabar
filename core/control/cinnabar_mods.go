package control

import (
	"encoding/json"
	"net"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

// serveCinnabarMods returns only the current authenticated snapshot, without network I/O or credentials.
func (server *Server) serveCinnabarMods(conn net.Conn, id uint64, raw json.RawMessage) error {
	reply := responseWriter{server: server, conn: conn, id: id}
	var params struct {
		Remove *bridgecontract.CinnabarModEntry `json:"remove"`
	}
	if len(raw) != 0 && !decodeStrict(raw, &params) {
		return reply.invalid()
	}
	if params.Remove != nil {
		if api, ok := server.cinnabarAPI().(interface {
			RemoveMarketplace(bridgecontract.CinnabarModEntry) bool
		}); ok {
			api.RemoveMarketplace(*params.Remove)
		}
	}
	result := bridgecontract.CinnabarModLibrary{Mods: []bridgecontract.CinnabarModEntry{}}
	if api, ok := server.cinnabarAPI().(interface {
		MarketplaceLibrary() bridgecontract.CinnabarModLibrary
	}); ok {
		result = api.MarketplaceLibrary()
	}
	return reply.ok(struct {
		SchemaVersion uint32 `json:"schema_version"`
		bridgecontract.CinnabarModLibrary
	}{bridgecontract.ControlSchemaVersion, result})
}
