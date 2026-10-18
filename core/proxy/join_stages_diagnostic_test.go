//go:build joinprofile

package proxy

import (
	"bytes"
	"encoding/binary"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"log/slog"
	"strings"
	"testing"
)

// TestDiagnosticJoinStages distinguishes login success from spawn and logs each stage once.
func TestDiagnosticJoinStages(t *testing.T) {
	var output bytes.Buffer
	logger := slog.New(slog.NewTextHandler(&output, &slog.HandlerOptions{Level: slog.LevelDebug}))
	d := withDiagnosticJoinStages(minecraft.Dialer{}, logger)
	emit := func(id uint32, body []byte) { d.PacketObserver.Func(packet.Header{PacketID: id}, body, nil, nil) }
	emit(packet.IDPlayStatus, []byte{0})
	var status [4]byte
	emit(packet.IDPlayStatus, status[:])
	binary.BigEndian.PutUint32(status[:], 3)
	emit(packet.IDPlayStatus, status[:])
	emit(packet.IDPlayStatus, status[:])
	emit(packet.IDTransfer, []byte("private-target"))
	emit(packet.IDTransfer, nil)
	emit(packet.IDSetLocalPlayerAsInitialised, nil)
	emit(packet.IDLevelChunk, []byte("secret-payload"))
	for _, stage := range []string{"login_success", "player_spawn", "server_transfer", "player_initialized"} {
		if strings.Count(output.String(), "stage="+stage+" ") != 1 {
			t.Fatalf("stage %s was not logged once: %s", stage, output.String())
		}
	}
	if strings.Contains(output.String(), "first_level_chunk") || strings.Contains(output.String(), "secret-payload") || strings.Contains(output.String(), "private-target") {
		t.Fatal("observer continued past initialization or retained packet data")
	}
}
