package proxy

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"io"
	"log/slog"
	"net"
	"testing"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/pion/webrtc/v4"
)

func TestFriendTransportAcceptsOffersWithOptionalIdentity(t *testing.T) {
	for _, test := range []struct {
		name                   string
		identified, mismatched bool
	}{
		{name: "without identity"},
		{name: "with identity", identified: true},
		{name: "identity bound to another key", identified: true, mismatched: true},
	} {
		name := test.name
		t.Run(name, func(t *testing.T) {
			log := slog.New(slog.DiscardHandler)
			hub := &memorySignalHub{peers: map[string]*memorySignaling{}}
			server, client := hub.peer("host"), hub.peer("friend")
			t.Cleanup(func() { _ = server.Close(); _ = client.Close() })
			var settings webrtc.SettingEngine
			settings.SetIncludeLoopbackCandidate(true)
			settings.SetIPFilter(func(ip net.IP) bool { return ip.IsLoopback() && ip.To4() != nil })
			api := webrtc.NewAPI(webrtc.WithSettingEngine(settings))
			network := friendNetwork(server, log)
			network.ListenConfig.API = api
			network.ListenConfig.DisableTrickleICE = true
			listener, err := network.Listen("")
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = listener.Close() })
			ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
			defer cancel()
			stop := context.AfterFunc(ctx, func() { _ = listener.Close() })
			defer stop()
			dialer := nethernet.Dialer{Log: log, API: api, DisableTrickleICE: true}
			if test.identified {
				key, err := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
				if err != nil {
					t.Fatal(err)
				}
				publicKey := &key.PublicKey
				if test.mismatched {
					other, err := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
					if err != nil {
						t.Fatal(err)
					}
					publicKey = &other.PublicKey
				}
				dialer.Identity = &nethernet.Identity{PrivateKey: key, Token: syntheticMultiplayerToken(t, publicKey), Domain: "https://authorization.example/"}
			}
			conn, err := dialer.DialContext(ctx, server.NetworkID(), client)
			if test.mismatched {
				if conn != nil {
					_ = conn.Close()
				}
				if !isIdentityNotAllowed(err) {
					t.Fatalf("invalid identity error = %v, want identity rejection", err)
				}
				return
			}
			if err != nil {
				t.Fatalf("friend transport rejected %s before Minecraft Login: %v", name, err)
			}
			t.Cleanup(func() { _ = conn.Close() })
			peer, err := listener.Accept()
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = peer.Close() })
			deadline, _ := ctx.Deadline()
			_ = conn.SetDeadline(deadline)
			_ = peer.SetDeadline(deadline)
			const payload = "friend login transport"
			if _, err := conn.Write([]byte(payload)); err != nil {
				t.Fatal(err)
			}
			buf := make([]byte, len(payload))
			if _, err := io.ReadFull(peer, buf); err != nil || string(buf) != payload {
				t.Fatalf("friend transport payload = %q, %v", buf, err)
			}
		})
	}
}
