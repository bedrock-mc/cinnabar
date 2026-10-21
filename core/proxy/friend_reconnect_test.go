package proxy

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"sync"
	"testing"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/pion/webrtc/v4"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// fakeFriendDirectory issues the same nonce to every XUID; its first failCloses closes fail.
type fakeFriendDirectory struct {
	mu         sync.Mutex
	failCloses int
	withdrawn  bool
}

func (*fakeFriendDirectory) nonce(string) (string, bool)          { return "nonce", true }
func (*fakeFriendDirectory) setMembers(int)                       {}
func (*fakeFriendDirectory) invite(context.Context, string) error { return nil }

func (d *fakeFriendDirectory) Close() error {
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.failCloses > 0 {
		d.failCloses--
		return errors.New("session directory unreachable")
	}
	d.withdrawn = true
	return nil
}

// isWithdrawn reports whether a close has succeeded.
func (d *fakeFriendDirectory) isWithdrawn() bool {
	d.mu.Lock()
	defer d.mu.Unlock()
	return d.withdrawn
}

// friendHostFixture relays friends signaled in memory into an offline loopback server.
type friendHostFixture struct {
	host        *FriendHost
	hub         *memorySignalHub
	api         *webrtc.API
	chat        chan string // chat lines the loopback server received
	mu          sync.Mutex
	opened      []*memorySignaling
	directories []*fakeFriendDirectory
	failCloses  int // failed closes for each new directory
	joined      int
}

func startFriendHostFixture(t *testing.T, failCloses int) *friendHostFixture {
	t.Helper()
	log := slog.New(slog.DiscardHandler)
	server, err := minecraft.ListenConfig{AuthenticationDisabled: true, ErrorLog: log}.Listen("raknet", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = server.Close() })
	f := &friendHostFixture{hub: &memorySignalHub{peers: map[string]*memorySignaling{}}, chat: make(chan string, 16), failCloses: failCloses}
	go f.serveWorld(server)
	var settings webrtc.SettingEngine
	settings.SetIncludeLoopbackCandidate(true)
	settings.SetIPFilter(func(ip net.IP) bool { return ip.IsLoopback() && ip.To4() != nil })
	f.api = webrtc.NewAPI(webrtc.WithSettingEngine(settings))
	f.host = newFriendHost("host", log, func(context.Context) (*friendEndpoint, error) {
		f.mu.Lock()
		signaling := f.hub.peer(fmt.Sprintf("host-%d", len(f.opened)))
		f.opened = append(f.opened, signaling)
		directory := &fakeFriendDirectory{failCloses: f.failCloses}
		f.directories = append(f.directories, directory)
		f.mu.Unlock()
		network := friendNetwork(signaling, log)
		network.ListenConfig.API = f.api
		network.ListenConfig.DisableTrickleICE = true
		return &friendEndpoint{network: network, directory: directory, closeSignaling: signaling.Close}, nil
	})
	f.host.prepared.resolveTarget = withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{Address: server.Addr().String(), Transport: localworld.TransportRakNet}, true, nil
	}, nil)
	production := f.host.config
	f.host.config = func(directory friendDirectory) minecraft.ListenConfig {
		config := production(directory)
		config.AuthenticationDisabled = true
		allow := config.Allow
		config.Allow = func(addr net.Addr, identity login.IdentityData, client login.ClientData) (string, bool) {
			identity.XUID = "friend" // offline logins carry no XUID
			return allow(addr, identity, client)
		}
		return config
	}
	if err := f.host.start(t.Context()); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = f.host.Close() })
	return f
}

// serveWorld spawns every relayed friend and records their chat.
func (f *friendHostFixture) serveWorld(server *minecraft.Listener) {
	for {
		accepted, err := server.Accept()
		if err != nil {
			return
		}
		go func(conn *minecraft.Conn) {
			defer conn.Close()
			if conn.StartGame(minecraft.GameData{WorldName: "Fixture", EntityUniqueID: 1, EntityRuntimeID: 1}) != nil {
				return
			}
			for {
				pk, err := conn.ReadPacket()
				if err != nil {
					return
				}
				if text, ok := pk.(*packet.Text); ok {
					f.chat <- text.Message
				}
			}
		}(accepted.(*minecraft.Conn))
	}
}

// loseSignaling ends the host's signaling connection and waits until its replacement is
// installed and listening.
func (f *friendHostFixture) loseSignaling(t *testing.T) {
	t.Helper()
	lost := f.host.current()
	f.mu.Lock()
	signaling := f.opened[len(f.opened)-1]
	f.mu.Unlock()
	_ = signaling.Close()
	testwait.Eventually(t, 10*time.Second, "replacement endpoint", func() bool { return f.host.current() != lost })
}

// join spawns a friend through the host's newest signaling connection.
func (f *friendHostFixture) join(t *testing.T) *minecraft.Conn {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
	defer cancel()
	f.mu.Lock()
	f.joined++
	client := f.hub.peer(fmt.Sprintf("friend-%d", f.joined))
	host := f.opened[len(f.opened)-1].NetworkID()
	f.mu.Unlock()
	t.Cleanup(func() { _ = client.Close() })
	network := minecraft.NetherNet{Signaling: client, Dialer: nethernet.Dialer{Log: slog.New(slog.DiscardHandler), API: f.api, DisableTrickleICE: true}}
	conn, err := minecraft.Dialer{
		IdentityData: login.IdentityData{DisplayName: client.NetworkID(), Identity: uuid.NewString()},
		ClientData:   login.ClientData{Nonce: "nonce"},
		ErrorLog:     slog.New(slog.DiscardHandler),
	}.DialContextNetwork(ctx, network, host)
	if err != nil {
		t.Fatalf("friend join: %v", err)
	}
	t.Cleanup(func() { _ = conn.Close() })
	if err := conn.DoSpawnContext(ctx); err != nil {
		t.Fatalf("friend spawn: %v", err)
	}
	return conn
}

// say sends a chat line and waits until the loopback server receives it.
func (f *friendHostFixture) say(t *testing.T, conn *minecraft.Conn, message string) {
	t.Helper()
	if err := conn.WritePacket(&packet.Text{TextType: packet.TextTypeChat, Message: message}); err != nil {
		t.Fatalf("send %q: %v", message, err)
	}
	if err := conn.Flush(); err != nil {
		t.Fatalf("send %q: %v", message, err)
	}
	var received []string
	testwait.Eventually(t, 10*time.Second, "server receives "+message, func() bool {
		select {
		case line := <-f.chat:
			received = append(received, line)
		default:
		}
		return len(received) > 0 && received[len(received)-1] == message
	})
}

// Losing signaling must not end friends already playing; new friends join through its replacement.
func TestFriendHostKeepsFriendsAcrossSignalingLoss(t *testing.T) {
	f := startFriendHostFixture(t, 0)
	first := f.join(t)
	f.say(t, first, "before")

	f.loseSignaling(t)
	select {
	case <-f.host.Done():
		t.Fatal("host stopped after losing signaling")
	default:
	}
	f.say(t, first, "after")
	f.say(t, f.join(t), "joined")
}

// A session whose withdrawal keeps failing is retried until it is withdrawn.
func TestFriendHostWithdrawsSessionsThatFailedToClose(t *testing.T) {
	f := startFriendHostFixture(t, 2)
	f.loseSignaling(t)
	f.mu.Lock()
	lost := f.directories[0]
	f.mu.Unlock()
	testwait.Eventually(t, 10*time.Second, "lost session withdrawn", lost.isWithdrawn)
}

// Admission consults the session its listener was opened for, which is not yet current while the
// listener starts accepting.
func TestFriendAdmissionUsesItsOwnSession(t *testing.T) {
	host := newFriendHost("host", slog.New(slog.DiscardHandler), nil)
	t.Cleanup(func() { _ = host.prepared.shutdown() })
	allow := host.config(&fakeFriendDirectory{}).Allow
	if reason, ok := allow(nil, login.IdentityData{XUID: "friend"}, login.ClientData{Nonce: "nonce"}); !ok {
		t.Fatalf("friend with its session's nonce refused: %s", reason)
	}
}
