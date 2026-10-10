package proxy

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// Session messages are streamnet frames on the session endpoint; the first byte names the kind.
// The Rust protocol crate owns the same contract.
const (
	sessionKindConnect    byte = 1 // client: JSON sessionConnectRequest
	sessionKindBatch      byte = 2 // either way: one network batch of length-prefixed packets
	sessionKindHandoff    byte = 3 // core: u32 JSON length, JSON sessionHandoff, startup packets as a batch
	sessionKindPackData   byte = 4 // core: u32 pack index, then that pack's next archive bytes
	sessionKindTransfer   byte = 5 // core, terminal: JSON sessionTransfer
	sessionKindDisconnect byte = 6 // core, terminal: JSON sessionDisconnect
)

// sessionPackChunkBytes bounds one PackData frame well under streamnet.MaxFrameLen.
const sessionPackChunkBytes = 4 << 20

var errMalformedSession = errors.New("proxy: malformed session message")

// sessionConnectRequest is the client's only setup message: where to join and its login client data.
type sessionConnectRequest struct {
	Protocol    int32          `json:"protocol"`
	Target      *sessionTarget `json:"target,omitempty"` // nil keeps the core's current selection
	ClientCache bool           `json:"client_cache"`     // the client resolves blob-cache chunks
	// ClientData holds the Bedrock login client-data claims, as gophertunnel's login.ClientData names them.
	// Its ThirdPartyName names an offline login; a signed-in core joins as its account.
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
	Packs         []sessionPack   `json:"packs"`          // application order; uncached archives follow as PackData
}

// sessionIdentity is the upstream login's canonical player identity.
type sessionIdentity struct {
	DisplayName string `json:"display_name"`
	XUID        string `json:"xuid"`
	UUID        string `json:"uuid"`
}

// sessionPack describes one selected archive; ContentKey is secret.
type sessionPack struct {
	UUID       string                `json:"uuid"`
	Version    string                `json:"version"`
	SubPack    string                `json:"sub_pack"`
	ContentKey string                `json:"content_key"`
	Size       uint64                `json:"size"`
	Cache      *sessionCachedArchive `json:"cache,omitempty"`
}

// sessionCachedArchive names a pinned cache file; Size and SHA256 must both match before use.
type sessionCachedArchive struct {
	Path   string   `json:"path"`
	SHA256 [32]byte `json:"sha256"`
}

// sessionTransfer ends a session at a server transfer; a targetless Connect follows it.
type sessionTransfer struct {
	Address     string `json:"address"`
	Port        uint16 `json:"port"`
	ReloadWorld bool   `json:"reload_world"`
}

// sessionDisconnect ends a session with the server's reason or a join-failure lang key.
type sessionDisconnect struct {
	Reason          int32  `json:"reason"`
	Message         string `json:"message"`
	FilteredMessage string `json:"filtered_message"`
	HideScreen      bool   `json:"hide_screen"`
}

// decodeSessionConnect rejects any frame but one well-formed Connect.
func decodeSessionConnect(frame []byte) (sessionConnectRequest, error) {
	var request sessionConnectRequest
	if len(frame) == 0 || frame[0] != sessionKindConnect {
		return request, fmt.Errorf("%w: expected connect", errMalformedSession)
	}
	decoder := json.NewDecoder(bytes.NewReader(frame[1:]))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil {
		return request, fmt.Errorf("%w: connect: %v", errMalformedSession, err)
	}
	if _, err := decoder.Token(); !errors.Is(err, io.EOF) {
		return request, fmt.Errorf("%w: connect has trailing data", errMalformedSession)
	}
	if len(request.ClientData) == 0 {
		return request, fmt.Errorf("%w: connect has no client data", errMalformedSession)
	}
	return request, nil
}

// appendSessionBatchPacket appends one encoded packet with its varuint32 length, as a Bedrock batch holds it.
func appendSessionBatchPacket(batch, data []byte) []byte {
	batch = binary.AppendUvarint(batch, uint64(len(data)))
	return append(batch, data...)
}

// splitSessionBatch returns the packets of a batch body as subslices; empty batches and packets are malformed.
func splitSessionBatch(body []byte) ([][]byte, error) {
	var packets [][]byte
	for len(body) != 0 {
		length, n := readSessionVaruint32(body)
		if n <= 0 || length == 0 || length > uint64(len(body)-n) || length > streamnet.MaxFrameLen {
			return nil, fmt.Errorf("%w: batch packet length", errMalformedSession)
		}
		body = body[n:]
		packets = append(packets, body[:length:length])
		body = body[length:]
	}
	if len(packets) == 0 {
		return nil, fmt.Errorf("%w: empty batch", errMalformedSession)
	}
	return packets, nil
}

// sessionPacketID reads the packet ID from an encoded packet's header.
func sessionPacketID(data []byte) (uint32, bool) {
	header, n := readSessionVaruint32(data)
	if n <= 0 {
		return 0, false
	}
	return uint32(header) & 0x3ff, true
}

// readSessionVaruint32 is binary.Uvarint limited to the five bytes and 32 bits of a varuint32; n <= 0 rejects.
func readSessionVaruint32(data []byte) (value uint64, n int) {
	value, n = binary.Uvarint(data[:min(len(data), binary.MaxVarintLen32)])
	if n <= 0 || value > 0xffffffff {
		return 0, -1
	}
	return value, n
}

// encodeSessionHandoff lays out a sessionHandoff frame; startup holds every packet through StartGame.
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
		frame = appendSessionBatchPacket(frame, data)
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

// writeSessionPacks streams uncached archives in PackData frames; nil entries keep the indices of cached packs.
func writeSessionPacks(writeFrame func([]byte) error, packs []*resource.Pack) error {
	var frame []byte
	for index, pack := range packs {
		if pack == nil {
			continue
		}
		size := pack.Size()
		for offset := 0; offset < size; {
			n := min(sessionPackChunkBytes, size-offset)
			if cap(frame) < 5+n {
				frame = make([]byte, 5+n)
			}
			frame = frame[:5+n]
			putSessionPackHeader(frame, uint32(index))
			if read, err := pack.ReadAt(frame[5:], int64(offset)); read != n {
				return errors.Join(io.ErrUnexpectedEOF, err)
			}
			if err := writeFrame(frame); err != nil {
				return err
			}
			offset += n
		}
	}
	return nil
}
