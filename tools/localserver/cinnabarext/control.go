package cinnabarext

import (
	"cmp"
	"errors"
	"maps"
	"slices"
	"time"

	"github.com/google/uuid"
)

// SendControl sends a typed negotiation or acknowledgment reply, retaining it when the
// data budget is exhausted. A newer reply on the same route replaces the pending one.
// Pending replies expire with their connection or world epoch. A BurstExceeded error
// means the encoded reply can never fit; ordinary data should use Send instead.
func (s *Server) SendControl(player uuid.UUID, message Message) error {
	payload, err := message.Encode()
	if err != nil {
		return err
	}
	// Own the record: the caller may reuse mutable fields after returning.
	raw, err := Encode(payload)
	if err != nil {
		return err
	}
	if err := Decode(raw, &payload); err != nil {
		return err
	}
	route := message.Route()
	s.mu.Lock()
	c := s.active[player]
	s.mu.Unlock()
	if c == nil {
		return errors.New("control recipient unavailable")
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	err = c.sendLocked(route.Bundle, route.Channel, route.Schema, payload)
	if err != nil && !errors.Is(err, ErrRateExceeded) {
		var burst *BurstExceeded
		if errors.As(err, &burst) {
			c.rejectedControls++
			s.log.Warn("client part control rejected", "bundle", route.Bundle, "channel", route.Channel,
				"reason", err.Error(), "rejected_controls", c.rejectedControls)
		}
		return err
	}
	delete(c.controls, route)
	if err == nil {
		return nil
	}
	if c.controls == nil {
		c.controls = make(map[Route]Record)
	}
	c.controls[route] = payload
	if !c.retryingControls {
		c.retryingControls = true
		go c.retryControls()
	}
	return nil
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
		if err := c.sendLocked(route.Bundle, route.Channel, route.Schema, c.controls[route]); !errors.Is(err, ErrRateExceeded) {
			delete(c.controls, route)
		}
	}
	return len(c.controls) == 0
}

// compareControlRoutes makes retries deterministic across bundles and channels.
func compareControlRoutes(a, b Route) int {
	return cmp.Or(cmp.Compare(a.Bundle, b.Bundle), cmp.Compare(a.Channel, b.Channel), cmp.Compare(a.Schema, b.Schema))
}
