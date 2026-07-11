package main

import (
	"context"
	"io"
	"testing"
	"time"
)

func TestStdinEOFCancelsCoreContext(t *testing.T) {
	reader, writer := io.Pipe()
	ctx, stop := contextWithStdinEOF(context.Background(), reader)
	defer stop()

	if err := writer.Close(); err != nil {
		t.Fatalf("close core stdin: %v", err)
	}
	select {
	case <-ctx.Done():
	case <-time.After(time.Second):
		t.Fatal("stdin EOF did not cancel the core context")
	}
}

func TestParentCancellationStillStopsCoreContext(t *testing.T) {
	reader, writer := io.Pipe()
	parent, cancelParent := context.WithCancel(context.Background())
	ctx, stop := contextWithStdinEOF(parent, reader)
	defer stop()
	defer writer.Close()

	cancelParent()
	select {
	case <-ctx.Done():
	case <-time.After(time.Second):
		t.Fatal("parent cancellation did not cancel the core context")
	}
}
