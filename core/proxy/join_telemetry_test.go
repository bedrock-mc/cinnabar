package proxy

import (
	"bytes"
	"context"
	"crypto/ecdsa"
	"encoding/json"
	"io"
	"log/slog"
	"net"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"golang.org/x/oauth2"
)

func TestJoinTelemetryPreservesObserverAndRecordsEachMilestoneOnce(t *testing.T) {
	var output bytes.Buffer
	telemetry := &joinTelemetry{attempt: 3, started: time.Now(), logger: slog.New(slog.NewJSONHandler(&output, nil))}
	var observed int
	dialer := withJoinTelemetry(minecraft.Dialer{PacketFunc: func(_ packet.Header, payload []byte, _, _ net.Addr) {
		observed++
		if string(payload) != "private payload" {
			t.Fatal("packet payload was changed")
		}
	}}, telemetry)
	for _, id := range []uint32{packet.IDLogin, packet.IDLogin, packet.IDStartGame, packet.IDResourcePacksInfo} {
		dialer.PacketFunc(packet.Header{PacketID: id}, []byte("private payload"), nil, nil)
	}
	if observed != 4 {
		t.Fatalf("original observer called %d times", observed)
	}
	var phases []string
	decoder := json.NewDecoder(bytes.NewReader(output.Bytes()))
	for {
		var row map[string]any
		if err := decoder.Decode(&row); err == io.EOF {
			break
		} else if err != nil {
			t.Fatal(err)
		}
		if row["attempt_id"] != float64(3) || row["msg"] != "JOIN_PHASE" {
			t.Fatal("join timing lost its attempt identity")
		}
		phases = append(phases, row["phase"].(string))
	}
	if !slices.Equal(phases, []string{"login", "start_game"}) {
		t.Fatalf("recorded milestones = %v", phases)
	}
	if strings.Contains(output.String(), "private payload") {
		t.Fatal("join telemetry exposed packet contents")
	}
}

type telemetryTokenFixture struct {
	oauth2.TokenSource
	key *ecdsa.PublicKey
}

func (fixture *telemetryTokenFixture) MultiplayerToken(_ context.Context, key *ecdsa.PublicKey) (string, error) {
	fixture.key = key
	return "private credential", nil
}

func TestJoinTelemetryPreservesKeyBoundTokenSource(t *testing.T) {
	var output bytes.Buffer
	telemetry := &joinTelemetry{started: time.Now(), logger: slog.New(slog.NewJSONHandler(&output, nil))}
	fixture := &telemetryTokenFixture{TokenSource: oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "oauth credential"})}
	dialer := withJoinTelemetry(minecraft.Dialer{TokenSource: fixture}, telemetry)
	key := new(ecdsa.PublicKey)
	token, err := dialer.TokenSource.(minecraft.MultiplayerTokenSource).MultiplayerToken(t.Context(), key)
	if err != nil || token != "private credential" || fixture.key != key {
		t.Fatal("measuring mint changed the credential or proof key")
	}
	oauth, err := dialer.TokenSource.Token()
	if err != nil || oauth.AccessToken != "oauth credential" {
		t.Fatal("measuring mint changed the OAuth source")
	}
	if !strings.Contains(output.String(), "multiplayer_token") || strings.Contains(output.String(), "credential") {
		t.Fatal("mint timing is missing or exposed a credential")
	}

	offline := withJoinTelemetry(minecraft.Dialer{}, telemetry)
	if offline.TokenSource != nil {
		t.Fatal("join telemetry enabled authentication for an offline session")
	}
}

func TestCompletedJoinTelemetryDoesNoAllocationsOrLogging(t *testing.T) {
	var output bytes.Buffer
	telemetry := &joinTelemetry{logger: slog.New(slog.NewJSONHandler(&output, nil))}
	telemetry.complete.Store(true)
	var calls uint64
	dialer := withJoinTelemetry(minecraft.Dialer{PacketFunc: func(packet.Header, []byte, net.Addr, net.Addr) {
		calls++
	}}, telemetry)
	payload := []byte{1, 2, 3}
	allocations := testing.AllocsPerRun(1000, func() {
		dialer.PacketFunc(packet.Header{PacketID: packet.IDLevelChunk}, payload, nil, nil)
	})
	if allocations != 0 || output.Len() != 0 || calls == 0 {
		t.Fatalf("completed telemetry allocated %g times, logged %d bytes, observer calls %d", allocations, output.Len(), calls)
	}
}
