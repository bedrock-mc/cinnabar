//go:build integration

package hostbridge

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"io"
	"log/slog"
	"net"
	"slices"
	"strconv"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/internal/hostcontract"
)

// TestBridgeActualListenerWorldAndDuplex exercises the actual process, Listener and session.Conn owners.
// The synthetic IPC identity is not evidence of Xbox/SDP/Login admission.
func TestBridgeActualListenerWorldAndDuplex(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	parent, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer parent.Close()
	l, err := New(hostcontract.MAX_PLAYERS)
	if err != nil {
		t.Fatal(err)
	}
	defer l.Close()
	pack := fixturePack(t)
	configured := []*resource.Pack{pack}
	var final []*resource.Pack
	configuration := server.Config{MaxPlayers: hostcontract.MAX_PLAYERS, MaxChunkRadius: 4,
		ResourcesRequired: true, Resources: configured, Log: slog.New(slog.DiscardHandler),
		Listeners: []func(server.Config) (server.Listener, error){func(conf server.Config) (server.Listener, error) {
			final = slices.Clone(conf.Resources)
			return l.Factory(conf)
		}}}
	srv := configuration.New()
	// This fixture drives the real Conn directly; it never starts Server.listen/Accept.
	defer func() {
		for _, w := range []*world.World{srv.End(), srv.Nether(), srv.World()} {
			if err := w.Close(); err != nil {
				t.Error(err)
			}
		}
	}()
	configured[0] = nil // The final resource slice is retained by the real listener.
	bootstrap := hostcontract.Bootstrap{Address: parent.Addr().String(),
		Opening:    hex.EncodeToString(bytes.Repeat([]byte{8}, hostcontract.OPENING_BYTES)),
		Capability: hex.EncodeToString(bytes.Repeat([]byte{7}, hostcontract.CAPABILITY_BYTES))}
	if err := l.Start(ctx, bootstrap); err != nil {
		t.Fatal(err)
	}
	control := fixtureAccept(t, parent, bootstrap, 0)
	defer control.Close()
	var snapshot hostcontract.Snapshot
	fixtureReadJSON(t, control, hostcontract.KindSnapshot, &snapshot)
	if snapshot.ProtocolVersion != uint32(protocol.CurrentProtocol) || snapshot.MaxPlayers != hostcontract.MAX_PLAYERS || len(snapshot.Packs) != len(final) {
		t.Fatal("final resource/capacity snapshot was not retained")
	}
	checksum := pack.Checksum()
	if snapshot.Packs[0].Sha256 != hex.EncodeToString(checksum[:]) || snapshot.Packs[0].Size != uint64(pack.Size()) {
		t.Fatal("snapshot did not describe its original archive")
	}
	data, err := json.Marshal(login.ClientData{DeviceOS: protocol.DeviceAndroid, GameVersion: protocol.CurrentVersion,
		LanguageCode: "en_GB", SkinID: "fixture", ThirdPartyName: "Fixture", ServerAddress: "unused.invalid:19132"})
	if err != nil {
		t.Fatal(err)
	}
	open := hostcontract.Open{Peer: 1, RemainingMillis: 4000,
		Identity: hostcontract.Identity{Xuid: "1234", Uuid: "00000000-0000-4000-8000-000000000001", DisplayName: "Fixture"}, ClientData: data}
	if err := control.json(hostcontract.KindOpen, open); err != nil {
		t.Fatal(err)
	}
	peer := fixtureAccept(t, parent, bootstrap, open.Peer)
	defer peer.Close()
	var opened hostcontract.PeerResult
	fixtureReadJSON(t, control, hostcontract.KindOpened, &opened)
	if opened.Peer != open.Peer || opened.Code != 0 {
		t.Fatal("actual peer reservation failed")
	}
	// Pending real sockets retain visitor capacity, including the host's reserved slot.
	for id := uint64(2); id < hostcontract.MAX_PLAYERS; id++ {
		other := open
		other.Peer = id
		other.Identity.Xuid = strconv.FormatUint(1234+id, 10)
		other.Identity.Uuid = uuid.UUID{6: 0x40, 8: 0x80, 15: byte(id)}.String()
		if err := control.json(hostcontract.KindOpen, other); err != nil {
			t.Fatal(err)
		}
		reserved := fixtureAccept(t, parent, bootstrap, id)
		defer reserved.Close()
		fixtureReadJSON(t, control, hostcontract.KindOpened, &opened)
		if opened.Peer != id {
			t.Fatal("pending peer lost its actual reservation")
		}
	}
	full := open
	full.Peer = hostcontract.MAX_PLAYERS
	if err := control.json(hostcontract.KindOpen, full); err != nil {
		t.Fatal(err)
	}
	var refused hostcontract.PeerResult
	fixtureReadJSON(t, control, hostcontract.KindRejected, &refused)
	if refused.Peer != full.Peer || refused.Code == 0 {
		t.Fatal("pending peers did not retain bounded visitor capacity")
	}
	if err := peer.json(hostcontract.KindReadPack, hostcontract.Range{Archive: 0, Offset: uint64(pack.Size() - 2), Length: 2}); err != nil {
		t.Fatal(err)
	}
	kind, original, err := peer.read()
	if err != nil || kind != hostcontract.KindPackBytes {
		t.Fatal("original archive range failed")
	}
	var expected [2]byte
	if _, err := pack.ReadAt(expected[:], int64(pack.Size()-2)); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(original, expected[:]) {
		t.Fatal("read did not use original configured archive")
	}
	if err := peer.write(hostcontract.KindAttach, nil); err != nil {
		t.Fatal(err)
	}
	accepted, err := l.Accept()
	if err != nil {
		t.Fatal(err)
	}
	c := accepted.(*Conn)
	started := make(chan error, 1)
	go func() {
		started <- c.StartGameContext(ctx, minecraft.GameData{EntityRuntimeID: 17, EntityUniqueID: 17,
			WorldName: "fixture-world", ChunkRadius: 2, MultiPlayerCorrelationID: "fixture-correlation"})
	}()
	kind, world, err := peer.read()
	if err != nil || kind != hostcontract.KindWorld {
		t.Fatal("actual StartGameContext did not produce world output")
	}
	fixtureWorld(t, world, 17)
	if err := peer.json(hostcontract.KindInitialized, hostcontract.Initialized{CacheEnabled: true, Radius: 2}); err != nil {
		t.Fatal(err)
	}
	if err := <-started; err != nil {
		t.Fatal(err)
	}
	if !c.ClientCacheEnabled() || c.ChunkRadius() != 2 || !IsRemote(c.RemoteAddr()) || !l.RemotePlayer(mustUUID(t, open.Identity.Uuid)) {
		t.Fatal("actual initialization/visitor identity was not retained")
	}
	if err := c.WritePacket(&packet.SetTime{Time: 29}); err != nil {
		t.Fatal(err)
	}
	flushed := make(chan error, 1)
	go func() { flushed <- c.Flush() }()
	kind, outgoing, err := peer.read()
	if err != nil || kind != hostcontract.KindPacket {
		t.Fatal("actual output packet missing")
	}
	decoded := bytes.NewBuffer(outgoing)
	var header packet.Header
	if header.Read(decoded) != nil || header.PacketID != packet.IDSetTime {
		t.Fatal("actual output header changed")
	}
	var setTime packet.SetTime
	setTime.Marshal(minecraft.DefaultProtocol.NewReader(decoded, 0, true))
	if setTime.Time != 29 || decoded.Len() != 0 {
		t.Fatal("actual output payload changed")
	}
	kind, sequence, err := peer.read()
	if err != nil || kind != hostcontract.KindFlush {
		t.Fatal("actual output flush missing")
	}
	// Keep the Go output awaiting its receipt while the opposite input/control path progresses.
	input, err := encodePacket(&packet.RequestChunkRadius{ChunkRadius: 3}, 0)
	if err != nil {
		t.Fatal(err)
	}
	if err := peer.write(hostcontract.KindPacket, input); err != nil {
		t.Fatal(err)
	}
	var one [8]byte
	binary.BigEndian.PutUint64(one[:], 1)
	if err := peer.write(hostcontract.KindFlush, one[:]); err != nil {
		t.Fatal(err)
	}
	read := make(chan packet.Packet, 1)
	readErr := make(chan error, 1)
	go func() { pk, err := c.ReadPacket(); read <- pk; readErr <- err }()
	kind, receipt, err := peer.read()
	if err != nil || kind != hostcontract.KindFlushed || !bytes.Equal(receipt, one[:]) {
		t.Fatal("full output blocked opposite input receipt")
	}
	if err := <-readErr; err != nil {
		t.Fatal(err)
	}
	if pk := <-read; pk.(*packet.RequestChunkRadius).ChunkRadius != 3 {
		t.Fatal("actual input packet changed")
	}
	if err := peer.write(hostcontract.KindFlushed, sequence); err != nil {
		t.Fatal(err)
	}
	if err := <-flushed; err != nil {
		t.Fatal(err)
	}
	if err := peer.write(hostcontract.KindClose, nil); err != nil {
		t.Fatal(err)
	}
	kind, _, err = peer.read()
	if err != nil || kind != hostcontract.KindClosed {
		t.Fatal("final peer close missing")
	}
	if err := c.Close(); err != nil {
		t.Fatal(err)
	}
	if l.RemotePlayer(mustUUID(t, open.Identity.Uuid)) {
		t.Fatal("retired visitor retained its world capacity")
	}
	if err := control.write(hostcontract.KindClose, nil); err != nil {
		t.Fatal(err)
	}
	kind, _, err = control.read()
	if err != nil || kind != hostcontract.KindClosed {
		t.Fatal("final process close missing")
	}
}

// mustUUID parses only this fixture's explicit synthetic identity.
func mustUUID(t *testing.T, value string) uuid.UUID {
	t.Helper()
	id, err := uuid.Parse(value)
	if err != nil {
		t.Fatal(err)
	}
	return id
}

// fixtureAccept uses the same actual numeric TCP stream and mutual process handshake.
func fixtureAccept(t *testing.T, listener net.Listener, bootstrap hostcontract.Bootstrap, peer uint64) *wire {
	t.Helper()
	if tcp, ok := listener.(*net.TCPListener); ok {
		_ = tcp.SetDeadline(time.Now().Add(5 * time.Second))
	}
	c, err := listener.Accept()
	if err != nil {
		t.Fatal(err)
	}
	w := &wire{Conn: c}
	_ = c.SetDeadline(time.Now().Add(5 * time.Second))
	var hello hostcontract.Hello
	fixtureReadJSON(t, w, hostcontract.KindHello, &hello)
	if !validHello(hello, bootstrap, peer) {
		t.Fatal("wrong original process capability/peer")
	}
	if err := w.json(hostcontract.KindHello, hello); err != nil {
		t.Fatal(err)
	}
	return w
}

// fixtureReadJSON reads an actual framed generated record, not a mirrored state assertion.
func fixtureReadJSON(t *testing.T, w *wire, expected hostcontract.Kind, value any) {
	t.Helper()
	kind, body, err := w.read()
	if err != nil || kind != expected || decode(body, value) != nil {
		t.Fatalf("read process record %d", expected)
	}
}

// fixturePack is an original small redistributable archive read by the actual resource owner.
func fixturePack(t *testing.T) *resource.Pack {
	t.Helper()
	var body bytes.Buffer
	z := zip.NewWriter(&body)
	f, err := z.Create("manifest.json")
	if err != nil {
		t.Fatal(err)
	}
	_, err = io.WriteString(f, `{"format_version":2,"header":{"name":"bridge fixture","description":"fixture","uuid":"00000000-0000-4000-8000-000000000002","version":[1,0,0],"min_engine_version":[1,20,0]},"modules":[{"type":"resources","uuid":"00000000-0000-4000-8000-000000000003","version":[1,0,0]}]}`)
	if err != nil {
		t.Fatal(err)
	}
	if err := z.Close(); err != nil {
		t.Fatal(err)
	}
	p, err := resource.ReadBytes(body.Bytes())
	if err != nil {
		t.Fatal(err)
	}
	return p
}

// fixtureWorld decodes the actual GameData projection, including the original runtime ID.
func fixtureWorld(t *testing.T, body []byte, runtimeID uint64) {
	t.Helper()
	b := bytes.NewReader(body)
	var length uint32
	if binary.Read(b, binary.BigEndian, &length) != nil {
		t.Fatal("world policy prefix")
	}
	policyBytes := make([]byte, length)
	if _, err := io.ReadFull(b, policyBytes); err != nil {
		t.Fatal(err)
	}
	var policy hostcontract.WorldPolicy
	if decode(policyBytes, &policy) != nil {
		t.Fatal("world policy")
	}
	var got bool
	for range policy.PacketCount {
		if binary.Read(b, binary.BigEndian, &length) != nil {
			t.Fatal("world packet prefix")
		}
		frame := make([]byte, length)
		if _, err := io.ReadFull(b, frame); err != nil {
			t.Fatal(err)
		}
		buf := bytes.NewBuffer(frame)
		var header packet.Header
		if header.Read(buf) != nil {
			t.Fatal("world header")
		}
		if header.PacketID == packet.IDStartGame {
			var start packet.StartGame
			start.Marshal(minecraft.DefaultProtocol.NewReader(buf, 0, true))
			got = start.EntityRuntimeID == runtimeID && start.WorldName == "fixture-world"
		}
	}
	if !got || b.Len() != 0 {
		t.Fatal("actual world projection was not retained")
	}
}
