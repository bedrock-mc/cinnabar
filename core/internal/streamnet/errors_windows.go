//go:build windows

package streamnet

import (
	"errors"
	"syscall"
)

const (
	wsaNotConnected syscall.Errno = 10057
	wsaShutdown     syscall.Errno = 10058
)

func isPlatformTerminalError(err error) bool {
	for _, terminal := range []error{
		syscall.WSAECONNABORTED,
		syscall.WSAECONNRESET,
		wsaNotConnected,
		wsaShutdown,
		syscall.ERROR_BROKEN_PIPE,
		syscall.ERROR_NETNAME_DELETED,
		syscall.ERROR_OPERATION_ABORTED,
	} {
		if errors.Is(err, terminal) {
			return true
		}
	}
	return false
}
