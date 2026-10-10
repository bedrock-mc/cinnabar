package proxy

import (
	"bytes"
	"fmt"
	"net"
	"sync"
	"sync/atomic"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// sessionConn is the client leg of one session, shaped as the relay's downstream: batches in and
// out are Batch frames, and an upstream Transfer or final Disconnect becomes a terminal message.
type sessionConn struct {
	conn       *streamnet.FramedConn
	frames     chan sessionRead // the reader's frames, in order
	done       chan struct{}
	closeOnce  sync.Once
	closeErr   error
	clientPool packet.Pool
	serverPool packet.Pool
	shieldID   atomic.Int32  // from the ItemRegistry relayed to the client, for decoding its item stacks
	runtimeID  atomic.Uint64 // the player's runtime ID from the upstream StartGame

	writeMu sync.Mutex
	pending []byte // the Batch frame being formed, kind byte included
	ended   bool   // a terminal message was written
}

type sessionRead struct {
	frame []byte
	err   error
}

func newSessionConn(conn *streamnet.FramedConn) *sessionConn {
	session := &sessionConn{
		conn:       conn,
		frames:     make(chan sessionRead),
		done:       make(chan struct{}),
		clientPool: minecraft.DefaultProtocol.Packets(true),
		serverPool: minecraft.DefaultProtocol.Packets(false),
	}
	go session.read()
	return session
}

func (session *sessionConn) read() {
	for {
		frame, err := session.conn.ReadPacket()
		select {
		case session.frames <- sessionRead{frame: frame, err: err}:
		case <-session.done:
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
	if len(read.frame) == 0 || read.frame[0] != sessionKindBatch {
		return nil, fmt.Errorf("%w: expected batch", errMalformedSession)
	}
	packets, err := splitSessionBatch(read.frame[1:])
	if err != nil {
		return nil, err
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
					return session.endLocked(sessionKindTransfer, sessionTransfer{
						Address: transfer.Address, Port: transfer.Port, ReloadWorld: transfer.ReloadWorld,
					})
				}
			}
		}
	}
	if len(session.pending) == 0 {
		session.pending = append(session.pending, sessionKindBatch)
	}
	session.pending = appendSessionBatchPacket(session.pending, data)
	return nil
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
	err := session.endLocked(sessionKindDisconnect, sessionDisconnect{
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
	_, err := session.conn.Write(session.pending)
	// Write has returned, so the buffer is reused unless one large batch grew it.
	if cap(session.pending) > sessionPackChunkBytes {
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

// Close closes the connection once; reads and writes in progress fail.
func (session *sessionConn) Close() error {
	session.closeOnce.Do(func() {
		close(session.done)
		session.closeErr = session.conn.Close()
	})
	return session.closeErr
}

// Abort is Close: nothing buffered is worth delivering once the session is torn down.
func (session *sessionConn) Abort() error { return session.Close() }
