package proxy

import (
	"bytes"
	"fmt"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// rawSessionPackets reads packet IDs, decoding the packets decode selects; an undecodable one stays raw.
func rawSessionPackets(packets [][]byte, decode func(uint32) bool, pool packet.Pool, shieldID int32) ([]minecraft.RawPacket, error) {
	raws := make([]minecraft.RawPacket, len(packets))
	for index, data := range packets {
		id, ok := sessionPacketID(data)
		if !ok {
			return nil, fmt.Errorf("%w: packet header", errMalformedSession)
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
