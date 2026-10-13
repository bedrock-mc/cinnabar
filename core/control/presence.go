package control

import (
	"bytes"
	"encoding/json"
	"io"
	"net"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/xboxpresence"
)

const methodPresence = bridgecontract.MethodPresence

// SetPresence attaches the nonblocking account presence sink; nil means offline.
func (server *Server) SetPresence(set func(xboxpresence.State)) {
	server.mu.Lock()
	defer server.mu.Unlock()
	server.presence = set
}

// servePresence accepts bounded client state without waiting for an Xbox request.
func (server *Server) servePresence(conn net.Conn, id uint64, raw json.RawMessage) error {
	var state xboxpresence.State
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&state); err != nil || decoder.Decode(new(any)) != io.EOF {
		return server.writeResponse(conn, response{JSONRPC: bridgecontract.JSONRPCVersion, ID: id, Error: &responseError{Code: bridgecontract.RPCInvalidParams, Message: "Invalid params"}})
	}
	server.mu.Lock()
	set := server.presence
	server.mu.Unlock()
	if set != nil {
		set(state)
	}
	return server.writeResponse(conn, response{JSONRPC: bridgecontract.JSONRPCVersion, ID: id, Result: emptyResultV1{SchemaVersion: bridgecontract.ControlSchemaVersion}})
}
