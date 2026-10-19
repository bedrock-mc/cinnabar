package proxy

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"slices"
	"sync"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// scriptedIntercept selects fixed packet IDs and records every call it receives.
type scriptedIntercept struct {
	selects map[uint32]bool
	handle  func(Direction, packet.Packet) (Verdict, error)
	end     func(Direction) error
	packs   func() (above, below []*resource.Pack)

	mu     sync.Mutex
	events []string
	closed []error
}

func (intercept *scriptedIntercept) record(event string) {
	intercept.mu.Lock()
	intercept.events = append(intercept.events, event)
	intercept.mu.Unlock()
}

func (intercept *scriptedIntercept) recorded() []string {
	intercept.mu.Lock()
	defer intercept.mu.Unlock()
	return slices.Clone(intercept.events)
}

func (intercept *scriptedIntercept) Intercepts(_ Direction, id uint32) bool {
	return intercept.selects[id]
}

func (intercept *scriptedIntercept) BeginBatch(direction Direction) {
	intercept.record(fmt.Sprintf("begin %d", direction))
}

func (intercept *scriptedIntercept) HandlePacket(direction Direction, pk packet.Packet) (Verdict, error) {
	intercept.record(fmt.Sprintf("handle %d %T", direction, pk))
	if intercept.handle == nil {
		return Forward, nil
	}
	return intercept.handle(direction, pk)
}

func (intercept *scriptedIntercept) EndBatch(direction Direction) error {
	intercept.record(fmt.Sprintf("end %d", direction))
	if intercept.end == nil {
		return nil
	}
	return intercept.end(direction)
}

func (intercept *scriptedIntercept) Close(cause error) {
	intercept.mu.Lock()
	intercept.closed = append(intercept.closed, cause)
	intercept.mu.Unlock()
}

func (intercept *scriptedIntercept) ResourcePacks() (above, below []*resource.Pack) {
	if intercept.packs == nil {
		return nil, nil
	}
	return intercept.packs()
}

// rawRecordingDownstream records the IDs of packets forwarded as received bytes.
type rawRecordingDownstream struct {
	*fakeDownstream
	mu  sync.Mutex
	raw []uint32
}

func (session *rawRecordingDownstream) WritePacketRaw(data []byte) error {
	id, _ := sessionPacketID(data)
	session.mu.Lock()
	session.raw = append(session.raw, id)
	session.mu.Unlock()
	return session.fakeDownstream.WritePacketRaw(data)
}

// interceptorFunc adapts a function to an Interceptor.
type interceptorFunc func(context.Context, *InterceptedSession) (SessionInterceptor, error)

func (start interceptorFunc) InterceptSession(ctx context.Context, session *InterceptedSession) (SessionInterceptor, error) {
	return start(ctx, session)
}

// Verdicts and writes made while intercepting shape the one batch the client receives: unselected and
// forwarded packets keep their received bytes, a modified packet is re-encoded after packets written
// ahead of it, a dropped packet is gone, and EndBatch writes close the batch.
func TestInterceptorShapesTheForwardedBatch(t *testing.T) {
	up := newFakeUpstream(nil)
	up.useBatchReads = true
	down := &rawRecordingDownstream{fakeDownstream: newFakeDownstream(nil)}
	session := &InterceptedSession{client: down}
	untouched := &packet.SetTime{Time: 1}
	kept := &packet.Text{TextType: packet.TextTypeRaw, Message: "kept"}
	changed := &packet.SetTitle{Text: "old"}
	dropped := &packet.SetHealth{Health: 1}
	injected := &packet.Text{TextType: packet.TextTypeRaw, Message: "injected"}
	closing := &packet.SetTime{Time: 2}
	up.batchReads <- batchResult{packets: []packet.Packet{untouched, kept, changed, dropped}}
	up.batchReads <- batchResult{err: io.EOF}
	intercept := &scriptedIntercept{
		selects: map[uint32]bool{packet.IDText: true, packet.IDSetTitle: true, packet.IDSetHealth: true},
		handle: func(_ Direction, pk packet.Packet) (Verdict, error) {
			switch pk {
			case kept:
				return Forward, nil
			case changed:
				changed.Text = "new"
				return ForwardModified, session.WriteToClient(injected)
			case dropped:
				return Drop, nil
			}
			return Forward, fmt.Errorf("unexpected packet %T", pk)
		},
		end: func(Direction) error { return session.WriteToClient(closing) },
	}

	if err := pumpPacketsWithDelay(context.Background(), nil, intercept, up, down, false); !errors.Is(err, io.EOF) {
		t.Fatalf("pumpPacketsWithDelay() error = %v, want EOF", err)
	}
	want := [][]packet.Packet{{untouched, kept, injected, changed, closing}}
	if got := down.flushedBatches(); !slices.EqualFunc(got, want, slices.Equal) {
		t.Fatalf("batches = %#v, want %#v", got, want)
	}
	if changed.Text != "new" {
		t.Fatalf("modified title = %q", changed.Text)
	}
	if want := []uint32{packet.IDSetTime, packet.IDText}; !slices.Equal(down.raw, want) {
		t.Fatalf("forwarded as received = %v, want %v", down.raw, want)
	}
	wantEvents := []string{
		"begin 1", "handle 1 *packet.Text", "handle 1 *packet.SetTitle", "handle 1 *packet.SetHealth", "end 1",
	}
	if got := intercept.recorded(); !slices.Equal(got, wantEvents) {
		t.Fatalf("events = %q, want %q", got, wantEvents)
	}
}

// An interceptor error ends the relay in the packet's direction.
func TestInterceptorErrorEndsTheRelay(t *testing.T) {
	down := newFakeDownstream(nil)
	down.useBatchReads = true
	up := newFakeUpstream(nil)
	refused := errors.New("refused")
	down.batchReads <- batchResult{packets: []packet.Packet{&packet.Text{TextType: packet.TextTypeRaw, Message: "hi"}}}
	intercept := &scriptedIntercept{
		selects: map[uint32]bool{packet.IDText: true},
		handle:  func(Direction, packet.Packet) (Verdict, error) { return Forward, refused },
	}

	if err := pumpPacketsWithDelay(context.Background(), nil, intercept, down, up, true); !errors.Is(err, refused) {
		t.Fatalf("pumpPacketsWithDelay() error = %v, want %v", err, refused)
	}
	if got := up.written(); len(got) != 0 {
		t.Fatalf("forwarded %#v after the interceptor failed", got)
	}
	if got, want := intercept.recorded(), []string{"begin 0", "handle 0 *packet.Text"}; !slices.Equal(got, want) {
		t.Fatalf("events = %q, want %q", got, want)
	}
}

// The interceptor sees the upstream's game data and identity before the handoff, its packs surround the
// server's selection, and its error refuses the session.
func TestSessionPrepareStartsTheInterceptor(t *testing.T) {
	above := admissionPackWithUUID(t, "00000000-0000-0000-0000-00000000000a")
	below := admissionPackWithUUID(t, "00000000-0000-0000-0000-00000000000b")
	for _, test := range []struct {
		name    string
		failure error
	}{
		{name: "joined"},
		{name: "refused", failure: errors.New("refused")},
	} {
		t.Run(test.name, func(t *testing.T) {
			upstream := newFakeUpstream(nil)
			upstream.useBatchReads = true
			upstream.data = minecraft.GameData{EntityRuntimeID: 9}
			upstream.identity = login.IdentityData{DisplayName: "Canonical", XUID: "2535"}
			upstream.batchReads <- batchResult{packets: []packet.Packet{&packet.StartGame{EntityRuntimeID: 9}}}
			logger := slog.New(slog.NewTextHandler(io.Discard, nil))
			connections := newPreparedConnections("", nil, logger)
			connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
				return &preparedConnection{upstream: upstream, packStack: &selectedResourcePackStack{}}, nil
			}
			intercept := &scriptedIntercept{packs: func() ([]*resource.Pack, []*resource.Pack) {
				return []*resource.Pack{above}, []*resource.Pack{below, above}
			}}
			var seen *InterceptedSession
			server := &sessionServer{prepared: connections, transfers: new(TransferState), logger: logger,
				interceptor: interceptorFunc(func(_ context.Context, session *InterceptedSession) (SessionInterceptor, error) {
					seen = session
					if test.failure != nil {
						return nil, test.failure
					}
					return intercept, nil
				})}
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

			plan, prepared, err := server.prepare(ctx, cancel, session, downstream, nil)
			if test.failure != nil {
				if !errors.Is(err, test.failure) || prepared != nil {
					t.Fatalf("prepare() = %v, %v; want the interceptor's refusal", prepared, err)
				}
				if !upstream.isClosed() {
					t.Fatal("a refused session kept its upstream open")
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			defer prepared.close()
			if seen.GameData().EntityRuntimeID != 9 || seen.IdentityData().XUID != "2535" || seen.ClientData().ThirdPartyName != "Steve" {
				t.Fatalf("intercepted session = %+v, %+v, %q", seen.GameData(), seen.IdentityData(), seen.ClientData().ThirdPartyName)
			}
			if prepared.intercept != intercept {
				t.Fatal("prepared session does not own its interceptor")
			}
			var got []string
			for _, pack := range plan.handoff.Packs {
				got = append(got, pack.UUID)
			}
			if want := []string{above.UUID().String(), below.UUID().String()}; !slices.Equal(got, want) {
				t.Fatalf("handoff packs = %v, want %v", got, want)
			}
			if len(plan.packs) != 2 || plan.packs[0] != above || plan.packs[1] != below {
				t.Fatalf("pack archives = %v", plan.packs)
			}
		})
	}
}

// Cancellation during pack admission must close the interceptor before any handoff.
func TestCancelledPackDeliveryClosesTheInterceptor(t *testing.T) {
	upstream := newFakeUpstream(nil)
	upstream.useBatchReads = true
	upstream.batchReads <- batchResult{packets: []packet.Packet{&packet.StartGame{EntityRuntimeID: 9}}}
	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	connections := newPreparedConnections("", nil, logger)
	connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
		return &preparedConnection{upstream: upstream, packStack: &selectedResourcePackStack{}}, nil
	}
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	pack := testAdmissionPack(t)
	intercept := &scriptedIntercept{packs: func() ([]*resource.Pack, []*resource.Pack) {
		cancel()
		return []*resource.Pack{pack}, nil
	}}
	server := &sessionServer{prepared: connections, transfers: new(TransferState), logger: logger,
		interceptor: interceptorFunc(func(context.Context, *InterceptedSession) (SessionInterceptor, error) {
			return intercept, nil
		})}
	local, peer := net.Pipe()
	defer local.Close()
	defer peer.Close()
	done := make(chan error, 1)
	go func() {
		done <- server.serveConnWithPacks(ctx, local, &sessionPackDelivery{SessionPackStream: SessionPackStream{Join: 1}})
	}()
	frame, err := encodeSessionJSON(bridgecontract.SessionKindConnect, testSessionConnect(t))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := streamnet.NewFramedConn(peer).Write(frame); err != nil {
		t.Fatal(err)
	}
	if err := <-done; !errors.Is(err, context.Canceled) {
		t.Fatalf("serveConnWithPacks() = %v, want cancellation", err)
	}
	if len(intercept.closed) != 1 || !errors.Is(intercept.closed[0], context.Canceled) {
		t.Fatalf("interceptor close causes = %v, want one cancellation", intercept.closed)
	}
	if !upstream.isClosed() {
		t.Fatal("pack admission failure kept its upstream open")
	}
}

// Server packs keep their own position when an interceptor offers the same identity.
func TestAddInterceptedPacksKeepsServerPositions(t *testing.T) {
	server := admissionPackWithUUID(t, "00000000-0000-0000-0000-000000000001")
	added := admissionPackWithUUID(t, "00000000-0000-0000-0000-000000000002")
	selected := []sessionPack{{UUID: server.UUID().String(), Version: server.Version()}}
	gotSelected, gotPacks := addInterceptedPacks(selected, []*resource.Pack{server}, []*resource.Pack{server}, []*resource.Pack{added})
	if len(gotSelected) != 2 || gotSelected[0].UUID != server.UUID().String() || gotSelected[1].UUID != added.UUID().String() {
		t.Fatalf("selected = %+v", gotSelected)
	}
	if len(gotPacks) != 2 || gotPacks[0] != server || gotPacks[1] != added {
		t.Fatalf("packs = %v", gotPacks)
	}
}

// The rest of StartGame's batch is the interceptor's first server batch, so it can drop a packet
// before the client ever receives it.
func TestSessionHandoffInterceptsTheStartupBatch(t *testing.T) {
	local, peer := net.Pipe()
	session := newSessionConn(streamnet.NewFramedConn(local))
	frames := readSessionFrames(peer)
	plan := sessionPlan{
		startup: [][]byte{encodeTestPacket(&packet.StartGame{EntityRuntimeID: 9})},
		rest:    [][]byte{encodeTestPacket(&packet.Text{TextType: packet.TextTypeRaw, Message: "hidden"})},
	}
	intercept := &scriptedIntercept{
		selects: map[uint32]bool{packet.IDText: true},
		handle: func(_ Direction, pk packet.Packet) (Verdict, error) {
			if text, ok := pk.(*packet.Text); !ok || text.Message != "hidden" {
				return Forward, fmt.Errorf("unexpected packet %#v", pk)
			}
			return Drop, nil
		},
	}

	if err := writeSessionHandoff(session, plan, intercept); err != nil {
		t.Fatal(err)
	}
	_ = session.Close()
	var kinds []byte
	for frame := range frames {
		kinds = append(kinds, frame[0])
	}
	if want := []byte{bridgecontract.SessionKindHandoff}; !slices.Equal(kinds, want) {
		t.Fatalf("frame kinds = %v, want only the handoff", kinds)
	}
	if got, want := intercept.recorded(), []string{"begin 1", "handle 1 *packet.Text", "end 1"}; !slices.Equal(got, want) {
		t.Fatalf("events = %q, want %q", got, want)
	}
}
