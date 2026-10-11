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
	methodLocalWorld  = "local_world.v1"
	methodWorldInvite = "world_invite.v1"

	codeWorldFailed = -32000
	codeWorldBusy   = -32011
)

// Worlds receives the launcher's open local world behind local_world.v1; *localworld.Local implements it.
type Worlds interface {
	SetLocal(status localworld.Status) (opened bool, err error)
}

type openHookWorlds struct {
	Worlds
	onOpen func()
}

func (w openHookWorlds) SetLocal(status localworld.Status) (bool, error) {
	opened, err := w.Worlds.SetLocal(status)
	if opened {
		w.onOpen()
	}
	return opened, err
}

// WithOpenHook returns worlds that also call onOpen each time a world begins opening.
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

var worldMethods = map[string]struct{}{methodLocalWorld: {}, methodWorldInvite: {}}

func isWorldMethod(method string) bool {
	_, ok := worldMethods[method]
	return ok
}

// WorldResultV1 is the result of the world methods.
type WorldResultV1 struct {
	SchemaVersion uint32 `json:"schema_version"`
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
	var err error
	switch method {
	case methodLocalWorld:
		var status localworld.Status
		if !decodeParams(raw, &status) {
			return reply.invalid()
		}
		_, err = worlds.SetLocal(status)
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
	return reply.ok(&WorldResultV1{SchemaVersion: 1})
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
	case errors.Is(err, localworld.ErrNotOpen):
		return codeWorldBusy
	case errors.Is(err, localworld.ErrInvalid):
		return -32602
	}
	return codeWorldFailed
}

// worldErrorMessage exposes only sentinel-class messages; other errors may carry local paths.
func worldErrorMessage(err error) string {
	switch {
	case errors.Is(err, localworld.ErrNotOpen):
		return localworld.ErrNotOpen.Error()
	case errors.Is(err, localworld.ErrInvalid):
		return err.Error()
	}
	return "world operation failed"
}
