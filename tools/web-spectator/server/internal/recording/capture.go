package recording

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"time"

	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/replay"
	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/spectator"
)

// Detail is stable metadata; dynamic appearance, HUD and world events live in
// each complete snapshot. All assets are frozen and referenced by content hash.
type Detail struct {
	Mode             string             `json:"mode"`
	Ranked           bool               `json:"ranked"`
	Players          []spectator.Player `json:"players"`
	ArenaHash        string             `json:"arenaHash"`
	AppearanceHashes []string           `json:"appearanceHashes,omitempty"`
	SkinHashes       []string           `json:"skinHashes"`
}

func (s *Service) consume(e event) {
	var err error
	switch e.subject {
	case spectator.ReplayStartSubject:
		if s.active[e.opening.ID] != nil {
			s.abort(e.opening.ID)
			return
		}
		for _, frame := range e.opening.Frames {
			payload, marshalErr := json.Marshal(frame)
			if marshalErr != nil {
				s.abort(e.opening.ID)
				return
			}
			e.frame, e.data = frame, payload
			if err = s.frame(e); err != nil {
				return
			}
			if _, active := s.active[e.opening.ID]; !active {
				return
			}
		}
	case spectator.FrameSubject:
		err = s.frame(e)
	case spectator.SkinSubject:
		var skin spectator.SkinAsset
		if json.Unmarshal(e.data, &skin) != nil {
			return
		}
		if value := s.active[skin.ID]; value != nil && hasPlayer(value.detail, skin.PlayerID) {
			if skin.AppearanceID != "" {
				data, decodeErr := base64.StdEncoding.DecodeString(skin.Appearance)
				if decodeErr != nil || s.addAppearance(value, skin.AppearanceID, data) != nil {
					s.abort(skin.ID)
					return
				}
				for i := range value.detail.Players {
					if value.detail.Players[i].ID == skin.PlayerID && value.detail.Players[i].AppearanceID == "" {
						value.detail.Players[i].AppearanceID = skin.AppearanceID
					}
				}
			}
			var data []byte
			data, err = base64.StdEncoding.DecodeString(skin.PNG)
			if err == nil {
				var hash string
				hash, err = s.store.PutAsset("skin", data)
				if err == nil {
					err = value.recording.AddAsset(hash)
					if err != nil {
						s.store.ReleaseAsset(hash)
					}
					value.detail.addSkin(hash)
					for index := range value.detail.Players {
						player := &value.detail.Players[index]
						if player.ID == skin.PlayerID && player.SkinID == "" {
							player.SkinID, player.SkinModel = hash, skin.Model
						}
					}
				}
			}
			if err != nil {
				s.abort(skin.ID)
			}
		}
	case spectator.ClosedSubject:
		var closed spectator.Closed
		if json.Unmarshal(e.data, &closed) != nil {
			return
		}
		defer delete(s.failed, closed.ID)
		if value := s.active[closed.ID]; value != nil {
			if closed.Reason != "finished" || closed.ReplayIncomplete {
				s.abort(closed.ID)
				return
			}
			if final := closed.FinalFrame; final != nil {
				if final.Incomplete {
					s.abort(closed.ID)
					return
				}
				if final.UpdatedAt.Sub(value.lastFrameAt) > spectator.MaxReplayStartGap {
					s.abort(closed.ID)
					return
				}
				payload, marshalErr := json.Marshal(final)
				if marshalErr != nil {
					s.abort(closed.ID)
					return
				}
				if err = value.recording.Append(final.UpdatedAt.Sub(value.started).Milliseconds(), payload); err != nil {
					s.abort(closed.ID)
					return
				}
			}
			detail, _ := json.Marshal(value.detail)
			if err = value.recording.UpdateDetail(detail); err == nil {
				_, err = value.recording.Finish()
			}
			if err != nil {
				value.recording.Abort()
			}
			delete(s.active, closed.ID)
		}
	}
	if err != nil {
		s.log.Warn("replay capture skipped", "error", err)
	}
}

func (s *Service) frame(e event) error {
	id := e.frame.ID
	if e.frame.Incomplete {
		s.abort(id)
		return nil
	}
	if s.blocked {
		return nil
	}
	if _, blocked := s.failed[id]; blocked {
		return nil
	}
	value := s.active[id]
	if value == nil {
		if e.subject != spectator.ReplayStartSubject {
			s.abort(id)
			return nil
		}
		start := e.frame.MatchStartedAt
		if start == nil || start.IsZero() || e.frame.RoundActive || e.frame.UpdatedAt.Sub(*start) > 10*time.Second {
			s.abort(id)
			return nil
		}
		if len(s.active) >= spectator.MaxMatches {
			s.abort(id)
			return fmt.Errorf("replay recording limit reached")
		}
		arena, err := json.Marshal(e.arena)
		if err != nil {
			return err
		}
		arenaHash, err := s.store.PutAsset("arena", arena)
		if err != nil {
			s.abort(id)
			return err
		}
		assets := []string{arenaHash}
		// Skin publications precede frames with skin references. Reuse validated
		// live-cache bytes rather than fetching a player's later website skin.
		for _, player := range e.frame.Players {
			if player.SkinID == "" {
				continue
			}
			data := e.skins[player.ID]
			if len(data) == 0 {
				continue
			}
			hash, assetErr := s.store.PutAsset("skin", data)
			if assetErr != nil {
				for _, hash := range assets {
					s.store.ReleaseAsset(hash)
				}
				s.abort(id)
				return assetErr
			}
			assets = append(assets, hash)
		}
		info := Detail{Mode: e.frame.Mode, Ranked: e.frame.Ranked, Players: append([]spectator.Player(nil), e.frame.Players...), ArenaHash: arenaHash}
		for _, hash := range assets[1:] {
			info.addSkin(hash)
		}
		for i := range info.Players {
			if len(e.skins[info.Players[i].ID]) == 0 {
				info.Players[i].SkinID = ""
			}
		}
		detail, _ := json.Marshal(info)
		recording, err := s.store.Begin(replay.Metadata{ID: id, StartedAt: e.frame.UpdatedAt, Detail: detail}, assets)
		if err != nil {
			for _, hash := range assets {
				s.store.ReleaseAsset(hash)
			}
			s.abort(id)
			return err
		}
		value = &active{recording: recording, last: time.Now(), lastFrameAt: e.frame.UpdatedAt, started: e.frame.UpdatedAt, detail: info}
		s.active[id] = value
	} else if e.frame.UpdatedAt.Sub(value.lastFrameAt) > spectator.MaxReplayStartGap {
		s.abort(id)
		return fmt.Errorf("replay frame gap exceeds capture limit")
	}
	for _, player := range e.frame.Players {
		if player.SkinID == "" || len(e.skins[player.ID]) == 0 || value.detail.hasSkin(player.SkinID) {
			continue
		}
		hash, err := s.store.PutAsset("skin", e.skins[player.ID])
		if err != nil {
			s.abort(id)
			return err
		}
		if err = value.recording.AddAsset(hash); err != nil {
			s.store.ReleaseAsset(hash)
			s.abort(id)
			return err
		}
		value.detail.addSkin(hash)
		for i := range value.detail.Players {
			saved := &value.detail.Players[i]
			if saved.ID == player.ID && saved.SkinID == "" {
				saved.SkinID = hash
				saved.SkinModel = player.SkinModel
			}
		}
	}
	for _, player := range e.frame.Players {
		if player.AppearanceID != "" && !value.detail.hasAppearance(player.AppearanceID) && len(e.appearances[player.AppearanceID]) == 0 {
			s.abort(id)
			return fmt.Errorf("recorded appearance is unavailable")
		}
	}
	for hash, data := range e.appearances {
		if err := s.addAppearance(value, hash, data); err != nil {
			s.abort(id)
			return err
		}
	}
	// The first frame's timestamp is retained independently of later samples.
	if err := value.recording.Append(e.frame.UpdatedAt.Sub(value.started).Milliseconds(), e.data); err != nil {
		s.abort(id)
		return err
	}
	value.last = time.Now()
	value.lastFrameAt = e.frame.UpdatedAt
	return nil
}

func hasPlayer(detail Detail, id string) bool {
	for _, player := range detail.Players {
		if player.ID == id {
			return true
		}
	}
	return false
}
func (d Detail) hasSkin(hash string) bool {
	for _, skin := range d.SkinHashes {
		if skin == hash {
			return true
		}
	}
	return false
}
func (d *Detail) addSkin(hash string) {
	if !d.hasSkin(hash) {
		d.SkinHashes = append(d.SkinHashes, hash)
	}
}

func (d Detail) hasAppearance(hash string) bool {
	for _, candidate := range d.AppearanceHashes {
		if candidate == hash {
			return true
		}
	}
	return false
}
func (s *Service) addAppearance(value *active, hash string, data []byte) error {
	if value.detail.hasAppearance(hash) {
		return nil
	}
	if len(value.detail.AppearanceHashes) >= 64 {
		return fmt.Errorf("too many replay appearances")
	}
	saved, err := s.store.PutAsset("appearance", data)
	if err != nil {
		return err
	}
	if saved != hash {
		s.store.ReleaseAsset(saved)
		return fmt.Errorf("appearance identity mismatch")
	}
	if err = value.recording.AddAsset(saved); err != nil {
		s.store.ReleaseAsset(saved)
		return err
	}
	value.detail.AppearanceHashes = append(value.detail.AppearanceHashes, saved)
	return nil
}
