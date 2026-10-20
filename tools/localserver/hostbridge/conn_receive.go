package hostbridge

import (
	"encoding/binary"
	"io"
	"time"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/internal/hostcontract"
)

// receive never waits on application input; stop-and-wait leaves the one pending payload bounded.
func (c *Conn) receive(kind hostcontract.Kind, body []byte) error {
	switch kind {
	case hostcontract.KindReadPack:
		if c.attached.Load() {
			return errProtocol
		}
		var request hostcontract.Range
		if decode(body, &request) != nil {
			return errProtocol
		}
		b, err := c.listener.resources.read(request)
		if err != nil {
			return err
		}
		return c.wire.write(hostcontract.KindPackBytes, b)
	case hostcontract.KindAttach:
		if len(body) != 0 || !c.attached.CompareAndSwap(false, true) {
			return errProtocol
		}
		select {
		case c.listener.incoming <- c:
			return nil
		default:
			c.attached.Store(false)
			return errProtocol
		}
	case hostcontract.KindInitialized:
		var initialized hostcontract.Initialized
		if !c.started.Load() || !c.initialized.CompareAndSwap(false, true) || decode(body, &initialized) != nil ||
			initialized.Radius == 0 || initialized.Radius > c.listener.resources.snapshot.MaxChunkRadius {
			return errProtocol
		}
		c.init <- initialized
		return c.wire.setBase(time.Time{})
	case hostcontract.KindPacket:
		if !c.initialized.Load() || len(body) == 0 {
			return errProtocol
		}
		c.mu.Lock()
		defer c.mu.Unlock()
		if c.pending != nil || c.remoteClose {
			return errProtocol
		}
		c.pending = body
		return nil
	case hostcontract.KindFlush:
		if len(body) != 8 {
			return errProtocol
		}
		sequence := binary.BigEndian.Uint64(body)
		c.mu.Lock()
		if c.pending == nil || c.pendingID != 0 || c.receiveID == ^uint64(0) || sequence != c.receiveID+1 {
			c.mu.Unlock()
			return errProtocol
		}
		c.receiveID, c.pendingID = sequence, sequence
		c.mu.Unlock()
		select {
		case c.notify <- struct{}{}:
		default:
		}
		return nil
	case hostcontract.KindFlushed:
		if len(body) != 8 {
			return errProtocol
		}
		sequence := binary.BigEndian.Uint64(body)
		c.mu.Lock()
		if sequence == 0 || sequence != c.waitingID {
			c.mu.Unlock()
			return errProtocol
		}
		c.waitingID = 0
		c.mu.Unlock()
		select {
		case c.flushed <- sequence:
			return nil
		default:
			return errProtocol
		}
	case hostcontract.KindClose:
		if len(body) != 0 {
			return errProtocol
		}
		c.mu.Lock()
		c.remoteClose = true
		c.mu.Unlock()
		c.closeOnce.Do(func() { close(c.closing) })
		return nil
	case hostcontract.KindClosed:
		return io.EOF
	default:
		return errProtocol
	}
}

// flushLoop owns the selected Go listener's original periodic output policy and graceful close.
func (c *Conn) flushLoop() {
	ticker := time.NewTicker(time.Duration(hostcontract.FLUSH_MILLIS) * time.Millisecond)
	defer ticker.Stop()
	for {
		select {
		case <-c.ctx.Done():
			return
		case <-c.closing:
			if c.Flush() == nil {
				_ = c.wire.write(hostcontract.KindClosed, nil)
			}
			c.stop()
			return
		case <-ticker.C:
			if c.Flush() != nil {
				c.stop()
				return
			}
		}
	}
}
