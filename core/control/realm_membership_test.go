package control

import (
	"context"
	"net"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/catalog"
)

// membershipStub records whether resolving a code or accepting it was requested.
type membershipStub struct {
	stubServices
	accepted bool
	code     string
}

// RealmMembership returns an isolated invitation preview without an account service.
func (s *membershipStub) RealmMembership(_ context.Context, code string, accept bool) (catalog.Realm, error) {
	s.code, s.accepted = code, accept
	return catalog.Realm{Name: "Fixture Realm", Target: "realm_id/7"}, nil
}

func TestRealmMembershipSeparatesPreviewFromAcceptance(t *testing.T) {
	s := &membershipStub{}
	dir := startServices(t, NewStore(), s)
	for _, accept := range []bool{false, true} {
		params := `{"code":"fixture-code","accept":false}`
		if accept {
			params = `{"code":"fixture-code","accept":true}`
		}
		response := rpc(t, dir, "realm_membership.v1", params)
		if response.Error != nil {
			t.Fatalf("membership call failed: %+v", response.Error)
		}
		if s.code != "fixture-code" || s.accepted != accept {
			t.Fatalf("request: %q %v", s.code, s.accepted)
		}
	}
}

// blockingMembership observes cancellation propagated from a closed control stream.
type blockingMembership struct {
	stubServices
	started   chan struct{}
	cancelled chan struct{}
}

// RealmMembership waits until its request context is cancelled.
func (s *blockingMembership) RealmMembership(ctx context.Context, _ string, _ bool) (catalog.Realm, error) {
	close(s.started)
	<-ctx.Done()
	close(s.cancelled)
	return catalog.Realm{}, ctx.Err()
}

func TestRealmMembershipClosedStreamCancelsPreview(t *testing.T) {
	s := &blockingMembership{started: make(chan struct{}), cancelled: make(chan struct{})}
	serverConn, clientConn := net.Pipe()
	server := &Server{requestIOTimeout: time.Second}
	done := make(chan struct{})
	go func() {
		defer close(done)
		_, _ = server.serveRealmMembership(serverConn, []byte(`{"code":"fixture","accept":false}`), s)
		_ = serverConn.Close()
	}()
	<-s.started
	_ = clientConn.Close()
	<-s.cancelled
	<-done
}
