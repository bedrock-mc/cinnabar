package cinnabarext

import (
	"errors"

	"github.com/google/uuid"
)

// Route identifies a declared channel in a client bundle. Generated bindings own these values.
type Route struct {
	Bundle  string
	Channel string
	Schema  uint16
}

// Message is the small contract generated message types implement.
type Message interface {
	Route() Route
	Encode() (Record, error)
}

// MessageType decodes a generated message on its declared route.
type MessageType[T any] struct {
	Route  Route
	Decode func(Record) (T, error)
}

// Send validates and sends a typed message to an authenticated player UUID.
func (s *Server) Send(player uuid.UUID, message Message) bool {
	payload, err := message.Encode()
	if err != nil {
		return false
	}
	route := message.Route()
	return s.SendRecord(player, route.Bundle, route.Channel, route.Schema, payload)
}

// Broadcast sends to the supplied audience and returns the number admitted locally.
func (s *Server) Broadcast(players []uuid.UUID, message Message) int {
	payload, err := message.Encode()
	if err != nil {
		return 0
	}
	route := message.Route()
	sent := 0
	for _, player := range players {
		if s.SendRecord(player, route.Bundle, route.Channel, route.Schema, payload) {
			sent++
		}
	}
	return sent
}

// OnMessage registers a typed receiver before listening. Go requires this generic function
// at package scope. The callback runs on the packet reader and must not block; schedule
// world access through Dragonfly transactions rather than retaining a player pointer.
func OnMessage[T any](s *Server, kind MessageType[T], receive func(uuid.UUID, T)) error {
	if kind.Decode == nil || receive == nil {
		return errors.New("message decoder and receiver are required")
	}
	bundle := s.bundles[kind.Route.Bundle]
	declared := false
	if bundle != nil {
		for _, channel := range bundle.Manifest.Channels {
			if channel.ID == kind.Route.Channel && channel.Schema == kind.Route.Schema && channel.Direction == ToServer {
				declared = true
				break
			}
		}
	}
	if !declared {
		return errors.New("receiver requires a declared to_server channel")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.handlers[kind.Route] != nil {
		return errors.New("receiver already registered")
	}
	s.handlers[kind.Route] = func(player uuid.UUID, record Record) {
		message, err := kind.Decode(record)
		if err == nil {
			receive(player, message)
		}
	}
	return nil
}
