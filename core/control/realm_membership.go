package control

import (
	"context"
	"encoding/json"
	"errors"
	"net"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/catalog"
)

const methodRealmMembership = "realm_membership.v1"

var errMembershipUnavailable = errors.New("control: Realm membership unavailable")

// RealmMembershipServices separates invitation preview from explicit membership acceptance.
type RealmMembershipServices interface {
	RealmMembership(context.Context, string, bool) (catalog.Realm, error)
}

// serveRealmMembership cancels the request when its one-shot control connection closes.
func (server *Server) serveRealmMembership(conn net.Conn, raw json.RawMessage, services Services) (any, error) {
	var params struct {
		Code   *string `json:"code"`
		Accept *bool   `json:"accept"`
	}
	if !decodeParams(raw, &params) || params.Code == nil || params.Accept == nil {
		return nil, errInvalidParams
	}
	code, err := catalog.RealmInviteCode(*params.Code)
	if err != nil {
		return nil, ErrInvalidTarget
	}
	membership, ok := services.(RealmMembershipServices)
	if !ok {
		return nil, errMembershipUnavailable
	}
	ctx, cancel := context.WithTimeout(context.Background(), serviceCallTimeout)
	defer cancel()
	if err := conn.SetReadDeadline(time.Time{}); err != nil {
		return nil, err
	}
	finished := make(chan struct{})
	go func() {
		defer close(finished)
		var extra [1]byte
		_, _ = conn.Read(extra[:])
		cancel()
	}()
	defer func() { _ = conn.SetReadDeadline(time.Now()); <-finished }()
	realm, err := membership.RealmMembership(ctx, code, *params.Accept)
	if err != nil {
		return nil, err
	}
	return struct {
		SchemaVersion uint32        `json:"schema_version"`
		Code          string        `json:"code"`
		Realm         catalog.Realm `json:"realm"`
	}{SchemaVersion: 1, Code: code, Realm: realm}, nil
}
