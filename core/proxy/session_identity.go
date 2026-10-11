package proxy

import (
	"crypto/md5"
	"encoding/binary"

	"github.com/google/uuid"
)

// initialSelfSignedID derives a login identifier from the full signed client ID's bit pattern
// and exact device string bytes. The caller must supply its actual, nonempty device ID.
func initialSelfSignedID(clientRandomID int64, deviceID string) string {
	var clientBytes [8]byte
	binary.LittleEndian.PutUint64(clientBytes[:], uint64(clientRandomID))
	hash := md5.New()
	_, _ = hash.Write(clientBytes[:])
	_, _ = hash.Write([]byte(deviceID))
	digest := hash.Sum(nil)
	first := binary.LittleEndian.Uint64(digest[:8])&0xffffffffffff0fff | 0x3000
	second := binary.LittleEndian.Uint64(digest[8:])&0x3fffffffffffffff | 0x8000000000000000
	// UUID fields print the two digest words from high to low.
	var id uuid.UUID
	binary.BigEndian.PutUint64(id[:8], first)
	binary.BigEndian.PutUint64(id[8:], second)
	return id.String()
}
