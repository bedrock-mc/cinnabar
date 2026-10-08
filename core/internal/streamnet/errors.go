package streamnet

import (
	"context"
	"errors"
	"io"
	"net"
	"os"
)

// IsClosed reports ordinary shutdown only when every joined cause is a close.
// Local transport failures are classified at the framing boundary, not on upstream writes.
func IsClosed(err error) bool { return allClosed(err, false) }

// allClosed walks joined causes without hiding a framing or application failure.
func allClosed(err error, localTransport bool) bool {
	if err == nil {
		return false
	}
	if _, ok := err.(*terminalError); ok {
		return true
	}
	if joined, ok := err.(interface{ Unwrap() []error }); ok {
		children := joined.Unwrap()
		if len(children) == 0 {
			return false
		}
		for _, child := range children {
			if !allClosed(child, localTransport) {
				return false
			}
		}
		return true
	}
	if wrapped, ok := err.(interface{ Unwrap() error }); ok {
		if child := wrapped.Unwrap(); child != nil {
			return allClosed(child, localTransport)
		}
	}
	return errors.Is(err, io.EOF) || errors.Is(err, net.ErrClosed) || errors.Is(err, context.Canceled) ||
		(localTransport && (errors.Is(err, io.ErrClosedPipe) || errors.Is(err, os.ErrClosed) || isPlatformTerminalError(err)))
}

// classifyTerminalError preserves local transport causes and marks ordinary shutdown.
func classifyTerminalError(err error) error {
	if allClosed(err, true) {
		return &terminalError{err}
	}
	return err
}

type terminalError struct{ error }

// Unwrap preserves the original transport error for errors.Is and errors.As.
func (err *terminalError) Unwrap() error { return err.error }

// Is adds the standard closed-connection classification to a terminal transport failure.
func (err *terminalError) Is(target error) bool { return target == net.ErrClosed }
