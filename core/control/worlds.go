package control

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/localworld"
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

// Worlds is the local-world service behind the world_* methods; *localworld.Manager implements it.
type Worlds interface {
	List() ([]localworld.World, error)
	Create(localworld.Spec) (localworld.World, error)
	Update(id string, update localworld.Update) (localworld.World, error)
	Delete(id string) error
	Open(id string, opts ...localworld.OpenOptions) error
	AcceptEULA() error
	Prefs(ctx context.Context, update localworld.PrefsUpdate) (localworld.Prefs, error)
	Close() error
	SetPaused(paused bool) error
	Status() localworld.Status
}

type openHookWorlds struct {
	Worlds
	onOpen func()
}

func (w openHookWorlds) Open(id string, opts ...localworld.OpenOptions) error {
	err := w.Worlds.Open(id, opts...)
	if err == nil {
		w.onOpen()
	}
	return err
}

// WithOpenHook returns worlds that also call onOpen after each successful Open.
func WithOpenHook(worlds Worlds, onOpen func()) Worlds {
	return openHookWorlds{Worlds: worlds, onOpen: onOpen}
}

// Inviter sends Xbox Live invites to the open world while it is hosted for friends.
type Inviter interface {
	Invite(ctx context.Context, xuid string) error
}

type invitingWorlds struct {
	Worlds
	invite func(context.Context, string) error
}

func (w invitingWorlds) Invite(ctx context.Context, xuid string) error { return w.invite(ctx, xuid) }

// WithInvites returns worlds that also serve world_invite.v1 through invite.
func WithInvites(worlds Worlds, invite func(context.Context, string) error) Worlds {
	return invitingWorlds{Worlds: worlds, invite: invite}
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
	SchemaVersion uint32             `json:"schema_version"`
	Worlds        []localworld.World `json:"worlds,omitempty"`
	World         *localworld.World  `json:"world,omitempty"`
	Status        *localworld.Status `json:"status,omitempty"`
	Prefs         *localworld.Prefs  `json:"prefs,omitempty"`
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
	result := &WorldResultV1{SchemaVersion: 1}
	var err error
	switch method {
	case methodWorldList, methodWorldClose, methodWorldStatus:
		if len(raw) != 0 {
			return reply.invalid()
		}
	}
	switch method {
	case methodWorldList:
		result.Worlds, err = worlds.List()
		if len(result.Worlds) > maxListedWorlds {
			result.Worlds = result.Worlds[:maxListedWorlds]
		}
	case methodWorldCreate:
		var spec localworld.Spec
		if !decodeParams(raw, &spec) {
			return reply.invalid()
		}
		var world localworld.World
		if world, err = worlds.Create(spec); err == nil {
			result.World = &world
		}
	case methodWorldUpdate:
		var params struct {
			ID *string `json:"id"`
			localworld.Update
		}
		if !decodeParams(raw, &params) || params.ID == nil ||
			(params.Name == nil && params.GameMode == nil && params.Difficulty == nil) {
			return reply.invalid()
		}
		var world localworld.World
		if world, err = worlds.Update(*params.ID, params.Update); err == nil {
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
			err = worlds.Delete(*params.ID)
		} else {
			err = worlds.Open(*params.ID, localworld.OpenOptions{ViewDistance: params.ViewDistance})
		}
	case methodPrefs:
		var params struct {
			DockerPromptDismissed *bool   `json:"docker_prompt_dismissed"`
			CreationBackend       *string `json:"creation_backend"`
			CreationGenerator     *string `json:"creation_generator"`
			Redetect              bool    `json:"redetect"`
		}
		if len(raw) != 0 && !decodeParams(raw, &params) {
			return reply.invalid()
		}
		ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		prefs, prefsErr := worlds.Prefs(ctx, localworld.PrefsUpdate{DockerPromptDismissed: params.DockerPromptDismissed, CreationBackend: params.CreationBackend, CreationGenerator: params.CreationGenerator, Redetect: params.Redetect})
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
		err = worlds.AcceptEULA()
	case methodWorldClose:
		err = worlds.Close()
	case methodWorldPause:
		var params struct {
			Paused *bool `json:"paused"`
		}
		if !decodeParams(raw, &params) || params.Paused == nil {
			return reply.invalid()
		}
		err = worlds.SetPaused(*params.Paused)
	case methodWorldInvite:
		var params struct {
			XUID string `json:"xuid"`
		}
		if !decodeParams(raw, &params) || !validXUID(params.XUID) {
			return reply.invalid()
		}
		inviter, ok := worlds.(Inviter)
		if !ok {
			err = localworld.ErrNotOpen
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
	case errors.Is(err, localworld.ErrNotFound):
		return codeWorldNotFound
	case errors.Is(err, localworld.ErrBusy), errors.Is(err, localworld.ErrInUse), errors.Is(err, localworld.ErrNotOpen), errors.Is(err, localworld.ErrRuntimePending):
		return codeWorldBusy
	case errors.Is(err, localworld.ErrInvalid):
		return -32602
	case errors.Is(err, localworld.ErrEULARequired):
		return codeEULARequired
	case errors.Is(err, localworld.ErrBackendUnavailable):
		return codeBackendAbsent
	}
	return codeWorldFailed
}

// worldErrorMessage exposes only sentinel-class messages; other errors may carry local paths.
func worldErrorMessage(err error) string {
	for _, known := range []error{localworld.ErrNotFound, localworld.ErrBusy, localworld.ErrInUse, localworld.ErrNotOpen, localworld.ErrRuntimePending, localworld.ErrEULARequired, localworld.ErrBackendUnavailable} {
		if errors.Is(err, known) {
			return known.Error()
		}
	}
	if errors.Is(err, localworld.ErrInvalid) {
		return err.Error()
	}
	return "world operation failed"
}
