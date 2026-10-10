// bedrock-replay feeds a saved server packet stream through the private local bridge.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// Replay ceilings leave room for framing inside the client's transport limits.
const maxBurstBytes = 8 << 20
const maxBurstPackets = 1024

type options struct {
	capturePath, socketDir, reportPath string
	packPaths                          []string
	burstPackets, burstBytes           int
	interval, timeout, hold            time.Duration
}

type packSummary struct {
	UUID, Version, SHA256 string
}

type runReport struct {
	Protocol       int32          `json:"protocol"`
	Version        string         `json:"version"`
	Capture        captureSummary `json:"capture"`
	Packs          []packSummary  `json:"packs"`
	BurstPackets   int            `json:"burst_packets"`
	BurstBytes     int            `json:"burst_bytes"`
	IntervalMS     float64        `json:"interval_ms"`
	HoldMS         float64        `json:"hold_ms"`
	StartedUnixMS  int64          `json:"started_unix_ms"`
	ReplayUnixMS   int64          `json:"replay_started_unix_ms"`
	FinishedUnixMS int64          `json:"finished_unix_ms"`
	Replay         replayResult   `json:"replay"`
	Complete       bool           `json:"complete"`
	EndReason      string         `json:"end_reason,omitempty"`
	Error          string         `json:"error,omitempty"`
}

// main owns the replay deadline and reports a nonzero exit for incomplete runs.
func main() {
	opts, err := parseOptions(os.Args[1:])
	if errors.Is(err, flag.ErrHelp) {
		return
	}
	if err == nil {
		ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
		defer stop()
		ctx, cancel := context.WithTimeout(ctx, opts.timeout)
		defer cancel()
		err = run(ctx, opts, os.Stdout)
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

// parseOptions accepts local fixture paths only; the command has no upstream dial option.
func parseOptions(args []string) (options, error) {
	var opts options
	flags := flag.NewFlagSet("bedrock-replay", flag.ContinueOnError)
	flags.StringVar(&opts.capturePath, "capture", "", "saved little-endian packet-id/length/body capture")
	flags.StringVar(&opts.socketDir, "socket-dir", "", "private local bridge directory")
	flags.StringVar(&opts.reportPath, "report", "", "write replay identities and burst timing as JSON")
	flags.Func("resource-pack", "local archive to offer during login; repeat in stack order", func(path string) error {
		opts.packPaths = append(opts.packPaths, path)
		return nil
	})
	flags.IntVar(&opts.burstPackets, "burst-packets", 32, "maximum packets in each replay burst")
	flags.IntVar(&opts.burstBytes, "burst-bytes", maxBurstBytes, "maximum serialized packet bytes in each burst")
	flags.DurationVar(&opts.interval, "burst-interval", 50*time.Millisecond, "fixed delay between scheduled bursts; zero sends immediately")
	flags.DurationVar(&opts.timeout, "timeout", 2*time.Minute, "whole-run deadline including login and client shutdown")
	flags.DurationVar(&opts.hold, "hold", 0, "close the fixture this long after delivery; zero waits for client exit")
	if err := flags.Parse(args); err != nil {
		return opts, err
	}
	if flags.NArg() != 0 || opts.capturePath == "" || opts.socketDir == "" || opts.reportPath == "" {
		return opts, errors.New("capture, socket-dir and report are required; positional arguments are not supported")
	}
	if opts.burstPackets < 1 || opts.burstPackets > maxBurstPackets || opts.burstBytes < 1 || opts.burstBytes > maxBurstBytes || opts.interval < 0 || opts.timeout <= 0 || opts.hold < 0 {
		return opts, errors.New("invalid replay bounds")
	}
	return opts, nil
}

// run accepts a session Connect, replays exact saved packets, and waits for the client to exit.
func run(ctx context.Context, opts options, output io.Writer) (result error) {
	file, err := os.Open(opts.capturePath)
	if err != nil {
		return err
	}
	captured, err := readCapture(file)
	closeErr := file.Close()
	if err = errors.Join(err, closeErr); err != nil {
		return err
	}
	bursts, err := planSessionBursts(captured.packets, opts.burstPackets, opts.burstBytes)
	if err != nil {
		return err
	}
	report := runReport{Protocol: minecraft.DefaultProtocol.ID(), Version: minecraft.DefaultProtocol.Ver(),
		Capture: captured.summary, BurstPackets: opts.burstPackets, BurstBytes: opts.burstBytes,
		IntervalMS: float64(opts.interval) / float64(time.Millisecond), HoldMS: float64(opts.hold) / float64(time.Millisecond),
		StartedUnixMS: time.Now().UnixMilli()}
	reportFile, err := os.OpenFile(opts.reportPath, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}
	defer func() {
		report.FinishedUnixMS = time.Now().UnixMilli()
		if result != nil {
			report.Error = result.Error()
		}
		data, err := json.MarshalIndent(report, "", "  ")
		if err == nil {
			_, err = reportFile.Write(append(data, '\n'))
		}
		result = errors.Join(result, err, reportFile.Close())
	}()
	var packs []*resource.Pack
	for _, path := range opts.packPaths {
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		pack, err := resource.ReadBytes(data)
		if err != nil {
			return err
		}
		digest := sha256.Sum256(data)
		report.Packs = append(report.Packs, packSummary{pack.UUID().String(), pack.Version(), hex.EncodeToString(digest[:])})
		packs = append(packs, pack)
	}
	listener, err := streamnet.ListenSession(opts.socketDir)
	if err != nil {
		return err
	}
	defer listener.Close()
	stopClose := context.AfterFunc(ctx, func() { _ = listener.Close() })
	defer stopClose()
	fmt.Fprintln(output, "BEDROCK_REPLAY_READY", opts.socketDir)
	accepted, err := listener.Accept()
	if err != nil {
		return errors.Join(err, ctx.Err())
	}
	conn := streamnet.NewFramedConn(accepted)
	defer conn.Close()
	stopConn := context.AfterFunc(ctx, func() { _ = conn.Close() })
	defer stopConn()
	sink, err := acceptReplaySession(conn, packs)
	if err != nil {
		return errors.Join(err, ctx.Err())
	}
	readDone := make(chan error, 1)
	go func() {
		for {
			if _, err := conn.ReadPacket(); err != nil {
				readDone <- err
				return
			}
		}
	}()
	report.ReplayUnixMS = time.Now().UnixMilli()
	report.Replay, err = replayBursts(ctx, sink, bursts, opts.interval)
	if err != nil {
		return err
	}
	report.Complete = report.Replay.SHA256 == report.Capture.ReplaySHA256 && report.Replay.Packets == report.Capture.ReplayPackets
	if !report.Complete {
		return errors.New("replayed packet witness differs from the capture")
	}
	fmt.Fprintln(output, "BEDROCK_REPLAY_DELIVERED", report.Replay.Packets, report.Replay.SHA256)
	var fixtureEnd <-chan time.Time
	if opts.hold > 0 {
		timer := time.NewTimer(opts.hold)
		defer timer.Stop()
		fixtureEnd = timer.C
	}
	report.EndReason, err = waitReplayEnd(ctx, readDone, fixtureEnd)
	return err
}

// waitReplayEnd distinguishes normal completion from cancellation that also closes the transport.
func waitReplayEnd(ctx context.Context, readDone <-chan error, fixtureEnd <-chan time.Time) (string, error) {
	var reason string
	var err error
	select {
	case <-ctx.Done():
	case <-fixtureEnd:
		reason = "fixture_end"
	case err = <-readDone:
		if errors.Is(err, net.ErrClosed) || errors.Is(err, io.EOF) {
			reason, err = "client_exit", nil
		}
	}
	// The deadline callbacks close the listener and the accepted session. Both cases can
	// therefore be ready together; a random select choice must not turn a timeout into success.
	if ctx.Err() != nil {
		return "", ctx.Err()
	}
	return reason, err
}
