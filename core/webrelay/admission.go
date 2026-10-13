package webrelay

import (
	"net"
	"sync"
)

// connectionAdmission retains one slot until the transport and every handler have ended.
type connectionAdmission struct {
	mu      sync.Mutex
	refs    int
	closed  bool
	release func()
}

// newConnectionAdmission starts with the transport as its first owner.
func newConnectionAdmission(release func()) *connectionAdmission {
	return &connectionAdmission{refs: 1, release: release}
}

// retain counts a handler without charging another connection slot.
func (admission *connectionAdmission) retain() (func(), error) {
	admission.mu.Lock()
	defer admission.mu.Unlock()
	if admission.closed {
		return nil, net.ErrClosed
	}
	admission.refs++
	return sync.OnceFunc(admission.drop), nil
}

// drop releases one owner and returns the admission slot after the last owner finishes.
func (admission *connectionAdmission) drop() {
	admission.mu.Lock()
	admission.refs--
	last := admission.refs == 0
	admission.mu.Unlock()
	if last {
		admission.release()
	}
}

// close prevents new handlers and releases transport ownership once.
func (admission *connectionAdmission) close() {
	admission.mu.Lock()
	if admission.closed {
		admission.mu.Unlock()
		return
	}
	admission.closed = true
	admission.mu.Unlock()
	admission.drop()
}
