package control

import (
	"bytes"
	"encoding/json"
	"io"
	"net"

	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

const methodPacketDelay = "packet_delay.v1"

func (server *Server) SetPacketDelay(delay *proxy.PacketDelay) {
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
		DelayMS       uint32                   `json:"delay_ms"`
		LeaseMS       int64                    `json:"lease_ms"`
		SchemaVersion uint32                   `json:"schema_version"`
		SessionID     uint64                   `json:"session_id"`
		Position      *proxy.ForwardedPosition `json:"position,omitempty"`
	}{*params.DelayMS, proxy.PacketDelayLease.Milliseconds(), 1, session, position}})
}
