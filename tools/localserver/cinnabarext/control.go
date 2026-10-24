package cinnabarext

import (
	"cmp"
	"maps"
	"slices"
	"time"

	"github.com/google/uuid"
)

type controlResult uint8

const (
	controlRejected controlResult = iota
	controlDeferred
	controlSent
)

// SendControl sends a typed negotiation or acknowledgment reply, retaining it when the
// data budget is exhausted. A newer reply on the same route replaces the pending one.
// Pending replies expire with their connection or world epoch. False means the reply
// was invalid or the recipient was unavailable; ordinary data should use Send instead.
func (s *Server) SendControl(player uuid.UUID, message Message) bool {
	payload, err := message.Encode()
	if err != nil {
		return false
	}
	// Own the record: the caller may reuse mutable fields after returning.
	raw, err := Encode(payload)
	if err != nil || Decode(raw, &payload) != nil {
		return false
	}
	route := message.Route()
	s.mu.Lock()
	c := s.active[player]
	s.mu.Unlock()
	if c == nil {
		return false
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	result := c.sendLocked(route.Bundle, route.Channel, route.Schema, payload)
	if result == controlRejected {
		return false
	}
	delete(c.controls, route)
	if result != controlDeferred {
		return result == controlSent
	}
	if c.controls == nil {
		c.controls = make(map[Route]Record)
	}
	c.controls[route] = payload
	if !c.retryingControls {
		c.retryingControls = true
		go c.retryControls()
	}
	return true
}

// retryControls checks pending replies as rate credit refills and exits after they drain.
func (c *Conn) retryControls() {
	ticker := time.NewTicker(time.Second / MaxMessagesPerSecond)
	defer ticker.Stop()
	for range ticker.C {
		c.mu.Lock()
		done := c.retryControlTick()
		if done {
			c.retryingControls = false
		}
		c.mu.Unlock()
		if done {
			return
		}
	}
}

// retryControlTick retries retained replies under c.mu and reports whether the worker can exit.
func (c *Conn) retryControlTick() bool {
	for _, route := range slices.SortedFunc(maps.Keys(c.controls), compareControlRoutes) {
		if c.sendLocked(route.Bundle, route.Channel, route.Schema, c.controls[route]) != controlDeferred {
			delete(c.controls, route)
		}
	}
	return len(c.controls) == 0
}

// compareControlRoutes makes retries deterministic across bundles and channels.
func compareControlRoutes(a, b Route) int {
	return cmp.Or(cmp.Compare(a.Bundle, b.Bundle), cmp.Compare(a.Channel, b.Channel), cmp.Compare(a.Schema, b.Schema))
}
