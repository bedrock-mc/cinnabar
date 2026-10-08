package localworld

import (
	"fmt"
	"net"
	"strconv"

	"github.com/df-mc/go-nethernet/discovery"
)

func bdsLANAddress(port int) string {
	return net.JoinHostPort(localServerHost, strconv.Itoa(orDefault(port, discovery.DefaultPort)))
}

const (
	// DefaultBDSPort is BDS's conventional TCP signaling port.
	DefaultBDSPort       = 19132
	localServerHost      = "127.0.0.1"
	bdsContainerHTTPPort = DefaultBDSPort
	bdsContainerUDPPort  = 32000
	maxTransportPort     = 1<<16 - 1
)

func checkBDSLANPort(hostPort, players, lanPort int) error {
	if lanPort < 1 || lanPort > maxTransportPort || (lanPort >= hostPort && lanPort < hostPort+players) {
		return fmt.Errorf("localworld: LAN discovery port %d is invalid or overlaps the BDS UDP window", lanPort)
	}
	conn, err := net.ListenPacket("udp", net.JoinHostPort(localServerHost, strconv.Itoa(lanPort)))
	if err != nil {
		return fmt.Errorf("localworld: reserve LAN discovery port %d: %w", lanPort, err)
	}
	return conn.Close()
}

func udpPortRange(base, count int) string {
	if count == 1 {
		return strconv.Itoa(base)
	}
	return fmt.Sprintf("%d-%d", base, base+count-1)
}

// BDS documents [ip:]external:internal; Docker's address must not leak into the SDP answer.
func bdsUDPMapping(external, internal, count int) string {
	return localServerHost + ":" + udpPortRange(external, count) + ":" + udpPortRange(internal, count)
}

// freeBDSAddress reserves the requested TCP/UDP window, or selects one when port is zero.
// Ports are released before server startup, as for the existing RakNet runner.
func freeBDSAddress(players, port int) (string, error) {
	if players < 1 || players > maxTransportPort-bdsContainerUDPPort+1 {
		return "", fmt.Errorf("localworld: player limit %d does not fit the NetherNet UDP allocation window", players)
	}
	if port < 0 || port > maxTransportPort || (port != 0 && port > maxTransportPort-players+1) {
		return "", fmt.Errorf("localworld: host port %d does not fit the NetherNet UDP allocation window for %d players", port, players)
	}
	if port != 0 {
		return availableBDSAddress(players, port)
	}
	for range 32 {
		address, err := availableBDSAddress(players, port)
		if err != nil {
			return "", err
		}
		if address != "" {
			return address, nil
		}
	}
	return "", fmt.Errorf("localworld: no available loopback NetherNet port range for %d players", players)
}

func availableBDSAddress(players, requestedPort int) (string, error) {
	tcp, err := net.Listen("tcp", net.JoinHostPort(localServerHost, strconv.Itoa(requestedPort)))
	if err != nil {
		return "", fmt.Errorf("localworld: reserve signaling port: %w", err)
	}
	defer tcp.Close()
	address := tcp.Addr().String()
	port := portOf(address)
	if port > maxTransportPort-players+1 {
		return "", nil
	}
	udp := make([]net.PacketConn, 0, players)
	defer func() {
		for _, conn := range udp {
			_ = conn.Close()
		}
	}()
	for offset := range players {
		conn, err := net.ListenPacket("udp", net.JoinHostPort(localServerHost, strconv.Itoa(port+offset)))
		if err != nil {
			if requestedPort != 0 {
				return "", fmt.Errorf("localworld: reserve UDP transport port %d: %w", port+offset, err)
			}
			return "", nil
		}
		udp = append(udp, conn)
	}
	return address, nil
}
