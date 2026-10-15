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
	Lookup(ctx context.Context, xuids, uuids []string) (cinnabarapi.LookupResult, error)
	CacheTTLMS() int64
	LookupLimit() int
}

// CinnabarReleaseV1 is a release the Cinnabar API announced.
type CinnabarReleaseV1 = bridgecontract.CinnabarRelease

type cinnabarLookupResultV1 struct {
	SchemaVersion uint32   `json:"schema_version"`
	Cinnabar      []string `json:"cinnabar"`
	CinnabarUUIDs []string `json:"cinnabar_uuids"`
	CacheTTLMS    int64    `json:"cache_ttl_ms"`
	Resolved      []string `json:"resolved"`
	ResolvedUUIDs []string `json:"resolved_uuids"`
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
		ClientBuild   uint64  `json:"client_build"`
		Channel       *string `json:"channel"`
		Visible       *bool   `json:"visible"`
	}
	if !decodeStrict(raw, &params) || params.ClientVersion == nil || params.Channel == nil || params.Visible == nil {
		return reply.invalid()
	}
	config := cinnabarapi.Config{ClientVersion: *params.ClientVersion, ClientBuild: params.ClientBuild, Channel: *params.Channel, Visible: *params.Visible}
	if !config.Valid() {
		return reply.invalid()
	}
	if api := server.cinnabarAPI(); api != nil {
		api.Configure(config)
	}
	return reply.ok(emptyResultV1{SchemaVersion: 1})
}

// serveCinnabarLookup forwards one bounded identifier batch and answers with its Cinnabar subset.
func (server *Server) serveCinnabarLookup(conn net.Conn, id uint64, raw json.RawMessage) error {
	reply := responseWriter{server: server, conn: conn, id: id}
	var params struct {
		XUIDs []string `json:"xuids"`
		UUIDs []string `json:"uuids"`
	}
	if !decodeStrict(raw, &params) || !cinnabarapi.ValidLookup(params.XUIDs, params.UUIDs) {
		return reply.invalid()
	}
	api := server.cinnabarAPI()
	server.mu.Lock()
	logger := server.logger
	server.mu.Unlock()
	if logger != nil {
		logger.Debug("Cinnabar control lookup received", "requested", len(params.XUIDs), "requested_uuids", len(params.UUIDs), "available", api != nil)
	}
	if api == nil {
		return reply.fail(codeServiceFailed, "Service unavailable")
	}
	ctx, cancel := context.WithTimeout(context.Background(), cinnabarLookupTimeout)
	defer cancel()
	limit := api.LookupLimit()
	batch := params.XUIDs[:min(len(params.XUIDs), limit)]
	uuidBatch := params.UUIDs[:min(len(params.UUIDs), limit-len(batch))]
	found, err := api.Lookup(ctx, batch, uuidBatch)
	if logger != nil {
		logger.Debug("Cinnabar control lookup completed", "requested", len(batch), "requested_uuids", len(uuidBatch), "found", len(found.Cinnabar), "found_uuids", len(found.CinnabarUUIDs), "success", err == nil)
	}
	switch {
	case errors.Is(err, cinnabarapi.ErrRateLimited):
		return reply.fail(codeRateLimited, "Rate limited")
	case err != nil:
		return reply.fail(codeServiceFailed, "Service unavailable")
	}
	if found.Cinnabar == nil {
		found.Cinnabar = []string{}
	}
	if found.CinnabarUUIDs == nil {
		found.CinnabarUUIDs = []string{}
	}
	return reply.ok(cinnabarLookupResultV1{SchemaVersion: 1, Cinnabar: found.Cinnabar, CinnabarUUIDs: found.CinnabarUUIDs, CacheTTLMS: api.CacheTTLMS(), Resolved: batch, ResolvedUUIDs: uuidBatch})
}

// decodeStrict decodes exactly one object with no unknown fields.
func decodeStrict(raw json.RawMessage, value any) bool {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	return len(raw) != 0 && decoder.Decode(value) == nil && decoder.Decode(new(any)) == io.EOF
}
