package proxy

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/binary"
	"errors"
	"io"
	"net"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// webPackPlan declares the exact compressed bytes of a generated archive.
func webPackPlan(t *testing.T) sessionPlan {
	t.Helper()
	pack := testAdmissionPack(t)
	return sessionPlan{handoff: sessionHandoff{Packs: []sessionPack{{Size: uint64(pack.Size())}}},
		packs: []*resource.Pack{pack}, startup: [][]byte{{0x0b}}, rest: [][]byte{{0x09, 0}}}
}

// capturePackStream records bytes and the single FIN sent by the archive worker.
type capturePackStream struct {
	bytes.Buffer
	closes int
}

// Close records the completion barrier.
func (stream *capturePackStream) Close() error { stream.closes++; return nil }

func TestWebPackStreamCarriesJoinIndexLengthAndDigest(t *testing.T) {
	plan := webPackPlan(t)
	var stream capturePackStream
	delivery := sessionPackDelivery{SessionPackStream: SessionPackStream{Join: 7, Open: func(context.Context) (io.WriteCloser, error) { return &stream, nil }}}
	if err := delivery.prepare(t.Context(), &plan); err != nil {
		t.Fatal(err)
	}
	if err := delivery.write(t.Context(), plan.packs); err != nil {
		t.Fatal(err)
	}
	if stream.closes != 1 {
		t.Fatalf("FIN count = %d", stream.closes)
	}
	var join uint64
	if err := binary.Read(&stream.Buffer, binary.BigEndian, &join); err != nil || join != delivery.Join || plan.handoff.PackStream != join {
		t.Fatal("join association", err)
	}
	var size uint32
	if err := binary.Read(&stream.Buffer, binary.BigEndian, &size); err != nil {
		t.Fatal(err)
	}
	frame := stream.Bytes()
	if int(size) != len(frame) || frame[0] != bridgecontract.SessionKindPackData || binary.BigEndian.Uint32(frame[1:5]) != 0 {
		t.Fatal("incorrect archive frame")
	}
	if sha256.Sum256(frame[5:]) != *plan.handoff.Packs[0].SHA256 || uint64(len(frame)-5) != plan.handoff.Packs[0].Size {
		t.Fatal("archive length or digest mismatch")
	}
}

func TestStalledWebPackOpenDoesNotBlockPlayOrCancellation(t *testing.T) {
	local, peer := net.Pipe()
	defer peer.Close()
	session := newSessionConn(streamnet.NewFramedConn(local))
	defer session.Close()
	frames := readSessionFrames(peer)
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	opened := make(chan struct{})
	delivery := sessionPackDelivery{cancel: cancel, conn: local, SessionPackStream: SessionPackStream{Join: 3, Open: func(ctx context.Context) (io.WriteCloser, error) {
		close(opened)
		<-ctx.Done()
		return nil, ctx.Err()
	}}}
	plan := webPackPlan(t)
	if err := delivery.prepare(ctx, &plan); err != nil {
		t.Fatal(err)
	}
	if err := writeSessionHandoff(session, plan); err != nil {
		t.Fatal(err)
	}
	<-opened
	if (<-frames)[0] != bridgecontract.SessionKindHandoff || (<-frames)[0] != bridgecontract.SessionKindBatch {
		t.Fatal("pack open blocked the play FIFO")
	}
	cancel()
	if err := <-delivery.done; !errors.Is(err, context.Canceled) {
		t.Fatal("archive worker did not cancel", err)
	}
}

func TestWebPackFallbackKeepsSerialPlayDelivery(t *testing.T) {
	plan := webPackPlan(t)
	delivery := sessionPackDelivery{SessionPackStream: SessionPackStream{Join: 1}}
	if err := delivery.prepare(t.Context(), &plan); err != nil {
		t.Fatal(err)
	}
	if plan.handoff.PackStream != 0 || plan.packSink != nil || plan.handoff.Packs[0].SHA256 == nil {
		t.Fatal("fallback did not retain verified inline delivery")
	}
}
