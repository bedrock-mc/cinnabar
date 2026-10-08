package localworld

import (
	"context"
	"fmt"
	"net"
	"slices"
	"strconv"
	"strings"
	"testing"

	"github.com/df-mc/go-nethernet/discovery"
)

func TestBDSNetherNetPortPublicationMatchesAdvertisedCandidates(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name    string
		players int
	}{
		{name: "single player", players: 1},
		{name: "shared local comparison", players: 8},
	} {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			const port = 5000
			args := containerArgs(testSpec(), testImage, "fixture-version", t.TempDir(), port, test.players, false, 0)
			httpMapping := fmt.Sprintf("%s:%d:%d/tcp", localServerHost, port, bdsContainerHTTPPort)
			udpMapping := fmt.Sprintf("%s:%s:%s/udp", localServerHost, udpPortRange(port, test.players), udpPortRange(bdsContainerUDPPort, test.players))
			candidateMapping := "SERVER_UDP_PORTS=" + bdsUDPMapping(port, bdsContainerUDPPort, test.players)
			for _, want := range []string{httpMapping, udpMapping, candidateMapping, "TRANSPORT=" + string(TransportNetherNetHTTP), "ONLINE_MODE=false"} {
				if !slices.Contains(args, want) {
					t.Fatalf("container arguments omit %q: %v", want, args)
				}
			}
			if !slices.Contains(args, "--health-cmd") || !strings.Contains(strings.Join(args, " "), "/v1/join") {
				t.Fatal("NetherNet health must check HTTP status, not RakNet pong")
			}
			props := string(serverProperties(testSpec(), port, test.players, false))
			if !strings.Contains(props, "server-udp-ports="+bdsUDPMapping(port, port, test.players)+"\n") {
				t.Fatalf("native UDP allocation omitted player-sized loopback candidates:\n%s", props)
			}
			if strings.Contains(props, "server-portv6=") {
				t.Fatal("NetherNet ignores the obsolete separate IPv6 RakNet port")
			}
		})
	}
}

func TestBDSLANVisibilityIsExplicitAndContainerDiscoveryIsLoopbackOnly(t *testing.T) {
	t.Parallel()
	for _, visible := range []bool{false, true} {
		args := containerArgs(testSpec(), testImage, "fixture-version", t.TempDir(), DefaultBDSPort, 1, visible, 0)
		mapping := fmt.Sprintf("%s:%d:%d/udp", localServerHost, discovery.DefaultPort, discovery.DefaultPort)
		if slices.Contains(args, mapping) != visible {
			t.Fatalf("LAN discovery publication = %v, want %v: %v", slices.Contains(args, mapping), visible, args)
		}
		if !slices.Contains(args, "ENABLE_LAN_VISIBILITY="+strconv.FormatBool(visible)) {
			t.Fatal("container LAN property must match the explicit option")
		}
		props := string(serverProperties(testSpec(), DefaultBDSPort, 1, visible))
		if !strings.Contains(props, "enable-lan-visibility="+strconv.FormatBool(visible)+"\n") {
			t.Fatal("native LAN property must match the explicit option")
		}
	}
}

func TestBDSAlternateLANHostPortAndValidation(t *testing.T) {
	t.Parallel()
	lanPort := discovery.DefaultPort + 1
	args := containerArgs(testSpec(), testImage, "fixture-version", t.TempDir(), DefaultBDSPort, 8, true, lanPort)
	want := fmt.Sprintf("%s:%d:%d/udp", localServerHost, lanPort, discovery.DefaultPort)
	if !slices.Contains(args, want) {
		t.Fatalf("alternate LAN host mapping omitted %q", want)
	}
	for _, invalid := range []int{0, -1, maxTransportPort + 1, DefaultBDSPort, DefaultBDSPort + 7} {
		if err := checkBDSLANPort(DefaultBDSPort, 8, invalid); err == nil {
			t.Fatalf("invalid or overlapping LAN port %d was accepted", invalid)
		}
	}
	listener, err := net.ListenPacket("udp", net.JoinHostPort(localServerHost, "0"))
	if err != nil {
		t.Fatal(err)
	}
	port := portOf(listener.LocalAddr().String())
	if err := checkBDSLANPort(DefaultBDSPort, 8, port); err == nil {
		listener.Close()
		t.Fatal("occupied LAN discovery port must fail before Docker launch")
	}
	if err := listener.Close(); err != nil {
		t.Fatal(err)
	}
	if err := checkBDSLANPort(DefaultBDSPort, 8, port); err != nil {
		t.Fatal(err)
	}
}

func TestFreeBDSAddressReservesBothProtocolsAndReleasesAllPorts(t *testing.T) {
	t.Parallel()
	const players = 3
	address, err := freeBDSAddress(players, 0)
	if err != nil {
		t.Fatal(err)
	}
	tcp, err := net.Listen("tcp", address)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = tcp.Close() })
	for offset := range players {
		udp, err := net.ListenPacket("udp", net.JoinHostPort(localServerHost, strconv.Itoa(portOf(address)+offset)))
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = udp.Close() })
	}
	for _, invalid := range []int{0, -1, maxTransportPort} {
		if _, err := freeBDSAddress(invalid, 0); err == nil {
			t.Fatalf("player count %d must not allocate a wrapping or empty UDP window", invalid)
		}
	}
}

func TestFixedBDSHostPortReservesExactWindowAndRejectsCollisions(t *testing.T) {
	const players = 3
	address, err := freeBDSAddress(players, 0)
	if err != nil {
		t.Fatal(err)
	}
	port := portOf(address)
	fixed, err := freeBDSAddress(players, port)
	if err != nil || fixed != address {
		t.Fatalf("fixed address = %q, %v; want %q", fixed, err, address)
	}
	t.Run("TCP collision", func(t *testing.T) {
		listener, err := net.Listen("tcp", address)
		if err != nil {
			t.Fatal(err)
		}
		defer listener.Close()
		if _, err := freeBDSAddress(players, port); err == nil {
			t.Fatal("occupied fixed TCP port must fail instead of selecting another port")
		}
	})
	t.Run("UDP collision releases reservations", func(t *testing.T) {
		udp, err := net.ListenPacket("udp", net.JoinHostPort(localServerHost, strconv.Itoa(port+players-1)))
		if err != nil {
			t.Fatal(err)
		}
		defer udp.Close()
		if _, err := freeBDSAddress(players, port); err == nil || !strings.Contains(err.Error(), "UDP transport") {
			t.Fatalf("occupied fixed UDP window must fail, got %v", err)
		}
		listener, err := net.Listen("tcp", address)
		if err != nil {
			t.Fatalf("failed UDP allocation leaked its TCP reservation: %v", err)
		}
		defer listener.Close()
		firstUDP, err := net.ListenPacket("udp", address)
		if err != nil {
			t.Fatalf("failed UDP allocation leaked a prior UDP reservation: %v", err)
		}
		defer firstUDP.Close()
	})
	for _, invalid := range []int{-1, maxTransportPort + 1, maxTransportPort} {
		if _, err := freeBDSAddress(players, invalid); err == nil {
			t.Fatalf("invalid or wrapping fixed host port %d must fail", invalid)
		}
	}
}

func TestConnectionTargetKeepsLocalBackendTransport(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name      string
		backend   string
		transport Transport
	}{
		{name: "official bds", backend: BackendBDS, transport: TransportNetherNetHTTP},
		{name: "dragonfly", backend: BackendDragonfly, transport: TransportRakNet},
	} {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			m := NewManager(newTestStore(t), &fakeRunner{}, nil)
			m.state = StateRunning
			m.world = World{Backend: test.backend}
			m.inst = &fakeInstance{}
			target, ok, err := m.ConnectionTarget(context.Background())
			if err != nil || !ok || target.Transport != test.transport || target.Address != m.inst.Address() {
				t.Fatalf("local target = %+v, %v, %v", target, ok, err)
			}
		})
	}
}

func TestConnectionTargetUsesExplicitBDSLANEndpointAndSelectedLevel(t *testing.T) {
	t.Parallel()
	m := NewManager(newTestStore(t), &fakeRunner{}, nil)
	m.state = StateRunning
	m.world = World{ID: "selected-level", Backend: BackendBDS}
	m.inst = &containerInstance{Instance: &fakeInstance{}, runner: BDSRunner{LANVisible: true, LANHostPort: discovery.DefaultPort + 1}}
	target, ok, err := m.ConnectionTarget(t.Context())
	if err != nil || !ok || target.Transport != TransportNetherNetLAN || target.LANAddress != bdsLANAddress(discovery.DefaultPort+1) || target.LevelName != m.world.ID || target.Address != m.inst.Address() {
		t.Fatalf("explicit LAN target = %+v, %v, %v", target, ok, err)
	}
}
