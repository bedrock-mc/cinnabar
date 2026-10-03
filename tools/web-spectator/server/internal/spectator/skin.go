package spectator

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"image/png"
	"time"
)

const maxSkinBytes = 8 << 20
const skinLifetime = 35 * time.Second

type SkinAsset struct {
	Version      int       `json:"version"`
	ID           string    `json:"id"`
	PlayerID     string    `json:"playerId"`
	SkinID       string    `json:"skinId"`
	AppearanceID string    `json:"appearanceId,omitempty"`
	Appearance   string    `json:"appearance,omitempty"`
	PNG          string    `json:"png"`
	Model        string    `json:"model"`
	Width        int       `json:"width"`
	Height       int       `json:"height"`
	UpdatedAt    time.Time `json:"updatedAt"`
}
type skinKey struct{ match, player, hash string }
type skinEntry struct {
	png          []byte
	appearance   []byte
	appearanceID string
	updated      time.Time
}

func validSkinID(id string) bool {
	if len(id) != 64 {
		return false
	}
	for _, r := range id {
		if !(r >= '0' && r <= '9' || r >= 'a' && r <= 'f') {
			return false
		}
	}
	return true
}
func validSkinModel(model string) bool {
	return model == "classic" || model == "slim" || model == "unknown"
}
func (s *Store) acceptSkin(asset SkinAsset, now time.Time) error {
	if asset.Version != Version || !ValidID(asset.ID) || !cleanLabel(asset.PlayerID, 128) || !validSkinID(asset.SkinID) || !validSkinModel(asset.Model) || !fresh(asset.UpdatedAt, now) || !(asset.Width == 64 || asset.Width == 128 || asset.Width == 256) || !(asset.Height == asset.Width || asset.Height == asset.Width/2) {
		return errors.New("invalid spectator skin")
	}
	data, err := base64.StdEncoding.DecodeString(asset.PNG)
	if err != nil || len(data) == 0 || len(data) > 512<<10 {
		return errors.New("invalid spectator PNG")
	}
	digest := sha256.Sum256(data)
	if hex.EncodeToString(digest[:]) != asset.SkinID {
		return errors.New("spectator skin hash mismatch")
	}
	config, err := png.DecodeConfig(bytes.NewReader(data))
	if err != nil || config.Width != asset.Width || config.Height != asset.Height {
		return errors.New("spectator skin dimensions mismatch")
	}
	appearance, err := decodeAppearance(asset)
	if err != nil {
		return err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if _, closed := s.closed[asset.ID]; closed || now.Before(s.blockedUntil) {
		return errors.New("spectator skin match closed")
	}
	key := skinKey{asset.ID, asset.PlayerID, asset.SkinID}
	if entry, found := s.skins[key]; found {
		if s.skinBytes-len(entry.appearance)+len(appearance) > maxSkinBytes {
			return errors.New("spectator skin cache full")
		}
		s.skinBytes += len(appearance) - len(entry.appearance)
		entry.appearance, entry.appearanceID = appearance, asset.AppearanceID
		entry.updated = now
		s.skins[key] = entry
		return nil
	}
	if len(s.skins) >= 256 || s.skinBytes+len(data)+len(appearance) > maxSkinBytes {
		return errors.New("spectator skin cache full")
	}
	// Publications may precede their first frame after reconnect. Bound these
	// pending entries by the same lifetime and byte budget as active skins.
	s.skins[key] = skinEntry{png: data, appearance: appearance, appearanceID: asset.AppearanceID, updated: now}
	s.skinBytes += len(data) + len(appearance)
	return nil
}
func (s *Store) Skin(match, player, hash string, now time.Time) []byte {
	s.mu.Lock()
	defer s.mu.Unlock()
	entry := s.skins[skinKey{match, player, hash}]
	if now.Sub(entry.updated) > skinLifetime {
		return nil
	}
	return entry.png // immutable after insertion
}
func (s *Store) discardSkins(match string) {
	for key, entry := range s.skins {
		if key.match == match {
			delete(s.skins, key)
			s.skinBytes -= len(entry.png) + len(entry.appearance)
		}
	}
}

func decodeAppearance(asset SkinAsset) ([]byte, error) {
	if asset.AppearanceID == "" && asset.Appearance == "" {
		return nil, nil
	}
	if !validSkinID(asset.AppearanceID) {
		return nil, errors.New("invalid appearance identity")
	}
	data, err := base64.StdEncoding.DecodeString(asset.Appearance)
	if err != nil || len(data) > 384<<10 {
		return nil, errors.New("invalid appearance size")
	}
	hash := sha256.Sum256(data)
	if hex.EncodeToString(hash[:]) != asset.AppearanceID {
		return nil, errors.New("appearance hash mismatch")
	}
	var detail struct {
		Geometry      json.RawMessage `json:"geometry"`
		ResourcePatch json.RawMessage `json:"resourcePatch"`
		CapePNG       string          `json:"capePNG"`
		Animations    []struct {
			PNG        string `json:"png"`
			Kind       int    `json:"kind"`
			Frames     int    `json:"frames"`
			Expression int    `json:"expression"`
		} `json:"animations"`
	}
	if json.Unmarshal(data, &detail) != nil || !json.Valid(detail.Geometry) || !json.Valid(detail.ResourcePatch) || len(detail.Geometry) > 256<<10 || len(detail.ResourcePatch) > 16<<10 {
		return nil, errors.New("invalid appearance geometry")
	}
	if detail.CapePNG != "" {
		cape, err := base64.StdEncoding.DecodeString(detail.CapePNG)
		if err != nil || len(cape) > 128<<10 {
			return nil, errors.New("invalid cape")
		}
		cfg, err := png.DecodeConfig(bytes.NewReader(cape))
		if err != nil || cfg.Width < 1 || cfg.Height < 1 || cfg.Width > 256 || cfg.Height > 256 {
			return nil, errors.New("invalid cape dimensions")
		}
	}
	if len(detail.Animations) > 3 {
		return nil, errors.New("too many skin animations")
	}
	for _, animation := range detail.Animations {
		data, err := base64.StdEncoding.DecodeString(animation.PNG)
		if err != nil || len(data) > 128<<10 || animation.Kind < 0 || animation.Kind > 2 || animation.Frames < 1 || animation.Frames > 256 || animation.Expression < 0 || animation.Expression > 1 {
			return nil, errors.New("invalid skin animation")
		}
		cfg, err := png.DecodeConfig(bytes.NewReader(data))
		if err != nil || cfg.Width < 1 || cfg.Height < 1 || cfg.Width > 512 || cfg.Height > 512 || animation.Frames > cfg.Height {
			return nil, errors.New("invalid animation dimensions")
		}
	}
	return data, nil
}
func (s *Store) Appearance(match, player, skinHash, appearanceHash string, now time.Time) []byte {
	s.mu.Lock()
	defer s.mu.Unlock()
	entry := s.skins[skinKey{match, player, skinHash}]
	if entry.appearanceID != appearanceHash || now.Sub(entry.updated) > skinLifetime {
		return nil
	}
	return entry.appearance
}
