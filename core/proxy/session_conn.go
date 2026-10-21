package proxy

import (
	"bytes"
	"fmt"
	"io"
	"net"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/latencytrace"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// sessionConn is the client leg of one session, shaped as the relay's downstream: batches in and
// out are Batch frames, and an upstream Transfer or final Disconnect becomes a terminal message.
type sessionConn struct {
	conn       *streamnet.FramedConn
	frames     chan sessionRead // the reader's frames, in order
	done       chan struct{}
	readDone   chan struct{}
	closeOnce  sync.Once
	closeErr   error
	clientPool packet.Pool
	serverPool packet.Pool
	shieldID   atomic.Int32  // from the ItemRegistry relayed to the client, for decoding its item stacks
	runtimeID  atomic.Uint64 // the player's runtime ID from the upstream StartGame

	terminalDelivered atomic.Bool // a complete terminal frame reached the local transport

	respawnMu          sync.Mutex
	immediateRespawn   atomic.Bool
	injectedUpstream   [][]byte
	lastRespawnTrigger time.Time

	writeMu sync.Mutex
	pending []byte // the Batch frame being formed, kind byte included
	ended   bool   // a terminal message was written

	trace       *latencytrace.Trace // nil unless tracing; the rest are guarded by writeMu
	traceConn   int64
	traceFrames int64    // Batch frames written to the client
	traceIDs    []uint32 // packet IDs of the forming batch
	traceRead   int64    // when the forming batch's upstream batch was read and decoded
}

type sessionRead struct {
	frame []byte
	err   error
}

func newSessionConn(conn *streamnet.FramedConn) *sessionConn {
	session := &sessionConn{
		conn:       conn,
		frames:     make(chan sessionRead, 256),
		done:       make(chan struct{}),
		readDone:   make(chan struct{}),
		clientPool: minecraft.DefaultProtocol.Packets(true),
		serverPool: minecraft.DefaultProtocol.Packets(false),
	}
	if trace := latencytrace.Active(); trace != nil {
		session.trace, session.traceConn = trace, trace.NextConn()
	}
	go session.read()
	return session
}

func (session *sessionConn) read() {
	defer close(session.readDone)
	for {
		frame, err := session.conn.ReadPacket()
		select {
		case session.frames <- sessionRead{frame: frame, err: err}:
		case <-session.done:
			if err == nil && session.terminalDelivered.Load() {
				// The session has ended; consume in-flight client bytes until its close.
				_, _ = io.Copy(io.Discard, session.conn.Conn)
			}
			return
		}
		if err != nil {
			return
		}
	}
}

// GameData carries the player's runtime ID, so packet-delay tracking recognizes the player's own MovePlayer.
func (session *sessionConn) GameData() minecraft.GameData {
	return minecraft.GameData{EntityRuntimeID: session.runtimeID.Load()}
}

// watchPeer cancels the session when the client leaves or sends anything before its handoff;
// the returned stop is idempotent and returns once the watcher no longer reads.
func (session *sessionConn) watchPeer(cancel func(error)) (stop func()) {
	stopped, done := make(chan struct{}), make(chan struct{})
	go func() {
		defer close(done)
		select {
		case read := <-session.frames:
			cause := read.err
			if cause == nil {
				cause = fmt.Errorf("%w: client message before handoff", errMalformedSession)
			}
			cancel(cause)
		case <-stopped:
		}
	}()
	return sync.OnceFunc(func() {
		close(stopped)
		<-done
	})
}

func encodeSessionPacket(pk packet.Packet, shieldID int32) []byte {
	buf := new(bytes.Buffer)
	(&packet.Header{PacketID: pk.ID()}).Write(buf)
	pk.Marshal(minecraft.DefaultProtocol.NewWriter(buf, shieldID))
	return buf.Bytes()
}

func (session *sessionConn) triggerRespawnCompletion() {
	runtimeID := session.runtimeID.Load()
	if runtimeID == 0 {
		return
	}
	session.respawnMu.Lock()
	defer session.respawnMu.Unlock()
	if time.Since(session.lastRespawnTrigger) < 400*time.Millisecond {
		return
	}
	session.lastRespawnTrigger = time.Now()

	actionPk := encodeSessionPacket(&packet.PlayerAction{
		EntityRuntimeID: runtimeID,
		ActionType:      protocol.PlayerActionRespawn,
		BlockFace:       -1,
	}, session.shieldID.Load())

	respawnPk := encodeSessionPacket(&packet.Respawn{
		EntityRuntimeID: runtimeID,
		State:           packet.RespawnStateClientReadyToSpawn,
	}, session.shieldID.Load())

	session.injectedUpstream = append(session.injectedUpstream, actionPk, respawnPk)
}

// ReadBatchRaw returns the next client batch; only Batch frames may follow the handoff.
func (session *sessionConn) ReadBatchRaw(decode func(uint32) bool) ([]minecraft.RawPacket, error) {
	var read sessionRead
	select {
	case read = <-session.frames:
	case <-session.done:
		return nil, net.ErrClosed
	}
	if read.err != nil {
		return nil, read.err
	}
	if len(read.frame) == 0 || read.frame[0] != bridgecontract.SessionKindBatch {
		return nil, fmt.Errorf("%w: expected batch", errMalformedSession)
	}
	packets, err := splitSessionBatch(read.frame[1:])
	if err != nil {
		return nil, err
	}

	session.respawnMu.Lock()
	if len(session.injectedUpstream) > 0 {
		packets = append(packets, session.injectedUpstream...)
		session.injectedUpstream = nil
	}
	session.respawnMu.Unlock()

	for _, raw := range packets {
		if id, ok := sessionPacketID(raw); ok {
			switch id {
			case packet.IDRespawn:
				runtimeID := session.runtimeID.Load()
				if runtimeID != 0 {
					actionPk := encodeSessionPacket(&packet.PlayerAction{
						EntityRuntimeID: runtimeID,
						ActionType:      protocol.PlayerActionRespawn,
						BlockFace:       -1,
					}, session.shieldID.Load())
					packets = append(packets, actionPk)
				}
			case packet.IDPlayerAction:
				if pk, ok := decodeSessionPacket(session.clientPool, raw, 0, false).(*packet.PlayerAction); ok {
					if pk.ActionType == protocol.PlayerActionRespawn {
						runtimeID := session.runtimeID.Load()
						if runtimeID != 0 {
							respawnPk := encodeSessionPacket(&packet.Respawn{
								EntityRuntimeID: runtimeID,
								State:           packet.RespawnStateClientReadyToSpawn,
							}, session.shieldID.Load())
							packets = append(packets, respawnPk)
						}
					}
				}
			}
		}
	}

	return rawSessionPackets(packets, decode, session.clientPool, session.shieldID.Load())
}

// WritePacketRaw adds an upstream packet to the forming batch. A usable Transfer instead flushes
// the batch, ends the session with a Transfer message and returns errSessionEnded.
func (session *sessionConn) WritePacketRaw(data []byte) error {
	session.writeMu.Lock()
	defer session.writeMu.Unlock()
	if session.ended {
		return errSessionEnded
	}
	if id, ok := sessionPacketID(data); ok {
		switch id {
		case packet.IDItemRegistry:
			session.observeItemRegistry(data)
		case packet.IDTransfer:
			if transfer, ok := decodeSessionPacket(session.serverPool, data, 0, false).(*packet.Transfer); ok {
				if _, err := transferAddress(transfer.Address, transfer.Port); err == nil {
					return session.endLocked(bridgecontract.SessionKindTransfer, sessionTransfer{
						Address: transfer.Address, Port: transfer.Port, ReloadWorld: transfer.ReloadWorld,
					})
				}
			}
		case packet.IDStartGame:
			if pk, ok := decodeSessionPacket(session.serverPool, data, 0, false).(*packet.StartGame); ok {
				if pk.EntityRuntimeID != 0 {
					session.runtimeID.Store(pk.EntityRuntimeID)
				}
				for _, rule := range pk.GameRules {
					if strings.EqualFold(rule.Name, "doimmediaterespawn") {
						if val, ok := rule.Value.(bool); ok {
							session.immediateRespawn.Store(val)
						}
					}
				}
			}
		case packet.IDGameRulesChanged:
			if pk, ok := decodeSessionPacket(session.serverPool, data, 0, false).(*packet.GameRulesChanged); ok {
				for _, rule := range pk.GameRules {
					if strings.EqualFold(rule.Name, "doimmediaterespawn") {
						if val, ok := rule.Value.(bool); ok {
							session.immediateRespawn.Store(val)
						}
					}
				}
			}
		case packet.IDRespawn:
			if pk, ok := decodeSessionPacket(session.serverPool, data, 0, false).(*packet.Respawn); ok {
				if pk.State == packet.RespawnStateReadyToSpawn {
					session.triggerRespawnCompletion()
				}
			}
		case packet.IDUpdateAttributes:
			if pk, ok := decodeSessionPacket(session.serverPool, data, 0, false).(*packet.UpdateAttributes); ok {
				if pk.EntityRuntimeID == session.runtimeID.Load() {
					for _, attr := range pk.Attributes {
						if attr.Name == "minecraft:health" && attr.Value <= 0.0 {
							if session.immediateRespawn.Load() {
								session.triggerRespawnCompletion()
							}
						}
					}
				}
			}
		}
	}
	if len(session.pending) == 0 {
		session.pending = append(session.pending, bridgecontract.SessionKindBatch)
	}
	session.pending = appendSessionBatchPacket(session.pending, data)
	if session.trace != nil {
		id, _ := sessionPacketID(data)
		session.traceIDs = append(session.traceIDs, id)
	}
	return nil
}

// traceBatchRead stamps the forming batch with when its upstream batch was read and decoded.
func (session *sessionConn) traceBatchRead(readAt int64) {
	session.writeMu.Lock()
	session.traceRead = readAt
	session.writeMu.Unlock()
}

// observeItemRegistry records the shield runtime ID from an ItemRegistry the client receives.
func (session *sessionConn) observeItemRegistry(data []byte) {
	if id, ok := sessionPacketID(data); !ok || id != packet.IDItemRegistry {
		return
	}
	if registry, ok := decodeSessionPacket(session.serverPool, data, 0, false).(*packet.ItemRegistry); ok {
		for _, item := range registry.Items {
			if item.Name == "minecraft:shield" {
				session.shieldID.Store(int32(item.RuntimeID))
			}
		}
	}
}

// WritePacket encodes value and adds it to the forming batch.
func (session *sessionConn) WritePacket(value packet.Packet) error {
	buf := new(bytes.Buffer)
	(&packet.Header{PacketID: value.ID()}).Write(buf)
	value.Marshal(minecraft.DefaultProtocol.NewWriter(buf, session.shieldID.Load()))
	return session.WritePacketRaw(buf.Bytes())
}

// WritePacketImmediate delivers the relay's final Disconnect as a Disconnect message.
func (session *sessionConn) WritePacketImmediate(packets ...packet.Packet) error {
	for _, value := range packets {
		if disconnect, ok := value.(*packet.Disconnect); ok {
			return session.DisconnectPacket(*disconnect)
		}
		if err := session.WritePacket(value); err != nil {
			return err
		}
	}
	return session.Flush()
}

// DisconnectPacket flushes the forming batch and ends the session with a Disconnect message.
func (session *sessionConn) DisconnectPacket(value packet.Disconnect) error {
	session.writeMu.Lock()
	defer session.writeMu.Unlock()
	if session.ended {
		return nil // a Transfer already ended the session
	}
	err := session.endLocked(bridgecontract.SessionKindDisconnect, sessionDisconnect{
		Reason: value.Reason, Message: value.Message, FilteredMessage: value.FilteredMessage,
		HideScreen: value.HideDisconnectionScreen,
	})
	if err == errSessionEnded {
		return nil
	}
	return err
}

// Flush writes the forming batch as one Batch frame.
func (session *sessionConn) Flush() error {
	session.writeMu.Lock()
	defer session.writeMu.Unlock()
	return session.flushLocked()
}

func (session *sessionConn) flushLocked() error {
	if len(session.pending) == 0 {
		return nil
	}
	var began int64
	if session.trace != nil {
		began = session.trace.Now()
	}
	_, err := session.conn.Write(session.pending)
	if session.trace != nil {
		if err == nil { // a failed write never reached the client
			session.traceFrames++
			session.trace.RecordIDs(latencytrace.Inbound, session.traceIDs, session.traceConn, session.traceFrames, session.traceRead, began, session.trace.Now())
		}
		session.traceIDs, session.traceRead = session.traceIDs[:0], 0
	}
	// Write has returned, so the buffer is reused unless one large batch grew it.
	if cap(session.pending) > bridgecontract.SessionPackChunkBytes {
		session.pending = nil
	} else {
		session.pending = session.pending[:0]
	}
	return err
}

// endLocked flushes, writes the terminal message and refuses later writes.
func (session *sessionConn) endLocked(kind byte, value any) error {
	if err := session.flushLocked(); err != nil {
		return err
	}
	session.ended = true
	frame, err := encodeSessionJSON(kind, value)
	if err != nil {
		return err
	}
	if _, err := session.conn.Write(frame); err != nil {
		return err
	}
	session.terminalDelivered.Store(true)
	return errSessionEnded
}

// writeFrame writes one complete message ahead of any forming batch.
func (session *sessionConn) writeFrame(frame []byte) error {
	session.writeMu.Lock()
	defer session.writeMu.Unlock()
	if session.ended {
		return errSessionEnded
	}
	_, err := session.conn.Write(frame)
	return err
}

// Close preserves a delivered terminal frame while the TCP peer finishes its pending writes.
func (session *sessionConn) Close() error {
	session.closeOnce.Do(func() {
		close(session.done)
		session.drainTerminal()
		session.closeErr = session.conn.Close()
	})
	return session.closeErr
}

// sessionTerminalDrainTimeout bounds teardown when a TCP peer never acknowledges the close.
const sessionTerminalDrainTimeout = time.Second

// drainTerminal half-closes TCP and lets the sole reader drain late client data before closing.
func (session *sessionConn) drainTerminal() {
	conn, ok := session.conn.Conn.(*net.TCPConn)
	if !ok || !session.terminalDelivered.Load() {
		return
	}
	if conn.SetReadDeadline(time.Now().Add(sessionTerminalDrainTimeout)) != nil || conn.CloseWrite() != nil {
		return
	}
	<-session.readDone
}

// Abort ends the session, preserving only a terminal frame that was already delivered.
func (session *sessionConn) Abort() error { return session.Close() }
