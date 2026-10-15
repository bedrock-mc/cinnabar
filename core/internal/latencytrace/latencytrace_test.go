package latencytrace

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// Close writes every queued record in order, with its fields and packet IDs.
func TestCloseWritesRecordsInOrder(t *testing.T) {
	dir := t.TempDir()
	trace, err := Open(dir, "test")
	if err != nil {
		t.Fatal(err)
	}
	start := trace.Now()
	trace.Record(UDPRead, start, 1200, 0x84, 0, 0)
	trace.RecordIDs(Inbound, 1, 2, start, start+3, start+5, []uint32{111, 40})
	trace.Close()

	data, err := os.ReadFile(filepath.Join(dir, fmt.Sprintf("test-%d.trace", os.Getpid())))
	if err != nil {
		t.Fatal(err)
	}
	lines := strings.Split(strings.TrimSpace(string(data)), "\n")
	want := []string{
		fmt.Sprintf("U %d 1200 132 0 0", start),
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
