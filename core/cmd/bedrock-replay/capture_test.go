package main

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"io"
	"math"
	"reflect"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// captureRecord writes the fixture's original id/length/body envelope.
func captureRecord(id uint32, body []byte) []byte {
	result := make([]byte, 8+len(body))
	binary.LittleEndian.PutUint32(result, id)
	binary.LittleEndian.PutUint32(result[4:], uint32(len(body)))
	copy(result[8:], body)
	return result
}

// TestCaptureReplayPreservesOpaqueBodies exercises unknown packets and repeated post-login metadata.
func TestCaptureReplayPreservesOpaqueBodies(t *testing.T) {
	var data []byte
	for _, item := range []struct {
		id   uint32
		body []byte
	}{
		{packet.IDNetworkSettings, []byte{1}},
		{packet.IDResourcePackStack, []byte{2}},
		{packet.IDSetTime, []byte{3}},
		{packet.IDStartGame, []byte{4}},
		{packet.IDResourcePacksInfo, []byte{5}},
		{0x3fe, []byte{0, 128, 255, 9}},
	} {
		data = append(data, captureRecord(item.id, item.body)...)
	}
	captured, err := readCapture(bytes.NewReader(data))
	if err != nil {
		t.Fatal(err)
	}
	if captured.summary.Records != 6 || captured.summary.HandshakeSkipped != 2 || captured.summary.ReplayPackets != 4 {
		t.Fatalf("wrong capture accounting: %+v", captured.summary)
	}
	if captured.packets[0].id != packet.IDSetTime || captured.packets[2].id != packet.IDResourcePacksInfo {
		t.Fatal("changed the pre-StartGame queue or removed a post-StartGame packet")
	}
	bursts, err := planBursts(captured.packets, 2, 8)
	if err != nil {
		t.Fatal(err)
	}
	sink := new(recordingSink)
	result, err := replayBursts(context.Background(), sink, bursts, 0)
	if err != nil {
		t.Fatal(err)
	}
	// An independent wire oracle must detect changes made while reading the capture too.
	expected := [][]byte{
		{byte(packet.IDSetTime), 3},
		{byte(packet.IDStartGame), 4},
		{byte(packet.IDResourcePacksInfo), 5},
		{0xfe, 0x07, 0, 128, 255, 9},
	}
	if !reflect.DeepEqual(sink.packets, expected) || result.SHA256 != captured.summary.ReplaySHA256 || result.Packets != 4 {
		t.Fatalf("packet content/order witness differs: %+v", result)
	}
	if sink.flushes != len(bursts) || result.Bursts[0].FirstRecord != 2 || result.Bursts[len(bursts)-1].LastRecord != 5 {
		t.Fatal("burst metadata lost the source record boundaries")
	}
}

// TestCaptureRejectsIncompleteOrUnsafeInput checks every truncation and offline transfer rejection.
func TestCaptureRejectsIncompleteOrUnsafeInput(t *testing.T) {
	valid := captureRecord(packet.IDStartGame, []byte{10, 20, 30})
	for cut := 0; cut < len(valid); cut++ {
		if _, err := readCapture(bytes.NewReader(valid[:cut])); err == nil {
			t.Fatalf("accepted truncated capture at byte %d", cut)
		}
	}
	for _, suffix := range [][]byte{
		{1}, captureRecord(packet.IDTransfer, nil), captureRecord(packet.IDStartGame, nil), captureRecord(0x400, nil),
	} {
		if _, err := readCapture(bytes.NewReader(append(bytes.Clone(valid), suffix...))); err == nil {
			t.Fatal("accepted invalid capture suffix")
		}
	}
}

type recordingSink struct {
	packets [][]byte
	flushes int
	short   bool
	fail    bool
}

// Write retains an independent copy so later reuse would be visible to the comparison.
func (sink *recordingSink) Write(wire []byte) (int, error) {
	if sink.short {
		return len(wire) - 1, nil
	}
	sink.packets = append(sink.packets, bytes.Clone(wire))
	return len(wire), nil
}

// Flush can model a failed transport without claiming queued packets were delivered.
func (sink *recordingSink) Flush() error {
	sink.flushes++
	if sink.fail {
		return io.ErrClosedPipe
	}
	return nil
}

// TestReplayBoundsAndCancellation covers transport failures and prompt cancellation before a burst.
func TestReplayBoundsAndCancellation(t *testing.T) {
	packets := []capturedPacket{{record: 3, wire: []byte{1, 2}}, {record: 4, wire: []byte{3, 4}}}
	for _, bounds := range [][2]int{{0, 10}, {1, 0}, {1, 1}} {
		if _, err := planBursts(packets, bounds[0], bounds[1]); err == nil {
			t.Fatal("accepted invalid burst bounds")
		}
	}
	bursts, err := planBursts(packets, 2, 3)
	if err != nil || len(bursts) != 2 {
		t.Fatalf("byte limit did not split bursts: %v", err)
	}
	if _, err := replayBursts(context.Background(), new(recordingSink), append(bursts, bursts...), time.Duration(math.MaxInt64)); err == nil {
		t.Fatal("accepted an overflowing burst schedule")
	}
	for _, sink := range []*recordingSink{{short: true}, {fail: true}} {
		result, err := replayBursts(context.Background(), sink, bursts, 0)
		if err == nil || result.Packets != 0 || result.SHA256 != "" {
			t.Fatalf("failed transport reported completion: %+v, %v", result, err)
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	sink := new(recordingSink)
	if _, err := replayBursts(ctx, sink, bursts, time.Hour); !errors.Is(err, context.Canceled) || len(sink.packets) != 0 {
		t.Fatalf("cancelled replay sent packets: %v", err)
	}
}
