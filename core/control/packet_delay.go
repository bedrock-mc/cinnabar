package control

import (
	"bytes"
	"encoding/json"
	"io"
	"net"
	"time"
)

const methodPacketDelay = "packet_delay.v1"

// PacketDelay is the relay's leased packet delay behind packet_delay.v1.
type PacketDelay interface {
	// SetWithPosition refreshes the lease; it fails for a delay above the relay's limit.
	SetWithPosition(delayMS uint32, showPosition bool) error
	// PositionSnapshot returns the relay session and, while shown, the last forwarded position.
	PositionSnapshot() (session uint64, position *Position)
	// Lease is how long one request keeps the delay.
	Lease() time.Duration
}

// Position is the last player position the delayed relay forwarded.
type Position struct {
	X float32 `json:"x"`
	Y float32 `json:"y"`
	Z float32 `json:"z"`
}

func (server *Server) SetPacketDelay(delay PacketDelay) {
	server.mu.Lock()
	server.packetDelay = delay
	server.mu.Unlock()
}

func (server *Server) servePacketDelay(conn net.Conn, id uint64, raw json.RawMessage) error {
	server.mu.Lock()
	delay := server.packetDelay
	server.mu.Unlock()
	if delay == nil {
		return server.writeResponse(conn, response{JSONRPC: "2.0", ID: id, Error: &responseError{Code: -32601, Message: "Method not found"}})
	}
	var params struct {
		DelayMS      *uint32 `json:"delay_ms"`
		ShowPosition bool    `json:"show_real_position,omitempty"`
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&params); err != nil || decoder.Decode(new(any)) != io.EOF || params.DelayMS == nil || delay.SetWithPosition(*params.DelayMS, params.ShowPosition) != nil {
		return server.writeResponse(conn, response{JSONRPC: "2.0", ID: id, Error: &responseError{Code: -32602, Message: "Invalid params"}})
	}
	session, position := delay.PositionSnapshot()
	return server.writeResponse(conn, response{JSONRPC: "2.0", ID: id, Result: struct {
		DelayMS       uint32    `json:"delay_ms"`
		LeaseMS       int64     `json:"lease_ms"`
		SchemaVersion uint32    `json:"schema_version"`
		SessionID     uint64    `json:"session_id"`
		Position      *Position `json:"position,omitempty"`
	}{*params.DelayMS, delay.Lease().Milliseconds(), 1, session, position}})
}
