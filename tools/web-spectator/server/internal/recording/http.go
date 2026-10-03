package recording

import (
	"compress/gzip"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/replay"
)

const maxWindowPayloadBytes = 64 << 20
const maxWindowFrames = 16_384

var errWindowTooLarge = errors.New("replay window exceeds size limit")

type HTTP struct {
	store     *replay.Store
	downloads chan struct{}
}

func NewHTTP(store *replay.Store) *HTTP {
	return &HTTP{store: store, downloads: make(chan struct{}, 2)}
}

type View struct {
	ID         string    `json:"id"`
	StartedAt  time.Time `json:"startedAt"`
	DurationMS int64     `json:"durationMs"`
	Bytes      int64     `json:"bytes"`
	Detail
}

func view(m replay.Manifest) View {
	v := View{ID: m.Metadata.ID, StartedAt: m.Metadata.StartedAt, DurationMS: m.DurationMS, Bytes: m.FileBytes}
	_ = json.Unmarshal(m.Metadata.Detail, &v.Detail)
	return v
}

func (h *HTTP) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Cache-Control", "private, no-store")
	w.Header().Set("X-Content-Type-Options", "nosniff")
	if r.Method != http.MethodGet && r.Method != http.MethodHead {
		reply(w, 405, map[string]string{"error": "Replay routes are read-only."})
		return
	}
	_ = http.NewResponseController(w).SetWriteDeadline(time.Now().Add(15 * time.Second))
	path := strings.TrimPrefix(r.URL.Path, "/api/replays")
	if path == "" || path == "/" {
		all := h.store.List()
		// List is bounded; later pages are fetched explicitly rather than retaining
		// every stored replay in either the browser or Minecraft client.
		limit := 100
		if supplied := r.URL.Query().Get("limit"); supplied != "" {
			parsed, parseErr := strconv.Atoi(supplied)
			if parseErr != nil || parsed < 1 || parsed > 100 {
				reply(w, 400, map[string]string{"error": "Invalid replay limit."})
				return
			}
			limit = parsed
		}
		if player := strings.TrimSpace(r.URL.Query().Get("player")); player != "" {
			filtered := all[:0]
			for _, m := range all {
				for _, p := range view(m).Players {
					if strings.EqualFold(p.Name, player) || p.ID == player {
						filtered = append(filtered, m)
						break
					}
				}
			}
			all = filtered
		}
		offset, err := strconv.Atoi(r.URL.Query().Get("offset"))
		if r.URL.Query().Get("offset") == "" {
			offset, err = 0, nil
		}
		if err != nil || offset < 0 {
			reply(w, 400, map[string]string{"error": "Invalid replay offset."})
			return
		}
		offset = min(offset, len(all))
		end := min(len(all), offset+limit)
		values := make([]View, 0, end-offset)
		for _, m := range all[offset:end] {
			values = append(values, view(m))
		}
		used, cap := h.store.Stats()
		reply(w, 200, map[string]any{"replays": values, "total": len(all), "offset": offset, "storageBytes": used, "storageLimitBytes": cap})
		return
	}
	id, operation, _ := strings.Cut(strings.TrimPrefix(path, "/"), "/")
	manifest, err := h.store.Get(id)
	if err != nil {
		replayError(w, err)
		return
	}
	info := view(manifest)
	if operation == "" {
		reply(w, 200, info)
		return
	}
	if hash, ok := strings.CutPrefix(operation, "appearance/"); ok {
		if !info.hasAppearance(hash) {
			replayError(w, replay.ErrNotFound)
			return
		}
		if !h.acquire(w) {
			return
		}
		defer func() { <-h.downloads }()
		data, err := h.store.ReadAsset(id, hash)
		if err != nil {
			replayError(w, err)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		if r.Method != http.MethodHead {
			_, _ = w.Write(data)
		}
		return
	}
	if hash, ok := strings.CutPrefix(operation, "skins/"); ok {
		if !info.hasSkin(hash) {
			replayError(w, replay.ErrNotFound)
			return
		}
		if !h.acquire(w) {
			return
		}
		defer func() { <-h.downloads }()
		data, err := h.store.ReadAsset(id, hash)
		if err != nil {
			replayError(w, err)
			return
		}
		w.Header().Set("Content-Type", "image/png")
		if r.Method != http.MethodHead {
			_, _ = w.Write(data)
		}
		return
	}
	if operation == "arena" {
		if !h.acquire(w) {
			return
		}
		defer func() { <-h.downloads }()
		data, err := h.store.ReadAsset(id, info.ArenaHash)
		if err != nil {
			replayError(w, err)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		if r.Method != http.MethodHead {
			_, _ = w.Write(data)
		}
		return
	}
	if operation != "frames" {
		reply(w, 404, map[string]string{"error": "Replay route not found."})
		return
	}
	from, err := strconv.ParseInt(r.URL.Query().Get("fromMs"), 10, 64)
	if err != nil || from < 0 {
		reply(w, 400, map[string]string{"error": "Invalid replay start time."})
		return
	}
	to, err := strconv.ParseInt(r.URL.Query().Get("toMs"), 10, 64)
	if err != nil || to <= from || to-from > 10_000 {
		reply(w, 400, map[string]string{"error": "Replay windows must be at most ten seconds."})
		return
	}
	if !h.acquire(w) {
		return
	}
	defer func() { <-h.downloads }()
	if r.Method == http.MethodHead {
		w.Header().Set("Content-Type", "application/json")
		return
	}
	h.frames(w, r, manifest, info, from, to)
}

func reply(w http.ResponseWriter, status int, value any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(value)
}
func replayError(w http.ResponseWriter, err error) {
	status := 503
	if errors.Is(err, replay.ErrNotFound) {
		status = 404
	}
	reply(w, status, map[string]string{"error": "This replay is unavailable or has been replaced by a newer recording."})
}

func (h *HTTP) frames(w http.ResponseWriter, r *http.Request, m replay.Manifest, info View, from, to int64) {
	frames, err := h.window(r.Context(), m, from, to)
	if err != nil {
		if r.Context().Err() == nil {
			replayError(w, err)
		}
		return
	}
	w.Header().Set("Content-Type", "application/json")
	var writer io.Writer = w
	if strings.Contains(r.Header.Get("Accept-Encoding"), "gzip") {
		w.Header().Set("Content-Encoding", "gzip")
		w.Header().Set("Vary", "Accept-Encoding")
		gz, _ := gzip.NewWriterLevel(w, gzip.BestSpeed)
		defer gz.Close()
		writer = gz
	}
	_, _ = io.WriteString(writer, `{"frames":[`)
	for index, payload := range frames {
		if index > 0 {
			if _, err := io.WriteString(writer, ","); err != nil {
				return
			}
		}
		if _, err := writer.Write(fillSkinReferences(payload, info)); err != nil {
			return
		}
	}
	_, _ = io.WriteString(writer, `]}`)
}

func (h *HTTP) window(ctx context.Context, m replay.Manifest, from, to int64) ([]json.RawMessage, error) {
	// One independent compressed chunk is decoded at a time. A seek includes
	// the preceding snapshot for continuous interpolation at the boundary.
	index := m.ChunkAt(from)
	if index > 0 {
		index--
	}
	selected := make([]json.RawMessage, 0)
	var selectedBytes int
	var preceding json.RawMessage
	appendFrame := func(payload json.RawMessage) error {
		if len(selected) >= maxWindowFrames || len(payload) > maxWindowPayloadBytes-selectedBytes {
			return errWindowTooLarge
		}
		selected = append(selected, payload)
		selectedBytes += len(payload)
		return nil
	}
	for ; index < len(m.Chunks); index++ {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if m.Chunks[index].StartMS > to {
			break
		}
		frames, err := h.store.ReadChunk(m.Metadata.ID, index)
		if err != nil {
			return nil, err
		}
		for _, frame := range frames {
			if frame.TimeMS <= from {
				preceding = frame.Payload
				continue
			}
			if frame.TimeMS > to {
				break
			}
			if preceding != nil {
				if err := appendFrame(preceding); err != nil {
					return nil, err
				}
				preceding = nil
			}
			if err := appendFrame(frame.Payload); err != nil {
				return nil, err
			}
		}
	}
	if preceding != nil {
		if err := appendFrame(preceding); err != nil {
			return nil, err
		}
	}
	return selected, nil
}

func fillSkinReferences(payload json.RawMessage, info View) json.RawMessage {
	var raw struct {
		Players []struct {
			ID     string `json:"id"`
			SkinID string `json:"skinId"`
		} `json:"players"`
	}
	// Early snapshots may predate asynchronous skin encoding. Fill only absent
	// references from the frozen final metadata, never from current profiles.
	if json.Unmarshal(payload, &raw) == nil {
		var object map[string]json.RawMessage
		changed := false
		var players []map[string]json.RawMessage
		if json.Unmarshal(payload, &object) == nil && json.Unmarshal(object["players"], &players) == nil {
			for i, p := range raw.Players {
				if p.SkinID == "" || !info.hasSkin(p.SkinID) {
					for _, saved := range info.Players {
						if saved.ID == p.ID && saved.SkinID != "" {
							players[i]["skinId"], _ = json.Marshal(saved.SkinID)
							players[i]["skinModel"], _ = json.Marshal(saved.SkinModel)
							players[i]["appearanceId"], _ = json.Marshal(saved.AppearanceID)
							changed = true
							break
						}
					}
				}
			}
			if changed {
				object["players"], _ = json.Marshal(players)
				payload, _ = json.Marshal(object)
			}
		}
	}
	return payload
}

// Bound decoded arena data and chunk windows across slow HTTP readers too.
func (h *HTTP) acquire(w http.ResponseWriter) bool {
	select {
	case h.downloads <- struct{}{}:
		return true
	default:
	}
	w.Header().Set("Retry-After", "2")
	reply(w, 429, map[string]string{"error": "Replay downloads are busy. Try again shortly."})
	return false
}
