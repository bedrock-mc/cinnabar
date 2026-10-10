package main

import (
	"encoding/json"
	"errors"
	"fmt"

	"github.com/hashimthearab/rust-mcbe/core/internal/sessionwire"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// planSessionBursts keeps startup through StartGame in one handoff; play uses the requested bounds.
func planSessionBursts(packets []capturedPacket, countLimit, byteLimit int) ([]burst, error) {
	for index, captured := range packets {
		if captured.id != packet.IDStartGame {
			continue
		}
		startup := burst{packets: packets[:index+1]}
		for _, p := range startup.packets {
			startup.bytes += len(p.wire)
		}
		rest, err := planBursts(packets[index+1:], countLimit, byteLimit)
		return append([]burst{startup}, rest...), err
	}
	return nil, errors.New("replay has no StartGame")
}

// replaySession writes the first burst as a handoff, then writes raw session batches.
type replaySession struct {
	conn    *streamnet.FramedConn
	handoff *sessionwire.Handoff
	packs   []*resource.Pack
	pending [][]byte
}

// acceptReplaySession validates the client's Connect before allowing captured packets to be sent.
func acceptReplaySession(conn *streamnet.FramedConn, packs []*resource.Pack) (*replaySession, error) {
	frame, err := conn.ReadPacket()
	if err != nil {
		return nil, err
	}
	request, err := sessionwire.DecodeConnect(frame)
	if err != nil {
		return nil, err
	}
	var client login.ClientData
	if err := json.Unmarshal(request.ClientData, &client); err != nil {
		return nil, err
	}
	if request.Protocol != minecraft.DefaultProtocol.ID() || client.GameVersion != minecraft.DefaultProtocol.Ver() {
		return nil, fmt.Errorf("unsupported replay protocol %d/%s", request.Protocol, client.GameVersion)
	}
	// Connect omits device claims filled in by the core for upstream login. Replay
	// has no upstream login, so it only needs the protocol and game version above.
	handoff := &sessionwire.Handoff{Identity: sessionwire.Identity{DisplayName: client.ThirdPartyName}}
	for _, pack := range packs {
		handoff.Packs = append(handoff.Packs, sessionwire.Pack{
			UUID: pack.UUID().String(), Version: pack.Version(), ContentKey: pack.ContentKey(), Size: uint64(pack.Size()),
		})
	}
	return &replaySession{conn: conn, handoff: handoff, packs: packs}, nil
}

// Write retains a captured packet until its scheduled burst is flushed.
func (s *replaySession) Write(data []byte) (int, error) {
	s.pending = append(s.pending, data)
	return len(data), nil
}

// writeFrame sends one session message with the stream length prefix.
func (s *replaySession) writeFrame(frame []byte) error {
	_, err := s.conn.Write(frame)
	return err
}

// Flush sends startup and archives once, then preserves each replay burst as one batch.
func (s *replaySession) Flush() error {
	if s.handoff != nil {
		frame, err := sessionwire.EncodeHandoff(*s.handoff, s.pending)
		if err != nil {
			return err
		}
		if err := s.writeFrame(frame); err != nil {
			return err
		}
		if err := sessionwire.WritePacks(s.writeFrame, s.packs); err != nil {
			return err
		}
		s.handoff, s.packs = nil, nil
	} else if len(s.pending) != 0 {
		frame := []byte{sessionwire.KindBatch}
		for _, data := range s.pending {
			frame = sessionwire.AppendBatchPacket(frame, data)
		}
		if err := s.writeFrame(frame); err != nil {
			return err
		}
	}
	s.pending = nil
	return nil
}
