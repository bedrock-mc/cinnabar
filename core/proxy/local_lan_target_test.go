package proxy

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/df-mc/go-nethernet/discovery"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/sandertv/gophertunnel/minecraft/p2p"
)

type fixtureLANResponses struct {
	responses map[uint64][]byte
	ctx       context.Context
}

func (f fixtureLANResponses) Responses() map[uint64][]byte { return f.responses }
func (f fixtureLANResponses) Context() context.Context     { return f.ctx }

func TestLocalLANPeerRequiresAdvertisedOfflineAdmissionAndNonce(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name       string
		offline    bool
		nonce      string
		connection int32
		wantErr    bool
	}{
		{name: "offline advertised", offline: true, nonce: "fixture-nonce", connection: p2p.ConnectionTypeSignalingOverLAN},
		{name: "online only", nonce: "fixture-nonce", connection: p2p.ConnectionTypeSignalingOverLAN, wantErr: true},
		{name: "no nonce", offline: true, connection: p2p.ConnectionTypeSignalingOverLAN, wantErr: true},
		{name: "unsupported signaling", offline: true, nonce: "fixture-nonce", connection: p2p.ConnectionTypeSignalingOverWebSocket, wantErr: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			data := discovery.ServerData{LevelName: "selected", AcceptsSelfSignedAuth: test.offline, Nonce: test.nonce, ConnectionType: test.connection}
			response, err := data.MarshalBinary()
			if err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithTimeout(t.Context(), 10*time.Millisecond)
			defer cancel()
			id, got, err := findLocalLANPeer(ctx, fixtureLANResponses{responses: map[uint64][]byte{42: response}, ctx: t.Context()}, "selected")
			if (err != nil) != test.wantErr || (!test.wantErr && (id != 42 || got.Nonce != test.nonce)) {
				t.Fatalf("peer = %d, nonce_present=%v, %v", id, got.Nonce != "", err)
			}
		})
	}
}

func TestLocalLANPeerSkipsMalformedAndUnselectedMetadata(t *testing.T) {
	t.Parallel()
	data := discovery.ServerData{LevelName: "other world", AcceptsSelfSignedAuth: true, Nonce: "fixture", ConnectionType: p2p.ConnectionTypeSignalingOverLAN}
	response, err := data.MarshalBinary()
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	_, _, err = findLocalLANPeer(ctx, fixtureLANResponses{responses: map[uint64][]byte{1: {}, 2: response}, ctx: t.Context()}, "selected")
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("unselected/malformed peer = %v", err)
	}
}

func TestLocalLANTargetRejectsMissingLevelAndNonLoopbackWithoutDiscovery(t *testing.T) {
	t.Parallel()
	for _, target := range []localworld.ConnectionTarget{
		{LANAddress: "127.0.0.1:5000"},
		{LANAddress: "example.test:5000", LevelName: "selected"},
		{LANAddress: "192.0.2.10:5000", LevelName: "selected"},
		{LANAddress: "127.0.0.1:0", LevelName: "selected"},
		{LANAddress: "127.0.0.1:65536", LevelName: "selected"},
	} {
		if got, err := resolveLocalLANTarget(t.Context(), target); err == nil || got != nil {
			t.Fatalf("invalid target = %+v, %v", got, err)
		}
	}
}

func TestLocalLANCancelledResolutionDoesNotStartDiscovery(t *testing.T) {
	t.Parallel()
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	_, err := resolveLocalLANTarget(ctx, localworld.ConnectionTarget{LANAddress: "127.0.0.1:5000", LevelName: "selected"})
	if !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}
