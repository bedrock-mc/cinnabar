package main

import (
	"archive/zip"
	"bufio"
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"log/slog"
	"net"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/extension"
)

// extensionTestdata holds the goldens that `cinnabar-cxb write-fixtures` writes.
var extensionTestdata = filepath.Join("extension", "testdata")

// clientPartFlags are the -extension flags for audience: the fixture server seed and a directory
// holding one .cxb with the golden signed manifest.
func clientPartFlags(t *testing.T, audience string) []string {
	t.Helper()
	var seeds struct {
		Server struct {
			Seed string `json:"seed"`
		} `json:"server"`
	}
	data, err := os.ReadFile(filepath.Join(extensionTestdata, "test_seeds.json"))
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(data, &seeds); err != nil {
		t.Fatal(err)
	}
	keys := t.TempDir()
	key := filepath.Join(keys, "server.seed")
	if err := os.WriteFile(key, []byte(seeds.Server.Seed+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	manifest, err := os.ReadFile(filepath.Join(extensionTestdata, "manifest_signed.json"))
	if err != nil {
		t.Fatal(err)
	}
	var buf bytes.Buffer
	w := zip.NewWriter(&buf)
	f, err := w.CreateHeader(&zip.FileHeader{Name: extension.ManifestPath, Method: zip.Store})
	if err == nil {
		_, err = f.Write(manifest)
	}
	if err == nil {
		err = w.Close()
	}
	if err != nil {
		t.Fatal(err)
	}
	cxb := t.TempDir()
	if err := os.WriteFile(filepath.Join(cxb, "benergistics.cxb"), buf.Bytes(), 0o644); err != nil {
		t.Fatal(err)
	}
	return []string{"-extension-key", key, "-extension-audience", audience, "-extension-cxb", cxb}
}

// markerPack is where startup writes the marker pack for the world in dir.
func markerPack(dir string) string {
	return filepath.Join(settings{dir: dir}.resourcesDir(), extension.MarkerPackDir)
}

// offered verifies the marker in dir's marker pack as the client does and returns its offer and
// digest.
func offered(t *testing.T, dir string) (extension.Offer, string) {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(markerPack(dir), filepath.FromSlash(extension.MarkerPath)))
	if err != nil {
		t.Fatal(err)
	}
	var marker extension.Marker
	if err := extension.Decode(data, &marker); err != nil {
		t.Fatal(err)
	}
	var offer extension.Offer
	digest, err := marker.Offer.Verify(marker.ServerKey, extension.OfferDomain, extension.MaxMarkerBytes/2, &offer)
	if err != nil {
		t.Fatal(err)
	}
	return offer, digest
}

// The three -extension flags come together or not at all.
func TestExtensionFlagsGoTogether(t *testing.T) {
	base := []string{"-dir", "d", "-addr", "127.0.0.1:1"}
	all := []string{"-extension-key", "k", "-extension-audience", "127.0.0.1:19132", "-extension-cxb", "c"}
	for i := 0; i < len(all); i += 2 {
		partial := slices.Delete(slices.Clone(all), i, i+2)
		if _, err := parseSettings(append(slices.Clone(base), partial...), io.Discard); err == nil || !strings.Contains(err.Error(), all[i]) {
			t.Fatalf("without %s: err = %v, want one naming it", all[i], err)
		}
	}
	s, err := parseSettings(append(base, all...), io.Discard)
	if err != nil || s.extensionKey != "k" || s.extensionAudience != "127.0.0.1:19132" || s.extensionCXB != "c" {
		t.Fatalf("settings = %+v, %v", s, err)
	}
}

// The runtime lets one callback stage a client message as large as wire v2 carries, on a
// channel with the longest id, and nests its values exactly as deep as the wire's fields, so
// whatever a client part may receive an Experience may send.
func TestRuntimeClientSendsFitTheWire(t *testing.T) {
	var runtime struct {
		MaxClientSendBytes int `json:"max_client_send_bytes"`
		MaxValueDepth      int `json:"max_value_depth"`
	}
	var wire struct {
		MaxMessageBytes    int `json:"max_message_bytes"`
		MaxIdentifierBytes int `json:"max_identifier_bytes"`
		MaxFieldDepth      int `json:"max_field_depth"`
	}
	for path, into := range map[string]any{
		filepath.Join("experience", "testdata", "protocol", "limits.json"): &runtime,
		filepath.Join(extensionTestdata, "constants.json"):                 &wire,
	} {
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		if err := json.Unmarshal(data, into); err != nil {
			t.Fatalf("%s: %v", path, err)
		}
	}
	if fit := wire.MaxMessageBytes + wire.MaxIdentifierBytes; runtime.MaxClientSendBytes < fit {
		t.Errorf("the runtime's MAX_CLIENT_SEND_BYTES = %d holds no %d-byte message on a %d-byte channel",
			runtime.MaxClientSendBytes, wire.MaxMessageBytes, wire.MaxIdentifierBytes)
	}
	if runtime.MaxValueDepth != wire.MaxFieldDepth {
		t.Errorf("the runtime's MAX_VALUE_DEPTH = %d, the wire's MAX_FIELD_DEPTH = %d",
			runtime.MaxValueDepth, wire.MaxFieldDepth)
	}
}

// Each start with the flags offers the bundles under the next revision; a start without them
// removes the marker pack and keeps the revision for the next offer.
func TestClientPartsAreOfferedAtStartup(t *testing.T) {
	dir := t.TempDir()
	const audience = "127.0.0.1:19132"
	args := append([]string{"-dir", dir, "-addr", "127.0.0.1:0"}, clientPartFlags(t, audience)...)
	for revision := uint64(1); revision <= 2; revision++ {
		var stdout bytes.Buffer
		if err := run(args, strings.NewReader("stop\n"), &stdout, io.Discard); err != nil {
			t.Fatalf("run: %v", err)
		}
		if stdout.String() != "ready\n" {
			t.Fatalf("stdout = %q", stdout.String())
		}
		offer, _ := offered(t, dir)
		if offer.Revision != revision || offer.Audience != audience || len(offer.Packages) != 1 {
			t.Fatalf("start %d offered revision %d for %q with %d packages", revision, offer.Revision, offer.Audience, len(offer.Packages))
		}
	}
	if err := run([]string{"-dir", dir, "-addr", "127.0.0.1:0"}, strings.NewReader("stop\n"), io.Discard, io.Discard); err != nil {
		t.Fatalf("run without the flags: %v", err)
	}
	if _, err := os.Stat(markerPack(dir)); !errors.Is(err, fs.ErrNotExist) {
		t.Fatalf("the marker pack survived a start without the flags: %v", err)
	}
	if data, err := os.ReadFile(filepath.Join(dir, extension.RevisionFile)); err != nil || strings.TrimSpace(string(data)) != "2" {
		t.Fatalf("revision file %q, %v", data, err)
	}
}

// A bad flag fails startup before "ready", naming what is wrong.
func TestClientPartStartupFailures(t *testing.T) {
	for _, c := range []struct {
		name, want string
		change     func(flags []string)
	}{
		{"a missing key", "-extension-key", func(flags []string) { flags[1] = filepath.Join(t.TempDir(), "none") }},
		{"a bad audience", "audience", func(flags []string) { flags[3] = "127.0.0.1" }},
		{"no bundles", "-extension-cxb", func(flags []string) { flags[5] = t.TempDir() }},
	} {
		t.Run(c.name, func(t *testing.T) {
			flags := clientPartFlags(t, "127.0.0.1:19132")
			c.change(flags)
			var stdout bytes.Buffer
			err := run(append([]string{"-dir", t.TempDir(), "-addr", "127.0.0.1:0"}, flags...), strings.NewReader("stop\n"), &stdout, io.Discard)
			if err == nil || !strings.Contains(err.Error(), c.want) || stdout.Len() != 0 {
				t.Fatalf("run: %v, stdout %q; want an error naming %s", err, stdout.String(), c.want)
			}
		})
	}
}

// Dragonfly loads the marker pack from the world's resource pack directory as an optional pack.
func TestMarkerPackLoadsAsAnOptionalPack(t *testing.T) {
	dir := t.TempDir()
	cfg, err := parseSettings(append([]string{"-dir", dir, "-addr", "127.0.0.1:0"}, clientPartFlags(t, "127.0.0.1:19132")...), io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	log := slog.New(slog.DiscardHandler)
	ext, err := startClientParts(cfg, log)
	if err != nil {
		t.Fatal(err)
	}
	conf, err := cfg.userConfig().Config(log)
	if err != nil {
		t.Fatal(err)
	}
	defer conf.WorldProvider.Close()
	defer conf.PlayerProvider.Close()
	want, err := extension.Encode(ext.Marker())
	if err != nil {
		t.Fatal(err)
	}
	loaded := slices.ContainsFunc(conf.Resources, func(p *resource.Pack) bool {
		data, err := p.ReadFile(extension.MarkerPath)
		return err == nil && bytes.Equal(data, want)
	})
	if !loaded || conf.ResourcesRequired {
		t.Fatalf("marker pack loaded: %v; packs required: %v", loaded, conf.ResourcesRequired)
	}
}

// startServer runs the server with args until the test ends.
func startServer(t *testing.T, args []string) {
	t.Helper()
	stdinR, stdinW := io.Pipe()
	stdoutR, stdoutW := io.Pipe()
	done := make(chan error, 1)
	go func() {
		done <- run(args, stdinR, stdoutW, io.Discard)
		stdoutW.Close()
	}()
	if line, err := bufio.NewReader(stdoutR).ReadString('\n'); err != nil || line != "ready\n" {
		t.Fatalf("server printed %q, %v", line, err)
	}
	go io.Copy(io.Discard, stdoutR)
	t.Cleanup(func() {
		_, _ = stdinW.Write([]byte("stop\n"))
		if err := <-done; err != nil {
			t.Errorf("run: %v", err)
		}
	})
}

// freeAddr is a loopback UDP address nothing listens on.
func freeAddr(t *testing.T) string {
	t.Helper()
	pc, err := net.ListenPacket("udp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer pc.Close()
	return pc.LocalAddr().String()
}

// readUntil reads packets until match takes one, failing on a disconnect or after 10 seconds.
func readUntil(t *testing.T, conn *minecraft.Conn, what string, match func(packet.Packet) bool) {
	t.Helper()
	if err := conn.SetReadDeadline(time.Now().Add(10 * time.Second)); err != nil {
		t.Fatal(err)
	}
	for {
		pk, err := conn.ReadPacket()
		if err != nil {
			t.Fatalf("no %s: %v", what, err)
		}
		if match(pk) {
			return
		}
	}
}

// Over real RakNet, a joined player's Hello gets the signed Accept from Dragonfly's wrapped
// connection; a violation afterwards falls back without disconnecting the player.
func TestClientPartHandshakeOverRakNet(t *testing.T) {
	dir, addr := t.TempDir(), freeAddr(t)
	startServer(t, append([]string{"-dir", dir, "-addr", addr}, clientPartFlags(t, addr)...))
	offer, digest := offered(t, dir)

	conn, err := minecraft.Dialer{IdentityData: login.IdentityData{DisplayName: "Steve"}}.DialTimeout("raknet", addr, 20*time.Second)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	if err := conn.DoSpawn(); err != nil {
		t.Fatal(err)
	}
	hello := extension.Hello{
		Version:         extension.WireVersion,
		API:             extension.APIVersion,
		Capabilities:    extension.ImplementedPermissions,
		OfferDigest:     digest,
		ClientChallenge: strings.Repeat("c1", 32),
		Connection:      strings.Repeat("c0", 32),
	}
	carrier := func(m extension.Message) {
		t.Helper()
		data, err := extension.Encode(m)
		if err != nil {
			t.Fatal(err)
		}
		if err := conn.WritePacket(&packet.ScriptMessage{Identifier: extension.Carrier, Data: data}); err != nil {
			t.Fatal(err)
		}
	}
	carrier(extension.Control{Hello: &hello})
	readUntil(t, conn, "Accept", func(pk packet.Packet) bool {
		msg, ok := pk.(*packet.ScriptMessage)
		if !ok || msg.Identifier != extension.Carrier {
			return false
		}
		var reply extension.Control
		var accept extension.Accept
		if err := extension.Decode(msg.Data, &reply); err != nil || reply.Accept == nil {
			t.Fatalf("carrier reply %s: %v", msg.Data, err)
		}
		if _, err := reply.Accept.Verify(offer.ServerKey, extension.AcceptDomain, extension.MaxPayloadBytes, &accept); err != nil || accept.Hello != hello {
			t.Fatalf("Accept %+v: %v", accept, err)
		}
		return true
	})

	carrier(extension.Control{Hello: &hello})
	const radius = 4
	if err := conn.WritePacket(&packet.RequestChunkRadius{ChunkRadius: radius, MaxChunkRadius: radius}); err != nil {
		t.Fatal(err)
	}
	readUntil(t, conn, "chunk radius after the fallback", func(pk packet.Packet) bool {
		update, ok := pk.(*packet.ChunkRadiusUpdated)
		return ok && update.ChunkRadius == radius
	})
}
