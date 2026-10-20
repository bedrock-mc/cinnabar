package hostbridge

import (
	"encoding/binary"
	"io"
	"time"

	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/internal/hostcontract"
)

// WritePacket serializes admission into the same finite byte/count buffer used by Flush.
func (c *Conn) WritePacket(pk packet.Packet) error {
	c.admission.Lock()
	defer c.admission.Unlock()
	body, err := encodePacket(pk, c.shield.Load())
	if err != nil {
		return err
	}
	weight := min(len(body), hostcontract.QUEUED_BYTES)
	for {
		if c.ctx.Err() != nil {
			return io.EOF
		}
		c.mu.Lock()
		closing := c.remoteClose
		c.mu.Unlock()
		if closing {
			return io.EOF
		}
		c.bufferMu.Lock()
		if c.weight+weight <= hostcontract.QUEUED_BYTES && len(c.buffer) < hostcontract.MAX_PACKET_COUNT {
			c.buffer = append(c.buffer, body)
			c.weight += weight
			c.bufferMu.Unlock()
			return nil
		}
		c.bufferMu.Unlock()
		if err := c.Flush(); err != nil {
			return err
		}
	}
}

// Flush retains original FIFO through actual external-send receipts, without locking the control reader.
// Per-packet IPC receipts are the initial correctness policy, not a throughput qualification.
func (c *Conn) Flush() error {
	c.flushMu.Lock()
	defer c.flushMu.Unlock()
	c.bufferMu.Lock()
	batch := c.buffer
	c.buffer, c.weight = nil, 0
	c.bufferMu.Unlock()
	for _, body := range batch {
		if c.sendID == ^uint64(0) {
			c.stop()
			return errProtocol
		}
		c.sendID++
		c.mu.Lock()
		c.waitingID = c.sendID
		c.mu.Unlock()
		deadline := time.Now().Add(time.Duration(hostcontract.CLOSE_MILLIS) * time.Millisecond)
		if !c.initialized.Load() && c.establishDeadline.Before(deadline) {
			deadline = c.establishDeadline
		}
		if c.wire.writeBefore(hostcontract.KindPacket, body, deadline) != nil {
			c.stop()
			return io.EOF
		}
		var sequence [8]byte
		binary.BigEndian.PutUint64(sequence[:], c.sendID)
		if c.wire.writeBefore(hostcontract.KindFlush, sequence[:], deadline) != nil {
			c.stop()
			return io.EOF
		}
		timer := time.NewTimer(time.Until(deadline))
		select {
		case id := <-c.flushed:
			timer.Stop()
			if id != c.sendID {
				c.stop()
				return errProtocol
			}
		case <-timer.C:
			c.stop()
			return io.EOF
		case <-c.ctx.Done():
			timer.Stop()
			return io.EOF
		}
	}
	return nil
}

// WritePacketImmediate includes older queued output before the final disconnect flush.
func (c *Conn) WritePacketImmediate(packets ...packet.Packet) error {
	for _, pk := range packets {
		if err := c.WritePacket(pk); err != nil {
			return err
		}
	}
	return c.Flush()
}
