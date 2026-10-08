package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
	"hash"
	"io"

	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const maxCaptureBytes = 512 << 20

type capturedPacket struct {
	record int
	id     uint32
	wire   []byte
}

type capture struct {
	packets []capturedPacket
	summary captureSummary
}

type captureSummary struct {
	SHA256           string         `json:"sha256"`
	ReplaySHA256     string         `json:"replay_sha256"`
	Bytes            int            `json:"bytes"`
	Records          int            `json:"records"`
	HandshakeSkipped int            `json:"handshake_records_regenerated"`
	ReplayPackets    int            `json:"replay_packets"`
	PacketCounts     map[uint32]int `json:"packet_counts"`
	OriginalTiming   string         `json:"original_timing"`
}

// readCapture validates the whole file before any listener or client can use it.
func readCapture(reader io.Reader) (capture, error) {
	data, err := io.ReadAll(io.LimitReader(reader, maxCaptureBytes+1))
	if err != nil {
		return capture{}, err
	}
	if len(data) > maxCaptureBytes {
		return capture{}, errors.New("capture exceeds the 512 MiB fixture limit")
	}
	digest := sha256.Sum256(data)
	result := capture{summary: captureSummary{
		SHA256: hex.EncodeToString(digest[:]), Bytes: len(data),
		PacketCounts: make(map[uint32]int), OriginalTiming: "not recorded; fixed burst schedule",
	}}
	started := false
	streamHash := sha256.New()
	for at := 0; at < len(data); {
		if len(data)-at < 8 {
			return capture{}, fmt.Errorf("record %d: truncated capture header", result.summary.Records)
		}
		id, length := binary.LittleEndian.Uint32(data[at:]), binary.LittleEndian.Uint32(data[at+4:])
		at += 8
		if uint64(length) > uint64(len(data)-at) {
			return capture{}, fmt.Errorf("record %d: truncated capture body", result.summary.Records)
		}
		body := data[at : at+int(length)]
		at += int(length)
		record := result.summary.Records
		result.summary.Records++
		if id > 0x3ff {
			return capture{}, fmt.Errorf("record %d: packet ID does not fit the Bedrock header", record)
		}
		// A captured transfer must never turn an offline measurement into a live connection.
		if id == packet.IDTransfer {
			return capture{}, fmt.Errorf("record %d: server transfer is not allowed in offline replay", record)
		}
		if id == packet.IDStartGame {
			if started {
				return capture{}, fmt.Errorf("record %d: replay requires exactly one StartGame", record)
			}
			started = true
		}
		if !started && loginPacket(id) {
			result.summary.HandshakeSkipped++
			continue
		}
		var header bytes.Buffer
		if err := (&packet.Header{PacketID: id}).Write(&header); err != nil {
			return capture{}, err
		}
		wire := append(header.Bytes(), body...)
		result.packets = append(result.packets, capturedPacket{record: record, id: id, wire: wire})
		result.summary.PacketCounts[id]++
		hashPacket(streamHash, wire)
	}
	if !started {
		return capture{}, errors.New("capture has no StartGame")
	}
	result.summary.ReplayPackets = len(result.packets)
	result.summary.ReplaySHA256 = hex.EncodeToString(streamHash.Sum(nil))
	return result, nil
}

// loginPacket identifies only the pre-StartGame packets replaced by a fresh local login.
func loginPacket(id uint32) bool {
	switch id {
	case packet.IDNetworkSettings, packet.IDServerToClientHandshake, packet.IDPlayStatus,
		packet.IDResourcePacksInfo, packet.IDResourcePackStack:
		return true
	default:
		return false
	}
}

// hashPacket includes a length boundary so different packet partitions cannot share a witness.
func hashPacket(digest hash.Hash, wire []byte) {
	var size [4]byte
	binary.LittleEndian.PutUint32(size[:], uint32(len(wire)))
	_, _ = digest.Write(size[:])
	_, _ = digest.Write(wire)
}
