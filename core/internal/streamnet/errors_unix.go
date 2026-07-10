//go:build !windows

package streamnet

import (
	"errors"
	"syscall"
)

func isPlatformTerminalError(err error) bool {
	return errors.Is(err, syscall.EPIPE) ||
		errors.Is(err, syscall.ECONNRESET) ||
		errors.Is(err, syscall.ECONNABORTED) ||
		errors.Is(err, syscall.ENOTCONN)
}
