package control

import (
	"bytes"
	"encoding/json"
	"github.com/hashimthearab/rust-mcbe/core/xboxpresence"
	"io"
	"net"
)

const methodPresence = "presence.v1"

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
		return server.writeResponse(conn, response{JSONRPC: "2.0", ID: id, Error: &responseError{Code: -32602, Message: "Invalid params"}})
	}
	server.mu.Lock()
	set := server.presence
	server.mu.Unlock()
	if set != nil {
		set(state)
	}
	return server.writeResponse(conn, response{JSONRPC: "2.0", ID: id, Result: emptyResultV1{SchemaVersion: 1}})
}
