package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// fixtureCapture joins committed wire fixtures with the minimal missing login completion packets.
func fixtureCapture(t *testing.T) []byte {
	t.Helper()
	result := committedFixture(t, "start_game.bin")
	for _, value := range []packet.Packet{
		&packet.ItemRegistry{}, &packet.ChunkRadiusUpdated{ChunkRadius: 1},
		&packet.PlayStatus{Status: packet.PlayStatusPlayerSpawn},
	} {
		var body bytes.Buffer
		value.Marshal(protocol.NewWriter(&body, 0))
		result = append(result, captureRecord(value.ID(), body.Bytes())...)
	}
	result = append(result, committedFixture(t, "add_actor.bin")...)
	return append(result, committedFixture(t, "text.bin")...)
}

// committedFixture verifies the checked-in authority and removes only its batch and subclient envelope.
func committedFixture(t *testing.T, name string) []byte {
	t.Helper()
	root := filepath.Join("..", "..", "..", "crates", "protocol", "fixtures")
	manifest, err := os.ReadFile(filepath.Join(root, "manifest.json"))
	if err != nil {
		t.Fatal(err)
	}
	var entries []struct {
		File   string `json:"file"`
		ID     uint32 `json:"id"`
		SHA256 string `json:"sha256"`
	}
	if err := json.Unmarshal(manifest, &entries); err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if entry.File != name {
			continue
		}
		data, err := os.ReadFile(filepath.Join(root, name))
		if err != nil {
			t.Fatal(err)
		}
		digest := sha256.Sum256(data)
		if hex.EncodeToString(digest[:]) != entry.SHA256 {
			t.Fatalf("committed fixture %s does not match its manifest", name)
		}
		packets, err := packet.NewDecoder(bytes.NewReader(data)).Decode()
		if err != nil || len(packets) != 1 {
			t.Fatalf("fixture %s is not a single packet: %v", name, err)
		}
		body := bytes.NewBuffer(packets[0])
		var header packet.Header
		if err := header.Read(body); err != nil || header.PacketID != entry.ID {
			t.Fatalf("fixture %s header differs from its manifest: %v", name, err)
		}
		return captureRecord(header.PacketID, body.Bytes())
	}
	t.Fatalf("fixture %s is missing from the committed manifest", name)
	return nil
}
