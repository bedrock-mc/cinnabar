// Package sessionwire defines the local client session messages shared by the core and replay.
package sessionwire

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
// The Rust bridge crate owns the same contract.
const (
	KindConnect    byte = 1 // client: JSON ConnectRequest
	KindBatch      byte = 2 // either way: one network batch of length-prefixed packets
	KindHandoff    byte = 3 // core: u32 JSON length, JSON Handoff, startup packets as a batch
	KindPackData   byte = 4 // core: u32 pack index, then that pack's next archive bytes
	KindTransfer   byte = 5 // core, terminal: JSON Transfer
	KindDisconnect byte = 6 // core, terminal: JSON Disconnect
)

// PackChunkBytes bounds one PackData frame well under streamnet.MaxFrameLen.
const PackChunkBytes = 4 << 20

var ErrMalformed = errors.New("proxy: malformed session message")

// ConnectRequest is the client's only setup message: where to join and its login client data.
type ConnectRequest struct {
	Protocol    int32   `json:"protocol"`
	Target      *Target `json:"target,omitempty"` // nil keeps the core's current selection
	ClientCache bool    `json:"client_cache"`     // the client resolves blob-cache chunks
	// ClientData holds the Bedrock login client-data claims, as gophertunnel's login.ClientData names them.
	// Its ThirdPartyName names an offline login; a signed-in core joins as its account.
	ClientData json.RawMessage `json:"client_data"`
}

// Target is a connect.v1 target.
type Target struct {
	Kind  string `json:"kind"`
	Value string `json:"value"`
}

// Handoff is everything the client needs before play; StartGame travels in the startup packets.
type Handoff struct {
	Identity      Identity `json:"identity"`
	ClientCache   bool     `json:"client_cache"`   // the upstream login advertised blob-cache support
	PacksRequired bool     `json:"packs_required"` // the offer or the stack required the packs
	Packs         []Pack   `json:"packs"`          // application order; uncached archives follow as PackData
}

// Identity is the upstream login's canonical player identity.
type Identity struct {
	DisplayName string `json:"display_name"`
	XUID        string `json:"xuid"`
	UUID        string `json:"uuid"`
}

// Pack describes one selected archive; ContentKey is secret.
type Pack struct {
	UUID       string         `json:"uuid"`
	Version    string         `json:"version"`
	SubPack    string         `json:"sub_pack"`
	ContentKey string         `json:"content_key"`
	Size       uint64         `json:"size"`
	Cache      *CachedArchive `json:"cache,omitempty"`
}

// CachedArchive names a pinned cache file; Size and SHA256 must both match before use.
type CachedArchive struct {
	Path   string   `json:"path"`
	SHA256 [32]byte `json:"sha256"`
}

// Transfer ends a session at a server transfer; a targetless Connect follows it.
type Transfer struct {
	Address     string `json:"address"`
	Port        uint16 `json:"port"`
	ReloadWorld bool   `json:"reload_world"`
}

// Disconnect ends a session with the server's reason or a join-failure lang key.
type Disconnect struct {
	Reason          int32  `json:"reason"`
	Message         string `json:"message"`
	FilteredMessage string `json:"filtered_message"`
	HideScreen      bool   `json:"hide_screen"`
}

// DecodeConnect rejects any frame but one well-formed Connect.
func DecodeConnect(frame []byte) (ConnectRequest, error) {
	var request ConnectRequest
	if len(frame) == 0 || frame[0] != KindConnect {
		return request, fmt.Errorf("%w: expected connect", ErrMalformed)
	}
	decoder := json.NewDecoder(bytes.NewReader(frame[1:]))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil {
		return request, fmt.Errorf("%w: connect: %v", ErrMalformed, err)
	}
	if _, err := decoder.Token(); !errors.Is(err, io.EOF) {
		return request, fmt.Errorf("%w: connect has trailing data", ErrMalformed)
	}
	if len(request.ClientData) == 0 {
		return request, fmt.Errorf("%w: connect has no client data", ErrMalformed)
	}
	return request, nil
}

// AppendBatchPacket appends one encoded packet with its varuint32 length, as a Bedrock batch holds it.
func AppendBatchPacket(batch, data []byte) []byte {
	batch = binary.AppendUvarint(batch, uint64(len(data)))
	return append(batch, data...)
}

// SplitBatch returns the packets of a batch body as subslices; empty batches and packets are malformed.
func SplitBatch(body []byte) ([][]byte, error) {
	var packets [][]byte
	for len(body) != 0 {
		length, n := ReadVaruint32(body)
		if n <= 0 || length == 0 || length > uint64(len(body)-n) || length > streamnet.MaxFrameLen {
			return nil, fmt.Errorf("%w: batch packet length", ErrMalformed)
		}
		body = body[n:]
		packets = append(packets, body[:length:length])
		body = body[length:]
	}
	if len(packets) == 0 {
		return nil, fmt.Errorf("%w: empty batch", ErrMalformed)
	}
	return packets, nil
}

// PacketID reads the packet ID from an encoded packet's header.
func PacketID(data []byte) (uint32, bool) {
	header, n := ReadVaruint32(data)
	if n <= 0 {
		return 0, false
	}
	return uint32(header) & 0x3ff, true
}

// ReadVaruint32 is binary.Uvarint limited to the five bytes and 32 bits of a varuint32; n <= 0 rejects.
func ReadVaruint32(data []byte) (value uint64, n int) {
	value, n = binary.Uvarint(data[:min(len(data), binary.MaxVarintLen32)])
	if n <= 0 || value > 0xffffffff {
		return 0, -1
	}
	return value, n
}

// EncodeHandoff lays out a Handoff frame; startup holds every packet through StartGame.
func EncodeHandoff(handoff Handoff, startup [][]byte) ([]byte, error) {
	if handoff.Packs == nil {
		handoff.Packs = []Pack{}
	}
	metadata, err := json.Marshal(handoff)
	if err != nil {
		return nil, err
	}
	frame := make([]byte, 5, 5+len(metadata))
	frame[0] = KindHandoff
	binary.BigEndian.PutUint32(frame[1:5], uint32(len(metadata)))
	frame = append(frame, metadata...)
	for _, data := range startup {
		frame = AppendBatchPacket(frame, data)
	}
	if len(frame) > streamnet.MaxFrameLen {
		return nil, fmt.Errorf("proxy: session handoff is %d bytes; maximum is %d", len(frame), streamnet.MaxFrameLen)
	}
	return frame, nil
}

// PutPackHeader starts a PackData frame for pack index; the archive bytes follow it.
func PutPackHeader(frame []byte, index uint32) {
	frame[0] = KindPackData
	binary.BigEndian.PutUint32(frame[1:5], index)
}

// EncodeJSON lays out a message whose body is one JSON value.
func EncodeJSON(kind byte, value any) ([]byte, error) {
	body, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	return append([]byte{kind}, body...), nil
}

// WritePacks streams uncached archives in PackData frames; nil entries keep the indices of cached packs.
func WritePacks(writeFrame func([]byte) error, packs []*resource.Pack) error {
	var frame []byte
	for index, pack := range packs {
		if pack == nil {
			continue
		}
		size := pack.Size()
		for offset := 0; offset < size; {
			n := min(PackChunkBytes, size-offset)
			if cap(frame) < 5+n {
				frame = make([]byte, 5+n)
			}
			frame = frame[:5+n]
			PutPackHeader(frame, uint32(index))
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
