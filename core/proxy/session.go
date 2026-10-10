package proxy

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"sync"
	"time"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// sessionConnectTimeout bounds the wait for Connect; a client sends it as soon as it connects.
const sessionConnectTimeout = 10 * time.Second

// errSessionEnded marks a session ended by a terminal message; it classifies as an ordinary close.
var errSessionEnded = fmt.Errorf("proxy: session ended: %w", net.ErrClosed)

// sessionServer serves the session endpoint, where the core makes the only Minecraft login.
// Each connection carries one upstream session: Connect, the handoff, then raw batches both ways.
type sessionServer struct {
	listener     net.Listener
	prepared     *preparedConnections
	transfers    *TransferState
	onDisconnect func(DisconnectInfo)
	selectTarget func(ctx context.Context, kind, value string) (string, error) // nil rejects targeted Connects
	dialTarget   func(ctx context.Context, address string) (*resolvedUpstreamTarget, error)
	delay        *PacketDelay
	logger       *slog.Logger

	acceptDone chan struct{}
	closing    chan struct{} // closed by close, ending an accept retry wait
	sessions   sync.WaitGroup
	mu         sync.Mutex
	closed     bool
	conns      map[net.Conn]struct{}
}

// start accepts sessions until close; ctx ends every session.
func (server *sessionServer) start(ctx context.Context) {
	server.conns = make(map[net.Conn]struct{})
	server.acceptDone = make(chan struct{})
	server.closing = make(chan struct{})
	go server.accept(ctx)
}

// sessionAcceptRetryLimit caps the backoff after a failed Accept, such as one out of file descriptors.
const sessionAcceptRetryLimit = time.Second

func (server *sessionServer) accept(ctx context.Context) {
	defer close(server.acceptDone)
	var retry time.Duration
	for {
		conn, err := server.listener.Accept()
		if err != nil {
			if errors.Is(err, net.ErrClosed) || ctx.Err() != nil {
				return
			}
			retry = min(max(2*retry, 5*time.Millisecond), sessionAcceptRetryLimit)
			server.logger.Warn("session accept failed; retrying", "error", err, "retry", retry)
			select {
			case <-time.After(retry):
				continue
			case <-ctx.Done():
			case <-server.closing:
			}
			return
		}
		retry = 0
		server.mu.Lock()
		if server.closed {
			server.mu.Unlock()
			_ = conn.Close()
			return
		}
		server.conns[conn] = struct{}{}
		server.sessions.Add(1)
		server.mu.Unlock()
		go func() {
			defer server.sessions.Done()
			err := server.serveConn(ctx, conn)
			server.mu.Lock()
			delete(server.conns, conn)
			server.mu.Unlock()
			_ = conn.Close()
			if err != nil && !streamnet.IsClosed(err) {
				server.logger.Warn("session ended", "error", err)
			}
		}()
	}
}

// close stops accepting, ends every session and waits for them.
func (server *sessionServer) close() error {
	server.mu.Lock()
	if !server.closed {
		close(server.closing)
	}
	server.closed = true
	conns := make([]net.Conn, 0, len(server.conns))
	for conn := range server.conns {
		conns = append(conns, conn)
	}
	server.mu.Unlock()
	err := server.listener.Close()
	for _, conn := range conns {
		_ = conn.Close()
	}
	<-server.acceptDone
	server.sessions.Wait()
	return err
}

func (server *sessionServer) serveConn(ctx context.Context, raw net.Conn) error {
	framed := streamnet.NewFramedConn(raw)
	if err := raw.SetReadDeadline(time.Now().Add(sessionConnectTimeout)); err != nil {
		return err
	}
	frame, err := framed.ReadPacket()
	if err != nil {
		return err
	}
	if err := raw.SetReadDeadline(time.Time{}); err != nil {
		return err
	}
	request, err := decodeSessionConnect(frame)
	var downstream *sessionDownstream
	if err == nil {
		downstream, err = newSessionDownstream(request)
	}
	if err != nil {
		return errors.Join(err, refuseSessionConnect(raw, framed, err))
	}
	sessionCtx, cancel := context.WithCancelCause(ctx)
	defer cancel(nil)
	session := newSessionConn(framed)
	defer func() { _ = session.Close() }()

	plan, prepared, err := server.prepare(sessionCtx, cancel, session, downstream, request.Target)
	if err != nil {
		if sessionCtx.Err() == nil {
			relayPreLoginDisconnect(session, err)
			reportDisconnect(server.onDisconnect, err)
		}
		return err
	}
	return handOffSession(sessionCtx, session, prepared, plan)
}

// sessionPlan is what the handoff carries, taken from a prepared upstream.
type sessionPlan struct {
	handoff sessionHandoff
	packs   []*resource.Pack // content of handoff.Packs, in the same order
	startup [][]byte         // packets through StartGame
	rest    [][]byte         // the rest of StartGame's batch
}

// prepare joins upstream while watching the client, then reads startup and selects the packs.
// A target is resolved for this session alone. The returned connection owns the upstream leg;
// the caller still owns session.
func (server *sessionServer) prepare(
	ctx context.Context,
	cancel context.CancelCauseFunc,
	session *sessionConn,
	downstream *sessionDownstream,
	target *sessionTarget,
) (plan sessionPlan, prepared *preparedConnection, err error) {
	stopWatch := session.watchPeer(cancel)
	defer stopWatch()
	if target != nil {
		if server.selectTarget == nil || server.dialTarget == nil {
			return plan, nil, errors.New("proxy: session targets are unavailable")
		}
		address, err := server.selectTarget(ctx, target.Kind, target.Value)
		if err != nil {
			return plan, nil, err
		}
		downstream.resolve = func(ctx context.Context) (*resolvedUpstreamTarget, error) {
			return server.dialTarget(ctx, address)
		}
	}
	err = server.prepared.tracked(ctx, func(prepareCtx context.Context) error {
		connected, err := server.prepared.connectPrepared(prepareCtx, downstream)
		if err != nil {
			return errors.Join(err, connected.close())
		}
		prepared = connected
		return nil
	})
	if err != nil {
		return plan, nil, err
	}
	prepared.packetDelay = server.delay
	// The dial returned at StartGame; the observing wrappers below hide the upstream's game data.
	if game, ok := prepared.upstream.(interface{ GameData() minecraft.GameData }); ok {
		session.runtimeID.Store(game.GameData().EntityRuntimeID)
	}
	prepared.upstream = observeTransfers(prepared.upstream, server.transfers, server.logger)
	defer func() {
		if err != nil {
			err = errors.Join(err, prepared.close())
			prepared = nil
		}
	}()
	owner := prepared
	stopRead := context.AfterFunc(ctx, func() { _ = owner.close() })
	plan.startup, plan.rest, err = readSessionStartup(prepared.upstream)
	if !stopRead() && err == nil {
		err = context.Cause(ctx)
	}
	if err != nil {
		return plan, prepared, err
	}
	// The caller reports a disconnect read above, so the disconnect observer wraps only the relay.
	prepared.upstream = observeDisconnects(prepared.upstream, server.onDisconnect)
	selected, packs, err := selectSessionPacks(prepared.packStack, server.logger)
	prepared.packAdmission.observePolicyOutcome(prepared.packStack, err == nil)
	if err != nil {
		return plan, prepared, err
	}
	stopWatch()
	if err = context.Cause(ctx); err != nil {
		return plan, prepared, err
	}
	identity := prepared.upstream.IdentityData()
	plan.handoff = sessionHandoff{
		Identity:      sessionIdentity{DisplayName: identity.DisplayName, XUID: identity.XUID, UUID: identity.Identity},
		ClientCache:   server.prepared.upstreamClientCache && downstream.clientCache,
		PacksRequired: prepared.packStack.required,
		Packs:         selected,
	}
	plan.packs = packs
	return plan, prepared, nil
}

// handOffSession writes the handoff, the pack archives and the rest of StartGame's batch, then relays.
func handOffSession(ctx context.Context, session *sessionConn, prepared *preparedConnection, plan sessionPlan) error {
	if err := writeSessionHandoff(session, plan); err != nil {
		if errors.Is(err, errSessionEnded) {
			err = nil
		}
		return errors.Join(err, prepared.close())
	}
	prepared.packAdmission.observeLocalHandoff(prepared.packStack)
	return servePreparedConnection(ctx, session, prepared)
}

func writeSessionHandoff(session *sessionConn, plan sessionPlan) error {
	for _, data := range plan.startup {
		session.observeItemRegistry(data)
	}
	frame, err := encodeSessionHandoff(plan.handoff, plan.startup)
	if err != nil {
		return err
	}
	if err := session.writeFrame(frame); err != nil {
		return err
	}
	if err := writeSessionPacks(session, plan.packs); err != nil {
		return err
	}
	for _, data := range plan.rest {
		if err := session.WritePacketRaw(data); err != nil {
			return err
		}
	}
	return session.Flush()
}

// writeSessionPacks streams each archive in PackData frames of at most sessionPackChunkBytes.
func writeSessionPacks(session *sessionConn, packs []*resource.Pack) error {
	var frame []byte
	for index, pack := range packs {
		size := pack.Size()
		for offset := 0; offset < size; {
			n := min(sessionPackChunkBytes, size-offset)
			if cap(frame) < 5+n {
				frame = make([]byte, 5+n)
			}
			frame = frame[:5+n]
			putSessionPackHeader(frame, uint32(index))
			if read, err := pack.ReadAt(frame[5:], int64(offset)); read != n {
				return errors.Join(io.ErrUnexpectedEOF, err)
			}
			if err := session.writeFrame(frame); err != nil {
				return err
			}
			offset += n
		}
	}
	return nil
}

// readSessionStartup reads upstream batches through StartGame, which a HandoffAtStartGame dial has
// already received. The packets up to StartGame travel in the handoff; the rest of its batch follows.
func readSessionStartup(upstream packetSession) (startup, rest [][]byte, err error) {
	size := 0
	for {
		batch, err := upstream.ReadBatchRaw(nil)
		if err != nil {
			return nil, nil, err
		}
		for index, raw := range batch {
			if size += len(raw.Data); size > streamnet.MaxFrameLen {
				return nil, nil, errors.New("proxy: session startup packets exceed one frame")
			}
			startup = append(startup, raw.Data)
			if raw.ID == packet.IDStartGame {
				for _, after := range batch[index+1:] {
					rest = append(rest, after.Data)
				}
				return startup, rest, nil
			}
		}
	}
}

// selectSessionPacks lists the archives to apply from the projected offer and the server's stack.
func selectSessionPacks(stack *selectedResourcePackStack, logger *slog.Logger) ([]sessionPack, []*resource.Pack, error) {
	if stack == nil {
		return nil, nil, errResourcePackStackUnavailable
	}
	var offers []sessionOffer
	for _, entry := range stack.offer.TexturePacks() {
		offers = append(offers, sessionOffer{info: entry.Info(), pack: entry.Pack()})
	}
	var entries []sessionStackEntry
	for _, entry := range stack.snapshot.Entries() {
		entries = append(entries, sessionStackEntry{uuid: entry.UUID(), version: entry.Version(), subPack: entry.SubPackName()})
	}
	return chooseSessionPacks(offers, entries, stack.required, logger)
}

// sessionOffer is one offered pack with its acquired content.
type sessionOffer struct {
	info protocol.TexturePackInfo
	pack *resource.Pack
}

// sessionStackEntry is one ResourcePackStack entry as the server sent it.
type sessionStackEntry struct {
	uuid, version, subPack string
}

// chooseSessionPacks selects archives in stack order: an offer repeating an identity keeps its first
// entry, as vanilla requests each identity once; built-in packs need no archive; and an unavailable
// pack, a repeated stack entry or a sub-pack that differs from the offer is skipped, or refuses the
// join when the packs are required. A nil logger drops the skip count.
func chooseSessionPacks(offers []sessionOffer, entries []sessionStackEntry, required bool, logger *slog.Logger) ([]sessionPack, []*resource.Pack, error) {
	refuse := &PackAdmissionError{Reason: PackAdmissionRequiredUnsupported, PackCount: len(offers)}
	byIdentity := make(map[string]sessionOffer, len(offers))
	repeated := 0
	for _, offer := range offers {
		id := resourcePackIdentity(offer.info.UUID.String(), offer.info.Version)
		if _, ok := byIdentity[id]; ok {
			repeated++
			continue
		}
		byIdentity[id] = offer
	}
	if repeated != 0 && logger != nil {
		logger.Warn("ignoring repeated resource-pack offer entries", "count", repeated)
	}
	var selected []sessionPack
	var packs []*resource.Pack
	seen := make(map[string]bool)
	for _, entry := range entries {
		if minecraft.IsBuiltinResourcePack(entry.uuid, entry.version) {
			continue
		}
		// Parsed as the client's selection did, so a stack may spell an offered UUID in any case.
		var id string
		var offer sessionOffer
		if parsed, err := uuid.Parse(entry.uuid); err == nil {
			id = resourcePackIdentity(parsed.String(), entry.version)
			offer = byIdentity[id]
		}
		if offer.pack == nil || seen[id] || offer.info.SubPackName != entry.subPack {
			if required {
				return nil, nil, refuse
			}
			continue
		}
		seen[id] = true
		selected = append(selected, sessionPack{
			UUID: offer.info.UUID.String(), Version: offer.info.Version, SubPack: entry.subPack,
			ContentKey: offer.info.ContentKey, Size: uint64(max(offer.pack.Size(), 0)),
		})
		packs = append(packs, offer.pack)
	}
	return selected, packs, nil
}

// sessionDownstream is the login a Connect asks the core to make upstream.
type sessionDownstream struct {
	identity    login.IdentityData
	clientData  login.ClientData
	clientCache bool
	resolve     func(context.Context) (*resolvedUpstreamTarget, error) // nil uses the core's selection
}

// sessionJoinDownstream lets a session narrow the core's static upstream blob-cache opt-in to the
// client's answer and replace the shared target selection with its own target.
type sessionJoinDownstream interface {
	sessionClientCache() bool
	sessionResolveTarget() func(context.Context) (*resolvedUpstreamTarget, error)
}

func (downstream *sessionDownstream) IdentityData() login.IdentityData { return downstream.identity }
func (downstream *sessionDownstream) ClientData() login.ClientData     { return downstream.clientData }
func (downstream *sessionDownstream) Proto() minecraft.Protocol        { return minecraft.DefaultProtocol }
func (downstream *sessionDownstream) sessionClientCache() bool         { return downstream.clientCache }
func (downstream *sessionDownstream) sessionResolveTarget() func(context.Context) (*resolvedUpstreamTarget, error) {
	return downstream.resolve
}

// sessionRefusedError carries the lang key a refused Connect is answered with.
type sessionRefusedError struct {
	key string
	err error
}

func (err *sessionRefusedError) Error() string { return err.err.Error() }
func (err *sessionRefusedError) Unwrap() error { return err.err }

// refuseSessionConnect answers a refused Connect with a Disconnect naming why, before the caller closes.
func refuseSessionConnect(raw net.Conn, framed *streamnet.FramedConn, cause error) error {
	key := "disconnectionScreen.cantConnect"
	var refused *sessionRefusedError
	if errors.As(cause, &refused) {
		key = refused.key
	}
	frame, err := encodeSessionJSON(sessionKindDisconnect, sessionDisconnectMessage{Message: key})
	if err != nil {
		return err
	}
	if err := raw.SetWriteDeadline(time.Now().Add(sessionConnectTimeout)); err != nil {
		return err
	}
	_, err = framed.Write(frame)
	return err
}

// newSessionDownstream accepts only the pinned protocol and valid login client data.
func newSessionDownstream(request sessionConnectRequest) (*sessionDownstream, error) {
	var clientData login.ClientData
	if err := json.Unmarshal(request.ClientData, &clientData); err != nil {
		return nil, fmt.Errorf("%w: client data: %v", errMalformedSessionMessage, err)
	}
	pinned := minecraft.DefaultProtocol
	if request.Protocol != pinned.ID() || clientData.GameVersion != pinned.Ver() {
		// A newer client protocol finds this core outdated; any other mismatch is the client's, as
		// gophertunnel's listener answers it.
		key := "disconnectionScreen.outdatedClient"
		if request.Protocol > pinned.ID() {
			key = "disconnectionScreen.outdatedServer"
		}
		return nil, &sessionRefusedError{key: key, err: fmt.Errorf("unsupported session protocol %d/%s; want %d/%s", request.Protocol, clientData.GameVersion, pinned.ID(), pinned.Ver())}
	}
	if err := clientData.Validate(); err != nil {
		return nil, fmt.Errorf("%w: client data: %v", errMalformedSessionMessage, err)
	}
	return &sessionDownstream{
		identity:    login.IdentityData{DisplayName: clientData.ThirdPartyName},
		clientData:  clientData,
		clientCache: request.ClientCache,
	}, nil
}
