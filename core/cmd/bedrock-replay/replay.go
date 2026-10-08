package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"math"
	"time"
)

type packetSink interface {
	Write([]byte) (int, error)
	Flush() error
}

type burst struct {
	packets []capturedPacket
	bytes   int
}

type burstSample struct {
	FirstRecord int     `json:"first_record"`
	LastRecord  int     `json:"last_record"`
	Packets     int     `json:"packets"`
	Bytes       int     `json:"bytes"`
	DueMS       float64 `json:"due_ms"`
	FlushedMS   float64 `json:"flushed_ms"`
}

type replayResult struct {
	Packets int           `json:"packets"`
	SHA256  string        `json:"sha256"`
	Bursts  []burstSample `json:"bursts"`
}

// planBursts fixes packet boundaries before timing and never splits a captured packet.
func planBursts(packets []capturedPacket, countLimit, byteLimit int) ([]burst, error) {
	if countLimit < 1 || byteLimit < 1 {
		return nil, errors.New("burst packet and byte limits must be positive")
	}
	var result []burst
	for _, captured := range packets {
		if len(captured.wire) > byteLimit {
			return nil, fmt.Errorf("record %d exceeds the burst byte limit", captured.record)
		}
		if len(result) == 0 || len(result[len(result)-1].packets) == countLimit ||
			result[len(result)-1].bytes+len(captured.wire) > byteLimit {
			result = append(result, burst{})
		}
		last := &result[len(result)-1]
		last.packets = append(last.packets, captured)
		last.bytes += len(captured.wire)
	}
	return result, nil
}

// replayBursts preserves bytes and order while recording transport backpressure against the schedule.
func replayBursts(ctx context.Context, sink packetSink, bursts []burst, interval time.Duration) (replayResult, error) {
	result := replayResult{Bursts: make([]burstSample, 0, len(bursts))}
	if interval < 0 || (len(bursts) > 1 && interval > time.Duration(math.MaxInt64/int64(len(bursts)-1))) {
		return result, errors.New("burst schedule exceeds the duration range")
	}
	digest := sha256.New()
	started := time.Now()
	for index, burst := range bursts {
		due := time.Duration(index) * interval
		if err := waitUntil(ctx, started.Add(due)); err != nil {
			return result, err
		}
		for _, captured := range burst.packets {
			written, err := sink.Write(captured.wire)
			if err != nil {
				return result, err
			}
			if written != len(captured.wire) {
				return result, io.ErrShortWrite
			}
		}
		if err := sink.Flush(); err != nil {
			return result, err
		}
		for _, captured := range burst.packets {
			hashPacket(digest, captured.wire)
		}
		result.Packets += len(burst.packets)
		result.Bursts = append(result.Bursts, burstSample{
			FirstRecord: burst.packets[0].record, LastRecord: burst.packets[len(burst.packets)-1].record,
			Packets: len(burst.packets), Bytes: burst.bytes,
			DueMS:     float64(due) / float64(time.Millisecond),
			FlushedMS: float64(time.Since(started)) / float64(time.Millisecond),
		})
	}
	result.SHA256 = hex.EncodeToString(digest.Sum(nil))
	return result, nil
}

// waitUntil responds to cancellation even when the next burst is far in the future.
func waitUntil(ctx context.Context, deadline time.Time) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	timer := time.NewTimer(time.Until(deadline))
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-timer.C:
		return nil
	}
}
