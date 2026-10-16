package latencytrace

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

// Close writes every queued record in order, with its fields and packet IDs.
func TestCloseWritesRecordsInOrder(t *testing.T) {
	dir := t.TempDir()
	trace, err := Open(dir, "test")
	if err != nil {
		t.Fatal(err)
	}
	start := trace.Now()
	trace.Record(RoundTrip, start, 1200)
	trace.RecordIDs(Inbound, []uint32{111, 40}, 1, 2, start, start+3, start+5)
	trace.Close()

	data, err := os.ReadFile(filepath.Join(dir, fmt.Sprintf("test-%d.trace", os.Getpid())))
	if err != nil {
		t.Fatal(err)
	}
	lines := strings.Split(strings.TrimSpace(string(data)), "\n")
	want := []string{
		fmt.Sprintf("R %d 1200", start),
		fmt.Sprintf("I 1 2 %d %d %d 111,40", start, start+3, start+5),
	}
	if len(lines) != 3 || !strings.HasPrefix(lines[0], "# test pid=") || lines[1] != want[0] || lines[2] != want[1] {
		t.Fatalf("trace = %q, want header then %q", lines, want)
	}
}

// The clock never runs backwards and starts at the wall-clock anchor.
func TestNowIsMonotonicFromTheAnchor(t *testing.T) {
	trace, err := Open(t.TempDir(), "clock")
	if err != nil {
		t.Fatal(err)
	}
	defer trace.Close()
	first := trace.Now()
	if first < trace.anchorUnix || trace.Now() < first {
		t.Fatalf("clock %d before anchor %d or running backwards", first, trace.anchorUnix)
	}
}

// Finish drains the process trace opened from the environment and leaves tracing off.
func TestFinishWritesTheProcessTrace(t *testing.T) {
	dir := t.TempDir()
	t.Setenv(bridgecontract.LatencyTraceEnv, dir)
	trace := Active()
	if trace == nil {
		t.Fatal("the environment names a trace directory")
	}
	trace.Record(RoundTrip, 1, 2)
	Finish()
	Finish()
	if Active() != nil {
		t.Fatal("a finished trace stays off")
	}
	data, err := os.ReadFile(filepath.Join(dir, fmt.Sprintf("core-%d.trace", os.Getpid())))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasSuffix(string(data), "R 1 2\n") {
		t.Fatalf("trace = %q, want the queued record", data)
	}
}

// Connection IDs stay distinct across core processes, so a reconnect's new core cannot reuse one.
func TestNextConnCarriesTheProcess(t *testing.T) {
	trace, err := Open(t.TempDir(), "conn")
	if err != nil {
		t.Fatal(err)
	}
	defer trace.Close()
	first, second := trace.NextConn(), trace.NextConn()
	if first>>24 != int64(os.Getpid()) || second != first+1 {
		t.Fatalf("ids %d, %d do not carry pid %d and a count", first, second, os.Getpid())
	}
}
