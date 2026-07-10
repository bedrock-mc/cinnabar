package main

import (
	"errors"
	"fmt"
	"io"
	"net"
	"os"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
)

func main() {
	if err := run(os.Args[1:]); err != nil {
		_, _ = fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run(args []string) (err error) {
	if len(args) != 1 {
		return errors.New("usage: frame-echo <socket-directory>")
	}

	listener, err := streamnet.New(args[0]).Listen("")
	if err != nil {
		return fmt.Errorf("frame-echo: listen: %w", err)
	}
	defer func() { err = errors.Join(err, listener.Close()) }()

	conn, err := listener.Accept()
	if err != nil {
		return fmt.Errorf("frame-echo: accept: %w", err)
	}
	defer func() { err = errors.Join(err, conn.Close()) }()

	reader, ok := conn.(interface {
		ReadPacket() ([]byte, error)
	})
	if !ok {
		return fmt.Errorf("frame-echo: accepted connection has type %T without ReadPacket", conn)
	}

	for {
		payload, readErr := reader.ReadPacket()
		if readErr != nil {
			if errors.Is(readErr, io.ErrUnexpectedEOF) {
				return fmt.Errorf("frame-echo: truncated frame: %w", readErr)
			}
			if errors.Is(readErr, io.EOF) || errors.Is(readErr, net.ErrClosed) {
				return nil
			}
			return fmt.Errorf("frame-echo: read frame: %w", readErr)
		}

		written, writeErr := conn.Write(payload)
		if writeErr != nil {
			return fmt.Errorf("frame-echo: echo frame: %w", writeErr)
		}
		if written != len(payload) {
			return fmt.Errorf("frame-echo: echo frame: %w", io.ErrShortWrite)
		}
	}
}
