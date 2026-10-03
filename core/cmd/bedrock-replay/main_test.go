package main

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

type readyOutput struct {
	ready chan struct{}
	once  sync.Once
}

// Write signals listener readiness without blocking later replay progress messages.
func (output *readyOutput) Write(data []byte) (int, error) {
	if strings.Contains(string(data), "BEDROCK_REPLAY_READY") {
		output.once.Do(func() { close(output.ready) })
	}
	return len(data), nil
}

// TestLocalReplayRoundTrip proves the real encrypted local bridge preserves the captured packet bytes.
func TestLocalReplayRoundTrip(t *testing.T) {
	for _, end := range []string{"client_exit", "fixture_end"} {
		for _, withPack := range []bool{false, true} {
			t.Run(fmt.Sprintf("%s/pack=%t", end, withPack), func(t *testing.T) { testLocalReplayRoundTrip(t, end, withPack) })
		}
	}
}

// testLocalReplayRoundTrip checks packet and pack bytes with either side ending the session.
func testLocalReplayRoundTrip(t *testing.T, end string, withPack bool) {
	dir := t.TempDir()
	opts := options{capturePath: filepath.Join(dir, "capture.bin"), socketDir: filepath.Join(dir, "bridge"),
		reportPath: filepath.Join(dir, "report.json"), burstPackets: 2, burstBytes: 1 << 20, interval: time.Millisecond}
	if end == "fixture_end" {
		opts.hold = 100 * time.Millisecond
	}
	packPath := filepath.Join(dir, "test.mcpack")
	packBytes := fixtureResourcePack(t)
	if err := os.WriteFile(packPath, packBytes, 0o600); err != nil {
		t.Fatal(err)
	}
	if withPack {
		opts.packPaths = []string{packPath}
	}
	data := fixtureCapture(t)
	if err := os.WriteFile(opts.capturePath, data, 0o600); err != nil {
		t.Fatal(err)
	}
	expected, err := readCapture(bytes.NewReader(data))
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	output := &readyOutput{ready: make(chan struct{})}
	done := make(chan error, 1)
	go func() { done <- run(ctx, opts, output) }()
	joined := false
	defer func() {
		cancel()
		if !joined {
			<-done
		}
	}()
	select {
	case <-output.ready:
	case err := <-done:
		joined = true
		t.Fatalf("listener failed: %v", err)
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	conn, err := (minecraft.Dialer{
		IdentityData: login.IdentityData{DisplayName: "ReplayTest"},
		RelayStartup: true, ErrorLog: slog.New(slog.DiscardHandler),
	}).DialContextNetwork(ctx, streamnet.New(opts.socketDir), "")
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	packs := conn.ResourcePacks()
	packDigest := sha256.Sum256(packBytes)
	if withPack && (len(packs) != 1 || packs[0].Checksum() != packDigest) {
		t.Fatal("resource-pack download changed the offered archive")
	}
	if !withPack {
		// Gophertunnel's empty-pack fast path retains the generated stack as a deferred login packet.
		stack, err := conn.ReadBytes()
		if err != nil {
			t.Fatal(err)
		}
		var stackHeader packet.Header
		if err := stackHeader.Read(bytes.NewReader(stack)); err != nil || stackHeader.PacketID != packet.IDResourcePackStack {
			t.Fatalf("missing regenerated local resource-pack stack: id=%d err=%v", stackHeader.PacketID, err)
		}
	}
	for _, captured := range expected.packets {
		actual, err := conn.ReadBytes()
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Equal(actual, captured.wire) {
			var header packet.Header
			_ = header.Read(bytes.NewReader(actual))
			t.Fatalf("record %d changed across the local bridge: received id=%d bytes=%d, expected id=%d bytes=%d", captured.record, header.PacketID, len(actual), captured.id, len(captured.wire))
		}
	}
	if end == "client_exit" {
		if err := conn.Close(); err != nil {
			t.Fatal(err)
		}
	}
	select {
	case err := <-done:
		joined = true
		if err != nil {
			t.Fatal(err)
		}
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	var report runReport
	encoded, err := os.ReadFile(opts.reportPath)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(encoded, &report); err != nil {
		t.Fatal(err)
	}
	if !report.Complete || report.Error != "" || report.EndReason != end || report.Replay.SHA256 != expected.summary.ReplaySHA256 || len(report.Replay.Bursts) != 3 {
		t.Fatalf("invalid replay report: %+v", report)
	}
	if withPack && (len(report.Packs) != 1 || report.Packs[0].SHA256 != hex.EncodeToString(packDigest[:])) {
		t.Fatalf("pack identity missing from report: %+v", report.Packs)
	}
}

// TestReplayOptionsRejectInvalidRuns keeps malformed or upstream-like invocations from starting a listener.
func TestReplayOptionsRejectInvalidRuns(t *testing.T) {
	base := []string{"-capture", "fixture.bin", "-socket-dir", "local", "-report", "result.json"}
	for _, extra := range [][]string{{"-upstream", "example.test:19132"}, {"-burst-packets", "0"},
		{"-burst-interval", "-1s"}, {"-timeout", "0s"}, {"-hold", "-1s"}, {"unexpected"}} {
		if _, err := parseOptions(append(append([]string{}, base...), extra...)); err == nil {
			t.Fatalf("accepted invalid options %v", extra)
		}
	}
	if _, err := parseOptions(base); err != nil {
		t.Fatal(err)
	}
}

// fixtureResourcePack makes an original, minimal archive for a real local pack download.
func fixtureResourcePack(t *testing.T) []byte {
	t.Helper()
	var data bytes.Buffer
	archive := zip.NewWriter(&data)
	manifest, err := archive.Create("manifest.json")
	if err != nil {
		t.Fatal(err)
	}
	_, err = io.WriteString(manifest, `{"format_version":2,"header":{"name":"Replay test","description":"Synthetic fixture","uuid":"87ad314d-1b7e-4a06-8a7d-882798c777cf","version":[1,0,0]},"modules":[{"type":"resources","uuid":"86fad15a-f775-4d45-bd27-bd2e420a3325","version":[1,0,0]}]}`)
	if err != nil {
		t.Fatal(err)
	}
	if err := archive.Close(); err != nil {
		t.Fatal(err)
	}
	return data.Bytes()
}

// TestReplayDoesNotOverwriteEvidence rejects a reused report path before opening a listener.
func TestReplayDoesNotOverwriteEvidence(t *testing.T) {
	dir := t.TempDir()
	opts := options{capturePath: filepath.Join(dir, "capture.bin"), socketDir: filepath.Join(dir, "bridge"),
		reportPath: filepath.Join(dir, "report.json"), burstPackets: 2, burstBytes: maxBurstBytes}
	if err := os.WriteFile(opts.capturePath, fixtureCapture(t), 0o600); err != nil {
		t.Fatal(err)
	}
	const original = "previous measurement"
	if err := os.WriteFile(opts.reportPath, []byte(original), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := run(context.Background(), opts, io.Discard); !os.IsExist(err) {
		t.Fatalf("expected existing report rejection: %v", err)
	}
	data, err := os.ReadFile(opts.reportPath)
	if err != nil || string(data) != original {
		t.Fatalf("previous report changed: %v", err)
	}
}

// TestReplayDeadlineReportsIncomplete verifies that waiting for a client is bounded and recorded.
func TestReplayDeadlineReportsIncomplete(t *testing.T) {
	dir := t.TempDir()
	opts := options{capturePath: filepath.Join(dir, "capture.bin"), socketDir: filepath.Join(dir, "bridge"),
		reportPath: filepath.Join(dir, "report.json"), burstPackets: 2, burstBytes: maxBurstBytes}
	if err := os.WriteFile(opts.capturePath, fixtureCapture(t), 0o600); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	if err := run(ctx, opts, io.Discard); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("expected a bounded login timeout: %v", err)
	}
	data, err := os.ReadFile(opts.reportPath)
	if err != nil {
		t.Fatal(err)
	}
	var report runReport
	if err := json.Unmarshal(data, &report); err != nil {
		t.Fatal(err)
	}
	if report.Complete || report.Error == "" || report.Replay.Packets != 0 {
		t.Fatalf("timeout reported a completed replay: %+v", report)
	}
}

// TestExportReplayFixture optionally writes committed startup, actor and text fixtures for native harness diagnostics.
func TestExportReplayFixture(t *testing.T) {
	path := os.Getenv("CINNABAR_REPLAY_FIXTURE_OUT")
	if path == "" {
		t.Skip("set CINNABAR_REPLAY_FIXTURE_OUT to export a repository-based startup fixture")
	}
	file, err := os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	if _, err := file.Write(fixtureCapture(t)); err != nil {
		t.Fatal(err)
	}
}

// TestReplayEndPrioritizesCancellation prevents deadline-triggered EOF and timer ties reporting success.
func TestReplayEndPrioritizesCancellation(t *testing.T) {
	for _, fixtureEnd := range []bool{false, true} {
		for range 64 {
			ctx, cancel := context.WithCancel(context.Background())
			readDone := make(chan error, 1)
			holdDone := make(chan time.Time, 1)
			if fixtureEnd {
				holdDone <- time.Now()
			} else {
				readDone <- io.EOF
			}
			cancel()
			reason, err := waitReplayEnd(ctx, readDone, holdDone)
			if !errors.Is(err, context.Canceled) || reason != "" {
				t.Fatalf("cancellation reported successful end %q: %v", reason, err)
			}
		}
	}
}

// TestReplayEndAcceptsUninterruptedCompletion keeps genuine client exit and planned fixture EOF successful.
func TestReplayEndAcceptsUninterruptedCompletion(t *testing.T) {
	readDone := make(chan error, 1)
	readDone <- io.EOF
	if reason, err := waitReplayEnd(context.Background(), readDone, nil); err != nil || reason != "client_exit" {
		t.Fatalf("client exit: %q, %v", reason, err)
	}
	fixtureEnd := make(chan time.Time, 1)
	fixtureEnd <- time.Now()
	if reason, err := waitReplayEnd(context.Background(), nil, fixtureEnd); err != nil || reason != "fixture_end" {
		t.Fatalf("fixture end: %q, %v", reason, err)
	}
}
