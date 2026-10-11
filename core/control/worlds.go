package control

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"time"
)

const (
	methodWorldList   = "world_list.v1"
	methodWorldCreate = "world_create.v1"
	methodWorldUpdate = "world_update.v1"
	methodWorldDelete = "world_delete.v1"
	methodWorldOpen   = "world_open.v1"
	methodWorldClose  = "world_close.v1"
	methodWorldPause  = "world_pause.v1"
	methodWorldStatus = "world_status.v1"
	methodBDSEULA     = "bds_accept_eula.v1"
	methodPrefs       = "local_worlds_prefs.v1"
	methodWorldInvite = "world_invite.v1"

	// maxListedWorlds keeps a world_list response inside MaxFrameLen.
	maxListedWorlds = 200

	codeWorldFailed   = -32000
	codeWorldNotFound = -32010
	codeWorldBusy     = -32011
	codeEULARequired  = -32012
	codeBackendAbsent = -32013
)

// World failures the world_* methods report by code; a sentinel's message is safe to show.
var (
	ErrWorldNotFound           = errors.New("world not found")
	ErrWorldInvalid            = errors.New("invalid world settings") // its wrapping message names the bad setting
	ErrWorldBusy               = errors.New("another world is starting, running or stopping")
	ErrWorldInUse              = errors.New("world is open")
	ErrWorldNotOpen            = errors.New("no world is open")
	ErrWorldRuntimePending     = errors.New("still checking whether Docker is running; try again")
	ErrEULARequired            = errors.New("the Minecraft EULA must be accepted before the server is downloaded")
	ErrWorldBackendUnavailable = errors.New("world backend is not available on this platform")
)

// Worlds hosts local worlds behind the world_* methods.
type Worlds interface {
	List() ([]World, error)
	// Open begins starting a world; a zero viewDistance takes the backend default.
	Open(id string, viewDistance int) error
	Close() error
	Status() WorldStatus
}

// WorldEditor saves, edits and deletes worlds; a Worlds value may implement it.
type WorldEditor interface {
	Create(WorldSpec) (World, error)
	Update(id string, update WorldUpdate) (World, error)
	Delete(id string) error
}

// WorldBackend controls the server that hosts worlds; a Worlds value may implement it.
type WorldBackend interface {
	SetPaused(paused bool) error
	AcceptEULA() error
	Prefs(ctx context.Context, update WorldPrefsUpdate) (WorldPrefs, error)
}

// Inviter sends Xbox Live invites to the open world while it is hosted for friends.
type Inviter interface {
	Invite(ctx context.Context, xuid string) error
}

// World is one saved world.
type World struct {
	ID             string `json:"id"`
	Name           string `json:"name"`
	GameMode       string `json:"game_mode"`
	Generator      string `json:"generator"`
	Difficulty     string `json:"difficulty"`
	Backend        string `json:"backend"`
	Seed           int64  `json:"seed"`
	CreatedUnix    int64  `json:"created_unix"`
	LastPlayedUnix int64  `json:"last_played_unix"`
	SizeBytes      int64  `json:"size_bytes,omitempty"`
}

// WorldSpec is the user-chosen settings of a new world; empty fields take defaults and a nil Seed is random.
type WorldSpec struct {
	Name       string `json:"name"`
	GameMode   string `json:"game_mode,omitempty"`
	Generator  string `json:"generator,omitempty"`
	Difficulty string `json:"difficulty,omitempty"`
	Backend    string `json:"backend,omitempty"`
	Seed       *int64 `json:"seed,omitempty"`
}

// WorldUpdate changes a saved world's settings; nil fields are left alone.
type WorldUpdate struct {
	Name       *string `json:"name,omitempty"`
	GameMode   *string `json:"game_mode,omitempty"`
	Difficulty *string `json:"difficulty,omitempty"`
}

// WorldPrefs are the per-user local-world preferences.
type WorldPrefs struct {
	DockerPromptDismissed bool `json:"docker_prompt_dismissed"`
}

// WorldPrefsUpdate changes preferences; a nil field is left alone and Redetect re-probes for Docker.
type WorldPrefsUpdate struct {
	DockerPromptDismissed *bool
	Redetect              bool
}

// WorldStatus is the secret-safe view of the open world; Error never carries paths.
type WorldStatus struct {
	State                    string      `json:"state"`
	WorldID                  string      `json:"world_id,omitempty"`
	Backend                  string      `json:"backend,omitempty"`
	Paused                   bool        `json:"paused"`
	PauseSupported           bool        `json:"pause_supported"`
	Error                    string      `json:"error,omitempty"`
	Setup                    *WorldSetup `json:"setup,omitempty"`
	BackendUnavailableReason string      `json:"backend_unavailable_reason,omitempty"`
	MaxPlayers               int         `json:"max_players,omitempty"` // the host included; zero when unknown
}

// WorldSetup reports dedicated-server acquisition; Error never carries paths.
type WorldSetup struct {
	State             string `json:"state"`
	Version           string `json:"version,omitempty"`
	BytesDone         int64  `json:"bytes_done"`
	BytesTotal        int64  `json:"bytes_total"`
	LayersDone        int    `json:"layers_done,omitempty"`
	LayersTotal       int    `json:"layers_total,omitempty"`
	EULAAccepted      bool   `json:"eula_accepted"`
	Error             string `json:"error,omitempty"`
	Runtime           string `json:"runtime"`
	Reason            string `json:"reason,omitempty"`
	UnavailableReason string `json:"backend_unavailable_reason,omitempty"`
}

var worldMethods = map[string]struct{}{
	methodWorldList: {}, methodWorldCreate: {}, methodWorldUpdate: {}, methodWorldDelete: {},
	methodWorldOpen: {}, methodWorldClose: {}, methodWorldPause: {}, methodWorldStatus: {}, methodBDSEULA: {}, methodPrefs: {},
	methodWorldInvite: {},
}

func isWorldMethod(method string) bool {
	_, ok := worldMethods[method]
	return ok
}

// WorldResultV1 is the result of every world_* method; unused members are omitted.
type WorldResultV1 struct {
	SchemaVersion uint32       `json:"schema_version"`
	Worlds        []World      `json:"worlds,omitempty"`
	World         *World       `json:"world,omitempty"`
	Status        *WorldStatus `json:"status,omitempty"`
	Prefs         *WorldPrefs  `json:"prefs,omitempty"`
}

func decodeParams(raw json.RawMessage, into any) bool {
	if len(raw) == 0 {
		return false
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	return decoder.Decode(into) == nil && decoder.Decode(new(any)) == io.EOF
}

func (server *Server) serveWorld(conn net.Conn, id uint64, method string, raw json.RawMessage) error {
	reply := responseWriter{server: server, conn: conn, id: id}
	worlds := server.worldService()
	editor, _ := worlds.(WorldEditor)
	backend, _ := worlds.(WorldBackend)
	result := &WorldResultV1{SchemaVersion: 1}
	var err error
	switch method {
	case methodWorldList, methodWorldClose, methodWorldStatus:
		if len(raw) != 0 {
			return reply.invalid()
		}
	case methodWorldCreate, methodWorldUpdate, methodWorldDelete:
		if editor == nil {
			return reply.fail(-32601, "Method not found")
		}
	case methodWorldPause, methodBDSEULA, methodPrefs:
		if backend == nil {
			return reply.fail(-32601, "Method not found")
		}
	}
	switch method {
	case methodWorldList:
		result.Worlds, err = worlds.List()
		if len(result.Worlds) > maxListedWorlds {
			result.Worlds = result.Worlds[:maxListedWorlds]
		}
	case methodWorldCreate:
		var spec WorldSpec
		if !decodeParams(raw, &spec) {
			return reply.invalid()
		}
		var world World
		if world, err = editor.Create(spec); err == nil {
			result.World = &world
		}
	case methodWorldUpdate:
		var params struct {
			ID *string `json:"id"`
			WorldUpdate
		}
		if !decodeParams(raw, &params) || params.ID == nil ||
			(params.Name == nil && params.GameMode == nil && params.Difficulty == nil) {
			return reply.invalid()
		}
		var world World
		if world, err = editor.Update(*params.ID, params.WorldUpdate); err == nil {
			result.World = &world
		}
	case methodWorldDelete, methodWorldOpen:
		var params struct {
			ID           *string `json:"id"`
			ViewDistance int     `json:"view_distance"`
		}
		if !decodeParams(raw, &params) || params.ID == nil || params.ViewDistance < 0 || params.ViewDistance > 64 ||
			(method == methodWorldDelete && params.ViewDistance != 0) {
			return reply.invalid()
		}
		if method == methodWorldDelete {
			err = editor.Delete(*params.ID)
		} else {
			err = worlds.Open(*params.ID, params.ViewDistance)
		}
	case methodPrefs:
		var params struct {
			DockerPromptDismissed *bool `json:"docker_prompt_dismissed"`
			Redetect              bool  `json:"redetect"`
		}
		if len(raw) != 0 && !decodeParams(raw, &params) {
			return reply.invalid()
		}
		ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		prefs, prefsErr := backend.Prefs(ctx, WorldPrefsUpdate{DockerPromptDismissed: params.DockerPromptDismissed, Redetect: params.Redetect})
		cancel()
		err = prefsErr
		result.Prefs = &prefs
	case methodBDSEULA:
		var params struct {
			Accepted *bool `json:"accepted"`
		}
		if !decodeParams(raw, &params) || params.Accepted == nil || !*params.Accepted {
			return reply.invalid()
		}
		err = backend.AcceptEULA()
	case methodWorldClose:
		err = worlds.Close()
	case methodWorldPause:
		var params struct {
			Paused *bool `json:"paused"`
		}
		if !decodeParams(raw, &params) || params.Paused == nil {
			return reply.invalid()
		}
		err = backend.SetPaused(*params.Paused)
	case methodWorldInvite:
		var params struct {
			XUID string `json:"xuid"`
		}
		if !decodeParams(raw, &params) || !validXUID(params.XUID) {
			return reply.invalid()
		}
		inviter, ok := worlds.(Inviter)
		if !ok {
			err = ErrWorldNotOpen
			break
		}
		ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		err = inviter.Invite(ctx, params.XUID)
		cancel()
	}
	if err != nil {
		return reply.fail(worldErrorCode(err), worldErrorMessage(err))
	}
	if method == methodWorldOpen || method == methodWorldClose || method == methodWorldPause || method == methodWorldStatus || method == methodBDSEULA || method == methodPrefs {
		status := worlds.Status()
		result.Status = &status
	}
	return reply.ok(result)
}

// validXUID accepts the decimal Xbox user IDs invites address.
func validXUID(xuid string) bool {
	if xuid == "" || len(xuid) > 20 {
		return false
	}
	for _, r := range xuid {
		if r < '0' || r > '9' {
			return false
		}
	}
	return true
}

func worldErrorCode(err error) int {
	switch {
	case errors.Is(err, ErrWorldNotFound):
		return codeWorldNotFound
	case errors.Is(err, ErrWorldBusy), errors.Is(err, ErrWorldInUse), errors.Is(err, ErrWorldNotOpen), errors.Is(err, ErrWorldRuntimePending):
		return codeWorldBusy
	case errors.Is(err, ErrWorldInvalid):
		return -32602
	case errors.Is(err, ErrEULARequired):
		return codeEULARequired
	case errors.Is(err, ErrWorldBackendUnavailable):
		return codeBackendAbsent
	}
	return codeWorldFailed
}

// worldErrorMessage exposes only sentinel-class messages; other errors may carry local paths.
func worldErrorMessage(err error) string {
	for _, known := range []error{ErrWorldNotFound, ErrWorldBusy, ErrWorldInUse, ErrWorldNotOpen, ErrWorldRuntimePending, ErrEULARequired, ErrWorldBackendUnavailable} {
		if errors.Is(err, known) {
			return known.Error()
		}
	}
	if errors.Is(err, ErrWorldInvalid) {
		return err.Error()
	}
	return "world operation failed"
}
