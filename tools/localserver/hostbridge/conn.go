package hostbridge

import (
	"bytes"
	"context"
	"encoding/binary"
	"io"
	"log/slog"
	"net"
	"sync"
	"sync/atomic"
	"time"

	"github.com/df-mc/dragonfly/server/session"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/internal/hostcontract"
)

var _ session.Conn = (*Conn)(nil)

// Conn is the sole Dragonfly connection to one Rust-authenticated primary player.
type Conn struct {
	wire              *wire
	listener          *Listener
	peer              uint64
	identity          login.IdentityData
	client            login.ClientData
	ctx               context.Context
	cancel            context.CancelFunc
	done              chan struct{}
	release           func()
	releaseMu         sync.Once
	attached          atomic.Bool
	started           atomic.Bool
	initialized       atomic.Bool
	cache             atomic.Bool
	radius            atomic.Int32
	shield            atomic.Int32
	skipped           atomic.Uint64
	pool              packet.Pool
	init              chan hostcontract.Initialized
	flushed           chan uint64
	notify            chan struct{}
	closing           chan struct{}
	closeOnce         sync.Once
	establishDeadline time.Time

	mu          sync.Mutex
	pending     []byte
	pendingID   uint64
	receiveID   uint64
	waitingID   uint64
	remoteClose bool

	admission sync.Mutex
	bufferMu  sync.Mutex
	buffer    [][]byte
	weight    int
	flushMu   sync.Mutex
	sendID    uint64
}

// newConn retains the original authenticated projection and the one process-capacity reservation.
func newConn(parent context.Context, l *Listener, w *wire, open hostcontract.Open, release func()) (*Conn, error) {
	identity, client, err := metadata(open)
	if err != nil {
		return nil, err
	}
	ctx, cancel := context.WithCancel(parent)
	return &Conn{
		wire: w, listener: l, peer: open.Peer, identity: identity, client: client,
		ctx: ctx, cancel: cancel, done: make(chan struct{}), release: release,
		pool: minecraft.DefaultProtocol.Packets(true),
		init: make(chan hostcontract.Initialized, 1), flushed: make(chan uint64, 1),
		notify: make(chan struct{}, 1), closing: make(chan struct{}),
	}, nil
}

func (c *Conn) IdentityData() login.IdentityData { return c.identity }
func (c *Conn) ClientData() login.ClientData     { return c.client }
func (c *Conn) ClientCacheEnabled() bool         { return c.cache.Load() }
func (c *Conn) ChunkRadius() int                 { return int(c.radius.Load()) }
func (c *Conn) Latency() time.Duration           { return 0 } // IPC has no external association RTT observation.
func (c *Conn) RemoteAddr() net.Addr             { return peerAddr(c.peer) }

// stop interrupts all actual socket IO without erasing the one accepted receive prefix.
func (c *Conn) stop() {
	c.cancel()
	_ = c.wire.Close()
}

// releaseSlot retires the exact registry entry once, after its connection owner closes.
func (c *Conn) releaseSlot() {
	c.releaseMu.Do(func() {
		c.listener.forget(c)
		c.release()
	})
}

// Close aborts this connection, joins its owned IO workers and retires its original reservation.
func (c *Conn) Close() error {
	c.stop()
	<-c.done
	c.releaseSlot()
	return nil
}

// run owns the reader and finite flusher; neither goroutine can outlive this connection.
func (c *Conn) run() {
	watchDone := make(chan struct{})
	go func() { defer close(watchDone); <-c.ctx.Done(); _ = c.wire.Close() }()
	flusher := make(chan struct{})
	go func() {
		defer close(flusher)
		c.flushLoop()
	}()
	defer func() {
		c.stop()
		<-flusher
		<-watchDone
		close(c.done)
		if !c.attached.Load() {
			c.releaseSlot()
		}
	}()
	for {
		kind, body, err := c.wire.read()
		if err != nil || c.receive(kind, body) != nil {
			return
		}
	}
}

// ReadPacket consumes the single actual payload before returning its stop-and-wait receipt.
func (c *Conn) ReadPacket() (packet.Packet, error) {
	for {
		c.mu.Lock()
		var body []byte
		var sequence uint64
		if c.pendingID != 0 {
			body, sequence = c.pending, c.pendingID
			c.pending, c.pendingID = nil, 0
		}
		c.mu.Unlock()
		if body == nil {
			select {
			case <-c.notify:
				continue
			case <-c.ctx.Done():
				// Recheck accepted input before observing terminal closure.
				c.mu.Lock()
				accepted := c.pendingID != 0
				c.mu.Unlock()
				if accepted {
					continue
				}
				return nil, io.EOF
			}
		}
		pk, err := c.decodePacket(body)
		if err != nil {
			c.stop()
			return nil, err
		}
		var receipt [8]byte
		binary.BigEndian.PutUint64(receipt[:], sequence)
		if err := c.wire.write(hostcontract.KindFlushed, receipt[:]); err != nil && c.ctx.Err() == nil {
			c.stop()
			return nil, err
		}
		if pk != nil {
			return pk, nil
		}
	}
}

// decodePacket uses the actual selected packet pool and reader, skipping unused secondary/unknown data.
func (c *Conn) decodePacket(body []byte) (pk packet.Packet, err error) {
	defer func() {
		if recover() != nil {
			pk, err = nil, errProtocol
		}
	}()
	b := bytes.NewBuffer(body)
	var header packet.Header
	if header.Read(b) != nil {
		return nil, errProtocol
	}
	if header.SenderSubClient != 0 || header.TargetSubClient != 0 {
		c.skipPacket(header.PacketID)
		return nil, nil
	}
	factory := c.pool[header.PacketID]
	if factory == nil {
		c.skipPacket(header.PacketID)
		return nil, nil
	}
	pk = factory()
	pk.Marshal(minecraft.DefaultProtocol.NewReader(b, c.shield.Load(), true))
	if b.Len() != 0 {
		return nil, errProtocol
	}
	return pk, nil
}

// skipPacket counts only unused well-formed headers without formatting signed metadata.
func (c *Conn) skipPacket(id uint32) {
	slog.Debug("Skipped unused native world packet.", "packet", id, "skipped", c.skipped.Add(1))
}

// StartGameContext consumes only actual selected-world data and waits for Rust's real runtime-ID completion.
func (c *Conn) StartGameContext(ctx context.Context, data minecraft.GameData) error {
	if !c.attached.Load() || !c.started.CompareAndSwap(false, true) {
		return errProtocol
	}
	for _, item := range data.Items {
		if item.Name == "minecraft:shield" {
			c.shield.Store(int32(item.RuntimeID))
		}
	}
	body, err := worldBytes(data, c.listener.resources.snapshot.MaxChunkRadius, c.shield.Load())
	if err != nil || c.wire.write(hostcontract.KindWorld, body) != nil {
		return errProtocol
	}
	select {
	case initialized := <-c.init:
		if ctx.Err() != nil || c.ctx.Err() != nil {
			return errProtocol
		}
		c.cache.Store(initialized.CacheEnabled)
		c.radius.Store(int32(initialized.Radius))
		return nil
	case <-ctx.Done():
		return ctx.Err()
	case <-c.ctx.Done():
		return io.EOF
	}
}
