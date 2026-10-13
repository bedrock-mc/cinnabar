package control

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/cinnabarapi"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

const (
	methodCinnabarAPI    = bridgecontract.MethodCinnabarAPI
	methodCinnabarLookup = bridgecontract.MethodCinnabarLookup
	codeRateLimited      = bridgecontract.RPCCinnabarRateLimited
	// cinnabarLookupTimeout bounds one lookup beyond the client's own reply deadline.
	cinnabarLookupTimeout = 15 * time.Second
)

// CinnabarAPI is the optional connection that announces releases and identifies Cinnabar players.
type CinnabarAPI interface {
	Configure(cinnabarapi.Config)
	Lookup(ctx context.Context, xuids []string) ([]string, error)
}

// CinnabarReleaseV1 is a release the Cinnabar API announced.
type CinnabarReleaseV1 struct {
	Channel string `json:"channel"`
	Version string `json:"version"`
}

type cinnabarLookupResultV1 struct {
	SchemaVersion uint32   `json:"schema_version"`
	Cinnabar      []string `json:"cinnabar"`
}

// SetCinnabarAPI attaches the API connection; nil leaves lookups unavailable.
func (server *Server) SetCinnabarAPI(api CinnabarAPI) {
	server.mu.Lock()
	server.cinnabar = api
	server.mu.Unlock()
}

// ObserveCinnabarRelease publishes the newest announced release to events.v1.
func (store *Store) ObserveCinnabarRelease(release CinnabarReleaseV1) {
	store.mu.Lock()
	store.cinnabarRelease = &release
	store.mu.Unlock()
}

func (server *Server) cinnabarAPI() CinnabarAPI {
	server.mu.Lock()
	defer server.mu.Unlock()
	return server.cinnabar
}

// serveCinnabarAPI applies the client's version, channel and visibility without waiting for the network.
func (server *Server) serveCinnabarAPI(conn net.Conn, id uint64, raw json.RawMessage) error {
	reply := responseWriter{server: server, conn: conn, id: id}
	var params struct {
		ClientVersion *string `json:"client_version"`
		Channel       *string `json:"channel"`
		Visible       *bool   `json:"visible"`
	}
	if !decodeStrict(raw, &params) || params.ClientVersion == nil || params.Channel == nil || params.Visible == nil {
		return reply.invalid()
	}
	config := cinnabarapi.Config{ClientVersion: *params.ClientVersion, Channel: *params.Channel, Visible: *params.Visible}
	if !config.Valid() {
		return reply.invalid()
	}
	if api := server.cinnabarAPI(); api != nil {
		api.Configure(config)
	}
	return reply.ok(emptyResultV1{SchemaVersion: 1})
}

// serveCinnabarLookup forwards one bounded XUID batch and answers with its Cinnabar subset.
func (server *Server) serveCinnabarLookup(conn net.Conn, id uint64, raw json.RawMessage) error {
	reply := responseWriter{server: server, conn: conn, id: id}
	var params struct {
		XUIDs []string `json:"xuids"`
	}
	if !decodeStrict(raw, &params) || params.XUIDs == nil || len(params.XUIDs) > bridgecontract.CinnabarAPIMaxLookupXUIDs {
		return reply.invalid()
	}
	for _, xuid := range params.XUIDs {
		if !cinnabarapi.ValidXUID(xuid) {
			return reply.invalid()
		}
	}
	api := server.cinnabarAPI()
	if api == nil {
		return reply.fail(codeServiceFailed, "Service unavailable")
	}
	ctx, cancel := context.WithTimeout(context.Background(), cinnabarLookupTimeout)
	defer cancel()
	cinnabar, err := api.Lookup(ctx, params.XUIDs)
	switch {
	case errors.Is(err, cinnabarapi.ErrRateLimited):
		return reply.fail(codeRateLimited, "Rate limited")
	case err != nil:
		return reply.fail(codeServiceFailed, "Service unavailable")
	}
	if cinnabar == nil {
		cinnabar = []string{}
	}
	return reply.ok(cinnabarLookupResultV1{SchemaVersion: 1, Cinnabar: cinnabar})
}

// decodeStrict decodes exactly one object with no unknown fields.
func decodeStrict(raw json.RawMessage, value any) bool {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	return len(raw) != 0 && decoder.Decode(value) == nil && decoder.Decode(new(any)) == io.EOF
}
