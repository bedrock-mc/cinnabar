package main

import (
	"bytes"
	"io"
	"log/slog"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

// TestInstalledModPackage exercises a real package produced by the Rust SDK integration test.
func TestInstalledModPackage(t *testing.T) {
	dir := os.Getenv("CINNABAR_TEST_CMOD_WORLD")
	if dir == "" {
		t.Skip("missing fixture: SDK-built .cmod installation")
	}
	checkInstalledRegistry(t, dir)
	socket, err := net.ListenPacket("udp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	addr := socket.LocalAddr().String()
	if err := socket.Close(); err != nil {
		t.Fatal(err)
	}
	var stdout, stderr bytes.Buffer
	err = run([]string{"-dir", dir, "-addr", addr, "-experience-runtime", os.Getenv("CINNABAR_EXPERIENCE_RUNTIME_BIN")}, strings.NewReader("stop\n"), &stdout, &stderr)
	if err != nil {
		t.Fatalf("load installed package: %v\n%s", err, stderr.String())
	}
	if stdout.String() != "ready\n" {
		t.Fatalf("server did not start: %q", stdout.String())
	}
	if _, ok := world.CustomBlocks()["space:cube"]; !ok {
		t.Fatal("declared block was not registered")
	}
	if _, ok := world.ItemByName("space:token", 0); !ok {
		t.Fatal("declared item was not registered")
	}
	if _, err := os.Stat(filepath.Join(dir, "resources", "cinnabar-extension-offer", "manifest.json")); err != nil {
		t.Fatal(err)
	}
}

// checkInstalledRegistry confirms that the generated server entry point answers the client's contract.
func checkInstalledRegistry(t *testing.T, dir string) {
	t.Helper()
	sup, loaded, err := experience.StartSupervisor(os.Getenv("CINNABAR_EXPERIENCE_RUNTIME_BIN"), filepath.Join(dir, "mods", "space", "server"), slog.New(slog.NewTextHandler(os.Stderr, nil)))
	if err != nil {
		t.Fatal(err)
	}
	defer sup.Close()
	if len(loaded.Blocks) != 1 || loaded.Blocks[0].ID != "space:cube" || len(loaded.Items) != 1 || loaded.Items[0].ID != "space:token" {
		t.Fatalf("generated registration = %+v", loaded)
	}
	fingerprint := os.Getenv("CINNABAR_TEST_REGISTRY_FINGERPRINT")
	player := "00000000-0000-0000-0000-000000000001"
	request := experience.CallbackRequest{
		Info:  experience.Info{WorldID: "mod-test", DimensionID: "overworld", Tick: 1, EventSequence: 1},
		Actor: &player, WorldMinY: -64, WorldMaxY: 319,
		Snapshot: []experience.Cell{},
		Call:     experience.Call{ClientMessage: &experience.ClientMessageCall{Player: player, Channel: "space.registry_request", Schema: 1, Payload: []experience.Scalar{{Text: &fingerprint}}}},
	}
	outcome, err := sup.Call(request)
	if err != nil {
		t.Fatal(err)
	}
	if outcome.Committed == nil || len(outcome.Committed.Ops) != 1 || outcome.Committed.Ops[0].SendClient == nil {
		t.Fatalf("registry request failed: %+v", outcome)
	}
	send := outcome.Committed.Ops[0].SendClient
	if send.Channel != "space.registry" || send.Schema != 1 || send.Player != player || len(send.Payload) != 1 || send.Payload[0].Text == nil || *send.Payload[0].Text != fingerprint {
		t.Fatalf("registry confirmation = %+v", send)
	}
	mismatch := "mismatched-definitions"
	request.Call.ClientMessage.Payload = []experience.Scalar{{Text: &mismatch}}
	outcome, err = sup.Call(request)
	if err != nil || outcome.Rejected == nil {
		t.Fatalf("mismatched registry accepted: %+v, %v", outcome, err)
	}
}

// TestInstalledModSettings verifies that an ordinary world remains unchanged and incomplete mods fail closed.
func TestInstalledModSettings(t *testing.T) {
	s, err := parseSettings([]string{"-dir", t.TempDir(), "-addr", "127.0.0.1:19132"}, io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(s.dir, "mods", ".staging"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := s.loadMods(); err != nil {
		t.Fatal(err)
	}
	if s.runtime != "" || s.extensionKey != "" {
		t.Fatal("empty world enabled mods")
	}
	if err := os.MkdirAll(filepath.Join(s.dir, "mods", "broken"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := s.loadMods(); err == nil {
		t.Fatal("incomplete package accepted")
	}
}
