package control

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

const (
	methodLocalWorld  = bridgecontract.MethodLocalWorld
	methodWorldInvite = bridgecontract.MethodWorldInvite

	codeWorldFailed = bridgecontract.RPCWorldFailed
	codeWorldBusy   = bridgecontract.RPCWorldBusy
)

// Local-world failures the world methods report by code; a sentinel's message is safe to show.
var (
	ErrWorldInvalid = errors.New("invalid local world report") // its wrapping message names the bad field
	ErrWorldNotOpen = errors.New("no world is open")
)

// Worlds takes the launcher's open local world, which it runs, behind local_world.v1.
type Worlds interface {
	SetLocal(LocalWorld) error
}

// Inviter sends Xbox Live invites to the open world while it is hosted for friends.
type Inviter interface {
	Invite(ctx context.Context, xuid string) error
}

// LocalWorld is the launcher's report of its open world; Error never carries paths.
type LocalWorld struct {
	State  string            `json:"state"`
	World  *LocalWorldInfo   `json:"world,omitempty"`  // set unless idle
	Target *LocalWorldTarget `json:"target,omitempty"` // set while running
	Error  string            `json:"error,omitempty"`
}

// LocalWorldInfo identifies the open world to Xbox friends.
type LocalWorldInfo struct {
	ID                  string `json:"id"`
	Name                string `json:"name"`
	GameMode            string `json:"game_mode"`
	Backend             string `json:"backend"`
	MultiplayerDisabled bool   `json:"multiplayer_disabled,omitempty"`
}

// LocalWorldTarget is the running world's loopback server and how to dial it.
type LocalWorldTarget struct {
	Address    string `json:"address"`
	Transport  string `json:"transport"`
	LANAddress string `json:"lan_address,omitempty"`
	LevelName  string `json:"level_name,omitempty"`
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
		var report LocalWorld
		if !decodeParams(raw, &report) {
			return reply.invalid()
		}
		err = worlds.SetLocal(report)
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
	return reply.ok(&WorldResultV1{SchemaVersion: bridgecontract.ControlSchemaVersion})
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
	case errors.Is(err, ErrWorldNotOpen):
		return codeWorldBusy
	case errors.Is(err, ErrWorldInvalid):
		return bridgecontract.RPCInvalidParams
	}
	return codeWorldFailed
}

// worldErrorMessage exposes only sentinel-class messages; other errors may carry local paths.
func worldErrorMessage(err error) string {
	switch {
	case errors.Is(err, ErrWorldNotOpen):
		return ErrWorldNotOpen.Error()
	case errors.Is(err, ErrWorldInvalid):
		return err.Error()
	}
	return "world operation failed"
}
