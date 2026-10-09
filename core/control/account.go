package control

import (
	"bytes"
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
	methodAccountStatus  = "account_status.v1"
	methodSignOut        = "sign_out.v1"
	methodEvents         = "events.v1"
	methodServerTrust    = "server_trust_answer.v1"
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
	// ServerTrust is the join's pending question whether to trust a NetherNet server.
	ServerTrust *proxy.ServerTrustPrompt `json:"server_trust,omitempty"`
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
	case methodRealmMembership, methodRealmsList, methodFriendsList, methodFriendsPeople, methodConnect, methodAccountStatus, methodSignOut, methodEvents, methodServerTrust:
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
	if store.trustPrompt != nil {
		prompt := *store.trustPrompt
		events.ServerTrust = &prompt
	}
	return events
}

// ObserveServerTrust publishes a pending trust prompt, or withdraws it once it is no longer pending.
func (store *Store) ObserveServerTrust(prompt proxy.ServerTrustPrompt, pending bool) {
	store.mu.Lock()
	defer store.mu.Unlock()
	switch {
	case pending && (store.trustPrompt == nil || prompt.ID > store.trustPrompt.ID):
		store.trustPrompt = &prompt
	case store.trustPrompt != nil && store.trustPrompt.ID == prompt.ID:
		store.trustPrompt = nil
	}
}

// SetServerTrustAnswer installs the receiver of the client's trust answers.
func (store *Store) SetServerTrustAnswer(answer func(id uint64, trusted bool) bool) {
	store.mu.Lock()
	store.trustAnswer = answer
	store.mu.Unlock()
}

type serverTrustResultV1 struct {
	SchemaVersion uint32 `json:"schema_version"`
	Answered      bool   `json:"answered"` // false once the prompt is no longer pending
}

func (server *Server) serveServerTrust(reply responseWriter, raw json.RawMessage) error {
	var params struct {
		ID      *uint64 `json:"id"`
		Trusted *bool   `json:"trusted"`
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if len(raw) == 0 || decoder.Decode(&params) != nil || params.ID == nil || params.Trusted == nil {
		return reply.invalid()
	}
	server.store.mu.RLock()
	answer := server.store.trustAnswer
	server.store.mu.RUnlock()
	answered := answer != nil && answer(*params.ID, *params.Trusted)
	return reply.ok(serverTrustResultV1{SchemaVersion: 1, Answered: answered})
}

func (server *Server) serveService(conn net.Conn, id uint64, method string, raw json.RawMessage) error {
	reply := responseWriter{server: server, conn: conn, id: id}

	if method == methodServerTrust {
		return server.serveServerTrust(reply, raw)
	}
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
	case methodRealmMembership:
		result, err := server.serveRealmMembership(conn, raw, services)
		if errors.Is(err, errInvalidParams) {
			return reply.invalid()
		}
		if errors.Is(err, errMembershipUnavailable) {
			return reply.fail(codeServicesDisabled, "Realm membership unavailable")
		}
		if err != nil {
			return failService(err)
		}
		return reply.ok(result)
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
	case methodFriendsPeople:
		people, supported := services.(PeopleServices)
		if len(raw) != 0 {
			return reply.invalid()
		}
		if !supported {
			return reply.fail(codeServicesDisabled, "Launcher services unavailable")
		}
		list, err := people.People(ctx)
		if err != nil {
			return failService(err)
		}
		return reply.ok(peopleResult(list))
	case methodConnect:
		var params struct {
			Kind  *string `json:"kind"`
			Value *string `json:"value"`
		}
		if !decodeParams(raw, &params) || params.Kind == nil || params.Value == nil {
			return reply.invalid()
		}
		if len(*params.Value) == 0 || len(*params.Value) > maxTargetValueLen {
			return reply.fail(codeInvalidTarget, "Invalid target")
		}
		switch *params.Kind {
		case TargetRakNet, TargetRealm, TargetFriend, TargetGathering:
		default:
			return reply.fail(codeInvalidTarget, "Invalid target")
		}
		if err := services.Connect(ctx, *params.Kind, *params.Value); err != nil {
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
