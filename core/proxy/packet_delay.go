package proxy

import (
	"errors"
	"sync"
	"time"
)

const (
	MaxPacketDelayMS  uint32 = 1000
	PacketDelayLease         = 3 * time.Second
	maxDelayedBatches        = 64
	maxDelayedBytes          = 8 << 20
)

// PacketDelay owns a short lease for application-packet latency in both relay directions.
// Its zero value forwards immediately. Transport acknowledgements and login are unaffected.
type PacketDelay struct {
	mu            sync.Mutex
	delay         time.Duration
	expires       time.Time
	changed       chan struct{}
	release       uint64
	session       uint64
	showPosition  bool
	position      *ForwardedPosition
	positionEpoch uint64
}

func (delay *PacketDelay) Set(delayMS uint32) error {
	return delay.SetWithPosition(delayMS, false)
}

func (delay *PacketDelay) SetWithPosition(delayMS uint32, showPosition bool) error {
	if delayMS > MaxPacketDelayMS {
		return errors.New("packet delay exceeds limit")
	}
	delay.mu.Lock()
	defer delay.mu.Unlock()
	duration := time.Duration(delayMS) * time.Millisecond
	if duration == 0 || !showPosition || delay.showPosition != showPosition || !time.Now().Before(delay.expires) {
		delay.clearPositionLocked()
	}
	delay.showPosition = showPosition && duration != 0
	if duration == 0 && delay.delay != 0 {
		delay.release++
	}
	// Heartbeats refresh the lease without waking both packet schedulers every time.
	if delay.delay != duration {
		delay.notifyLocked()
	}
	delay.delay = duration
	delay.expires = time.Now().Add(PacketDelayLease)
	return nil
}

func (delay *PacketDelay) Reset() {
	if delay == nil {
		return
	}
	delay.mu.Lock()
	defer delay.mu.Unlock()
	delay.resetLocked()
}

func (delay *PacketDelay) beginSession() uint64 {
	if delay == nil {
		return 0
	}
	delay.mu.Lock()
	defer delay.mu.Unlock()
	delay.session++
	delay.resetLocked()
	return delay.session
}

// A retiring relay must not clear a newer session's refreshed lease.
func (delay *PacketDelay) endSession(session uint64) {
	if delay == nil {
		return
	}
	delay.mu.Lock()
	defer delay.mu.Unlock()
	if delay.session == session {
		delay.resetLocked()
	}
}

func (delay *PacketDelay) resetLocked() {
	delay.showPosition = false
	delay.clearPositionLocked()
	delay.delay = 0
	delay.release++
	delay.expires = time.Time{}
	delay.notifyLocked()
}

func (delay *PacketDelay) notifyLocked() {
	if delay.changed != nil {
		close(delay.changed)
	}
	delay.changed = make(chan struct{})
}

func (delay *PacketDelay) snapshot(now time.Time) (time.Duration, time.Time, <-chan struct{}, uint64) {
	if delay == nil {
		return 0, time.Time{}, nil, 0
	}
	delay.mu.Lock()
	defer delay.mu.Unlock()
	if delay.changed == nil {
		delay.changed = make(chan struct{})
	}
	if delay.delay != 0 && !now.Before(delay.expires) {
		delay.showPosition = false
		delay.clearPositionLocked()
		delay.delay = 0
		delay.release++
		delay.notifyLocked()
	}
	return delay.delay, delay.expires, delay.changed, delay.release
}

type delayedBatch struct {
	result batchReadResult
	due    time.Time
	bytes  int
}

// A fixed FIFO bounds retained batches; one oversized input batch may exceed the byte cap.
type delayedBatches struct {
	items              [maxDelayedBatches]delayedBatch
	head, count, bytes int
}

func (queue *delayedBatches) push(result batchReadResult, due time.Time) {
	value := delayedBatch{result: result, due: due}
	for _, packet := range result.packets {
		value.bytes += len(packet.Data)
	}
	queue.items[(queue.head+queue.count)%len(queue.items)] = value
	queue.count++
	queue.bytes += value.bytes
}

func (queue *delayedBatches) front() *delayedBatch { return &queue.items[queue.head] }

func (queue *delayedBatches) pop() batchReadResult {
	value := queue.items[queue.head]
	queue.items[queue.head] = delayedBatch{}
	queue.head = (queue.head + 1) % len(queue.items)
	queue.count--
	queue.bytes -= value.bytes
	return value.result
}

func (queue *delayedBatches) full() bool {
	return queue.count == len(queue.items) || queue.bytes >= maxDelayedBytes
}
