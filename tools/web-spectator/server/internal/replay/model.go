// Package replay stores bounded, independently seekable compressed recordings.
package replay

import (
	"encoding/json"
	"errors"
	"time"
)

const DefaultMaxBytes int64 = 25_000_000_000
const FormatVersion = 1
const maxChunkBytes = 4 << 20
const maxManifestBytes = 1 << 20
const maxBufferedBytes = 16 << 20
const maxActiveRecordings = 64

var ErrQuota = errors.New("replay storage quota exceeded")
var ErrClosed = errors.New("recording is closed")
var ErrNotFound = errors.New("replay not found")

type Config struct {
	Directory string
	MaxBytes  int64
}

// Metadata is deliberately renderer-independent. Detail carries versioned match
// and participant information; Frame.Payload carries complete state snapshots.
type Metadata struct {
	ID        string          `json:"id"`
	StartedAt time.Time       `json:"startedAt"`
	Detail    json.RawMessage `json:"detail,omitempty"`
}

type Frame struct {
	TimeMS  int64           `json:"timeMs"`
	Payload json.RawMessage `json:"payload"`
}

type Chunk struct {
	Index   int   `json:"index"`
	StartMS int64 `json:"startMs"`
	EndMS   int64 `json:"endMs"`
	Frames  int   `json:"frames"`
	Bytes   int64 `json:"bytes"`
}

type Manifest struct {
	Version    int       `json:"version"`
	Metadata   Metadata  `json:"metadata"`
	FinishedAt time.Time `json:"finishedAt"`
	Assets     []string  `json:"assets"`
	Chunks     []Chunk   `json:"chunks"`
	DurationMS int64     `json:"durationMs"`
	FileBytes  int64     `json:"fileBytes"`
}

// ChunkAt returns the chunk covering or immediately preceding the requested
// timestamp. Each chunk starts with a complete snapshot, never a dependency on
// an earlier chunk. Playback interpolates between the recorded snapshots.
func (m Manifest) ChunkAt(timeMS int64) int {
	index := 0
	for i, chunk := range m.Chunks {
		if chunk.StartMS > timeMS {
			break
		}
		index = i
	}
	return index
}
