// Package latencytrace records relay timing events for offline packet-latency analysis. It is off
// unless bridgecontract.LatencyTraceEnv names a directory; the client writes its half there too.
package latencytrace

import (
	"bufio"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"strconv"
	"sync"
	"sync/atomic"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

// Event kinds and their fields; times are trace-clock nanoseconds. Connections carry the ID the
// handoff sends the client, and frames the per-connection bridge batch-frame number.
const (
	Inbound       = 'I' // bridge frame to the client: conn, frame, upstream batch read and decoded, write began, written; packet IDs
	Outbound      = 'O' // client frame upstream: conn, frame, read, handed to RakNet; packet IDs
	StackLatency  = 'L' // NetworkStackLatency: direction (0 in, 1 out), t, timestamp field
	RoundTrip     = 'R' // t, RakNet RTT in ns
	queueCapacity = 1 << 16
	flushInterval = 200 * time.Millisecond
)

// Trace is one process's open trace file.
type Trace struct {
	anchor     time.Time
	anchorUnix int64
	records    chan record
	dropped    atomic.Uint64
	conns      atomic.Int64
	stop       chan struct{}
	done       chan struct{}
	closeOnce  sync.Once
}

type record struct {
	kind   byte
	values [6]int64
	count  int
	ids    []uint32
}

var (
	activeOnce  sync.Once
	activeTrace atomic.Pointer[Trace]
)

// Active returns the process trace, or nil when tracing is off or finished.
func Active() *Trace {
	activeOnce.Do(func() {
		dir := os.Getenv(bridgecontract.LatencyTraceEnv)
		if dir == "" {
			return
		}
		trace, err := Open(dir, "core")
		if err != nil {
			slog.Warn("latency trace disabled", "error", err)
			return
		}
		activeTrace.Store(trace)
	})
	return activeTrace.Load()
}

// Finish writes and closes the process trace, if one was opened, and keeps tracing off after.
func Finish() {
	activeOnce.Do(func() {})
	if trace := activeTrace.Swap(nil); trace != nil {
		trace.Close()
	}
}

// Open starts a trace file named after role and the process ID in dir.
func Open(dir, role string) (*Trace, error) {
	path := filepath.Join(dir, fmt.Sprintf("%s-%d.trace", role, os.Getpid()))
	file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0o600)
	if err != nil {
		return nil, err
	}
	anchor := time.Now()
	trace := &Trace{
		anchor:     anchor,
		anchorUnix: anchor.UnixNano(),
		records:    make(chan record, queueCapacity),
		stop:       make(chan struct{}),
		done:       make(chan struct{}),
	}
	go trace.write(file, role)
	return trace, nil
}

// Now is the trace clock: Unix nanoseconds at Open plus monotonic time since, so intervals
// within a process are monotonic and both processes share one epoch.
func (trace *Trace) Now() int64 { return trace.anchorUnix + int64(time.Since(trace.anchor)) }

// NextConn returns a bridge connection ID unique across core processes: the process ID above a
// per-process count from 1. The client adopts it from the handoff.
func (trace *Trace) NextConn() int64 { return int64(os.Getpid())<<24 | trace.conns.Add(1) }

// Record queues an event of up to six values without blocking; a full queue drops it and
// counts the drop.
func (trace *Trace) Record(kind byte, values ...int64) {
	trace.RecordIDs(kind, nil, values...)
}

// RecordIDs is Record with a packet ID list, which it copies.
func (trace *Trace) RecordIDs(kind byte, ids []uint32, values ...int64) {
	value := record{kind: kind, count: min(len(values), 6)}
	copy(value.values[:], values)
	if len(ids) != 0 {
		value.ids = append([]uint32(nil), ids...)
	}
	trace.enqueue(value)
}

func (trace *Trace) enqueue(value record) {
	select {
	case trace.records <- value:
	default:
		trace.dropped.Add(1)
	}
}

// Close writes every queued event and closes the file; later calls do nothing.
func (trace *Trace) Close() {
	trace.closeOnce.Do(func() { close(trace.stop) })
	<-trace.done
}

func (trace *Trace) write(file *os.File, role string) {
	defer close(trace.done)
	defer file.Close()
	out := bufio.NewWriterSize(file, 1<<16)
	defer out.Flush()
	fmt.Fprintf(out, "# %s pid=%d anchor_unix_ns=%d\n", role, os.Getpid(), trace.anchorUnix)
	ticker := time.NewTicker(flushInterval)
	defer ticker.Stop()
	var line []byte
	var reported uint64
	for {
		select {
		case value := <-trace.records:
			line = appendRecord(line[:0], value)
			_, _ = out.Write(line)
		case <-ticker.C:
			if dropped := trace.dropped.Load(); dropped != reported {
				reported = dropped
				fmt.Fprintf(out, "X %d %d\n", trace.Now(), dropped)
			}
			_ = out.Flush()
		case <-trace.stop:
			for {
				select {
				case value := <-trace.records:
					line = appendRecord(line[:0], value)
					_, _ = out.Write(line)
				default:
					return
				}
			}
		}
	}
}

// appendRecord formats `kind v0 v1 ...[ id,id...]` and a newline.
func appendRecord(line []byte, value record) []byte {
	line = append(line, value.kind)
	for _, field := range value.values[:value.count] {
		line = append(line, ' ')
		line = strconv.AppendInt(line, field, 10)
	}
	for index, id := range value.ids {
		if index == 0 {
			line = append(line, ' ')
		} else {
			line = append(line, ',')
		}
		line = strconv.AppendUint(line, uint64(id), 10)
	}
	return append(line, '\n')
}
