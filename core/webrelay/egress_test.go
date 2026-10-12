package webrelay

import (
	"context"
	"errors"
	"net"
	"net/netip"
	"testing"
)

func TestPublicUpstreamRefusesNonPublicRanges(t *testing.T) {
	blocked := []string{
		"0.0.0.0", "0.1.2.3", "10.0.0.1", "100.64.0.1", "127.0.0.1", "169.254.169.254",
		"172.16.0.1", "192.0.0.1", "192.0.2.1", "192.88.99.1", "192.168.1.1", "198.18.0.1",
		"198.51.100.1", "203.0.113.1", "224.0.0.1", "240.0.0.1", "255.255.255.255",
		"::", "::1", "::ffff:127.0.0.1", "::ffff:10.0.0.1", "64:ff9b::a00:1", "64:ff9b:1::1",
		"100::1", "2001::1", "2001:db8::1", "2002:a00:1::1", "fc00::1", "fd12::1", "fe80::1",
		"fec0::1", "ff02::1",
	}
	for _, text := range blocked {
		if publicUpstream(netip.MustParseAddr(text)) {
			t.Errorf("publicUpstream(%s) = true, want false", text)
		}
	}
	for _, text := range []string{"1.1.1.1", "8.8.8.8", "104.16.0.1", "2606:4700::1111", "::ffff:8.8.8.8"} {
		if !publicUpstream(netip.MustParseAddr(text)) {
			t.Errorf("publicUpstream(%s) = false, want true", text)
		}
	}
}

func TestUpstreamDialerRefusesLoopbackAfterResolution(t *testing.T) {
	listener, err := net.ListenPacket("udp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	// "localhost" resolves to loopback, so the refusal must come from the resolved address.
	_, port, _ := net.SplitHostPort(listener.LocalAddr().String())
	_, err = newUpstreamDialer(false).DialContext(context.Background(), "udp", net.JoinHostPort("localhost", port))
	if !errors.Is(err, errBlockedUpstream) {
		t.Fatalf("dial localhost: got %v, want errBlockedUpstream", err)
	}
	conn, err := newUpstreamDialer(true).DialContext(context.Background(), "udp", listener.LocalAddr().String())
	if err != nil {
		t.Fatalf("dial with private upstreams allowed: %v", err)
	}
	_ = conn.Close()
}
