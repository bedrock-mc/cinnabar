package proxy

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"io"
	"net"
	"time"

	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// SessionPackStream selects web archive delivery. Open must return a writer that resets on
// ctx cancellation. A nil Open keeps serial delivery on the play connection.
type SessionPackStream struct {
	Join uint64
	Open func(context.Context) (io.WriteCloser, error)
}

// sessionPackDelivery owns the single archive worker for one join.
type sessionPackDelivery struct {
	SessionPackStream
	cancel context.CancelFunc
	conn   net.Conn
	done   chan error
}

// ServeConnWithPacks verifies web archive metadata and optionally sends archives independently
// of play. It joins the archive worker before returning; native ServeConn stays unchanged.
func (handler *SessionHandler) ServeConnWithPacks(ctx context.Context, conn net.Conn, stream SessionPackStream) error {
	if stream.Join == 0 {
		return errors.New("proxy: pack delivery requires a join identity")
	}
	ctx, cancel := context.WithCancel(ctx)
	delivery := &sessionPackDelivery{SessionPackStream: stream, cancel: cancel, conn: conn}
	err := handler.server.serveConnWithPacks(ctx, conn, delivery)
	cancel()
	if delivery.done != nil {
		err = errors.Join(err, <-delivery.done)
	}
	return err
}

// prepare admits the selection and declares the digest of its acquired compressed bytes.
func (delivery *sessionPackDelivery) prepare(ctx context.Context, plan *sessionPlan) error {
	if len(plan.packs) != len(plan.handoff.Packs) {
		return errors.New("proxy: invalid pack selection")
	}
	for index, pack := range plan.packs {
		declared := &plan.handoff.Packs[index]
		if pack == nil || declared.Cache != nil || uint64(pack.Size()) != declared.Size {
			return errors.New("proxy: invalid web pack archive")
		}
		digest := pack.Checksum()
		declared.SHA256 = &digest
		if err := ctx.Err(); err != nil {
			return err
		}
	}
	if delivery.Open == nil {
		return nil
	}
	plan.handoff.PackStream = delivery.Join
	plan.packSink = func(packs []*resource.Pack) error {
		delivery.done = make(chan error, 1)
		go func() {
			err := delivery.write(ctx, packs)
			if err != nil {
				delivery.cancel()
				_ = delivery.conn.SetDeadline(time.Now())
			}
			delivery.done <- err
		}()
		return nil
	}
	return nil
}

// write sends one join prefix, the indexed archive chunks, and FIN as the completion barrier.
func (delivery *sessionPackDelivery) write(ctx context.Context, packs []*resource.Pack) (err error) {
	writer, err := delivery.Open(ctx)
	if err != nil {
		return err
	}
	defer func() { err = errors.Join(err, writer.Close()) }()
	if _, err := io.Copy(writer, bytes.NewReader(binary.BigEndian.AppendUint64(nil, delivery.Join))); err != nil {
		return err
	}
	if err := writeSessionPacks(func(frame []byte) error {
		var header [4]byte
		binary.BigEndian.PutUint32(header[:], uint32(len(frame)))
		if _, err := io.Copy(writer, bytes.NewReader(header[:])); err != nil {
			return err
		}
		_, err := io.Copy(writer, bytes.NewReader(frame))
		return err
	}, packs); err != nil {
		return err
	}
	return nil
}
