package proxy

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// sessionFixture reads a wire fixture the Rust protocol tests share.
func sessionFixture(t *testing.T, name string) []byte {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("..", "..", "crates", "protocol", "fixtures", "session", name))
	if err != nil {
		t.Fatalf("read shared session fixture: %v", err)
	}
	return data
}

// The core's handoff, pack, batch and transfer frames are byte-for-byte what the Rust bridge decodes.
func TestSessionStreamMatchesBridgeFixture(t *testing.T) {
	local, peer := net.Pipe()
	session := newSessionConn(streamnet.NewFramedConn(local))
	defer session.Close()
	written := make(chan []byte, 1)
	go func() {
		data, _ := io.ReadAll(peer)
		written <- data
	}()

	handoff, err := encodeSessionHandoff(sessionHandoff{
		Identity:      sessionIdentity{DisplayName: "Steve", XUID: "2535400000000000", UUID: "00000000-0000-4000-8000-000000000001"},
		ClientCache:   true,
		PacksRequired: true,
		Packs:         []sessionPack{{UUID: "00112233-4455-6677-8899-aabbccddeeff", Version: "1.0.0", SubPack: "high", ContentKey: "key", Size: 3}},
	}, [][]byte{{0xb4, 0x01, 0x09}, {0x0b, 0x01, 0x02}})
	if err != nil {
		t.Fatal(err)
	}
	if err := session.writeFrame(handoff); err != nil {
		t.Fatal(err)
	}
	pack := make([]byte, 8)
	putSessionPackHeader(pack, 0)
	copy(pack[5:], "abc")
	if err := session.writeFrame(pack); err != nil {
		t.Fatal(err)
	}
	if err := session.WritePacketRaw([]byte{0x09, 0x00}); err != nil {
		t.Fatal(err)
	}
	if err := session.Flush(); err != nil {
		t.Fatal(err)
	}
	transfer := encodeTestPacket(&packet.Transfer{Address: "play.example.net", Port: 19132})
	if err := session.WritePacketRaw(transfer); !errors.Is(err, errSessionEnded) || !streamnet.IsClosed(err) {
		t.Fatalf("transfer write = %v, want the ordinary session end", err)
	}
	if err := session.WritePacketRaw([]byte{0x09, 0x00}); !errors.Is(err, errSessionEnded) {
		t.Fatalf("write after transfer = %v", err)
	}
	_ = local.Close()
	if got, want := <-written, sessionFixture(t, "core_stream.bin"); !bytes.Equal(got, want) {
		t.Fatalf("core stream = %x\nwant %x", got, want)
	}
}

// The Rust bridge's Connect fixture decodes to the request it encodes.
func TestSessionConnectFixtureDecodes(t *testing.T) {
	request, err := decodeSessionConnect(sessionFixture(t, "connect.bin"))
	if err != nil {
		t.Fatal(err)
	}
	var clientData login.ClientData
	if err := json.Unmarshal(request.ClientData, &clientData); err != nil {
		t.Fatal(err)
	}
	if request.Protocol != 2193 || request.Target == nil || *request.Target != (sessionTarget{Kind: "raknet", Value: "play.example.net:19132"}) ||
		!request.ClientCache || clientData.DeviceOS != 7 || clientData.GameVersion != "1.26.50" {
		t.Fatalf("connect = %+v, client data = %+v", request, clientData)
	}
}

func TestDecodeSessionConnectRejectsMalformedSetup(t *testing.T) {
	for name, frame := range map[string]string{
		"other kind":     "\x02{}",
		"empty":          "",
		"invalid json":   "\x01{",
		"unknown field":  "\x01{\"protocol\":1,\"client_data\":{},\"extra\":1}",
		"trailing value": "\x01{\"protocol\":1,\"client_data\":{}}{}",
		"trailing close": "\x01{\"protocol\":1,\"client_data\":{}}}",
		"trailing array": "\x01{\"protocol\":1,\"client_data\":{}}]",
		"no client data": "\x01{\"protocol\":1}",
	} {
		if _, err := decodeSessionConnect([]byte(frame)); !errors.Is(err, errMalformedSession) {
			t.Errorf("%s: err = %v", name, err)
		}
	}
}

func TestSessionDownstreamAcceptsOnlyThePinnedProtocolAndValidClientData(t *testing.T) {
	request := testSessionConnect(t)
	downstream, err := newSessionDownstream(request, nil)
	if err != nil {
		t.Fatal(err)
	}
	if downstream.IdentityData().DisplayName != "Steve" || !downstream.sessionClientCache() || downstream.Proto().ID() != minecraft.DefaultProtocol.ID() {
		t.Fatalf("downstream = %+v", downstream)
	}
	wrongProtocol := request
	wrongProtocol.Protocol++
	if _, err := newSessionDownstream(wrongProtocol, nil); err == nil {
		t.Fatal("another protocol was accepted")
	}
	invalid := request
	invalid.ClientData = json.RawMessage(`{"GameVersion":"` + minecraft.DefaultProtocol.Ver() + `","DeviceOS":0}`)
	if _, err := newSessionDownstream(invalid, nil); !errors.Is(err, errMalformedSession) {
		t.Fatalf("invalid client data: %v", err)
	}
}

// The core's device replaces whatever device the client claims, so a client that sends none still
// logs in, and every login claims the platform the core signs in as.
func TestSessionDownstreamClaimsTheCoreDevice(t *testing.T) {
	profile := device.New(protocol.DeviceAndroid)
	request := testSessionConnect(t)
	var claims map[string]any
	if err := json.Unmarshal(request.ClientData, &claims); err != nil {
		t.Fatal(err)
	}
	delete(claims, "DeviceOS")
	claims["DeviceModel"] = "JolyneClient"
	claims["CurrentInputMode"] = packet.InputModeMouse
	request.ClientData, _ = json.Marshal(claims)
	if _, err := newSessionDownstream(request, nil); err == nil {
		t.Fatal("a client claiming no device validated without the core's")
	}
	downstream, err := newSessionDownstream(request, &profile)
	if err != nil {
		t.Fatal(err)
	}
	// The upstream login carries the serialized claims, so check those rather than the struct.
	encoded, err := json.Marshal(downstream.ClientData())
	if err != nil {
		t.Fatal(err)
	}
	var data login.ClientData
	if err := json.Unmarshal(encoded, &data); err != nil {
		t.Fatal(err)
	}
	if data.DeviceOS != protocol.DeviceAndroid || data.DeviceModel != profile.Model || data.DeviceID != profile.ID ||
		data.DefaultInputMode != packet.InputModeTouch || data.CurrentInputMode != packet.InputModeMouse {
		t.Fatalf("serialized login = %s", encoded)
	}
}

func TestSplitBatchRoundTripsAndRejectsMalformedBodies(t *testing.T) {
	large := bytes.Repeat([]byte{7}, 300)
	body := appendSessionBatchPacket(appendSessionBatchPacket(nil, []byte{1, 2}), large)
	packets, err := splitSessionBatch(body)
	if err != nil || len(packets) != 2 || !bytes.Equal(packets[0], []byte{1, 2}) || !bytes.Equal(packets[1], large) {
		t.Fatalf("split = %v, %v", packets, err)
	}
	for name, body := range map[string][]byte{
		"empty":         nil,
		"zero length":   {0},
		"truncated":     {3, 1, 2},
		"bad varint":    {0x80},
		"trailing byte": {1, 9, 1},
		"wide length":   {0x81, 0x80, 0x80, 0x80, 0x80, 0x00, 9},
		"33-bit length": {0x81, 0x80, 0x80, 0x80, 0x10, 9},
	} {
		if _, err := splitSessionBatch(body); !errors.Is(err, errMalformedSession) {
			t.Errorf("%s: err = %v", name, err)
		}
	}
}

// The relay's final Disconnect leaves after the batch already formed, as a terminal message.
func TestSessionDisconnectFlushesTheFormingBatchFirst(t *testing.T) {
	local, peer := net.Pipe()
	session := newSessionConn(streamnet.NewFramedConn(local))
	defer session.Close()
	frames := readSessionFrames(peer)
	if err := session.WritePacketRaw([]byte{0x09, 0x00}); err != nil {
		t.Fatal(err)
	}
	if err := session.WritePacketImmediate(&packet.Disconnect{Reason: 5, Message: "bye", HideDisconnectionScreen: true}); err != nil {
		t.Fatal(err)
	}
	batch := <-frames
	if batch[0] != sessionKindBatch || !bytes.Equal(batch[1:], []byte{2, 0x09, 0x00}) {
		t.Fatalf("batch frame = %x", batch)
	}
	var disconnect sessionDisconnect
	if frame := <-frames; frame[0] != sessionKindDisconnect || json.Unmarshal(frame[1:], &disconnect) != nil ||
		disconnect != (sessionDisconnect{Reason: 5, Message: "bye", HideScreen: true}) {
		t.Fatalf("disconnect frame = %q", frame)
	}
	if err := session.Flush(); err != nil {
		t.Fatalf("flush after the end: %v", err)
	}
}

// An unusable transfer target stays an ordinary relayed packet, as the client skips it.
func TestSessionRelaysUnusableTransferAsPacket(t *testing.T) {
	local, peer := net.Pipe()
	session := newSessionConn(streamnet.NewFramedConn(local))
	defer session.Close()
	frames := readSessionFrames(peer)
	transfer := encodeTestPacket(&packet.Transfer{Address: " ", Port: 19132})
	if err := session.WritePacketRaw(transfer); err != nil {
		t.Fatal(err)
	}
	if err := session.Flush(); err != nil {
		t.Fatal(err)
	}
	if frame := <-frames; frame[0] != sessionKindBatch || !bytes.Equal(frame[1:], appendSessionBatchPacket(nil, transfer)) {
		t.Fatalf("frame = %x", frame)
	}
}

// The handoff lists acquired packs in stack order with the server's required bit and their sizes.
func TestSelectSessionPacksFollowsTheNegotiatedStack(t *testing.T) {
	second, err := resource.ReadBytes(admissionPackArchiveWithID(t, "11223344-5566-7788-99aa-bbccddeeff00"))
	if err != nil {
		t.Fatal(err)
	}
	packs := []*resource.Pack{testAdmissionPack(t), second}
	for _, required := range []bool{false, true} {
		upstream := negotiatedOfferUpstream(t, packs, required)
		stack, err := captureSelectedResourcePackStack(upstream, nil)
		if err != nil {
			t.Fatal(err)
		}
		selected, content, err := selectSessionPacks(stack, nil)
		if err != nil {
			t.Fatal(err)
		}
		if stack.required != required || len(selected) != 2 || len(content) != 2 {
			t.Fatalf("required=%t: selected %+v", required, selected)
		}
		for index, pack := range packs {
			if selected[index].UUID != pack.UUID().String() || selected[index].Version != pack.Version() ||
				selected[index].Size != uint64(pack.Size()) || content[index].UUID() != pack.UUID() {
				t.Fatalf("required=%t: pack %d = %+v", required, index, selected[index])
			}
		}
	}
	if _, _, err := selectSessionPacks(nil, nil); !errors.Is(err, errResourcePackStackUnavailable) {
		t.Fatalf("missing stack: %v", err)
	}
}

// A required stack naming a pack that is neither acquired nor built in refuses the join; an optional one skips it.
func TestSelectSessionPacksRejectsUnavailableRequiredPacks(t *testing.T) {
	second, err := resource.ReadBytes(admissionPackArchiveWithID(t, "11223344-5566-7788-99aa-bbccddeeff00"))
	if err != nil {
		t.Fatal(err)
	}
	first := testAdmissionPack(t)
	offer, _ := negotiatedOfferUpstream(t, []*resource.Pack{first}, false).ResourcePackOffer()
	stack, _ := negotiatedOfferUpstream(t, []*resource.Pack{first, second}, false).ResourcePackStack()
	for _, required := range []bool{false, true} {
		selected, _, err := selectSessionPacks(&selectedResourcePackStack{offer: offer, snapshot: stack, required: required}, nil)
		var admission *PackAdmissionError
		if required != errors.As(err, &admission) {
			t.Fatalf("required=%t: err = %v", required, err)
		}
		if !required && (len(selected) != 1 || selected[0].UUID != first.UUID().String()) {
			t.Fatalf("optional selection = %+v", selected)
		}
	}
}

// testSessionOffer offers pack under its own identity with a content key and sub-pack.
func testSessionOffer(pack *resource.Pack, key, subPack string) sessionOffer {
	return sessionOffer{info: protocol.TexturePackInfo{
		UUID: pack.UUID(), Version: pack.Version(), Size: uint64(pack.Size()), ContentKey: key, SubPackName: subPack,
	}, pack: pack}
}

// An offer repeating one identity keeps its first entry, as vanilla requests each identity once, for
// optional and required offers alike.
func TestChooseSessionPacksKeepsTheFirstOfARepeatedOfferIdentity(t *testing.T) {
	pack := testAdmissionPack(t)
	offers := []sessionOffer{testSessionOffer(pack, "first-key", ""), testSessionOffer(pack, "second-key", "high")}
	entries := []sessionStackEntry{{uuid: pack.UUID().String(), version: pack.Version()}}
	for _, required := range []bool{false, true} {
		selected, _, err := chooseSessionPacks(offers, entries, required, nil)
		if err != nil || len(selected) != 1 || selected[0].ContentKey != "first-key" || selected[0].SubPack != "" {
			t.Fatalf("required=%t: selected %+v, %v", required, selected, err)
		}
	}
	highStack := []sessionStackEntry{{uuid: pack.UUID().String(), version: pack.Version(), subPack: "high"}}
	if selected, _, err := chooseSessionPacks(offers, highStack, false, nil); err != nil || len(selected) != 0 {
		t.Fatalf("the repeated entry's sub-pack was selected: %+v, %v", selected, err)
	}
}

// A stack spelling an offered UUID in upper case still selects it; an unparsable one is unavailable.
func TestChooseSessionPacksParsesStackUUIDs(t *testing.T) {
	pack := testAdmissionPack(t)
	offers := []sessionOffer{testSessionOffer(pack, "", "")}
	upper := []sessionStackEntry{{uuid: strings.ToUpper(pack.UUID().String()), version: pack.Version()}}
	selected, _, err := chooseSessionPacks(offers, upper, true, nil)
	if err != nil || len(selected) != 1 || selected[0].UUID != pack.UUID().String() {
		t.Fatalf("upper-case stack entry = %+v, %v", selected, err)
	}
	invalid := []sessionStackEntry{{uuid: "not-a-uuid", version: pack.Version()}}
	if selected, _, err := chooseSessionPacks(offers, invalid, false, nil); err != nil || len(selected) != 0 {
		t.Fatalf("optional invalid entry = %+v, %v", selected, err)
	}
	if _, _, err := chooseSessionPacks(offers, invalid, true, nil); err == nil {
		t.Fatal("a required invalid entry was accepted")
	}
}

// An ItemRegistry carried in the handoff sets the shield ID used to decode the client's item stacks.
func TestSessionHandoffObservesStartupItemRegistry(t *testing.T) {
	local, peer := net.Pipe()
	session := newSessionConn(streamnet.NewFramedConn(local))
	defer session.Close()
	frames := readSessionFrames(peer)
	registry := encodeTestPacket(&packet.ItemRegistry{Items: []protocol.ItemEntry{{Name: "minecraft:shield", RuntimeID: 300}}})
	plan := sessionPlan{startup: [][]byte{registry, encodeTestPacket(&packet.StartGame{EntityRuntimeID: 9})}}
	if err := writeSessionHandoff(session, plan); err != nil {
		t.Fatal(err)
	}
	if frame := <-frames; frame[0] != sessionKindHandoff {
		t.Fatalf("frame kind %d", frame[0])
	}
	if got := session.shieldID.Load(); got != 300 {
		t.Fatalf("shield ID = %d", got)
	}
}

// Packet-delay tracking on the session path knows the player's runtime ID, so its MovePlayer counts.
func TestSessionPrepareExposesThePlayerRuntimeID(t *testing.T) {
	upstream := newFakeUpstream(nil)
	upstream.useBatchReads = true
	upstream.batchReads <- batchResult{packets: []packet.Packet{&packet.StartGame{EntityRuntimeID: 9}}}
	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	connections := newPreparedConnections("", nil, logger)
	connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
		return &preparedConnection{upstream: upstream, packStack: &selectedResourcePackStack{}}, nil
	}
	server := &sessionServer{prepared: connections, transfers: new(TransferState), logger: logger}
	local, peer := net.Pipe()
	defer peer.Close()
	session := newSessionConn(streamnet.NewFramedConn(local))
	defer session.Close()
	ctx, cancel := context.WithCancelCause(context.Background())
	defer cancel(nil)
	downstream, err := newSessionDownstream(testSessionConnect(t), nil)
	if err != nil {
		t.Fatal(err)
	}
	_, prepared, err := server.prepare(ctx, cancel, session, downstream, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer prepared.close()
	if got := ownRuntimeID(session, prepared.upstream); got != 9 {
		t.Fatalf("own runtime ID = %d, want the upstream's 9", got)
	}
}

// A real join: the core logs in upstream, hands off packs and StartGame, relays both ways and ends
// the session with a Transfer message it also records for the next Connect.
func TestSessionServerHandsOffAndRelaysARealJoin(t *testing.T) {
	pack := testAdmissionPack(t)
	upstreamListener, upstreamNetwork := newAdmissionTestListener(t, func(_ context.Context, conn *minecraft.Conn) error {
		return conn.ConfigureResourcePackOffer([]*resource.Pack{pack}, true)
	})
	transfers := new(TransferState)
	dir := t.TempDir()
	newTestSessionServer(t, dir, func(server *sessionServer) {
		server.transfers = transfers
		server.prepared.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
			return &resolvedUpstreamTarget{network: upstreamNetwork, offline: true}, nil
		}
	})
	upstreamDone := make(chan *minecraft.Conn, 1)
	go func() {
		accepted, err := upstreamListener.Accept()
		if err != nil {
			upstreamDone <- nil
			return
		}
		conn := accepted.(*minecraft.Conn)
		_ = conn.WritePacketImmediate(&packet.StartGame{EntityRuntimeID: 9})
		upstreamDone <- conn
	}()

	client, frames := dialTestSession(t, dir, testSessionConnect(t))
	handoffFrame := <-frames
	if handoffFrame[0] != sessionKindHandoff {
		t.Fatalf("first frame kind %d", handoffFrame[0])
	}
	length := binary.BigEndian.Uint32(handoffFrame[1:5])
	var handoff sessionHandoff
	if err := json.Unmarshal(handoffFrame[5:5+length], &handoff); err != nil {
		t.Fatal(err)
	}
	startup, err := splitSessionBatch(handoffFrame[5+length:])
	if err != nil {
		t.Fatal(err)
	}
	if !handoff.PacksRequired || len(handoff.Packs) != 1 || handoff.Packs[0].UUID != pack.UUID().String() ||
		handoff.Packs[0].Size != uint64(pack.Size()) || handoff.Identity.DisplayName != "Steve" {
		t.Fatalf("handoff = %+v", handoff)
	}
	if last := startup[len(startup)-1]; last[0] != packet.IDStartGame {
		t.Fatalf("startup ends with packet %d", last[0])
	}
	archive := make([]byte, pack.Size())
	if _, err := pack.ReadAt(archive, 0); err != nil {
		t.Fatal(err)
	}
	var received []byte
	for len(received) < len(archive) {
		frame := <-frames
		if frame[0] != sessionKindPackData || binary.BigEndian.Uint32(frame[1:5]) != 0 {
			t.Fatalf("pack frame = %x", frame[:min(len(frame), 8)])
		}
		received = append(received, frame[5:]...)
	}
	if !bytes.Equal(received, archive) {
		t.Fatal("handed-off archive differs from the server's pack")
	}

	upstream := <-upstreamDone
	if upstream == nil {
		t.Fatal("upstream accept failed")
	}
	defer upstream.Close()
	_ = upstream.WritePacket(&packet.Text{TextType: packet.TextTypeRaw, Message: "hello"})
	_ = upstream.Flush()
	batch := <-frames
	packets, err := splitSessionBatch(batch[1:])
	if batch[0] != sessionKindBatch || err != nil || len(packets) != 1 || packets[0][0] != packet.IDText {
		t.Fatalf("relayed frame = %x", batch)
	}

	chat := encodeTestPacket(&packet.Text{TextType: packet.TextTypeChat, SourceName: "Impostor", Message: "hi"})
	if _, err := client.Write(append([]byte{sessionKindBatch}, appendSessionBatchPacket(nil, chat)...)); err != nil {
		t.Fatal(err)
	}
	relayed, err := upstream.ReadBatch()
	if err != nil {
		t.Fatal(err)
	}
	if text, ok := relayed[0].(*packet.Text); !ok || text.Message != "hi" || text.SourceName != "Steve" {
		t.Fatalf("upstream read %#v, want chat under the session identity", relayed[0])
	}

	_ = upstream.WritePacket(&packet.Transfer{Address: "play.example.net", Port: 19132})
	_ = upstream.Flush()
	var transfer sessionTransfer
	if frame := <-frames; frame[0] != sessionKindTransfer || json.Unmarshal(frame[1:], &transfer) != nil ||
		transfer != (sessionTransfer{Address: "play.example.net", Port: 19132}) {
		t.Fatalf("transfer frame = %q", frame)
	}
	if next, ok := transfers.Pending(); !ok || next != "play.example.net:19132" {
		t.Fatalf("pending transfer = %q, %t", next, ok)
	}
	if _, ok := <-frames; ok {
		t.Fatal("session continued after its transfer")
	}
}

// A client that leaves before its handoff cancels the join and is sent nothing.
func TestSessionClientLeavingCancelsPreparation(t *testing.T) {
	dir := t.TempDir()
	started, cancelled := make(chan struct{}), make(chan struct{})
	newTestSessionServer(t, dir, func(server *sessionServer) {
		server.prepared.connectPrepared = func(ctx context.Context, _ dialerDownstream) (*preparedConnection, error) {
			close(started)
			<-ctx.Done()
			close(cancelled)
			return nil, ctx.Err()
		}
	})
	client, _ := dialTestSession(t, dir, testSessionConnect(t))
	<-started
	_ = client.Close()
	select {
	case <-cancelled:
	case <-time.After(2 * time.Second):
		t.Fatal("preparation outlived its client")
	}
}

// A join that fails before its handoff is reported with vanilla's lang key, as the listener did.
func TestSessionPreparationFailureSendsDisconnect(t *testing.T) {
	dir := t.TempDir()
	reported := make(chan DisconnectInfo, 1)
	newTestSessionServer(t, dir, func(server *sessionServer) {
		server.prepared.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
			return nil, &PackAdmissionError{Reason: PackAdmissionRequiredUnsupported, PackCount: 1}
		}
		server.onDisconnect = func(info DisconnectInfo) { reported <- info }
	})
	_, frames := dialTestSession(t, dir, testSessionConnect(t))
	var disconnect sessionDisconnect
	if frame := <-frames; frame[0] != sessionKindDisconnect || json.Unmarshal(frame[1:], &disconnect) != nil ||
		disconnect.Message != "disconnectionScreen.resourcePack" {
		t.Fatalf("frame = %q", frame)
	}
	if len(reported) != 0 {
		t.Fatal("a local failure was reported as a server disconnect")
	}
}

// A targeted Connect joins its own resolved target, never the shared selection another join may change;
// a core without target selection refuses it.
func TestSessionConnectTargetIsBoundToItsSession(t *testing.T) {
	dir := t.TempDir()
	dialed := make(chan string, 1)
	newTestSessionServer(t, dir, func(server *sessionServer) {
		server.selectTarget = func(_ context.Context, kind, value string) (string, error) {
			return kind + "/" + value, nil
		}
		server.dialTarget = func(_ context.Context, address string) (*resolvedUpstreamTarget, error) {
			dialed <- address
			return nil, errors.New("stop after resolution")
		}
		server.prepared.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
			dialed <- "shared selection"
			return nil, errors.New("joined the shared selection")
		}
	})
	request := testSessionConnect(t)
	request.Target = &sessionTarget{Kind: "realm", Value: "42"}
	_, frames := dialTestSession(t, dir, request)
	if frame := <-frames; frame[0] != sessionKindDisconnect || !bytes.Contains(frame, []byte("disconnectionScreen.cantConnect")) {
		t.Fatalf("frame = %q", frame)
	}
	if address := <-dialed; address != "realm/42" {
		t.Fatalf("dialed %q", address)
	}

	untargetable := t.TempDir()
	newTestSessionServer(t, untargetable, func(server *sessionServer) {
		server.prepared.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
			return nil, errors.New("joined without a usable target")
		}
	})
	_, frames = dialTestSession(t, untargetable, request)
	if frame := <-frames; frame[0] != sessionKindDisconnect || !bytes.Contains(frame, []byte("disconnectionScreen.cantConnect")) {
		t.Fatalf("untargetable core frame = %q", frame)
	}
}

// A client that leaves while its target is resolving cancels the resolution.
func TestSessionClientLeavingCancelsTargetResolution(t *testing.T) {
	dir := t.TempDir()
	started, cancelled := make(chan struct{}), make(chan struct{})
	newTestSessionServer(t, dir, func(server *sessionServer) {
		server.selectTarget = func(ctx context.Context, _, _ string) (string, error) {
			close(started)
			<-ctx.Done()
			close(cancelled)
			return "", ctx.Err()
		}
		server.dialTarget = func(context.Context, string) (*resolvedUpstreamTarget, error) {
			return nil, errors.New("dialed after the client left")
		}
	})
	request := testSessionConnect(t)
	request.Target = &sessionTarget{Kind: "gathering", Value: "00000000-0000-4000-8000-000000000001"}
	client, _ := dialTestSession(t, dir, request)
	<-started
	_ = client.Close()
	select {
	case <-cancelled:
	case <-time.After(2 * time.Second):
		t.Fatal("target resolution outlived its client")
	}
}

// flakyListener fails its first Accept as a process out of file descriptors would, then hands out conns.
type flakyListener struct {
	failed bool
	conns  chan net.Conn
	closed chan struct{}
}

func (listener *flakyListener) Accept() (net.Conn, error) {
	if !listener.failed {
		listener.failed = true
		return nil, &net.OpError{Op: "accept", Net: "unix", Err: syscall.EMFILE}
	}
	select {
	case conn := <-listener.conns:
		return conn, nil
	case <-listener.closed:
		return nil, net.ErrClosed
	}
}

func (listener *flakyListener) Close() error {
	select {
	case <-listener.closed:
	default:
		close(listener.closed)
	}
	return nil
}

func (*flakyListener) Addr() net.Addr { return &net.UnixAddr{Name: "session.sock", Net: "unix"} }

// A transient Accept failure keeps the published endpoint serving instead of leaving it dead.
func TestSessionServerKeepsAcceptingAfterTransientFailure(t *testing.T) {
	listener := &flakyListener{conns: make(chan net.Conn), closed: make(chan struct{})}
	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	connections := newPreparedConnections("", nil, logger)
	connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
		return nil, errors.New("served after the failed accept")
	}
	server := &sessionServer{listener: listener, prepared: connections, transfers: new(TransferState), logger: logger}
	ctx, cancel := context.WithCancel(context.Background())
	server.start(ctx)
	defer func() {
		cancel()
		connections.beginShutdown()
		_ = server.close()
		_ = connections.finishShutdown()
	}()
	local, peer := net.Pipe()
	defer peer.Close()
	select {
	case listener.conns <- local:
	case <-time.After(2 * time.Second):
		t.Fatal("the session endpoint stopped accepting after one failure")
	}
	client := streamnet.NewFramedConn(peer)
	frame, err := encodeSessionJSON(sessionKindConnect, testSessionConnect(t))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := client.Write(frame); err != nil {
		t.Fatal(err)
	}
	if reply, err := client.ReadPacket(); err != nil || reply[0] != sessionKindDisconnect {
		t.Fatalf("reply = %q, %v", reply, err)
	}
}

// A refused Connect is told why before the core closes: outdated client or core, or vanilla's cantConnect.
func TestSessionRefusedConnectSendsDisconnect(t *testing.T) {
	dir := t.TempDir()
	newTestSessionServer(t, dir, func(server *sessionServer) {
		server.prepared.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
			return nil, errors.New("a refused Connect was joined")
		}
	})
	request := testSessionConnect(t)
	older, newer := request, request
	older.Protocol--
	newer.Protocol++
	otherVersion := request
	otherVersion.ClientData = bytes.Replace(request.ClientData, []byte(minecraft.DefaultProtocol.Ver()), []byte("1.0.0"), 1)
	invalid := request
	invalid.ClientData = json.RawMessage(`{"GameVersion":"` + minecraft.DefaultProtocol.Ver() + `","DeviceOS":0}`)
	encode := func(request sessionConnectRequest) []byte {
		frame, err := encodeSessionJSON(sessionKindConnect, request)
		if err != nil {
			t.Fatal(err)
		}
		return frame
	}
	for name, test := range map[string]struct {
		frame []byte
		key   string
	}{
		"older protocol":     {encode(older), "disconnectionScreen.outdatedClient"},
		"newer protocol":     {encode(newer), "disconnectionScreen.outdatedServer"},
		"other game version": {encode(otherVersion), "disconnectionScreen.outdatedClient"},
		"invalid client":     {encode(invalid), "disconnectionScreen.cantConnect"},
		"malformed json":     {[]byte("\x01{"), "disconnectionScreen.cantConnect"},
	} {
		_, frames := dialTestSessionFrame(t, dir, test.frame)
		var disconnect sessionDisconnect
		frame, ok := <-frames
		if !ok || frame[0] != sessionKindDisconnect || json.Unmarshal(frame[1:], &disconnect) != nil || disconnect.Message != test.key {
			t.Fatalf("%s: frame = %q", name, frame)
		}
		if _, ok := <-frames; ok {
			t.Fatalf("%s: the session stayed open", name)
		}
	}
}

// Serve publishes the session endpoint beside the listener and releases it on shutdown.
func TestServePublishesAndReleasesSessionEndpoint(t *testing.T) {
	dir := t.TempDir()
	var output lockedBuffer
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() {
		done <- Serve(ctx, Config{SocketDir: dir, Upstream: "127.0.0.1:1", Logger: slog.New(slog.NewTextHandler(&output, nil))})
	}()
	readyCtx, stopWaiting := context.WithTimeout(context.Background(), 2*time.Second)
	defer stopWaiting()
	if !output.waitFor(readyCtx, "listener ready") {
		cancel()
		t.Fatalf("listener was not ready:\n%s", output.String())
	}
	network, address, err := streamnet.ResolveSession(dir)
	if err != nil {
		cancel()
		t.Fatal(err)
	}
	client, err := net.DialTimeout(network, address, time.Second)
	if err != nil {
		cancel()
		t.Fatal(err)
	}
	defer client.Close()
	cancel()
	select {
	case err := <-done:
		if err != nil && !errors.Is(err, context.Canceled) {
			t.Fatalf("Serve() = %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("Serve() remained blocked by a session connection")
	}
	successor, err := streamnet.ListenSession(dir)
	if err != nil {
		t.Fatalf("session endpoint lease leaked: %v", err)
	}
	_ = successor.Close()
}

// newTestSessionServer serves the session endpoint in dir until the test ends; configure runs before it starts.
func newTestSessionServer(t *testing.T, dir string, configure func(*sessionServer)) *sessionServer {
	t.Helper()
	listener, err := streamnet.ListenSession(dir)
	if err != nil {
		t.Fatal(err)
	}
	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	connections := newPreparedConnections("", nil, logger)
	server := &sessionServer{listener: listener, prepared: connections, transfers: new(TransferState), logger: logger}
	configure(server)
	ctx, cancel := context.WithCancel(context.Background())
	server.start(ctx)
	t.Cleanup(func() {
		cancel()
		connections.beginShutdown()
		_ = server.close()
		_ = connections.finishShutdown()
	})
	return server
}

// dialTestSession sends request and returns the connection and its incoming frames.
func dialTestSession(t *testing.T, dir string, request sessionConnectRequest) (*streamnet.FramedConn, <-chan []byte) {
	t.Helper()
	frame, err := encodeSessionJSON(sessionKindConnect, request)
	if err != nil {
		t.Fatal(err)
	}
	return dialTestSessionFrame(t, dir, frame)
}

// dialTestSessionFrame sends frame as the first message and returns the connection and its incoming frames.
func dialTestSessionFrame(t *testing.T, dir string, frame []byte) (*streamnet.FramedConn, <-chan []byte) {
	t.Helper()
	network, address, err := streamnet.ResolveSession(dir)
	if err != nil {
		t.Fatal(err)
	}
	raw, err := net.DialTimeout(network, address, time.Second)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = raw.Close() })
	_ = raw.SetDeadline(time.Now().Add(10 * time.Second))
	conn := streamnet.NewFramedConn(raw)
	if _, err := conn.Write(frame); err != nil {
		t.Fatal(err)
	}
	frames := make(chan []byte, 64)
	go func() {
		defer close(frames)
		for {
			frame, err := conn.ReadPacket()
			if err != nil {
				return
			}
			frames <- frame
		}
	}()
	return conn, frames
}

// readSessionFrames returns the frames written to the far end of a pipe.
func readSessionFrames(peer net.Conn) <-chan []byte {
	frames := make(chan []byte, 16)
	go func() {
		defer close(frames)
		conn := streamnet.NewFramedConn(peer)
		for {
			frame, err := conn.ReadPacket()
			if err != nil {
				return
			}
			frames <- frame
		}
	}()
	return frames
}

// testSessionConnect is an offline Connect whose client data passes validation.
func testSessionConnect(t *testing.T) sessionConnectRequest {
	t.Helper()
	clientData, err := json.Marshal(login.ClientData{
		DeviceOS:        7,
		GameVersion:     minecraft.DefaultProtocol.Ver(),
		LanguageCode:    "en_US",
		SkinID:          "skin",
		SkinData:        base64.StdEncoding.EncodeToString(make([]byte, 64*32*4)),
		SkinImageWidth:  64,
		SkinImageHeight: 32,
		ThirdPartyName:  "Steve",
	})
	if err != nil {
		t.Fatal(err)
	}
	return sessionConnectRequest{
		Protocol: minecraft.DefaultProtocol.ID(), ClientCache: true, ClientData: clientData,
	}
}

// negotiatedOfferUpstream dials a listener offering packs and returns the dialer's negotiated connection.
func negotiatedOfferUpstream(t *testing.T, packs []*resource.Pack, required bool) *minecraft.Conn {
	t.Helper()
	listener, network := newAdmissionTestListener(t, func(_ context.Context, conn *minecraft.Conn) error {
		return conn.ConfigureResourcePackOffer(packs, required)
	})
	done := make(chan error, 1)
	go func() {
		conn, err := listener.Accept()
		if err == nil {
			err = conn.(*minecraft.Conn).WritePacketImmediate(&packet.StartGame{EntityRuntimeID: 9})
		}
		done <- err
	}()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	conn, err := (minecraft.Dialer{Handoff: minecraft.HandoffAtStartGame, IdentityData: login.IdentityData{DisplayName: "Packs"}}).DialContextNetwork(ctx, network, "")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = conn.Close() })
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	return conn
}
