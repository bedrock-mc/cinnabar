package replay

import (
	"bytes"
	"encoding/binary"
	"fmt"
	"os"
)

const maxAssetBytes = 64 << 20
const assetHeaderBytes = 13

var assetMagic = []byte("ZRA1")

// Assets use content identity over original bytes. Their versioned envelope
// keeps PNG payloads verbatim and compresses arena JSON only when beneficial.
func (s *Store) encodeAsset(kind string, data []byte) []byte {
	payload := data
	codec := byte(0)
	if kind == "arena" || kind == "appearance" {
		compressed := s.encoder.EncodeAll(data, nil)
		if len(compressed) < len(data) {
			payload = compressed
			codec = 1
		}
	}
	encoded := make([]byte, assetHeaderBytes+len(payload))
	copy(encoded, assetMagic)
	encoded[4] = codec
	binary.LittleEndian.PutUint64(encoded[5:assetHeaderBytes], uint64(len(data)))
	copy(encoded[assetHeaderBytes:], payload)
	return encoded
}

func (s *Store) readAsset(path string) ([]byte, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Size() > maxAssetBytes+assetHeaderBytes {
		return nil, fmt.Errorf("invalid replay asset size")
	}
	encoded, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	if len(encoded) < assetHeaderBytes || !bytes.Equal(encoded[:4], assetMagic) {
		return nil, fmt.Errorf("invalid replay asset envelope")
	}
	length := binary.LittleEndian.Uint64(encoded[5:assetHeaderBytes])
	if length > maxAssetBytes {
		return nil, fmt.Errorf("replay asset exceeds decode limit")
	}
	payload := encoded[assetHeaderBytes:]
	switch encoded[4] {
	case 0:
	case 1:
		payload, err = s.assetDecoder.DecodeAll(payload, make([]byte, 0, int(length)))
		if err != nil {
			return nil, err
		}
	default:
		return nil, fmt.Errorf("unsupported replay asset codec")
	}
	if uint64(len(payload)) != length {
		return nil, fmt.Errorf("replay asset length mismatch")
	}
	return payload, nil
}
