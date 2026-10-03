// Package recording moves replay compression and disk IO off the NATS callback.
package recording

import (
	"bytes"
	"encoding/json"
	"log/slog"
	"sync"
	"sync/atomic"
	"time"

	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/replay"
	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/spectator"
)

const maxPendingBytes = 8 << 20
const maxFailedIDs = 1024

type event struct {
	subject     string
	data        []byte
	frame       spectator.Frame
	opening     spectator.ReplayStart
	arena       *spectator.Arena
	skins       map[string][]byte
	appearances map[string][]byte
	stamp       time.Time
	generation  int64
	bytes       int64
}
type active struct {
	recording   *replay.Recording
	last        time.Time
	lastFrameAt time.Time
	started     time.Time
	detail      Detail
}
type Service struct {
	store      *replay.Store
	live       *spectator.Store
	log        *slog.Logger
	queue      chan event
	pending    atomic.Int64
	lost       atomic.Bool
	generation atomic.Int64
	stop       chan struct{}
	done       chan struct{}
	closeOnce  sync.Once
	active     map[string]*active
	failed     map[string]time.Time
	blocked    bool
}

func New(store *replay.Store, live *spectator.Store, log *slog.Logger) *Service {
	if log == nil {
		log = slog.Default()
	}
	s := &Service{store: store, live: live, log: log, queue: make(chan event, 128), stop: make(chan struct{}), done: make(chan struct{}), active: make(map[string]*active), failed: make(map[string]time.Time)}
	go s.run()
	return s
}

// Accept receives only validated exports. It retains immutable skin/arena data
// immediately: the live cache may close before the compression worker runs.
func (s *Service) Accept(subject string, data []byte, now time.Time) {
	switch subject {
	case spectator.FrameSubject, spectator.SkinSubject, spectator.ClosedSubject:
	default:
		return
	}
	select {
	case <-s.stop:
		return
	default:
	}
	e := event{subject: subject, stamp: now, generation: s.generation.Load()}
	if subject == spectator.FrameSubject {
		if json.Unmarshal(data, &e.frame) != nil {
			return
		}
		live := s.live.Lookup(e.frame.ID)
		if live == nil {
			return
		}
		valid, _ := live.WithCurrent(now, func(_ spectator.Frame, arena *spectator.Arena) error { e.arena = arena; return nil })
		if !valid {
			return
		}
		e.skins = map[string][]byte{}
		e.appearances = map[string][]byte{}
		for _, player := range e.frame.Players {
			if player.SkinID == "" {
				continue
			}
			skin := s.live.Skin(e.frame.ID, player.ID, player.SkinID, now)
			if len(skin) > 0 {
				e.skins[player.ID] = skin
				e.bytes += int64(len(skin))
				if player.AppearanceID != "" {
					appearance := s.live.Appearance(e.frame.ID, player.ID, player.SkinID, player.AppearanceID, now)
					if len(appearance) > 0 {
						e.appearances[player.AppearanceID] = appearance
						e.bytes += int64(len(appearance))
					}
				}
			}
		}
	}
	e.bytes += int64(len(data))
	if s.pending.Add(e.bytes) > maxPendingBytes {
		s.pending.Add(-e.bytes)
		s.Disconnect()
		return
	}
	e.data = bytes.Clone(data)
	select {
	case s.queue <- e:
	default:
		s.pending.Add(-e.bytes)
		s.Disconnect()
	}
}

// AcceptReplayStart queues one validated atomic opening. A missing batch leaves
// the first live frame ineligible to start a recording.
func (s *Service) AcceptReplayStart(start spectator.ReplayStart, arena *spectator.Arena, data []byte, now time.Time) {
	select {
	case <-s.stop:
		return
	default:
	}
	e := event{subject: spectator.ReplayStartSubject, opening: start, arena: arena, data: bytes.Clone(data), stamp: now, generation: s.generation.Load(), bytes: int64(len(data))}
	if s.pending.Add(e.bytes) > maxPendingBytes {
		s.pending.Add(-e.bytes)
		s.Disconnect()
		return
	}
	select {
	case s.queue <- e:
	default:
		s.pending.Add(-e.bytes)
		s.Disconnect()
	}
}
func (s *Service) Close()      { s.closeOnce.Do(func() { close(s.stop) }); <-s.done }
func (s *Service) Disconnect() { s.generation.Add(1); s.lost.Store(true) }

func (s *Service) run() {
	defer close(s.done)
	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()
	for {
		select {
		case <-s.stop:
			s.abortAll()
			return
		case e := <-s.queue:
			s.pending.Add(-e.bytes)
			s.handleLoss()
			if e.generation != s.generation.Load() {
				if e.subject == spectator.FrameSubject {
					s.abort(e.frame.ID)
				} else if e.subject == spectator.ReplayStartSubject {
					s.abort(e.opening.ID)
				}
				continue
			}
			s.consume(e)
		case now := <-ticker.C:
			s.handleLoss()
			for id, recording := range s.active {
				if now.Sub(recording.last) > spectator.Freshness {
					s.abort(id)
				}
			}
		}
	}
}
func (s *Service) handleLoss() {
	if !s.lost.Swap(false) {
		return
	}
	s.abortAll()
	for _, frame := range s.live.List(time.Now()) {
		s.abort(frame.ID)
	}
	s.log.Warn("replay capture lost exports; incomplete recordings discarded")
}
func (s *Service) abort(id string) {
	if value := s.active[id]; value != nil {
		value.recording.Abort()
		delete(s.active, id)
	}
	if _, exists := s.failed[id]; exists || len(s.failed) < maxFailedIDs {
		s.failed[id] = time.Now()
	} else {
		s.blocked = true
		s.log.Error("replay closure ledger full; new recordings disabled until restart")
	}
}
func (s *Service) abortAll() {
	for id := range s.active {
		s.abort(id)
	}
}
