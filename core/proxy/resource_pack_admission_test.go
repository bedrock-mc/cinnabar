package proxy

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"math"
	"net"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

type offerTestDownstream struct {
	dialerTestDownstream
	configured      bool
	configuredStack bool
	stack           minecraft.ResourcePackStackSnapshot
	packs           []*resource.Pack
	required        bool
	err             error
	writes          []packet.Packet
	writeErr        error
	writePanic      any
	configPanic     any
	writeStarted    chan struct{}
	writeUnblock    <-chan struct{}
	writeStartOnce  sync.Once
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

func (downstream *offerTestDownstream) ConfigureResourcePackOffer(packs []*resource.Pack, required bool) error {
	if downstream.configPanic != nil {
		panic(downstream.configPanic)
	}
	downstream.configured = true
	downstream.packs = slices.Clone(packs)
	downstream.required = required
	return downstream.err
}

func (downstream *offerTestDownstream) ConfigureResourcePackStack(stack minecraft.ResourcePackStackSnapshot, required bool) error {
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
	if !downstream.configured {
		t.Fatal("downstream offer was not configured")
	}
	if !downstream.configuredStack || downstream.required {
		t.Fatalf("downstream offer = (stack=%t, required=%t), want selected optional stack", downstream.configuredStack, downstream.required)
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

func TestConfigureResourcePackOfferRejectsNonEmptyRequiredOffer(t *testing.T) {
	upstream := newFakeUpstream(nil)
	upstream.packs = []*resource.Pack{new(resource.Pack)}
	upstream.required = true
	downstream := new(offerTestDownstream)

	err := configureResourcePackOffer(downstream, &selectedResourcePackStack{packs: slices.Clone(upstream.packs), required: true})
	var admissionErr *PackAdmissionError
	if !errors.As(err, &admissionErr) {
		t.Fatalf("configureResourcePackOffer() error = %v, want *PackAdmissionError", err)
	}
	if admissionErr.Reason != PackAdmissionRequiredUnsupported || admissionErr.PackCount != 1 {
		t.Fatalf("admission error = %#v", admissionErr)
	}
	if downstream.configured {
		t.Fatal("required upstream offer was rewritten instead of rejected")
	}
	if len(downstream.writes) != 1 {
		t.Fatalf("downstream packet count = %d, want one typed Disconnect", len(downstream.writes))
	}
	disconnect, ok := downstream.writes[0].(*packet.Disconnect)
	if !ok {
		t.Fatalf("downstream packet type = %T, want *packet.Disconnect", downstream.writes[0])
	}
	if disconnect.Reason != packet.DisconnectReasonResourcePackProblem || disconnect.HideDisconnectionScreen || disconnect.Message != packAdmissionDisconnectMessage {
		t.Fatalf("disconnect = %#v", disconnect)
	}
}

func TestConfigureResourcePackOfferPreservesTypedFailureWhenDisconnectWriteFails(t *testing.T) {
	upstream := newFakeUpstream(nil)
	upstream.packs = []*resource.Pack{new(resource.Pack)}
	upstream.required = true
	writeErr := errors.New("write failed")
	downstream := &offerTestDownstream{writeErr: writeErr}

	err := configureResourcePackOffer(downstream, &selectedResourcePackStack{packs: slices.Clone(upstream.packs), required: true})
	var admissionErr *PackAdmissionError
	if !errors.As(err, &admissionErr) || !errors.Is(err, writeErr) {
		t.Fatalf("configureResourcePackOffer() error = %v, want admission and write errors", err)
	}
}

func TestConfigureResourcePackOfferAllowsEmptyRequiredBitAsEmptyOptional(t *testing.T) {
	upstream := newFakeUpstream(nil)
	upstream.required = true
	downstream := new(offerTestDownstream)

	if err := configureResourcePackOffer(downstream, &selectedResourcePackStack{required: true}); err != nil {
		t.Fatalf("configureResourcePackOffer() error = %v", err)
	}
	if !downstream.configured || len(downstream.packs) != 0 || downstream.required {
		t.Fatalf("downstream offer = (configured=%t, %d packs, required=%t), want configured empty optional offer", downstream.configured, len(downstream.packs), downstream.required)
	}
}

func TestSelectedResourcePackStackRetainsExactOrderAndOwnsClones(t *testing.T) {
	first := testAdmissionPack(t).WithDownloadURL("https://example.invalid/first")
	second := testAdmissionPack(t).WithDownloadURL("https://example.invalid/second")
	third := testAdmissionPack(t).WithDownloadURL("https://example.invalid/third")

	stack, err := newSelectedResourcePackStack([]*resource.Pack{third, first}, true, resourcePackSize)
	if err != nil {
		t.Fatalf("newSelectedResourcePackStack() error = %v", err)
	}
	if !stack.required || len(stack.packs) != 2 {
		t.Fatalf("selected stack = (required=%t, count=%d), want (true, 2)", stack.required, len(stack.packs))
	}
	if got := []string{stack.packs[0].DownloadURL(), stack.packs[1].DownloadURL()}; !slices.Equal(got, []string{"https://example.invalid/third", "https://example.invalid/first"}) {
		t.Fatalf("selected order = %v", got)
	}
	if stack.packs[0] == third || stack.packs[1] == first {
		t.Fatal("selected stack retained caller-owned pack pointers")
	}
	if slices.Contains(stack.packs, second) {
		t.Fatal("selected stack retained a pack omitted by the server stack")
	}
}

func TestSelectedStackPolicyDoesNotSubstituteOfferOrderOrCounts(t *testing.T) {
	offered := newFakeUpstream(nil)
	offered.packs = []*resource.Pack{testAdmissionPack(t), testAdmissionPack(t), testAdmissionPack(t)}
	telemetry := newResourcePackAdmissionTelemetry(1, nil)
	telemetry.observeOffer(offered)
	selected := &selectedResourcePackStack{packs: []*resource.Pack{offered.packs[2], offered.packs[0]}, required: true}

	err := configureResourcePackOffer(new(offerTestDownstream), selected)
	var admissionErr *PackAdmissionError
	if !errors.As(err, &admissionErr) || admissionErr.PackCount != 2 {
		t.Fatalf("admission error = %v, want selected count 2", err)
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
		t.Fatalf("downstream offer = (configured=%t, stack=%t, required=%t), want selected optional handoff", downstream.configured, downstream.configuredStack, downstream.required)
	}
}

func TestSelectedResourcePackStackFailsClosedForMissingNilAndBounds(t *testing.T) {
	if _, err := captureSelectedResourcePackStack(newFakeUpstream(nil)); !errors.Is(err, errResourcePackStackUnavailable) {
		t.Fatalf("missing post-negotiation snapshot error = %v", err)
	}
	if _, err := newSelectedResourcePackStack([]*resource.Pack{nil}, false, resourcePackSize); !errors.Is(err, errResourcePackStackInvalid) {
		t.Fatalf("nil selected pack error = %v", err)
	}
	tooMany := make([]*resource.Pack, minecraft.DefaultResourcePackMaxPacks+1)
	if _, err := newSelectedResourcePackStack(tooMany, false, resourcePackSize); !errors.Is(err, errResourcePackStackInvalid) {
		t.Fatalf("selected count overflow error = %v", err)
	}
	packs := []*resource.Pack{testAdmissionPack(t), testAdmissionPack(t)}
	index := 0
	overflowingSize := func(*resource.Pack) (uint64, bool) {
		index++
		if index == 1 {
			return math.MaxUint64, true
		}
		return 1, true
	}
	if _, err := newSelectedResourcePackStack(packs, false, overflowingSize); !errors.Is(err, errResourcePackStackInvalid) {
		t.Fatalf("selected byte overflow error = %v", err)
	}
	if err := configureResourcePackOffer(new(offerTestDownstream), nil); !errors.Is(err, errResourcePackStackUnavailable) {
		t.Fatalf("nil prepared stack policy error = %v", err)
	}
}

func TestSelectedResourcePackStackReleasedForAbortRelayAndIdempotence(t *testing.T) {
	for _, test := range []struct {
		name   string
		finish func(*preparedConnection) error
	}{
		{name: "abort", finish: (*preparedConnection).close},
		{name: "relay", finish: (*preparedConnection).releaseAfterRelay},
	} {
		t.Run(test.name, func(t *testing.T) {
			stack := &selectedResourcePackStack{packs: []*resource.Pack{testAdmissionPack(t)}}
			prepared := &preparedConnection{upstream: newFakeUpstream(nil), packStack: stack}
			if err := test.finish(prepared); err != nil {
				t.Fatalf("finish error = %v", err)
			}
			if stack.packs != nil {
				t.Fatal("selected pack references survived prepared connection release")
			}
			if err := prepared.close(); err != nil {
				t.Fatalf("idempotent close error = %v", err)
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
	if err := accepted.StartGameContext(ctx, prepared.upstream.GameData()); err != nil {
		t.Fatalf("start downstream game: %v", err)
	}
	clientResult := <-clientDone
	if clientResult.err != nil {
		t.Fatalf("client dial: %v", clientResult.err)
	}
	defer clientResult.conn.Close()
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
		telemetry: new(cacheBoundaryTelemetry),
		logger:    logger,
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
	if targetCloses.Load() != 1 || handler.count("PHASE2_CACHE_BOUNDARY") != 1 {
		t.Fatalf("target closes=%d telemetry calls=%d, want 1 each", targetCloses.Load(), handler.count("PHASE2_CACHE_BOUNDARY"))
	}
	_ = prepared.close()
	if targetCloses.Load() != 1 || handler.count("PHASE2_CACHE_BOUNDARY") != 1 {
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
	waitForPreparedClose(t, prepared)
	taken, err := takePreparedAfterAccept(connections, accepted)
	if err != nil || taken != nil {
		t.Fatalf("takePreparedAfterAccept = (%p, %v), want ordinary per-client teardown", taken, err)
	}
	assertPreparedClosedExactlyOnce(t, prepared, targetCloses)
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
	if got := strings.Count(output.String(), "msg=PHASE2_CACHE_BOUNDARY"); got != 1 {
		t.Fatalf("telemetry report count = %d, want 1; output=%q", got, output.String())
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
	if targetCloses.Load() != 1 || handler.count("PHASE2_CACHE_BOUNDARY") != 1 {
		t.Fatalf("target closes=%d telemetry calls=%d, want 1 each", targetCloses.Load(), handler.count("PHASE2_CACHE_BOUNDARY"))
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

func TestListenerBoundaryRequiredOfferDisconnectsBeforeLoginAndStaysPerClient(t *testing.T) {
	connections := newTestPreparedConnections()
	prepared, targetCloses := newTrackedPreparedConnection()
	prepared.upstream.(*fakeUpstream).packs = []*resource.Pack{new(resource.Pack)}
	prepared.upstream.(*fakeUpstream).required = true
	prepared.packStack = &selectedResourcePackStack{packs: []*resource.Pack{new(resource.Pack)}, required: true}
	connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
		return prepared, nil
	}
	serveCtx := context.Background()
	serverErrors := make(chan error, 1)
	prepare := func(ctx context.Context, conn *minecraft.Conn) error {
		err := connections.prepare(ctx, conn)
		reportPreparationError(serverErrors, err, serveCtx)
		return err
	}
	_, network := newAdmissionTestListener(t, prepare)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	var packetsMu sync.Mutex
	var packetIDs []uint32
	conn, err := (minecraft.Dialer{
		IdentityData: login.IdentityData{DisplayName: "RequiredTest"},
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
		t.Fatal("required-pack client dial succeeded")
	}
	packetsMu.Lock()
	gotPackets := slices.Clone(packetIDs)
	packetsMu.Unlock()
	if !slices.Contains(gotPackets, packet.IDDisconnect) {
		t.Fatalf("required-pack packets = %v, want Disconnect", gotPackets)
	}
	if slices.Contains(gotPackets, packet.IDPlayStatus) || slices.Contains(gotPackets, packet.IDResourcePacksInfo) {
		t.Fatalf("required-pack packets = %v, must not include LoginSuccess or ResourcePacksInfo", gotPackets)
	}
	select {
	case serverErr := <-serverErrors:
		t.Fatalf("required-pack rejection terminated Serve contract: %v", serverErr)
	default:
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

	waitForPreparedClose(t, prepared)
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
			waitForPreparedClose(t, prepared)
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
		logger: slog.New(slog.NewTextHandler(io.Discard, nil)),
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

func TestShutdownClosesTransportBeforeJoiningBackpressuredRequiredWrite(t *testing.T) {
	connections := newTestPreparedConnections()
	prepared, targetCloses := newTrackedPreparedConnection()
	prepared.upstream.(*fakeUpstream).packs = []*resource.Pack{new(resource.Pack)}
	prepared.upstream.(*fakeUpstream).required = true
	prepared.packStack = &selectedResourcePackStack{packs: []*resource.Pack{new(resource.Pack)}, required: true}
	connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
		return prepared, nil
	}
	writeStarted := make(chan struct{})
	writeUnblock := make(chan struct{})
	downstream := &offerTestDownstream{writeStarted: writeStarted, writeUnblock: writeUnblock}
	prepareDone := make(chan error, 1)
	go func() {
		prepareDone <- connections.prepareConnection(context.Background(), new(minecraft.Conn), downstream)
	}()
	<-writeStarted

	shutdownDone := make(chan error, 1)
	go func() {
		shutdownDone <- shutdownPreparedServer(connections, func() error {
			close(writeUnblock)
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
	var admissionErr *PackAdmissionError
	if err := <-prepareDone; !errors.As(err, &admissionErr) {
		t.Fatalf("prepare error = %v, want required-pack admission error", err)
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
	if got := strings.Count(output.String(), "msg=PHASE2_CACHE_BOUNDARY"); got != 1 {
		t.Fatalf("telemetry report count = %d, want 1; output=%q", got, output.String())
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
	connections.captureResourcePackStack = func(upstreamSession) (*selectedResourcePackStack, error) {
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
			name: "required disconnect write",
			upstream: func(any) upstreamSession {
				base := newFakeUpstream(nil)
				base.packs = []*resource.Pack{new(resource.Pack)}
				base.required = true
				return base
			},
			downstream: func(value any) *offerTestDownstream { return &offerTestDownstream{writePanic: value} },
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
				telemetry: new(cacheBoundaryTelemetry),
				logger:    logger,
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
			if got := strings.Count(output.String(), "msg=PHASE2_CACHE_BOUNDARY"); got != 1 {
				t.Fatalf("telemetry report count = %d, want 1", got)
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
	telemetryCalls := new(atomic.Int32)
	prepared := &preparedConnection{
		upstream:  upstream,
		packStack: &selectedResourcePackStack{},
		releaseTarget: func() error {
			targetCalls.Add(1)
			panic(panicValue)
		},
		telemetry: new(cacheBoundaryTelemetry),
		logger: slog.New(panicSlogHandler{
			calls:      telemetryCalls,
			panicValue: panicValue,
		}),
	}

	err := prepared.close()
	if err == nil || strings.Contains(err.Error(), "sensitive panic payload") || stringCalls.Load() != 0 {
		t.Fatalf("prepared close error = %q, String calls=%d", err, stringCalls.Load())
	}
	if upstream.abortCalls.Load() != 1 || upstream.closeCalls.Load() != 1 || targetCalls.Load() != 1 || telemetryCalls.Load() != 1 {
		t.Fatalf("cleanup calls abort=%d close=%d target=%d telemetry=%d, want all 1",
			upstream.abortCalls.Load(), upstream.closeCalls.Load(), targetCalls.Load(), telemetryCalls.Load())
	}
	_ = prepared.close()
	if upstream.abortCalls.Load() != 1 || upstream.closeCalls.Load() != 1 || targetCalls.Load() != 1 || telemetryCalls.Load() != 1 {
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
	if upstream.abortCalls.Load() != 1 || upstream.closeCalls.Load() != 1 || targetCalls.Load() != 1 || telemetryCalls.Load() != 1 {
		t.Fatalf("cleanup calls abort=%d close=%d target=%d telemetry=%d, want all 1",
			upstream.abortCalls.Load(), upstream.closeCalls.Load(), targetCalls.Load(), telemetryCalls.Load())
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
	connections.captureResourcePackStack = func(upstream upstreamSession) (*selectedResourcePackStack, error) {
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

func newTrackedPreparedConnection() (*preparedConnection, *atomic.Int32) {
	targetCloses := new(atomic.Int32)
	return &preparedConnection{
		upstream:  newFakeUpstream(nil),
		packStack: &selectedResourcePackStack{},
		releaseTarget: func() error {
			targetCloses.Add(1)
			return nil
		},
		logger: slog.New(slog.NewTextHandler(io.Discard, nil)),
	}, targetCloses
}

func waitForPreparedClose(t *testing.T, prepared *preparedConnection) {
	t.Helper()
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		if len(lifecycleEvents(prepared.upstream)) != 0 {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("prepared connection was not closed")
}

func assertPreparedClosedExactlyOnce(t *testing.T, prepared *preparedConnection, targetCloses *atomic.Int32) {
	t.Helper()
	if got := lifecycleEvents(prepared.upstream); !slices.Equal(got, []string{"abort", "close"}) {
		t.Fatalf("upstream lifecycle = %v, want [abort close]", got)
	}
	if got := targetCloses.Load(); got != 1 {
		t.Fatalf("target close count = %d, want 1", got)
	}
}
