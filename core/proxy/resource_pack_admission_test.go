package proxy

import (
	"archive/zip"
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

type offerTestDownstream struct {
	dialerTestDownstream
	configured      bool
	configuredOffer bool
	configuredStack bool
	offered         []*resource.Pack
	stack           minecraft.ResourcePackStackSnapshot
	required        bool
	offerRequired   bool
	err             error
	writes          []packet.Packet
	writeErr        error
	writePanic      any
	configPanic     any
	writeStarted    chan struct{}
	writeUnblock    <-chan struct{}
	writeStartOnce  sync.Once
}

func (downstream *offerTestDownstream) ConfigureResourcePackOfferSnapshot(offer minecraft.ResourcePackOfferSnapshot, required bool) error {
	if downstream.configPanic != nil {
		panic(downstream.configPanic)
	}
	downstream.configured = true
	downstream.configuredOffer = true
	downstream.offered = offer.Packs()
	downstream.offerRequired = required
	downstream.required = required
	return downstream.err
}

func (downstream *offerTestDownstream) WritePacketImmediate(packets ...packet.Packet) error {
	if downstream.writeStarted != nil {
		downstream.writeStartOnce.Do(func() { close(downstream.writeStarted) })
	}
	if downstream.writeUnblock != nil {
		<-downstream.writeUnblock
	}
	if downstream.writePanic != nil {
		panic(downstream.writePanic)
	}
	downstream.writes = append(downstream.writes, packets...)
	return downstream.writeErr
}

func (downstream *offerTestDownstream) ConfigureResourcePackStack(stack minecraft.ResourcePackStackSnapshot, required bool) error {
	if downstream.writeStarted != nil {
		downstream.writeStartOnce.Do(func() { close(downstream.writeStarted) })
	}
	if downstream.writeUnblock != nil {
		<-downstream.writeUnblock
	}
	if downstream.configPanic != nil {
		panic(downstream.configPanic)
	}
	downstream.configured = true
	downstream.configuredStack = true
	downstream.stack = stack
	downstream.required = required
	return downstream.err
}

func TestConfigureResourcePackOfferForwardsOptionalSelectedStack(t *testing.T) {
	upstream := newFakeUpstream(nil)
	upstream.packs = []*resource.Pack{new(resource.Pack), new(resource.Pack)}
	downstream := new(offerTestDownstream)

	stack := &selectedResourcePackStack{packs: slices.Clone(upstream.packs)}
	if err := configureResourcePackOffer(downstream, stack); err != nil {
		t.Fatalf("configureResourcePackOffer() error = %v", err)
	}
	if !downstream.configuredOffer || !downstream.configuredStack || downstream.required {
		t.Fatalf("downstream offer = (offer=%t, stack=%t, offered=%d, required=%t), want selected optional offer and stack", downstream.configuredOffer, downstream.configuredStack, len(downstream.offered), downstream.required)
	}
	if got := len(upstream.ResourcePacks()); got != 2 {
		t.Fatalf("retained upstream pack count = %d, want 2", got)
	}
}

func TestFailedOptionalConfigureDoesNotReportStrippedOutcome(t *testing.T) {
	wantErr := errors.New("configure failed")
	upstream := newFakeUpstream(nil)
	upstream.packs = []*resource.Pack{testAdmissionPack(t)}
	var snapshots []ResourcePackAdmissionSnapshot
	telemetry := newResourcePackAdmissionTelemetry(1, func(snapshot ResourcePackAdmissionSnapshot) {
		snapshots = append(snapshots, snapshot)
	})
	telemetry.observeOffer(upstream)
	prepared := &preparedConnection{upstream: upstream, packAdmission: telemetry, packStack: &selectedResourcePackStack{packs: slices.Clone(upstream.packs)}}
	connections := newTestPreparedConnections()
	connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
		return prepared, nil
	}
	downstream := &offerTestDownstream{err: wantErr}
	if err := connections.prepareConnection(context.Background(), new(minecraft.Conn), downstream); !errors.Is(err, wantErr) {
		t.Fatalf("prepareConnection() error = %v, want configure failure", err)
	}
	if len(snapshots) != 1 || snapshots[0].Offer != ResourcePackOfferOptional || snapshots[0].DownstreamOutcome != ResourcePackDownstreamNone {
		t.Fatalf("failed configure snapshot = %#v", snapshots)
	}
}

func TestSelectedStackCompatibilityDoesNotSubstituteOfferOrderOrCounts(t *testing.T) {
	offered := newFakeUpstream(nil)
	offered.packs = []*resource.Pack{testAdmissionPack(t), testAdmissionPack(t), testAdmissionPack(t)}
	telemetry := newResourcePackAdmissionTelemetry(1, nil)
	telemetry.observeOffer(offered)
	selected := &selectedResourcePackStack{packs: []*resource.Pack{offered.packs[2], offered.packs[0]}, required: true}

	downstream := new(offerTestDownstream)
	if err := configureResourcePackOffer(downstream, selected); err != nil {
		t.Fatalf("configureResourcePackOffer() error = %v", err)
	}
	if !downstream.configuredStack || downstream.required {
		t.Fatalf("downstream selected stack = (configured=%t, required=%t), want optional compatibility handoff", downstream.configuredStack, downstream.required)
	}
	if got := telemetry.snapshot().PackCount; got != 3 {
		t.Fatalf("offer telemetry count = %d, want downloaded offer count 3", got)
	}
}

func TestOptionalSelectedStackIsRetainedWhileDownstreamOfferIsForwarded(t *testing.T) {
	stack := &selectedResourcePackStack{packs: []*resource.Pack{testAdmissionPack(t), testAdmissionPack(t)}}
	downstream := new(offerTestDownstream)
	if err := configureResourcePackOffer(downstream, stack); err != nil {
		t.Fatalf("configureResourcePackOffer() error = %v", err)
	}
	if len(stack.packs) != 2 {
		t.Fatalf("retained selected count = %d, want 2", len(stack.packs))
	}
	if !downstream.configured || !downstream.configuredStack || downstream.required {
		t.Fatalf("downstream offer = (configured=%t, stack=%t, offered=%d, required=%t), want selected optional handoff", downstream.configured, downstream.configuredStack, len(downstream.offered), downstream.required)
	}
}

// An empty selection (every pack ignored) still configures an empty optional hop.
func TestConfigureResourcePackOfferStripsIgnoredSelection(t *testing.T) {
	downstream := new(offerTestDownstream)
	if err := configureResourcePackOffer(downstream, &selectedResourcePackStack{required: true}); err != nil {
		t.Fatalf("configureResourcePackOffer() error = %v", err)
	}
	if !downstream.configuredOffer || !downstream.configuredStack || downstream.required || len(downstream.offered) != 0 {
		t.Fatalf("downstream offer = (offer=%t, stack=%t, offered=%d, required=%t), want empty optional hop", downstream.configuredOffer, downstream.configuredStack, len(downstream.offered), downstream.required)
	}
}

func TestSelectedResourcePackStackRequiresNegotiatedSnapshots(t *testing.T) {
	if _, err := captureSelectedResourcePackStack(newFakeUpstream(nil), nil); !errors.Is(err, errResourcePackStackUnavailable) {
		t.Fatalf("missing post-negotiation snapshot error = %v", err)
	}
	if err := configureResourcePackOffer(new(offerTestDownstream), nil); !errors.Is(err, errResourcePackStackUnavailable) {
		t.Fatalf("nil prepared stack policy error = %v", err)
	}
}

func TestPreparedConnectionReleasesEveryResourceOnce(t *testing.T) {
	for _, relay := range []bool{false, true} {
		t.Run(fmt.Sprintf("relay=%t", relay), func(t *testing.T) {
			down, up := newFakeDownstream(nil), newFakeUpstream(nil)
			stack := &selectedResourcePackStack{packs: []*resource.Pack{testAdmissionPack(t)}}
			releases := 0
			prepared := &preparedConnection{
				upstream: up, packStack: stack,
				releaseTarget: func() error { releases++; return nil },
			}
			if relay {
				down.reads <- packetResult{err: io.EOF}
				if err := servePreparedConnection(context.Background(), down, prepared); err != nil {
					t.Fatal(err)
				}
			}
			for range 2 {
				if err := prepared.close(); err != nil {
					t.Fatal(err)
				}
			}
			if stack.packs != nil || releases != 1 {
				t.Fatalf("released packs = %v, target releases = %d", stack.packs == nil, releases)
			}
			if got := up.lifecycleEvents(); !slices.Equal(got, []string{"abort", "close"}) {
				t.Fatalf("upstream lifecycle = %v", got)
			}
			want := []string(nil)
			if relay {
				want = []string{"abort", "close"}
			}
			if got := down.lifecycleEvents(); !slices.Equal(got, want) {
				t.Fatalf("downstream lifecycle = %v, want %v", got, want)
			}
		})
	}
}

func TestPreparationErrorReportingPreservesSetupContractAndBoundsQueue(t *testing.T) {
	serveCtx := context.Background()
	errorsOut := make(chan error, 1)
	first := errors.New("dial failed")
	reportPreparationError(errorsOut, first, serveCtx)
	reportPreparationError(errorsOut, errors.New("second dial failed"), serveCtx)
	got := <-errorsOut
	if !errors.Is(got, first) || !strings.Contains(got.Error(), "proxy: prepare upstream") {
		t.Fatalf("reported error = %v, want wrapped first setup failure", got)
	}
	select {
	case extra := <-errorsOut:
		t.Fatalf("bounded error queue retained extra failure: %v", extra)
	default:
	}
}

func TestPreparationErrorReportingKeepsExpectedPerClientFailuresLocal(t *testing.T) {
	errorsOut := make(chan error, 1)
	reportPreparationError(errorsOut, &PackAdmissionError{Reason: PackAdmissionRequiredUnsupported, PackCount: 1}, context.Background())
	reportPreparationError(errorsOut, &preparationCancellationError{cause: context.Canceled}, context.Background())
	stoppedCtx, cancel := context.WithCancel(context.Background())
	cancel()
	reportPreparationError(errorsOut, errors.New("dial failed during shutdown"), stoppedCtx)
	select {
	case got := <-errorsOut:
		t.Fatalf("per-client/shutdown failure escaped to Serve: %v", got)
	default:
	}
}

func TestPreparationErrorReportingSurfacesUpstreamOrdinaryCloseDuringSetup(t *testing.T) {
	for _, setupErr := range []error{io.EOF, net.ErrClosed, context.Canceled} {
		errorsOut := make(chan error, 1)
		reportPreparationError(errorsOut, setupErr, context.Background())
		select {
		case got := <-errorsOut:
			if !errors.Is(got, setupErr) {
				t.Fatalf("reported error = %v, want %v", got, setupErr)
			}
		default:
			t.Fatalf("setup error %v was suppressed", setupErr)
		}
	}
}

func TestListenerBoundaryPreparesBeforeLoginAndHandsOffExactConnection(t *testing.T) {
	connections := newTestPreparedConnections()
	prepared, targetCloses := newTrackedPreparedConnection()
	prepared.upstream.(*fakeUpstream).packs = []*resource.Pack{new(resource.Pack)}
	var prepareCount atomic.Int32
	var preparedDownstream atomic.Pointer[minecraft.Conn]
	var eventsMu sync.Mutex
	var events []string
	connections.connectPrepared = func(_ context.Context, downstream dialerDownstream) (*preparedConnection, error) {
		prepareCount.Add(1)
		preparedDownstream.Store(downstream.(*minecraft.Conn))
		eventsMu.Lock()
		events = append(events, "prepare")
		eventsMu.Unlock()
		return prepared, nil
	}

	listener, network := newAdmissionTestListener(t, connections.prepare)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	clientDone := make(chan admissionDialResult, 1)
	go func() {
		client, err := (minecraft.Dialer{
			IdentityData: login.IdentityData{DisplayName: "AdmissionTest"},
			Protocol:     minecraft.DefaultProtocol,
			PacketFunc: func(header packet.Header, _ []byte, _, _ net.Addr) {
				if header.PacketID == packet.IDPlayStatus || header.PacketID == packet.IDResourcePacksInfo {
					eventsMu.Lock()
					events = append(events, fmt.Sprintf("packet:%d", header.PacketID))
					eventsMu.Unlock()
				}
			},
		}).DialContextNetwork(ctx, network, "")
		clientDone <- admissionDialResult{conn: client, err: err}
	}()

	acceptDone := make(chan acceptResult, 1)
	go func() {
		conn, err := listener.Accept()
		acceptDone <- acceptResult{conn: conn, err: err}
	}()
	acceptedResult := <-acceptDone
	if acceptedResult.err != nil {
		t.Fatalf("listener Accept: %v", acceptedResult.err)
	}
	accepted := acceptedResult.conn.(*minecraft.Conn)
	if got := prepareCount.Load(); got != 1 {
		t.Fatalf("prepare count = %d, want 1", got)
	}
	if got := preparedDownstream.Load(); got != accepted {
		t.Fatalf("prepared downstream = %p, accepted = %p", got, accepted)
	}
	taken, err := takePreparedAfterAccept(connections, accepted)
	if err != nil || taken != prepared {
		t.Fatalf("takePreparedAfterAccept = (%p, %v), want (%p, nil)", taken, err, prepared)
	}
	if err := accepted.StartGameContext(ctx, minecraft.GameData{EntityRuntimeID: 1}); err != nil {
		t.Fatalf("start downstream game: %v", err)
	}
	clientResult := <-clientDone
	if clientResult.err != nil {
		t.Fatalf("client dial: %v", clientResult.err)
	}
	defer clientResult.conn.Close()
	if len(clientResult.conn.ResourcePacks()) != 0 {
		t.Fatal("private compatibility hop exposed pack content")
	}
	if offer, ok := clientResult.conn.ResourcePackOffer(); !ok || len(offer.TexturePacks()) != 0 || offer.TexturePackRequired() {
		t.Fatalf("private compatibility offer = (available=%t, entries=%d, required=%t), want empty optional", ok, len(offer.TexturePacks()), offer.TexturePackRequired())
	}
	if stack, ok := clientResult.conn.ResourcePackStack(); !ok || len(stack.Entries()) != 0 || stack.Required() {
		t.Fatalf("private compatibility stack = (available=%t, entries=%d, required=%t), want empty optional", ok, len(stack.Entries()), stack.Required())
	}
	eventsMu.Lock()
	gotEvents := slices.Clone(events)
	eventsMu.Unlock()
	prepareIndex := slices.Index(gotEvents, "prepare")
	loginIndex := slices.Index(gotEvents, fmt.Sprintf("packet:%d", packet.IDPlayStatus))
	infoIndex := slices.Index(gotEvents, fmt.Sprintf("packet:%d", packet.IDResourcePacksInfo))
	if prepareIndex < 0 || loginIndex < 0 || infoIndex < 0 || prepareIndex > loginIndex || prepareIndex > infoIndex {
		t.Fatalf("listener events = %v, want prepare before LoginSuccess and ResourcePacksInfo", gotEvents)
	}
	if err := taken.close(); err != nil {
		t.Fatalf("close prepared: %v", err)
	}
	assertPreparedClosedExactlyOnce(t, prepared, targetCloses)
}

func TestListenerBoundaryLoggerPanicAfterTakeClosesTransferredOwnership(t *testing.T) {
	stringCalls := new(atomic.Int32)
	panicValue := sensitivePanic{stringCalls: stringCalls}
	handler := newSelectivePanicHandler("local client accepted", panicValue)
	logger := slog.New(handler)
	connections := newTestPreparedConnections()
	targetCloses := new(atomic.Int32)
	prepared := &preparedConnection{
		upstream:  newFakeUpstream(nil),
		packStack: &selectedResourcePackStack{},
		releaseTarget: func() error {
			targetCloses.Add(1)
			return nil
		},
	}
	connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
		return prepared, nil
	}
	listener, network := newAdmissionTestListener(t, connections.prepare)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	clientDone := make(chan admissionDialResult, 1)
	go func() {
		client, err := (minecraft.Dialer{
			IdentityData: login.IdentityData{DisplayName: "HandoffPanic"},
			Protocol:     minecraft.DefaultProtocol,
		}).DialContextNetwork(ctx, network, "")
		clientDone <- admissionDialResult{conn: client, err: err}
	}()
	acceptedRaw, err := listener.Accept()
	if err != nil {
		t.Fatalf("listener Accept: %v", err)
	}
	accepted := acceptedRaw.(*minecraft.Conn)
	taken, err := takePreparedAfterAccept(connections, accepted)
	if err != nil || taken != prepared {
		t.Fatalf("takePreparedAfterAccept = (%p, %v), want (%p, nil)", taken, err, prepared)
	}
	trackedDownstream := &trackedAcceptedDownstream{Conn: accepted}
	err = serveAcceptedConnection(ctx, trackedDownstream, taken, "test-socket", logger)
	if err == nil || !strings.Contains(err.Error(), "type proxy.sensitivePanic") {
		t.Fatalf("serveAcceptedConnection() error = %v, want type-only logger panic", err)
	}
	if stringCalls.Load() != 0 || strings.Contains(err.Error(), "sensitive panic payload") {
		t.Fatalf("panic payload formatted: error=%q String calls=%d", err, stringCalls.Load())
	}
	clientResult := <-clientDone
	if clientResult.conn != nil {
		_ = clientResult.conn.Close()
	}
	if clientResult.err == nil {
		t.Fatal("client dial succeeded after handoff logger panic")
	}
	if trackedDownstream.abortCalls.Load() != 1 || trackedDownstream.closeCalls.Load() != 1 {
		t.Fatalf("downstream cleanup abort=%d close=%d, want 1 each", trackedDownstream.abortCalls.Load(), trackedDownstream.closeCalls.Load())
	}
	if lifecycle := prepared.upstream.(*fakeUpstream).lifecycleEvents(); !slices.Equal(lifecycle, []string{"abort", "close"}) {
		t.Fatalf("upstream lifecycle = %v, want [abort close]", lifecycle)
	}
	if targetCloses.Load() != 1 {
		t.Fatalf("target closes=%d, want 1", targetCloses.Load())
	}
	_ = prepared.close()
	if targetCloses.Load() != 1 {
		t.Fatal("second prepared close repeated target or telemetry cleanup")
	}
}

func TestListenerBoundaryShutdownDuringPreparationJoinsHook(t *testing.T) {
	connections := newTestPreparedConnections()
	started := make(chan struct{})
	connectReturned := make(chan struct{})
	connections.connectPrepared = func(ctx context.Context, _ dialerDownstream) (*preparedConnection, error) {
		close(started)
		<-ctx.Done()
		close(connectReturned)
		return nil, ctx.Err()
	}
	listener, network := newAdmissionTestListener(t, connections.prepare)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	dialDone := make(chan error, 1)
	go func() {
		conn, err := (minecraft.Dialer{
			IdentityData: login.IdentityData{DisplayName: "ShutdownTest"},
			Protocol:     minecraft.DefaultProtocol,
		}).DialContextNetwork(ctx, network, "")
		if conn != nil {
			_ = conn.Close()
		}
		dialDone <- err
	}()
	<-started
	if err := connections.shutdown(); err != nil {
		t.Fatalf("shutdown: %v", err)
	}
	select {
	case <-connectReturned:
	default:
		t.Fatal("shutdown returned before in-flight preparation")
	}
	if err := <-dialDone; err == nil {
		t.Fatal("client dial succeeded after preparation shutdown")
	}
	_ = listener.Close()
}

func TestListenerBoundaryDisconnectBetweenAcceptAndTakeIsPerClient(t *testing.T) {
	connections := newTestPreparedConnections()
	prepared, targetCloses := newTrackedPreparedConnection()
	allowUpstreamClose := make(chan struct{})
	defer func() {
		select {
		case <-allowUpstreamClose:
		default:
			close(allowUpstreamClose)
		}
		select {
		case <-targetCloses.done:
		case <-time.After(time.Second):
		}
	}()
	upstream := &gatedCloseUpstream{
		fakeUpstream: prepared.upstream.(*fakeUpstream),
		closeStarted: make(chan struct{}),
		allowClose:   allowUpstreamClose,
	}
	prepared.upstream = upstream
	connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
		return prepared, nil
	}
	listener, network := newAdmissionTestListener(t, connections.prepare)
	dialCtx, cancelDial := context.WithTimeout(context.Background(), 5*time.Second)
	dialDone := make(chan error, 1)
	go func() {
		conn, err := (minecraft.Dialer{
			IdentityData: login.IdentityData{DisplayName: "DisconnectTest"},
			Protocol:     minecraft.DefaultProtocol,
		}).DialContextNetwork(dialCtx, network, "")
		if conn != nil {
			_ = conn.Close()
		}
		dialDone <- err
	}()
	acceptedRaw, err := listener.Accept()
	if err != nil {
		t.Fatalf("listener Accept: %v", err)
	}
	accepted := acceptedRaw.(*minecraft.Conn)
	cancelDial()
	if err := <-dialDone; err == nil {
		t.Fatal("client dial succeeded without StartGame")
	}
	select {
	case <-accepted.Context().Done():
	case <-time.After(time.Second):
		t.Fatal("accepted connection context was not cancelled")
	}
	select {
	case <-upstream.closeStarted:
	case <-time.After(time.Second):
		t.Fatal("prepared connection cleanup did not reach upstream Close")
	}
	if lifecycle := upstream.lifecycleEvents(); !slices.Equal(lifecycle, []string{"abort"}) || targetCloses.Load() != 0 {
		t.Fatalf("paused cleanup = (lifecycle=%v, target closes=%d), want ([abort], 0)", lifecycle, targetCloses.Load())
	}
	taken, err := takePreparedAfterAccept(connections, accepted)
	if err != nil || taken != nil {
		t.Fatalf("takePreparedAfterAccept = (%p, %v), want ordinary per-client teardown", taken, err)
	}
	close(allowUpstreamClose)
	waitForPreparedTransportCleanup(t, prepared, targetCloses)
	assertPreparedClosedExactlyOnce(t, prepared, targetCloses)
}

type gatedCloseUpstream struct {
	*fakeUpstream
	closeStarted chan struct{}
	allowClose   <-chan struct{}
}

func (upstream *gatedCloseUpstream) Close() error {
	close(upstream.closeStarted)
	<-upstream.allowClose
	return upstream.fakeUpstream.Close()
}

func TestListenerBoundaryContainsDialPanicBeforeLoginPackets(t *testing.T) {
	var output lockedBuffer
	logger := slog.New(slog.NewTextHandler(&output, nil))
	connections := newPreparedConnections("unused.invalid:19132", nil, logger)
	targetCloses := new(atomic.Int32)
	connections.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{
			address: "unused.invalid:19132",
			network: minecraft.RakNet{},
			friend: closerFunc(func() error {
				targetCloses.Add(1)
				return nil
			}),
		}, nil
	}
	var dialCount atomic.Int32
	connections.dialTarget = func(context.Context, *resolvedUpstreamTarget, minecraft.Dialer) (upstreamSession, error) {
		dialCount.Add(1)
		panic("listener dial panic")
	}
	_, network := newAdmissionTestListener(t, connections.prepare)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	var packetsMu sync.Mutex
	var packetIDs []uint32
	conn, err := (minecraft.Dialer{
		IdentityData: login.IdentityData{DisplayName: "PanicTest"},
		Protocol:     minecraft.DefaultProtocol,
		PacketFunc: func(header packet.Header, _ []byte, _, _ net.Addr) {
			packetsMu.Lock()
			packetIDs = append(packetIDs, header.PacketID)
			packetsMu.Unlock()
		},
	}).DialContextNetwork(ctx, network, "")
	if conn != nil {
		_ = conn.Close()
	}
	if err == nil {
		t.Fatal("client dial succeeded after preparation panic")
	}
	if got := dialCount.Load(); got != 1 {
		t.Fatalf("dial count = %d, want 1", got)
	}
	if got := targetCloses.Load(); got != 1 {
		t.Fatalf("target close count = %d, want 1", got)
	}
	packetsMu.Lock()
	gotPackets := slices.Clone(packetIDs)
	packetsMu.Unlock()
	if slices.Contains(gotPackets, packet.IDPlayStatus) || slices.Contains(gotPackets, packet.IDResourcePacksInfo) {
		t.Fatalf("packets after dial panic = %v, must not include LoginSuccess or ResourcePacksInfo", gotPackets)
	}
}

func TestListenerBoundaryConnectedLogPanicCleansAllOwnershipBeforeLogin(t *testing.T) {
	stringCalls := new(atomic.Int32)
	panicValue := sensitivePanic{stringCalls: stringCalls}
	handler := newSelectivePanicHandler("upstream connected", panicValue)
	logger := slog.New(handler)
	connections := newPreparedConnections("unused.invalid:19132", nil, logger)
	targetCloses := new(atomic.Int32)
	connections.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{
			address: "unused.invalid:19132",
			network: minecraft.RakNet{},
			friend: closerFunc(func() error {
				targetCloses.Add(1)
				return nil
			}),
		}, nil
	}
	upstream := newFakeUpstream(nil)
	connections.dialTarget = func(ctx context.Context, target *resolvedUpstreamTarget, _ minecraft.Dialer) (upstreamSession, error) {
		return connectUpstream(ctx, target.address, "offline", logger, func(context.Context, string) (upstreamSession, error) {
			return upstream, nil
		})
	}
	serverErrors := make(chan error, 1)
	prepare := func(ctx context.Context, conn *minecraft.Conn) error {
		err := connections.prepare(ctx, conn)
		reportPreparationError(serverErrors, err, context.Background())
		return err
	}
	_, network := newAdmissionTestListener(t, prepare)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	var packetsMu sync.Mutex
	var packetIDs []uint32
	conn, err := (minecraft.Dialer{
		IdentityData: login.IdentityData{DisplayName: "LogPanicTest"},
		Protocol:     minecraft.DefaultProtocol,
		PacketFunc: func(header packet.Header, _ []byte, _, _ net.Addr) {
			packetsMu.Lock()
			packetIDs = append(packetIDs, header.PacketID)
			packetsMu.Unlock()
		},
	}).DialContextNetwork(ctx, network, "")
	if conn != nil {
		_ = conn.Close()
	}
	if err == nil {
		t.Fatal("client dial succeeded after connected-log panic")
	}
	select {
	case setupErr := <-serverErrors:
		if !strings.Contains(setupErr.Error(), "type proxy.sensitivePanic") || stringCalls.Load() != 0 {
			t.Fatalf("setup error = %q, String calls=%d", setupErr, stringCalls.Load())
		}
	case <-time.After(time.Second):
		t.Fatal("connected-log panic was not surfaced")
	}
	if lifecycle := upstream.lifecycleEvents(); !slices.Equal(lifecycle, []string{"abort", "close"}) {
		t.Fatalf("upstream lifecycle = %v, want [abort close]", lifecycle)
	}
	if targetCloses.Load() != 1 {
		t.Fatalf("target closes=%d, want 1", targetCloses.Load())
	}
	packetsMu.Lock()
	gotPackets := slices.Clone(packetIDs)
	packetsMu.Unlock()
	if slices.Contains(gotPackets, packet.IDPlayStatus) || slices.Contains(gotPackets, packet.IDResourcePacksInfo) {
		t.Fatalf("packets after connected-log panic = %v, must not include LoginSuccess or ResourcePacksInfo", gotPackets)
	}
}

func TestListenerBoundaryRejectsMissingSelectedStackBeforeLoginPackets(t *testing.T) {
	connections := newTestPreparedConnections()
	prepared, targetCloses := newTrackedPreparedConnection()
	prepared.packStack = nil
	connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
		return prepared, nil
	}
	serverErrors := make(chan error, 1)
	prepare := func(ctx context.Context, conn *minecraft.Conn) error {
		err := connections.prepare(ctx, conn)
		reportPreparationError(serverErrors, err, context.Background())
		return err
	}
	_, network := newAdmissionTestListener(t, prepare)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	var packetsMu sync.Mutex
	var packetIDs []uint32
	conn, err := (minecraft.Dialer{
		IdentityData: login.IdentityData{DisplayName: "OfferPanicTest"},
		Protocol:     minecraft.DefaultProtocol,
		PacketFunc: func(header packet.Header, _ []byte, _, _ net.Addr) {
			packetsMu.Lock()
			packetIDs = append(packetIDs, header.PacketID)
			packetsMu.Unlock()
		},
	}).DialContextNetwork(ctx, network, "")
	if conn != nil {
		_ = conn.Close()
	}
	if err == nil {
		t.Fatal("client dial succeeded without a selected stack snapshot")
	}
	select {
	case setupErr := <-serverErrors:
		if !errors.Is(setupErr, errResourcePackStackUnavailable) {
			t.Fatalf("setup error = %v, want unavailable selected stack", setupErr)
		}
	case <-time.After(time.Second):
		t.Fatal("missing selected stack was not surfaced")
	}
	packetsMu.Lock()
	gotPackets := slices.Clone(packetIDs)
	packetsMu.Unlock()
	if slices.Contains(gotPackets, packet.IDPlayStatus) || slices.Contains(gotPackets, packet.IDResourcePacksInfo) {
		t.Fatalf("packets after missing stack = %v, must not include LoginSuccess or ResourcePacksInfo", gotPackets)
	}
	assertPreparedClosedExactlyOnce(t, prepared, targetCloses)
}

func TestPreparationRetainsRequiredUpstreamWhileConfiguringOptionalLocalHandoff(t *testing.T) {
	connections := newTestPreparedConnections()
	prepared, targetCloses := newTrackedPreparedConnection()
	prepared.upstream.(*fakeUpstream).packs = []*resource.Pack{new(resource.Pack)}
	prepared.upstream.(*fakeUpstream).required = true
	prepared.packStack = &selectedResourcePackStack{packs: []*resource.Pack{new(resource.Pack)}, required: true}
	connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
		return prepared, nil
	}
	downstream := new(offerTestDownstream)
	key := new(minecraft.Conn)
	if err := connections.prepareConnection(context.Background(), key, downstream); err != nil {
		t.Fatalf("prepareConnection() error = %v", err)
	}
	if !downstream.configuredStack || downstream.required || len(downstream.writes) != 0 {
		t.Fatalf("required compatibility handoff = (stack=%t, required=%t, writes=%d), want optional stack without Disconnect", downstream.configuredStack, downstream.required, len(downstream.writes))
	}
	retained, ok := connections.take(key)
	if !ok || retained != prepared || !retained.packStack.required {
		t.Fatalf("retained prepared connection = (%p, %t, required=%t), want exact required upstream ownership", retained, ok, retained != nil && retained.packStack.required)
	}
	if err := retained.close(); err != nil {
		t.Fatalf("close retained connection: %v", err)
	}
	assertPreparedClosedExactlyOnce(t, prepared, targetCloses)
}

func TestListenerBoundarySurfacesUnexpectedPreparationFailure(t *testing.T) {
	for name, setupErr := range map[string]error{
		"application error": errors.New("upstream unavailable"),
		"upstream EOF":      io.EOF,
		"upstream closed":   net.ErrClosed,
	} {
		t.Run(name, func(t *testing.T) {
			connections := newTestPreparedConnections()
			connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
				return nil, setupErr
			}
			serverErrors := make(chan error, 1)
			prepare := func(ctx context.Context, conn *minecraft.Conn) error {
				err := connections.prepare(ctx, conn)
				reportPreparationError(serverErrors, err, context.Background())
				return err
			}
			_, network := newAdmissionTestListener(t, prepare)
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			conn, dialErr := (minecraft.Dialer{
				IdentityData: login.IdentityData{DisplayName: "SetupFailTest"},
				Protocol:     minecraft.DefaultProtocol,
			}).DialContextNetwork(ctx, network, "")
			if conn != nil {
				_ = conn.Close()
			}
			if dialErr == nil {
				t.Fatal("client dial succeeded after setup failure")
			}
			select {
			case got := <-serverErrors:
				if !errors.Is(got, setupErr) || !strings.Contains(got.Error(), "proxy: prepare upstream") {
					t.Fatalf("server setup error = %v", got)
				}
			case <-time.After(time.Second):
				t.Fatal("unexpected preparation failure was not surfaced")
			}
		})
	}
}

type admissionDialResult struct {
	conn *minecraft.Conn
	err  error
}

func newAdmissionTestListener(t *testing.T, prepare func(context.Context, *minecraft.Conn) error) (*minecraft.Listener, minecraft.Network) {
	t.Helper()
	network := streamnet.New(filepath.Join(t.TempDir(), "socket"))
	listener, err := (minecraft.ListenConfig{
		AuthenticationDisabled:   true,
		AllowUnknownPackets:      true,
		EnableBatchReading:       true,
		ErrorLog:                 slog.New(slog.NewTextHandler(io.Discard, nil)),
		PrepareResourcePackOffer: prepare,
	}).ListenNetwork(network, "")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	return listener, network
}

func TestPinnedDialerIgnorePolicyAllowsOptionalAndRequiredOffersToReachStartGame(t *testing.T) {
	for _, test := range []struct {
		name     string
		offer    bool
		required bool
	}{
		{name: "no-pack"},
		{name: "optional", offer: true},
		{name: "required", offer: true, required: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			pack := testAdmissionPack(t)
			var packs []*resource.Pack
			if test.offer {
				packs = []*resource.Pack{pack}
			}
			listener, network := newAdmissionTestListener(t, func(_ context.Context, conn *minecraft.Conn) error {
				return conn.ConfigureResourcePackOffer(packs, test.required)
			})
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			var packetsMu sync.Mutex
			var packetIDs []uint32
			clientDone := make(chan admissionDialResult, 1)
			go func() {
				client, err := (minecraft.Dialer{
					IdentityData: login.IdentityData{DisplayName: "IgnorePolicy"},
					Protocol:     minecraft.DefaultProtocol,
					DownloadResourcePack: func(uuid.UUID, string, int, int) bool {
						return false
					},
					PacketFunc: func(header packet.Header, _ []byte, _, _ net.Addr) {
						packetsMu.Lock()
						packetIDs = append(packetIDs, header.PacketID)
						packetsMu.Unlock()
					},
				}).DialContextNetwork(ctx, network, "")
				clientDone <- admissionDialResult{conn: client, err: err}
			}()

			acceptedRaw, err := listener.Accept()
			if err != nil {
				t.Fatalf("listener Accept: %v", err)
			}
			accepted := acceptedRaw.(*minecraft.Conn)
			defer accepted.Close()
			startGameDone := make(chan error, 1)
			go func() {
				startGameDone <- accepted.StartGameContext(ctx, minecraft.GameData{EntityRuntimeID: 9})
			}()
			result := <-clientDone
			if result.err != nil {
				t.Fatalf("dial through ignored offer: %v", result.err)
			}
			defer result.conn.Close()
			if err := result.conn.Flush(); err != nil {
				t.Fatalf("flush final client admission acknowledgement: %v", err)
			}
			if err := <-startGameDone; err != nil {
				t.Fatalf("start game after client admission: %v", err)
			}
			if result.conn.GameData().EntityRuntimeID != 9 {
				t.Fatal("dial did not reach StartGame")
			}
			offer, ok := result.conn.ResourcePackOffer()
			wantCount := 0
			if test.offer {
				wantCount = 1
			}
			if !ok || len(offer.TexturePacks()) != wantCount || offer.TexturePackRequired() != test.required {
				t.Fatalf("observed offer = (available=%t, count=%d, required=%t)", ok, len(offer.TexturePacks()), offer.TexturePackRequired())
			}
			if len(result.conn.ResourcePacks()) != 0 {
				t.Fatal("ignored offer acquired local pack content")
			}
			telemetry := newResourcePackAdmissionTelemetry(1, nil)
			telemetry.observeOffer(result.conn)
			telemetry.observePolicyOutcome(&selectedResourcePackStack{required: test.required}, true)
			snapshot := telemetry.snapshot()
			wantOffer := ResourcePackOfferNone
			wantAcquisition := ResourcePackAcquisitionNone
			wantOutcome := ResourcePackDownstreamNone
			if test.offer {
				wantOffer = ResourcePackOfferOptional
				wantAcquisition = ResourcePackAcquisitionIgnored
				wantOutcome = ResourcePackDownstreamStrippedIgnored
			}
			if test.required {
				wantOffer = ResourcePackOfferRequired
			}
			wantBytes := uint64(0)
			if test.offer {
				wantBytes = uint64(pack.Size())
			}
			if snapshot.Offer != wantOffer || snapshot.PackCount != uint32(wantCount) || snapshot.TotalBytes != wantBytes || snapshot.Acquisition != wantAcquisition || snapshot.DownstreamOutcome != wantOutcome || snapshot.Application != ResourcePackApplicationUnavailable {
				t.Fatalf("compatibility telemetry = %#v", snapshot)
			}
			stack, ok := result.conn.ResourcePackStack()
			ignoredEntry := false
			for _, entry := range stack.Entries() {
				if entry.UUID() == pack.UUID().String() && entry.Version() == pack.Version() && entry.Pack() == nil {
					ignoredEntry = true
					break
				}
			}
			if !ok || ignoredEntry != test.offer {
				t.Fatalf("ignored stack = (available=%t, entries=%d), want advertised pack as metadata-only entry", ok, len(stack.Entries()))
			}
			packetsMu.Lock()
			gotPackets := slices.Clone(packetIDs)
			packetsMu.Unlock()
			if slices.Contains(gotPackets, packet.IDResourcePackDataInfo) || slices.Contains(gotPackets, packet.IDResourcePackChunkData) {
				t.Fatalf("ignored offer transferred local pack data: packet IDs %v", gotPackets)
			}
		})
	}
}

func TestPreparedConnectionsUsesExactDownstreamIdentityAndTakesOnce(t *testing.T) {
	connections := newTestPreparedConnections()
	firstDownstream := new(minecraft.Conn)
	secondDownstream := new(minecraft.Conn)
	first, _ := newTrackedPreparedConnection()
	second, _ := newTrackedPreparedConnection()

	if err := connections.store(context.Background(), firstDownstream, first); err != nil {
		t.Fatalf("store first: %v", err)
	}
	if err := connections.store(context.Background(), secondDownstream, second); err != nil {
		t.Fatalf("store second: %v", err)
	}
	if got, ok := connections.take(firstDownstream); !ok || got != first {
		t.Fatalf("take first = (%p, %t), want (%p, true)", got, ok, first)
	}
	if got, ok := connections.take(firstDownstream); ok || got != nil {
		t.Fatalf("second take first = (%p, %t), want (nil, false)", got, ok)
	}
	if got, ok := connections.take(secondDownstream); !ok || got != second {
		t.Fatalf("take second = (%p, %t), want (%p, true)", got, ok, second)
	}
	if err := first.close(); err != nil {
		t.Fatalf("close first: %v", err)
	}
	if err := second.close(); err != nil {
		t.Fatalf("close second: %v", err)
	}
}

func TestPreparedConnectionsCancellationBeforeTakeClosesExactlyOnce(t *testing.T) {
	connections := newTestPreparedConnections()
	downstream := new(minecraft.Conn)
	prepared, targetCloses := newTrackedPreparedConnection()
	ctx, cancel := context.WithCancel(context.Background())
	if err := connections.store(ctx, downstream, prepared); err != nil {
		t.Fatalf("store: %v", err)
	}
	cancel()

	waitForPreparedTransportCleanup(t, prepared, targetCloses)
	if got, ok := connections.take(downstream); ok || got != nil {
		t.Fatalf("take after cancellation = (%p, %t), want (nil, false)", got, ok)
	}
	if err := prepared.close(); err != nil {
		t.Fatalf("second close: %v", err)
	}
	assertPreparedClosedExactlyOnce(t, prepared, targetCloses)
}

func TestPreparedConnectionsTakeCancellationRaceHasOneOwner(t *testing.T) {
	for iteration := 0; iteration < 100; iteration++ {
		connections := newTestPreparedConnections()
		downstream := new(minecraft.Conn)
		prepared, targetCloses := newTrackedPreparedConnection()
		ctx, cancel := context.WithCancel(context.Background())
		if err := connections.store(ctx, downstream, prepared); err != nil {
			t.Fatalf("iteration %d store: %v", iteration, err)
		}

		start := make(chan struct{})
		var taken *preparedConnection
		var took bool
		var wait sync.WaitGroup
		wait.Add(2)
		go func() {
			defer wait.Done()
			<-start
			cancel()
		}()
		go func() {
			defer wait.Done()
			<-start
			taken, took = connections.take(downstream)
		}()
		close(start)
		wait.Wait()
		if took {
			if taken != prepared {
				t.Fatalf("iteration %d took %p, want %p", iteration, taken, prepared)
			}
			if err := taken.close(); err != nil {
				t.Fatalf("iteration %d close taken: %v", iteration, err)
			}
		} else {
			waitForPreparedTransportCleanup(t, prepared, targetCloses)
		}
		assertPreparedClosedExactlyOnce(t, prepared, targetCloses)
	}
}

func TestPreparedConnectionsRejectsDuplicateWithoutTakingOwnership(t *testing.T) {
	connections := newTestPreparedConnections()
	downstream := new(minecraft.Conn)
	first, _ := newTrackedPreparedConnection()
	duplicate, duplicateTargetCloses := newTrackedPreparedConnection()
	if err := connections.store(context.Background(), downstream, first); err != nil {
		t.Fatalf("store first: %v", err)
	}
	if err := connections.store(context.Background(), downstream, duplicate); err == nil {
		t.Fatal("duplicate store succeeded")
	}
	if len(duplicate.upstream.(*fakeUpstream).lifecycleEvents()) != 0 || duplicateTargetCloses.Load() != 0 {
		t.Fatal("registry closed duplicate even though store did not take ownership")
	}
	got, ok := connections.take(downstream)
	if !ok || got != first {
		t.Fatalf("take = (%p, %t), want original %p", got, ok, first)
	}
	_ = first.close()
	_ = duplicate.close()
}

func TestPreparedConnectionsShutdownCancelsAndJoinsInFlightPreparation(t *testing.T) {
	connections := newTestPreparedConnections()
	started := make(chan struct{})
	prepared, targetCloses := newTrackedPreparedConnection()
	connections.connectPrepared = func(ctx context.Context, _ dialerDownstream) (*preparedConnection, error) {
		close(started)
		<-ctx.Done()
		return prepared, nil
	}
	prepareDone := make(chan error, 1)
	go func() {
		prepareDone <- connections.prepare(context.Background(), new(minecraft.Conn))
	}()
	<-started

	if err := connections.shutdown(); err != nil {
		t.Fatalf("shutdown: %v", err)
	}
	if err := <-prepareDone; err == nil {
		t.Fatal("prepare error = nil after shutdown cancellation")
	}
	assertPreparedClosedExactlyOnce(t, prepared, targetCloses)
	if err := connections.store(context.Background(), new(minecraft.Conn), prepared); !errors.Is(err, context.Canceled) {
		t.Fatalf("post-shutdown store error = %v, want context cancellation", err)
	}
	if err := connections.shutdown(); err != nil {
		t.Fatalf("second shutdown: %v", err)
	}
	assertPreparedClosedExactlyOnce(t, prepared, targetCloses)
}

func TestPreparedConnectionsShutdownJoinsCancellationCleanup(t *testing.T) {
	connections := newTestPreparedConnections()
	downstream := new(minecraft.Conn)
	targetCloseStarted := make(chan struct{})
	allowTargetClose := make(chan struct{})
	prepared := &preparedConnection{
		upstream:  newFakeUpstream(nil),
		packStack: &selectedResourcePackStack{},
		releaseTarget: func() error {
			close(targetCloseStarted)
			<-allowTargetClose
			return nil
		},
	}
	ctx, cancel := context.WithCancel(context.Background())
	if err := connections.store(ctx, downstream, prepared); err != nil {
		t.Fatalf("store: %v", err)
	}
	cancel()
	<-targetCloseStarted
	shutdownDone := make(chan error, 1)
	go func() { shutdownDone <- connections.shutdown() }()
	select {
	case err := <-shutdownDone:
		t.Fatalf("shutdown returned before cancellation cleanup completed: %v", err)
	case <-time.After(20 * time.Millisecond):
	}
	close(allowTargetClose)
	if err := <-shutdownDone; err != nil {
		t.Fatalf("shutdown: %v", err)
	}
	if got := prepared.upstream.(*fakeUpstream).lifecycleEvents(); !slices.Equal(got, []string{"abort", "close"}) {
		t.Fatalf("upstream lifecycle = %v, want [abort close]", got)
	}
}

func TestShutdownClosesTransportBeforeJoiningBackpressuredRequiredCompatibilityConfigure(t *testing.T) {
	connections := newTestPreparedConnections()
	prepared, targetCloses := newTrackedPreparedConnection()
	prepared.upstream.(*fakeUpstream).packs = []*resource.Pack{new(resource.Pack)}
	prepared.upstream.(*fakeUpstream).required = true
	prepared.packStack = &selectedResourcePackStack{packs: []*resource.Pack{new(resource.Pack)}, required: true}
	connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
		return prepared, nil
	}
	configureStarted := make(chan struct{})
	configureUnblock := make(chan struct{})
	downstream := &offerTestDownstream{writeStarted: configureStarted, writeUnblock: configureUnblock}
	prepareDone := make(chan error, 1)
	go func() {
		prepareDone <- connections.prepareConnection(context.Background(), new(minecraft.Conn), downstream)
	}()
	<-configureStarted

	shutdownDone := make(chan error, 1)
	go func() {
		shutdownDone <- shutdownPreparedServer(connections, func() error {
			close(configureUnblock)
			return nil
		})
	}()
	select {
	case err := <-shutdownDone:
		if err != nil {
			t.Fatalf("shutdown: %v", err)
		}
	case <-time.After(time.Second):
		t.Fatal("shutdown waited for preparation before closing the backpressured transport")
	}
	if err := <-prepareDone; !errors.Is(err, context.Canceled) {
		t.Fatalf("prepare error = %v, want local shutdown cancellation", err)
	}
	assertPreparedClosedExactlyOnce(t, prepared, targetCloses)
}

func TestPreparedConnectionDialPanicClosesTargetAndReportsTelemetry(t *testing.T) {
	var output lockedBuffer
	logger := slog.New(slog.NewTextHandler(&output, nil))
	connections := newPreparedConnections("unused.invalid:19132", nil, logger)
	targetCloses := new(atomic.Int32)
	connections.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{
			address: "unused.invalid:19132",
			network: minecraft.RakNet{},
			friend: closerFunc(func() error {
				targetCloses.Add(1)
				return nil
			}),
		}, nil
	}
	connections.dialTarget = func(context.Context, *resolvedUpstreamTarget, minecraft.Dialer) (upstreamSession, error) {
		panic("dial panic")
	}

	prepared, err := connections.connect(context.Background(), dialerTestDownstream{protocol: minecraft.DefaultProtocol})
	if prepared != nil || err == nil || !strings.Contains(err.Error(), "panic while preparing upstream connection (type string)") {
		t.Fatalf("connect = (%v, %v), want contained dial panic", prepared, err)
	}
	if got := targetCloses.Load(); got != 1 {
		t.Fatalf("target close count = %d, want 1", got)
	}
}

func TestPostCaptureFailureReleasesSelectedStackReferences(t *testing.T) {
	connections := newPreparedConnections("unused.invalid:19132", nil, slog.New(slog.NewTextHandler(io.Discard, nil)))
	connections.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{address: "unused.invalid:19132", network: minecraft.RakNet{}}, nil
	}
	upstream := &panicOfferUpstream{fakeUpstream: newFakeUpstream(nil), resourcePacksPanic: "offer telemetry panic"}
	connections.dialTarget = func(context.Context, *resolvedUpstreamTarget, minecraft.Dialer) (upstreamSession, error) {
		return upstream, nil
	}
	stack := &selectedResourcePackStack{packs: []*resource.Pack{testAdmissionPack(t)}}
	connections.captureResourcePackStack = func(upstreamSession, func(*resource.Pack) bool) (*selectedResourcePackStack, error) {
		return stack, nil
	}

	prepared, err := connections.connect(context.Background(), dialerTestDownstream{protocol: minecraft.DefaultProtocol})
	if prepared != nil || err == nil || !strings.Contains(err.Error(), "panic while preparing upstream connection") {
		t.Fatalf("connect = (%v, %v), want contained post-capture failure", prepared, err)
	}
	if stack.packs != nil {
		t.Fatal("post-capture failure retained selected pack references")
	}
	if lifecycle := upstream.lifecycleEvents(); !slices.Equal(lifecycle, []string{"abort", "close"}) {
		t.Fatalf("upstream lifecycle = %v, want [abort close]", lifecycle)
	}
}

func TestConnectUpstreamConnectedLogPanicClosesLiveUpstreamTypeOnly(t *testing.T) {
	stringCalls := new(atomic.Int32)
	panicValue := sensitivePanic{stringCalls: stringCalls}
	handler := newSelectivePanicHandler("upstream connected", panicValue)
	upstream := newFakeUpstream(nil)

	got, err := connectUpstream(
		context.Background(),
		"unused.invalid:19132",
		"offline",
		slog.New(handler),
		func(context.Context, string) (upstreamSession, error) { return upstream, nil },
	)
	if got != nil || err == nil || !strings.Contains(err.Error(), "type proxy.sensitivePanic") {
		t.Fatalf("connectUpstream = (%v, %v), want type-only connected-log panic", got, err)
	}
	if stringCalls.Load() != 0 || strings.Contains(err.Error(), "sensitive panic payload") {
		t.Fatalf("panic payload formatted: error=%q String calls=%d", err, stringCalls.Load())
	}
	if lifecycle := upstream.lifecycleEvents(); !slices.Equal(lifecycle, []string{"abort", "close"}) {
		t.Fatalf("upstream lifecycle = %v, want [abort close]", lifecycle)
	}
	if handler.count("upstream connection starting") != 1 || handler.count("upstream connected") != 1 {
		t.Fatalf("logger calls = %#v", handler.snapshot())
	}
}

func TestDialMinecraftUpstreamNormalizesNilConnectionAndPreservesError(t *testing.T) {
	dialErr := errors.New("dial failed")
	upstream, err := dialMinecraftUpstream(
		context.Background(),
		minecraft.RakNet{},
		"unused.invalid:19132",
		func(context.Context, minecraft.Network, string) (*minecraft.Conn, error) {
			return nil, dialErr
		},
	)
	if upstream != nil || !errors.Is(err, dialErr) {
		t.Fatalf("dialMinecraftUpstream = (%v, %v), want nil and original dial error", upstream, err)
	}
}

func TestDialMinecraftUpstreamPreservesPartialConnectionAndError(t *testing.T) {
	dialErr := errors.New("dial failed after allocating connection")
	connection := new(minecraft.Conn)
	upstream, err := dialMinecraftUpstream(
		context.Background(),
		minecraft.RakNet{},
		"unused.invalid:19132",
		func(context.Context, minecraft.Network, string) (*minecraft.Conn, error) {
			return connection, dialErr
		},
	)
	if upstream != connection || !errors.Is(err, dialErr) {
		t.Fatalf("dialMinecraftUpstream = (%p, %v), want original connection %p and dial error", upstream, err, connection)
	}
}

func TestConnectUpstreamClosesNonNilDialResultWithError(t *testing.T) {
	upstream := newFakeUpstream(nil)
	dialErr := errors.New("dial returned session and error")
	got, err := connectUpstream(
		context.Background(),
		"unused.invalid:19132",
		"offline",
		slog.New(slog.NewTextHandler(io.Discard, nil)),
		func(context.Context, string) (upstreamSession, error) { return upstream, dialErr },
	)
	if got != nil || !errors.Is(err, dialErr) {
		t.Fatalf("connectUpstream = (%v, %v), want nil and dial error", got, err)
	}
	if lifecycle := upstream.lifecycleEvents(); !slices.Equal(lifecycle, []string{"abort", "close"}) {
		t.Fatalf("upstream lifecycle = %v, want [abort close]", lifecycle)
	}
}

func TestSelectedStackPolicyPanicsReleaseOwnershipWithoutFormattingPayload(t *testing.T) {
	tests := []struct {
		name       string
		upstream   func(any) upstreamSession
		downstream func(any) *offerTestDownstream
		stack      *selectedResourcePackStack
	}{
		{
			name: "required compatibility stack configure",
			upstream: func(any) upstreamSession {
				base := newFakeUpstream(nil)
				base.packs = []*resource.Pack{new(resource.Pack)}
				base.required = true
				return base
			},
			downstream: func(value any) *offerTestDownstream { return &offerTestDownstream{configPanic: value} },
			stack:      &selectedResourcePackStack{packs: []*resource.Pack{new(resource.Pack)}, required: true},
		},
		{
			name:       "configure offer",
			upstream:   func(any) upstreamSession { return newFakeUpstream(nil) },
			downstream: func(value any) *offerTestDownstream { return &offerTestDownstream{configPanic: value} },
			stack:      &selectedResourcePackStack{},
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			var output lockedBuffer
			logger := slog.New(slog.NewTextHandler(&output, nil))
			connections := newPreparedConnections("unused.invalid:19132", nil, logger)
			stringCalls := new(atomic.Int32)
			panicValue := sensitivePanic{stringCalls: stringCalls}
			targetCloses := new(atomic.Int32)
			upstream := test.upstream(panicValue)
			prepared := &preparedConnection{
				upstream:  upstream,
				packStack: test.stack,
				releaseTarget: func() error {
					targetCloses.Add(1)
					return nil
				},
			}
			connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
				return prepared, nil
			}

			err := connections.prepareConnection(context.Background(), new(minecraft.Conn), test.downstream(panicValue))
			if err == nil || !strings.Contains(err.Error(), "type proxy.sensitivePanic") {
				t.Fatalf("prepareConnection() error = %v, want type-only panic", err)
			}
			if strings.Contains(err.Error(), "sensitive panic payload") || stringCalls.Load() != 0 {
				t.Fatalf("panic payload formatted: error=%q String calls=%d", err, stringCalls.Load())
			}
			if got := targetCloses.Load(); got != 1 {
				t.Fatalf("target close count = %d, want 1", got)
			}
			if got := lifecycleEvents(upstream); !slices.Equal(got, []string{"abort", "close"}) {
				t.Fatalf("upstream lifecycle = %v, want [abort close]", got)
			}
		})
	}
}

func TestPreparedCleanupAttemptsEveryCallbackOnceWhenCallbacksPanic(t *testing.T) {
	stringCalls := new(atomic.Int32)
	panicValue := sensitivePanic{stringCalls: stringCalls}
	upstream := &cleanupPanicUpstream{fakeUpstream: newFakeUpstream(nil), panicValue: panicValue}
	targetCalls := new(atomic.Int32)
	prepared := &preparedConnection{
		upstream:  upstream,
		packStack: &selectedResourcePackStack{},
		releaseTarget: func() error {
			targetCalls.Add(1)
			panic(panicValue)
		},
	}

	err := prepared.close()
	if err == nil || strings.Contains(err.Error(), "sensitive panic payload") || stringCalls.Load() != 0 {
		t.Fatalf("prepared close error = %q, String calls=%d", err, stringCalls.Load())
	}
	if upstream.abortCalls.Load() != 1 || upstream.closeCalls.Load() != 1 || targetCalls.Load() != 1 {
		t.Fatalf("cleanup calls abort=%d close=%d target=%d, want all 1",
			upstream.abortCalls.Load(), upstream.closeCalls.Load(), targetCalls.Load())
	}
	_ = prepared.close()
	if upstream.abortCalls.Load() != 1 || upstream.closeCalls.Load() != 1 || targetCalls.Load() != 1 {
		t.Fatal("second close repeated a cleanup callback")
	}
}

func TestConnectFailureAttemptsEveryCleanupCallbackWhenCallbacksPanic(t *testing.T) {
	stringCalls := new(atomic.Int32)
	panicValue := sensitivePanic{stringCalls: stringCalls}
	upstream := &cleanupPanicUpstream{fakeUpstream: newFakeUpstream(nil), panicValue: panicValue}
	targetCalls := new(atomic.Int32)
	telemetryCalls := new(atomic.Int32)
	connections := newPreparedConnections("unused.invalid:19132", nil, slog.New(panicSlogHandler{
		calls:      telemetryCalls,
		panicValue: panicValue,
	}))
	connections.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{
			address: "unused.invalid:19132",
			network: minecraft.RakNet{},
			friend: closerFunc(func() error {
				targetCalls.Add(1)
				panic(panicValue)
			}),
		}, nil
	}
	dialErr := errors.New("dial failed")
	connections.dialTarget = func(context.Context, *resolvedUpstreamTarget, minecraft.Dialer) (upstreamSession, error) {
		return upstream, dialErr
	}

	prepared, err := connections.connect(context.Background(), dialerTestDownstream{protocol: minecraft.DefaultProtocol})
	if prepared != nil || !errors.Is(err, dialErr) || strings.Contains(err.Error(), "sensitive panic payload") || stringCalls.Load() != 0 {
		t.Fatalf("connect = (%v, %q), String calls=%d", prepared, err, stringCalls.Load())
	}
	if upstream.abortCalls.Load() != 1 || upstream.closeCalls.Load() != 1 || targetCalls.Load() != 1 {
		t.Fatalf("cleanup calls abort=%d close=%d target=%d, want all 1",
			upstream.abortCalls.Load(), upstream.closeCalls.Load(), targetCalls.Load())
	}
}

func TestServePreparedConnectionReleasesTransferredOwnershipExactlyOnce(t *testing.T) {
	downstream := newFakeDownstream(nil)
	prepared, targetCloses := newTrackedPreparedConnection()
	downstream.reads <- packetResult{err: io.EOF}

	if err := servePreparedConnection(context.Background(), downstream, prepared); err != nil {
		t.Fatalf("servePreparedConnection() error = %v, want ordinary EOF suppressed", err)
	}
	if got := downstream.lifecycleEvents(); !slices.Equal(got, []string{"abort", "close"}) {
		t.Fatalf("downstream lifecycle = %v, want [abort close]", got)
	}
	assertPreparedClosedExactlyOnce(t, prepared, targetCloses)
	if err := prepared.close(); err != nil {
		t.Fatalf("second prepared close: %v", err)
	}
	assertPreparedClosedExactlyOnce(t, prepared, targetCloses)
}

func newTestPreparedConnections() *preparedConnections {
	connections := newPreparedConnections("unused.invalid:19132", nil, slog.New(slog.NewTextHandler(io.Discard, nil)))
	connections.captureResourcePackStack = func(upstream upstreamSession, _ func(*resource.Pack) bool) (*selectedResourcePackStack, error) {
		var fake *fakeUpstream
		switch upstream := upstream.(type) {
		case *fakeUpstream:
			fake = upstream
		case *panicOfferUpstream:
			fake = upstream.fakeUpstream
		}
		if fake == nil {
			return &selectedResourcePackStack{}, nil
		}
		return &selectedResourcePackStack{packs: slices.Clone(fake.packs), required: fake.required}, nil
	}
	return connections
}

type closerFunc func() error

func (close closerFunc) Close() error { return close() }

type sensitivePanic struct {
	stringCalls *atomic.Int32
}

func (value sensitivePanic) String() string {
	value.stringCalls.Add(1)
	return "sensitive panic payload"
}

type panicOfferUpstream struct {
	*fakeUpstream
	resourcePacksPanic any
	requiredPanic      any
}

func (upstream *panicOfferUpstream) ResourcePacks() []*resource.Pack {
	if upstream.resourcePacksPanic != nil {
		panic(upstream.resourcePacksPanic)
	}
	return upstream.fakeUpstream.ResourcePacks()
}

func (upstream *panicOfferUpstream) TexturePacksRequired() bool {
	if upstream.requiredPanic != nil {
		panic(upstream.requiredPanic)
	}
	return upstream.fakeUpstream.TexturePacksRequired()
}

func lifecycleEvents(upstream upstreamSession) []string {
	switch upstream := upstream.(type) {
	case *fakeUpstream:
		return upstream.lifecycleEvents()
	case *panicOfferUpstream:
		return upstream.lifecycleEvents()
	case *gatedCloseUpstream:
		return upstream.lifecycleEvents()
	default:
		return nil
	}
}

type cleanupPanicUpstream struct {
	*fakeUpstream
	panicValue any
	abortCalls atomic.Int32
	closeCalls atomic.Int32
}

func (upstream *cleanupPanicUpstream) Abort() error {
	upstream.abortCalls.Add(1)
	panic(upstream.panicValue)
}

func (upstream *cleanupPanicUpstream) Close() error {
	upstream.closeCalls.Add(1)
	panic(upstream.panicValue)
}

type panicSlogHandler struct {
	calls      *atomic.Int32
	panicValue any
}

func (panicSlogHandler) Enabled(context.Context, slog.Level) bool { return true }

func (handler panicSlogHandler) Handle(context.Context, slog.Record) error {
	handler.calls.Add(1)
	panic(handler.panicValue)
}

func (handler panicSlogHandler) WithAttrs([]slog.Attr) slog.Handler { return handler }

func (handler panicSlogHandler) WithGroup(string) slog.Handler { return handler }

type selectivePanicHandler struct {
	mu           sync.Mutex
	counts       map[string]int
	panicMessage string
	panicValue   any
}

type trackedAcceptedDownstream struct {
	*minecraft.Conn
	abortCalls atomic.Int32
	closeCalls atomic.Int32
}

func (downstream *trackedAcceptedDownstream) Abort() error {
	downstream.abortCalls.Add(1)
	return downstream.Conn.Abort()
}

func (downstream *trackedAcceptedDownstream) Close() error {
	downstream.closeCalls.Add(1)
	return downstream.Conn.Close()
}

func newSelectivePanicHandler(message string, value any) *selectivePanicHandler {
	return &selectivePanicHandler{counts: make(map[string]int), panicMessage: message, panicValue: value}
}

func (*selectivePanicHandler) Enabled(context.Context, slog.Level) bool { return true }

func (handler *selectivePanicHandler) Handle(_ context.Context, record slog.Record) error {
	handler.mu.Lock()
	handler.counts[record.Message]++
	handler.mu.Unlock()
	if record.Message == handler.panicMessage {
		panic(handler.panicValue)
	}
	return nil
}

func (handler *selectivePanicHandler) WithAttrs([]slog.Attr) slog.Handler { return handler }

func (handler *selectivePanicHandler) WithGroup(string) slog.Handler { return handler }

func (handler *selectivePanicHandler) count(message string) int {
	handler.mu.Lock()
	defer handler.mu.Unlock()
	return handler.counts[message]
}

func (handler *selectivePanicHandler) snapshot() map[string]int {
	handler.mu.Lock()
	defer handler.mu.Unlock()
	result := make(map[string]int, len(handler.counts))
	for message, count := range handler.counts {
		result[message] = count
	}
	return result
}

type trackedTargetClose struct {
	calls atomic.Int32
	done  chan struct{}
}

func (tracker *trackedTargetClose) Load() int32 {
	return tracker.calls.Load()
}

func (tracker *trackedTargetClose) Close() error {
	if tracker.calls.Add(1) == 1 {
		close(tracker.done)
	}
	return nil
}

func newTrackedPreparedConnection() (*preparedConnection, *trackedTargetClose) {
	targetCloses := &trackedTargetClose{done: make(chan struct{})}
	return &preparedConnection{
		upstream:      newFakeUpstream(nil),
		packStack:     &selectedResourcePackStack{},
		releaseTarget: targetCloses.Close,
	}, targetCloses
}

func waitForPreparedTransportCleanup(t *testing.T, prepared *preparedConnection, targetCloses *trackedTargetClose) {
	t.Helper()
	select {
	case <-targetCloses.done:
	case <-time.After(time.Second):
		t.Fatalf("prepared connection cleanup did not complete; upstream lifecycle = %v, target closes = %d", lifecycleEvents(prepared.upstream), targetCloses.Load())
	}
}

func assertPreparedClosedExactlyOnce(t *testing.T, prepared *preparedConnection, targetCloses *trackedTargetClose) {
	t.Helper()
	if got := lifecycleEvents(prepared.upstream); !slices.Equal(got, []string{"abort", "close"}) {
		t.Fatalf("upstream lifecycle = %v, want [abort close]", got)
	}
	if got := targetCloses.Load(); got != 1 {
		t.Fatalf("target close count = %d, want 1", got)
	}
}

func encodeLatest(t *testing.T, pk packet.Packet) []byte {
	t.Helper()
	var buf bytes.Buffer
	pk.Marshal(minecraft.DefaultProtocol.NewWriter(&buf, 0))
	return buf.Bytes()
}

func packInfos(sizes ...uint64) *packet.ResourcePacksInfo {
	info := &packet.ResourcePacksInfo{TexturePackRequired: true}
	for _, size := range sizes {
		info.TexturePacks = append(info.TexturePacks, protocol.TexturePackInfo{UUID: uuid.New(), Version: "1.0.0", Size: size})
	}
	return info
}

func observedBudget(t *testing.T, info *packet.ResourcePacksInfo) (*resourcePackAcquisitionBudget, *[]error) {
	t.Helper()
	var causes []error
	budget := newResourcePackAcquisitionBudget(minecraft.DefaultProtocol, func(cause error) { causes = append(causes, cause) })
	budget.observe(packet.Header{PacketID: packet.IDResourcePacksInfo}, encodeLatest(t, info))
	return budget, &causes
}

func admitted(budget *resourcePackAcquisitionBudget, total int) []bool {
	result := make([]bool, total)
	for index := range total {
		result[index] = budget.admit(uuid.New(), "1.0.0", index, total)
	}
	return result
}

// Packs past the count or byte bound are ignored individually, never fatal.
func TestAcquisitionBudgetAdmitsOfferOrderWithinCountAndByteBounds(t *testing.T) {
	const mib = 1024 * 1024
	budget, _ := observedBudget(t, packInfos(10*mib, 65*mib, 60*mib, 60*mib, 8*mib))
	if got, want := admitted(budget, 5), []bool{true, false, true, false, true}; !slices.Equal(got, want) {
		t.Fatalf("admitted = %v, want %v", got, want)
	}
	sizes := make([]uint64, maxSelectedResourcePacks+2)
	for index := range sizes {
		sizes[index] = 1
	}
	budget, _ = observedBudget(t, packInfos(sizes...))
	got := admitted(budget, len(sizes))
	if slices.Index(got, false) != maxSelectedResourcePacks || got[len(got)-1] {
		t.Fatalf("admitted = %v, want the first %d only", got, maxSelectedResourcePacks)
	}
	if budget.admit(uuid.New(), "1.0.0", 0, len(sizes)+1) {
		t.Fatal("admit accepted a callback total that disagrees with the decoded offer")
	}
}

func TestAcquisitionBudgetDeclinesUndecodableOffer(t *testing.T) {
	var causes []error
	budget := newResourcePackAcquisitionBudget(minecraft.DefaultProtocol, func(cause error) { causes = append(causes, cause) })
	budget.observe(packet.Header{PacketID: packet.IDResourcePacksInfo}, []byte{1, 2, 3})
	if budget.admit(uuid.New(), "1.0.0", 0, 1) || len(causes) != 0 {
		t.Fatalf("undecodable offer admitted=%t causes=%v, want declined without cancellation", budget.admit(uuid.New(), "1.0.0", 0, 1), causes)
	}
}

func admissionPackWithUUID(t *testing.T, id string) *resource.Pack {
	t.Helper()
	var archive bytes.Buffer
	writer := zip.NewWriter(&archive)
	manifest, err := writer.Create("manifest.json")
	if err != nil {
		t.Fatal(err)
	}
	fmt.Fprintf(manifest, `{"format_version":2,"header":{"name":"t","description":"t","uuid":"%s","version":[1,0,0],"min_engine_version":[1,0,0]},"modules":[{"type":"resources","uuid":"ffeeddcc-bbaa-9988-7766-554433221100","version":[1,0,0]}]}`, id)
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	pack, err := resource.ReadBytes(archive.Bytes())
	if err != nil {
		t.Fatal(err)
	}
	return pack
}

func started(source minecraft.ResourcePackSource, id uuid.UUID, size uint64) minecraft.ResourcePackEvent {
	return minecraft.ResourcePackEvent{Kind: minecraft.ResourcePackStarted, Source: source, UUID: id, Version: "1.0.0", Size: size}
}

// A transfer larger than its offer, or for an unadvertised pack, is dropped from the handoff
// without cancelling the join; only the memory ceiling cancels.
func TestAcquisitionBudgetExcludesGrownTransfersAndCancelsOnlyPastMemoryCeiling(t *testing.T) {
	const mib = 1024 * 1024
	info := packInfos(mib, mib)
	grown, honest, unadvertised := info.TexturePacks[0].UUID, info.TexturePacks[1].UUID, uuid.New()
	budget, causes := observedBudget(t, info)
	budget.event(started(minecraft.ResourcePackSourceChunks, grown, 5*mib))
	budget.event(started(minecraft.ResourcePackSourceChunks, honest, mib))
	budget.event(started(minecraft.ResourcePackSourceChunks, unadvertised, mib))
	grownPack, honestPack := admissionPackWithUUID(t, grown.String()), admissionPackWithUUID(t, honest.String())
	if !budget.excludes(grownPack) || budget.excludes(honestPack) || !budget.excludes(admissionPackWithUUID(t, unadvertised.String())) || len(*causes) != 0 {
		t.Fatalf("causes = %v, want only grown+unadvertised dropped and no cancel", *causes)
	}

	over, overCauses := observedBudget(t, packInfos(mib))
	over.event(started(minecraft.ResourcePackSourceURL, uuid.New(), maxResourcePackTransferBytes+1))
	if len(*overCauses) != 1 || !errors.Is((*overCauses)[0], errResourcePackTransferTooLarge) {
		t.Fatalf("memory ceiling causes = %v", *overCauses)
	}
}

// cdnPackListener offers pack by URL from a CDN whose dialer-facing response is shaped by serve.
func cdnPackListener(t *testing.T, serve func(http.ResponseWriter, *http.Request, []byte)) (minecraft.Network, *minecraft.Listener) {
	t.Helper()
	archive := testAdmissionPackArchive(t)
	var fetched atomic.Bool
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if fetched.CompareAndSwap(false, true) {
			_, _ = w.Write(archive) // the listener's own read of the pack
			return
		}
		serve(w, r, archive)
	}))
	t.Cleanup(server.Close)
	t.Cleanup(server.CloseClientConnections) // runs first so Close need not wait on a stalled handler
	pack, err := resource.ReadURL(server.URL)
	if err != nil {
		t.Fatal(err)
	}
	listener, network := newAdmissionTestListener(t, func(_ context.Context, conn *minecraft.Conn) error {
		return conn.ConfigureResourcePackOffer([]*resource.Pack{pack}, true)
	})
	return network, listener
}

func dialBudgeted(ctx context.Context, network minecraft.Network, cancelled *atomic.Value) <-chan admissionDialResult {
	dialCtx, cancelDial := context.WithCancelCause(ctx)
	budget := newResourcePackAcquisitionBudget(minecraft.DefaultProtocol, func(cause error) {
		cancelled.Store(cause)
		cancelDial(cause)
	})
	dialer := withResourcePackAcquisitionBudget(minecraft.Dialer{
		IdentityData: login.IdentityData{DisplayName: "Budgeted"},
		Protocol:     minecraft.DefaultProtocol,
	}, budget)
	done := make(chan admissionDialResult, 1)
	go func() {
		defer cancelDial(nil)
		conn, err := dialer.DialContextNetwork(dialCtx, network, "")
		done <- admissionDialResult{conn: conn, err: err}
	}()
	return done
}

// A CDN download that trickles in without any game packet is never cancelled by the core.
func TestSlowCDNPackDownloadCompletesWithoutCancellation(t *testing.T) {
	var served atomic.Bool
	network, listener := cdnPackListener(t, func(w http.ResponseWriter, _ *http.Request, archive []byte) {
		w.Header().Set("Content-Length", strconv.Itoa(len(archive)))
		for offset := 0; offset < len(archive); offset += 16 {
			served.Store(offset+16 >= len(archive))
			_, _ = w.Write(archive[offset:min(offset+16, len(archive))])
			w.(http.Flusher).Flush()
			time.Sleep(20 * time.Millisecond)
		}
	})
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	var cancelled atomic.Value
	done := dialBudgeted(ctx, network, &cancelled)
	accepted, err := listener.Accept()
	if err != nil {
		t.Fatal(err)
	}
	defer accepted.Close()
	if err := accepted.(*minecraft.Conn).StartGameContext(ctx, minecraft.GameData{EntityRuntimeID: 9}); err != nil {
		t.Fatalf("start game: %v", err)
	}
	result := <-done
	if result.err != nil || cancelled.Load() != nil || !served.Load() {
		t.Fatalf("slow download: err=%v cancel=%v served=%t", result.err, cancelled.Load(), served.Load())
	}
	_ = result.conn.Close()
}

// A CDN that stops sending keeps the join waiting, as vanilla does, until the user cancels it.
func TestStalledCDNPackDownloadWaitsForTheUserToCancel(t *testing.T) {
	release := make(chan struct{})
	network, _ := cdnPackListener(t, func(w http.ResponseWriter, r *http.Request, archive []byte) {
		w.Header().Set("Content-Length", strconv.Itoa(len(archive)))
		_, _ = w.Write(archive[:len(archive)/2])
		w.(http.Flusher).Flush()
		select {
		case <-r.Context().Done():
		case <-release:
		}
	})
	defer close(release)
	ctx, cancelUser := context.WithCancel(context.Background())
	var cancelled atomic.Value
	done := dialBudgeted(ctx, network, &cancelled)
	select {
	case result := <-done:
		t.Fatalf("stalled download ended on its own: %v", result.err)
	case <-time.After(750 * time.Millisecond):
	}
	cancelUser()
	select {
	case result := <-done:
		if !errors.Is(result.err, context.Canceled) || cancelled.Load() != nil {
			t.Fatalf("user cancel: err=%v core cancel=%v", result.err, cancelled.Load())
		}
	case <-time.After(5 * time.Second):
		t.Fatal("user cancel did not abort the stalled download")
	}
}

// Like vanilla, totals grow as each download begins, cache hits leave the pack count, a pack
// finishes on its completion, and a failed URL download is reverted before its chunk fallback.
func TestAcquisitionBudgetReportsVanillaPackProgress(t *testing.T) {
	var reports []ConnectProgress
	info := packInfos(300, 700, 50)
	budget, _ := observedBudget(t, info)
	budget.onProgress = func(progress ConnectProgress) { reports = append(reports, progress) }
	first, second, cached := info.TexturePacks[0].UUID, info.TexturePacks[1].UUID, info.TexturePacks[2].UUID
	event := func(kind minecraft.ResourcePackEventKind, source minecraft.ResourcePackSource, id uuid.UUID, size uint64) {
		budget.event(minecraft.ResourcePackEvent{Kind: kind, Source: source, UUID: id, Version: "1.0.0", Size: size})
	}
	url, chunks := minecraft.ResourcePackSourceURL, minecraft.ResourcePackSourceChunks
	event(minecraft.ResourcePackFinished, minecraft.ResourcePackSourceCache, cached, 50)
	event(minecraft.ResourcePackStarted, url, first, 300)
	event(minecraft.ResourcePackReceived, url, first, 100)
	event(minecraft.ResourcePackFailed, url, first, 0)
	event(minecraft.ResourcePackStarted, chunks, first, 300)
	event(minecraft.ResourcePackReceived, chunks, first, 300)
	event(minecraft.ResourcePackFinished, chunks, first, 0)
	event(minecraft.ResourcePackStarted, url, second, 700)
	event(minecraft.ResourcePackReceived, url, second, 350)
	packs := func(done, total uint32, received, bytes uint64) ConnectProgress {
		return ConnectProgress{Stage: ConnectStagePacks, PacksDone: done, PacksTotal: total, ReceivedBytes: received, TotalBytes: bytes}
	}
	want := []ConnectProgress{
		packs(0, 2, 0, 0),
		packs(0, 2, 0, 300),
		packs(0, 2, 100, 300),
		packs(0, 2, 0, 0),
		packs(0, 2, 0, 300),
		packs(0, 2, 300, 300),
		packs(1, 2, 300, 300),
		packs(1, 2, 300, 1000),
		packs(1, 2, 650, 1000),
	}
	if !slices.Equal(reports, want) {
		t.Fatalf("reports = %+v\nwant %+v", reports, want)
	}
	budget.finish()
	event(minecraft.ResourcePackReceived, url, second, 350)
	if len(reports) != len(want) {
		t.Fatal("an event after the dial returned was reported")
	}
}

// A budgeted dialer reuses a required cached offer and hands its stack onward.
func TestBudgetedDialerAcquiresRequiredCachedOfferBeforeStartGame(t *testing.T) {
	pack := testAdmissionPack(t)
	listener, network := newAdmissionTestListener(t, func(_ context.Context, conn *minecraft.Conn) error {
		return conn.ConfigureResourcePackOffer([]*resource.Pack{pack}, true)
	})
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	dialCtx, cancelDial := context.WithCancelCause(ctx)
	defer cancelDial(nil)
	budget := newResourcePackAcquisitionBudget(minecraft.DefaultProtocol, cancelDial)
	dialer := withResourcePackAcquisitionBudget(minecraft.Dialer{
		IdentityData:      login.IdentityData{DisplayName: "Budgeted"},
		Protocol:          minecraft.DefaultProtocol,
		ResourcePackCache: &scriptedResourcePackCache{loadPack: pack},
	}, budget)
	clientDone := make(chan admissionDialResult, 1)
	go func() {
		client, err := dialer.DialContextNetwork(dialCtx, network, "")
		clientDone <- admissionDialResult{conn: client, err: err}
	}()
	acceptedRaw, err := listener.Accept()
	if err != nil {
		t.Fatalf("listener Accept: %v", err)
	}
	accepted := acceptedRaw.(*minecraft.Conn)
	defer accepted.Close()
	if err := accepted.StartGameContext(ctx, minecraft.GameData{EntityRuntimeID: 9}); err != nil {
		t.Fatalf("start game: %v", err)
	}
	result := <-clientDone
	if result.err != nil {
		t.Fatalf("budgeted dial: %v", result.err)
	}
	defer result.conn.Close()
	if budget.packs != 0 || budget.transferred != 0 {
		t.Fatal("cached offer unexpectedly downloaded pack bytes")
	}
	stack, err := captureSelectedResourcePackStack(result.conn, budget.excludes)
	if err != nil {
		t.Fatalf("capture stack: %v", err)
	}
	if len(stack.packs) != 1 || stack.packs[0].UUID() != pack.UUID() || !stack.offer.TexturePackRequired() {
		t.Fatalf("captured stack packs = %d, want the acquired required pack with the upstream offer", len(stack.packs))
	}
	if entries := stack.offer.TexturePacks(); len(entries) != 1 || entries[0].Info().UUID != pack.UUID() || entries[0].Info().Size != uint64(pack.Size()) {
		t.Fatalf("projected offer entries = %+v", entries)
	}
	forwarded := new(offerTestDownstream)
	if err := configureResourcePackOffer(forwarded, stack); err != nil || !forwarded.required {
		t.Fatalf("forwarded required = %t (%v), want the server's required bit", forwarded.required, err)
	}
	// A required pack that admission drops refuses the join, as vanilla cannot join without it.
	var admission *PackAdmissionError
	if _, err := captureSelectedResourcePackStack(result.conn, func(*resource.Pack) bool { return true }); !errors.As(err, &admission) {
		t.Fatalf("capture without a required pack = %v, want PackAdmissionError", err)
	}
	telemetry := newResourcePackAdmissionTelemetry(1, nil)
	telemetry.observeOffer(result.conn)
	if got := telemetry.snapshot(); got.Offer != ResourcePackOfferRequired || got.Acquisition != ResourcePackAcquisitionComplete {
		t.Fatalf("telemetry = %#v, want complete required acquisition", got)
	}
}

// A join reports vanilla's stages in order, and closing the client mid-download aborts the
// core's upstream dial and CDN request.
func TestJoinReportsStagesAndClientCancelAbortsTheDownload(t *testing.T) {
	aborted := make(chan struct{})
	upstreamNetwork, _ := cdnPackListener(t, func(w http.ResponseWriter, r *http.Request, archive []byte) {
		w.Header().Set("Content-Length", strconv.Itoa(len(archive)))
		_, _ = w.Write(archive[:len(archive)/2])
		w.(http.Flusher).Flush()
		<-r.Context().Done()
		close(aborted)
	})
	connections := newPreparedConnections("unused.invalid:19132", nil, slog.New(slog.DiscardHandler))
	var mu sync.Mutex
	var stages []ConnectStage
	downloading := make(chan struct{})
	var once sync.Once
	connections.connectProgress = func(progress ConnectProgress) {
		mu.Lock()
		defer mu.Unlock()
		if len(stages) == 0 || stages[len(stages)-1] != progress.Stage {
			stages = append(stages, progress.Stage)
		}
		if progress.Stage == ConnectStagePacks {
			once.Do(func() { close(downloading) })
		}
	}
	connections.resolveTarget = func(ctx context.Context) (*resolvedUpstreamTarget, error) {
		reportConnectStage(ctx, ConnectStageRealm)
		return &resolvedUpstreamTarget{network: upstreamNetwork, realm: true}, nil
	}
	connections.dialTarget = func(ctx context.Context, target *resolvedUpstreamTarget, dialer minecraft.Dialer) (upstreamSession, error) {
		return dialer.DialContextNetwork(ctx, target.network, "")
	}
	prepared := make(chan error, 1)
	_, network := newAdmissionTestListener(t, func(ctx context.Context, conn *minecraft.Conn) error {
		err := connections.prepare(ctx, conn)
		prepared <- err
		return err
	})
	clientCtx, cancelClient := context.WithCancel(context.Background())
	go func() {
		conn, err := (minecraft.Dialer{IdentityData: login.IdentityData{DisplayName: "Cancel"}, Protocol: minecraft.DefaultProtocol}).DialContextNetwork(clientCtx, network, "")
		if err == nil {
			_ = conn.Close()
		}
	}()
	select {
	case <-downloading:
	case <-time.After(10 * time.Second):
		t.Fatal("the pack stage was never reported")
	}
	cancelClient()
	select {
	case err := <-prepared:
		var cancelled *preparationCancellationError
		if !errors.As(err, &cancelled) {
			t.Fatalf("prepare error = %v, want a preparation cancellation", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("closing the client did not end the preparation")
	}
	select {
	case <-aborted:
	case <-time.After(5 * time.Second):
		t.Fatal("the CDN request outlived the cancelled join")
	}
	mu.Lock()
	defer mu.Unlock()
	if want := []ConnectStage{ConnectStageRealm, ConnectStageConnecting, ConnectStagePacks, ""}; !slices.Equal(stages, want) {
		t.Fatalf("stages = %q, want %q", stages, want)
	}
}

// With gophertunnel's flush ticker off, a chunk-downloaded pack must still complete: the
// download's own requests and completion reach the server.
func TestUnflushedDialerCompletesChunkPackDownload(t *testing.T) {
	pack := testAdmissionPack(t)
	listener, network := newAdmissionTestListener(t, func(_ context.Context, conn *minecraft.Conn) error {
		return conn.ConfigureResourcePackOffer([]*resource.Pack{pack}, true)
	})
	go func() {
		accepted, err := listener.Accept()
		if err != nil {
			return
		}
		_ = accepted.(*minecraft.Conn).WritePacketImmediate(&packet.StartGame{EntityRuntimeID: 1, EntityUniqueID: 1})
	}()
	connections := newPreparedConnections("unused.invalid:19132", nil, slog.New(slog.DiscardHandler))
	connections.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{network: network}, nil
	}
	connections.dialTarget = func(ctx context.Context, target *resolvedUpstreamTarget, dialer minecraft.Dialer) (upstreamSession, error) {
		if dialer.FlushRate >= 0 {
			t.Errorf("production dialer FlushRate = %v, want the ticker disabled", dialer.FlushRate)
		}
		return dialer.DialContextNetwork(ctx, target.network, "")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	prepared, err := connections.connect(ctx, dialerTestDownstream{protocol: minecraft.DefaultProtocol, identity: login.IdentityData{DisplayName: "Unflushed"}})
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer prepared.close()
	if len(prepared.packStack.packs) != 1 {
		t.Fatalf("acquired %d packs, want the chunk-downloaded pack", len(prepared.packStack.packs))
	}
}

func TestAcquisitionBudgetChecksActualArchiveIdentityAndSize(t *testing.T) {
	pack := testAdmissionPack(t)
	other := admissionPackWithUUID(t, "11223344-5566-7788-99aa-bbccddeeff00")
	tests := []struct {
		name     string
		info     []protocol.TexturePackInfo
		actual   *resource.Pack
		source   minecraft.ResourcePackSource
		excluded bool
	}{
		{name: "cached", source: minecraft.ResourcePackSourceCache},
		{name: "URL", source: minecraft.ResourcePackSourceURL},
		{name: "chunks", source: minecraft.ResourcePackSourceChunks},
		{
			name: "manifest UUID differs from transfer", actual: other,
			source: minecraft.ResourcePackSourceChunks, excluded: true,
		},
		{
			name:   "manifest version differs from transfer",
			info:   []protocol.TexturePackInfo{{UUID: pack.UUID(), Version: "different-version", Size: uint64(pack.Size())}},
			source: minecraft.ResourcePackSourceChunks, excluded: true,
		},
		{
			name:   "archive grew without a matching transfer event",
			info:   []protocol.TexturePackInfo{{UUID: pack.UUID(), Version: pack.Version(), Size: uint64(pack.Size() - 1)}},
			source: minecraft.ResourcePackSourceChunks, excluded: true,
		},
		{
			name: "another version cannot enlarge the admitted cap",
			info: []protocol.TexturePackInfo{
				{UUID: pack.UUID(), Version: pack.Version(), Size: uint64(pack.Size() - 1)},
				{UUID: pack.UUID(), Version: "different-version", Size: uint64(pack.Size() + 1)},
			},
			source: minecraft.ResourcePackSourceChunks, excluded: true,
		},
		{
			name:   "identity declined before download",
			info:   []protocol.TexturePackInfo{{UUID: pack.UUID(), Version: pack.Version(), Size: maxResourcePackArchiveBytes + 1}},
			source: minecraft.ResourcePackSourceCache, excluded: true,
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if test.info == nil {
				test.info = []protocol.TexturePackInfo{{UUID: pack.UUID(), Version: pack.Version(), Size: uint64(pack.Size())}}
			}
			if test.actual == nil {
				test.actual = pack
			}
			budget, _ := observedBudget(t, &packet.ResourcePacksInfo{TexturePacks: test.info})
			kind := minecraft.ResourcePackStarted
			if test.source == minecraft.ResourcePackSourceCache {
				kind = minecraft.ResourcePackFinished
			}
			transfer := test.info[0]
			budget.event(minecraft.ResourcePackEvent{
				Kind: kind, Source: test.source, UUID: transfer.UUID, Version: transfer.Version, Size: transfer.Size,
			})
			if got := budget.excludes(test.actual); got != test.excluded {
				t.Fatalf("actual archive excluded = %t, want %t", got, test.excluded)
			}
		})
	}
}

func TestSelectedStackPreservesRepeatedEntriesAndOrder(t *testing.T) {
	first := testAdmissionPack(t)
	second := admissionPackWithUUID(t, "11223344-5566-7788-99aa-bbccddeeff00")
	repeated := make([]*resource.Pack, maxSelectedResourcePacks+1)
	for i := range repeated {
		repeated[i] = second
	}
	repeated = append(repeated, first)
	for _, required := range []bool{false, true} {
		t.Run(fmt.Sprintf("required=%t", required), func(t *testing.T) {
			conn := negotiatedAdmissionPacks(t, []*resource.Pack{first, second}, required)
			selected := negotiatedAdmissionPacks(t, repeated, required)
			snapshot, _ := selected.ResourcePackStack()
			info := &packet.ResourcePacksInfo{}
			for _, pack := range []*resource.Pack{first, second} {
				info.TexturePacks = append(info.TexturePacks, protocol.TexturePackInfo{
					UUID: pack.UUID(), Version: pack.Version(), Size: uint64(pack.Size()),
				})
			}
			budget, _ := observedBudget(t, info)
			source := selectedOfferSource{Conn: conn, selection: snapshot}
			captured, err := captureSelectedResourcePackStack(source, budget.excludes)
			if err != nil {
				t.Fatal(err)
			}
			if len(captured.packs) != 2 || captured.packs[0].UUID() != first.UUID() || captured.packs[1].UUID() != second.UUID() {
				t.Fatal("captured archives lost offer order")
			}
			entries, original := captured.snapshot.Entries(), snapshot.Entries()
			if len(snapshot.Packs()) != len(repeated) || len(entries) != len(original) {
				t.Fatalf("captured %d stack entries, want %d including built-ins", len(entries), len(original))
			}
			for i, entry := range entries {
				if entry.UUID() != original[i].UUID() || entry.Version() != original[i].Version() || entry.SubPackName() != original[i].SubPackName() {
					t.Fatalf("stack entry %d lost server ordering", i)
				}
			}
			// Excluding one acquired identity must retain the required-pack rule.
			budget.admitOffer(&packet.ResourcePacksInfo{TexturePacks: info.TexturePacks[:1]}, true)
			filtered, err := captureSelectedResourcePackStack(source, budget.excludes)
			var admission *PackAdmissionError
			if required {
				if !errors.As(err, &admission) {
					t.Fatalf("missing required identity error = %v", err)
				}
			} else if err != nil {
				t.Fatal(err)
			} else if kept := filtered.snapshot.Packs(); len(kept) != 1 || kept[0].UUID() != first.UUID() || len(filtered.snapshot.Entries()) != len(original)-len(repeated)+1 {
				t.Fatal("optional projection did not retain the surviving archive and built-ins")
			}
		})
	}
}

// negotiatedAdmissionPacks obtains real offer and stack snapshots, including repeated entries.
func negotiatedAdmissionPacks(t *testing.T, packs []*resource.Pack, required bool) *minecraft.Conn {
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
	conn, err := (minecraft.Dialer{
		RelayStartup: true,
		IdentityData: login.IdentityData{DisplayName: "Selection"},
	}).DialContextNetwork(ctx, network, "")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = conn.Close() })
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	return conn
}
