package control

import (
	"context"
	"encoding/json"
	"errors"
	"net"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

const (
	methodRealmsList     = "realms_list.v1"
	methodFriendsList    = "friends_list.v1"
	methodConnect        = "connect.v1"
	methodPrepareConnect = "prepare_connect.v1"
	methodAccountStatus  = "account_status.v1"
	methodSignOut        = "sign_out.v1"
	methodEvents         = "events.v1"
	codeSignedOut        = -32020
	codeServiceFailed    = -32021
	codeInvalidTarget    = -32022
	codeServicesDisabled = -32023

	// serviceCallTimeout bounds one catalog or sign-out call.
	serviceCallTimeout = 45 * time.Second
	maxTargetValueLen  = 255
)

// Sign-in states reported in AuthV1.State.
const (
	AuthOffline      = "offline" // the core runs without a Microsoft account
	AuthSignedOut    = "signed_out"
	AuthAwaitingCode = "awaiting_code"
	AuthSignedIn     = "signed_in"
	AuthFailed       = "failed"
)

// Connect target kinds accepted by connect.v1.
const (
	TargetRakNet    = "raknet"
	TargetRealm     = "realm"
	TargetFriend    = "friend"
	TargetGathering = "gathering" // an experience ID, joined when the connect is selected
)

var (
	// ErrSignedOut is returned by Services when no Microsoft session is available.
	ErrSignedOut = errors.New("control: not signed in")
	// ErrInvalidTarget is returned by Services.Connect for a malformed target.
	ErrInvalidTarget = errors.New("control: invalid connect target")
)

// Services is the launcher backend behind the account, catalog and connect methods.
type Services interface {
	Realms(ctx context.Context) ([]catalog.Realm, error)
	Friends(ctx context.Context) ([]catalog.Friend, error)
	// Connect selects the upstream for the next client connection.
	Connect(ctx context.Context, kind, value string) error
	// SignOut deletes the cached Microsoft tokens.
	SignOut() error
}

// PrepareServices optionally prepares a selected transport without selecting a game session.
type PrepareServices interface {
	PrepareConnect(ctx context.Context, kind, value string) error
}

// AuthV1 is the sign-in state; secrets and raw errors never appear in it.
type AuthV1 struct {
	State           string `json:"state"`
	VerificationURI string `json:"verification_uri,omitempty"`
	UserCode        string `json:"user_code,omitempty"`
	Gamertag        string `json:"gamertag,omitempty"`
	Reason          string `json:"reason,omitempty"`
}

// DisconnectV1 is the server's most recent disconnect reason.
type DisconnectV1 struct {
	Reason   int32  `json:"reason"`
	Message  string `json:"message"`
	Sequence uint64 `json:"sequence"` // increases with every disconnect
}

// EventsV1 is the pollable event surface: auth state plus the newest disconnect and transfer.
type EventsV1 struct {
	SchemaVersion uint32        `json:"schema_version"`
	Auth          AuthV1        `json:"auth"`
	Disconnect    *DisconnectV1 `json:"disconnect,omitempty"`
	Transfer      *TransferV1   `json:"transfer,omitempty"`
	// Connect is the join's live stage while the core prepares it.
	Connect *proxy.ConnectProgress `json:"connect,omitempty"`
}

type accountResultV1 struct {
	SchemaVersion uint32 `json:"schema_version"`
	Account       AuthV1 `json:"account"`
}

type realmsResultV1 struct {
	SchemaVersion uint32          `json:"schema_version"`
	Realms        []catalog.Realm `json:"realms"`
}

type friendsResultV1 struct {
	SchemaVersion uint32           `json:"schema_version"`
	Friends       []catalog.Friend `json:"friends"`
}

type emptyResultV1 struct {
	SchemaVersion uint32 `json:"schema_version"`
}

func isServiceMethod(method string) bool {
	switch method {
	case methodRealmsList, methodFriendsList, methodConnect, methodPrepareConnect, methodAccountStatus, methodSignOut, methodEvents:
		return true
	}
	return isScreenMethod(method)
}

// SetAuth publishes the sign-in state.
func (store *Store) SetAuth(auth AuthV1) {
	store.mu.Lock()
	store.auth = auth
	store.mu.Unlock()
}

// Auth returns the sign-in state.
func (store *Store) Auth() AuthV1 {
	store.mu.RLock()
	defer store.mu.RUnlock()
	return store.auth
}

// ObserveDisconnect records the server's disconnect reason until the next admission attempt begins.
func (store *Store) ObserveDisconnect(info proxy.DisconnectInfo) {
	store.mu.Lock()
	store.disconnects++
	store.disconnect = &DisconnectV1{Reason: info.Reason, Message: info.Message, Sequence: store.disconnects}
	store.mu.Unlock()
}

// Events returns the pollable event snapshot.
func (store *Store) Events() EventsV1 {
	store.mu.RLock()
	defer store.mu.RUnlock()
	events := EventsV1{SchemaVersion: 1, Auth: store.auth}
	if store.disconnect != nil {
		pending := *store.disconnect
		events.Disconnect = &pending
	}
	if store.transfer != nil {
		pending := *store.transfer
		events.Transfer = &pending
	}
	if store.connect != nil {
		progress := *store.connect
		events.Connect = &progress
	}
	return events
}

func (server *Server) serveService(conn net.Conn, id uint64, method string, raw json.RawMessage) error {
	reply := responseWriter{server: server, conn: conn, id: id}

	switch method {
	case methodAccountStatus, methodEvents:
		if len(raw) != 0 {
			return reply.invalid()
		}
		if method == methodEvents {
			return reply.ok(server.store.Events())
		}
		return reply.ok(accountResultV1{SchemaVersion: 1, Account: server.store.Auth()})
	}
	services := server.launcherServices()
	if services == nil {
		return reply.fail(codeServicesDisabled, "Launcher services unavailable")
	}
	ctx, cancel := context.WithTimeout(context.Background(), serviceCallTimeout)
	defer cancel()
	failService := func(err error) error {
		switch {
		case errors.Is(err, ErrSignedOut):
			return reply.fail(codeSignedOut, "Not signed in")
		case errors.Is(err, ErrInvalidTarget):
			return reply.fail(codeInvalidTarget, "Invalid target")
		}
		server.logServiceFailure(method, err)
		return reply.fail(codeServiceFailed, "Service unavailable")
	}
	switch method {
	case methodRealmsList:
		if len(raw) != 0 {
			return reply.invalid()
		}
		realms, err := services.Realms(ctx)
		if err != nil {
			return failService(err)
		}
		if realms == nil {
			realms = []catalog.Realm{}
		}
		return reply.ok(realmsResultV1{SchemaVersion: 1, Realms: realms})
	case methodFriendsList:
		if len(raw) != 0 {
			return reply.invalid()
		}
		friends, err := services.Friends(ctx)
		if err != nil {
			return failService(err)
		}
		if friends == nil {
			friends = []catalog.Friend{}
		}
		return reply.ok(friendsResultV1{SchemaVersion: 1, Friends: friends})
	case methodConnect, methodPrepareConnect:
		var params struct {
			Kind  *string `json:"kind"`
			Value *string `json:"value"`
		}
		if !decodeParams(raw, &params) || params.Kind == nil || params.Value == nil {
			return reply.invalid()
		}
		cancelPreparation := method == methodPrepareConnect && *params.Kind == "" && *params.Value == ""
		if !cancelPreparation && (len(*params.Value) == 0 || len(*params.Value) > maxTargetValueLen) {
			return reply.fail(codeInvalidTarget, "Invalid target")
		}
		switch *params.Kind {
		case TargetRakNet, TargetRealm, TargetFriend, TargetGathering:
		case "":
			if !cancelPreparation {
				return reply.fail(codeInvalidTarget, "Invalid target")
			}
		default:
			return reply.fail(codeInvalidTarget, "Invalid target")
		}
		connect := services.Connect
		if method == methodPrepareConnect {
			preparation, supported := services.(PrepareServices)
			if !supported {
				return reply.fail(codeServicesDisabled, "Launcher services unavailable")
			}
			connect = preparation.PrepareConnect
		}
		if err := connect(ctx, *params.Kind, *params.Value); err != nil {
			return failService(err)
		}
		return reply.ok(emptyResultV1{SchemaVersion: 1})
	case methodSignOut:
		if len(raw) != 0 {
			return reply.invalid()
		}
		if err := services.SignOut(); err != nil {
			return failService(err)
		}
		return reply.ok(accountResultV1{SchemaVersion: 1, Account: server.store.Auth()})
	}
	if isScreenMethod(method) {
		screens, supported := services.(ScreenServices)
		if !supported {
			return reply.fail(codeServicesDisabled, "Launcher services unavailable")
		}
		result, err := screenResult(ctx, screens, method, raw)
		if errors.Is(err, errInvalidParams) {
			return reply.invalid()
		}
		if err != nil {
			return failService(err)
		}
		return reply.ok(result)
	}
	return reply.fail(-32601, "Method not found")
}
