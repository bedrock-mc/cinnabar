package proxy

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// Session messages are streamnet frames on the session endpoint; the first byte names the kind.
// The Rust bridge crate owns the same contract.
const (
	sessionKindConnect    byte = 1 // client: JSON sessionConnectRequest
	sessionKindBatch      byte = 2 // either way: one network batch of length-prefixed packets
	sessionKindHandoff    byte = 3 // core: u32 JSON length, JSON sessionHandoff, startup packets as a batch
	sessionKindPackData   byte = 4 // core: u32 pack index, then that pack's next archive bytes
	sessionKindTransfer   byte = 5 // core, terminal: JSON sessionTransferMessage
	sessionKindDisconnect byte = 6 // core, terminal: JSON sessionDisconnectMessage
)

// sessionPackChunkBytes bounds one PackData frame well under streamnet.MaxFrameLen.
const sessionPackChunkBytes = 4 << 20

var errMalformedSessionMessage = errors.New("proxy: malformed session message")

// sessionConnectRequest is the client's only setup message: where to join and its login client data.
type sessionConnectRequest struct {
	Protocol int32          `json:"protocol"`
	Target   *sessionTarget `json:"target,omitempty"` // nil keeps the core's current selection
	// DisplayName names an offline login; a signed-in core uses its account instead.
	DisplayName string `json:"display_name"`
	ClientCache bool   `json:"client_cache"` // the client resolves blob-cache chunks
	// ClientData holds the Bedrock login client-data claims, as gophertunnel's login.ClientData names them.
	ClientData json.RawMessage `json:"client_data"`
}

// sessionTarget is a connect.v1 target.
type sessionTarget struct {
	Kind  string `json:"kind"`
	Value string `json:"value"`
}

// sessionHandoff is everything the client needs before play; StartGame travels in the startup packets.
type sessionHandoff struct {
	Identity      sessionIdentity `json:"identity"`
	ClientCache   bool            `json:"client_cache"`   // the upstream login advertised blob-cache support
	PacksRequired bool            `json:"packs_required"` // the offer or the stack required the packs
	Packs         []sessionPack   `json:"packs"`          // application order; PackData frames follow in this order
}

// sessionIdentity is the upstream login's canonical player identity.
type sessionIdentity struct {
	DisplayName string `json:"display_name"`
	XUID        string `json:"xuid"`
	UUID        string `json:"uuid"`
}

// sessionPack describes one selected archive; ContentKey is secret.
type sessionPack struct {
	UUID       string `json:"uuid"`
	Version    string `json:"version"`
	SubPack    string `json:"sub_pack"`
	ContentKey string `json:"content_key"`
	Size       uint64 `json:"size"`
}

// sessionTransferMessage ends a session at a server transfer; a targetless Connect follows it.
type sessionTransferMessage struct {
	Address     string `json:"address"`
	Port        uint16 `json:"port"`
	ReloadWorld bool   `json:"reload_world"`
}

// sessionDisconnectMessage ends a session with the server's reason or a join-failure lang key.
type sessionDisconnectMessage struct {
	Reason          int32  `json:"reason"`
	Message         string `json:"message"`
	FilteredMessage string `json:"filtered_message"`
	HideScreen      bool   `json:"hide_screen"`
}

// decodeSessionConnect rejects any frame but one well-formed Connect.
func decodeSessionConnect(frame []byte) (sessionConnectRequest, error) {
	var request sessionConnectRequest
	if len(frame) == 0 || frame[0] != sessionKindConnect {
		return request, fmt.Errorf("%w: expected connect", errMalformedSessionMessage)
	}
	decoder := json.NewDecoder(bytes.NewReader(frame[1:]))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil {
		return request, fmt.Errorf("%w: connect: %v", errMalformedSessionMessage, err)
	}
	if decoder.More() {
		return request, fmt.Errorf("%w: connect has trailing data", errMalformedSessionMessage)
	}
	if len(request.ClientData) == 0 {
		return request, fmt.Errorf("%w: connect has no client data", errMalformedSessionMessage)
	}
	return request, nil
}

// appendBatchPacket appends one encoded packet with its varuint32 length, as a Bedrock batch holds it.
func appendBatchPacket(batch, data []byte) []byte {
	batch = binary.AppendUvarint(batch, uint64(len(data)))
	return append(batch, data...)
}

// splitBatch returns the packets of a batch body as subslices; empty batches and packets are malformed.
func splitBatch(body []byte) ([][]byte, error) {
	var packets [][]byte
	for len(body) != 0 {
		length, n := binary.Uvarint(body)
		if n <= 0 || length == 0 || length > uint64(len(body)-n) || length > streamnet.MaxFrameLen {
			return nil, fmt.Errorf("%w: batch packet length", errMalformedSessionMessage)
		}
		body = body[n:]
		packets = append(packets, body[:length:length])
		body = body[length:]
	}
	if len(packets) == 0 {
		return nil, fmt.Errorf("%w: empty batch", errMalformedSessionMessage)
	}
	return packets, nil
}

// rawSessionPackets reads packet IDs, decoding the packets decode selects; an undecodable one stays raw.
func rawSessionPackets(packets [][]byte, decode func(uint32) bool, pool packet.Pool, shieldID int32) ([]minecraft.RawPacket, error) {
	raws := make([]minecraft.RawPacket, len(packets))
	for index, data := range packets {
		id, ok := sessionPacketID(data)
		if !ok {
			return nil, fmt.Errorf("%w: packet header", errMalformedSessionMessage)
		}
		raws[index] = minecraft.RawPacket{ID: id, Data: data}
		if decode != nil && decode(id) {
			if value := decodeSessionPacket(pool, data, shieldID, true); value != nil {
				raws[index].Decoded = []packet.Packet{value}
			}
		}
	}
	return raws, nil
}

// sessionPacketID reads the packet ID from an encoded packet's header.
func sessionPacketID(data []byte) (uint32, bool) {
	header, n := binary.Uvarint(data)
	if n <= 0 || header > 0xffffffff {
		return 0, false
	}
	return uint32(header) & 0x3ff, true
}

// decodeSessionPacket decodes one encoded packet of the current protocol, or returns nil; limits
// applies gophertunnel's reader limits, as a listener does to client packets.
func decodeSessionPacket(pool packet.Pool, data []byte, shieldID int32, limits bool) (value packet.Packet) {
	buf := bytes.NewBuffer(data)
	var header packet.Header
	if header.Read(buf) != nil {
		return nil
	}
	newPacket, ok := pool[header.PacketID]
	if !ok {
		return nil
	}
	value = newPacket()
	defer func() {
		if recover() != nil {
			value = nil
		}
	}()
	value.Marshal(minecraft.DefaultProtocol.NewReader(buf, shieldID, limits))
	if buf.Len() != 0 {
		return nil
	}
	return value
}

// encodeSessionHandoff lays out a Handoff frame; startup holds every packet through StartGame.
func encodeSessionHandoff(handoff sessionHandoff, startup [][]byte) ([]byte, error) {
	if handoff.Packs == nil {
		handoff.Packs = []sessionPack{}
	}
	metadata, err := json.Marshal(handoff)
	if err != nil {
		return nil, err
	}
	frame := make([]byte, 5, 5+len(metadata))
	frame[0] = sessionKindHandoff
	binary.BigEndian.PutUint32(frame[1:5], uint32(len(metadata)))
	frame = append(frame, metadata...)
	for _, data := range startup {
		frame = appendBatchPacket(frame, data)
	}
	if len(frame) > streamnet.MaxFrameLen {
		return nil, fmt.Errorf("proxy: session handoff is %d bytes; maximum is %d", len(frame), streamnet.MaxFrameLen)
	}
	return frame, nil
}

// putSessionPackHeader starts a PackData frame for pack index; the archive bytes follow it.
func putSessionPackHeader(frame []byte, index uint32) {
	frame[0] = sessionKindPackData
	binary.BigEndian.PutUint32(frame[1:5], index)
}

// encodeSessionJSON lays out a message whose body is one JSON value.
func encodeSessionJSON(kind byte, value any) ([]byte, error) {
	body, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	return append([]byte{kind}, body...), nil
}
