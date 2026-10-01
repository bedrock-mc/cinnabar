package ingress

import (
	"fmt"
	"net/netip"
	"sync"
	"testing"
	"time"
)

func TestGlobalAndPerIPStreamLimitsReleasePrecisely(t *testing.T) {
	l := newLimits()
	now := time.Now()
	ip := netip.MustParseAddr("203.0.113.1")
	var releases []func()
	for i := 0; i < maxIPViewers; i++ {
		release, ok := l.admit(ip, true, now)
		if !ok {
			t.Fatal("early per-IP refusal")
		}
		releases = append(releases, release)
	}
	if _, ok := l.admit(ip, true, now); ok {
		t.Fatal("per-IP limit exceeded")
	}
	releases[0]()
	releases[0]()
	release, ok := l.admit(ip, true, now)
	if !ok {
		t.Fatal("released slot not reusable")
	}
	releases = append(releases, release)
	for i := maxIPViewers; i < maxViewers; i++ {
		release, ok := l.admit(netip.MustParseAddr(fmt.Sprintf("203.0.113.%d", i)), true, now)
		if !ok {
			t.Fatal("early global refusal")
		}
		releases = append(releases, release)
	}
	if _, ok := l.admit(netip.MustParseAddr("203.0.113.200"), true, now); ok {
		t.Fatal("global viewer limit exceeded")
	}
	for _, release := range releases {
		release()
	}
	if l.viewers != 0 {
		t.Fatal("viewer slots leaked")
	}
}

func TestHandshakeRateRefillAndBoundedPeerLedger(t *testing.T) {
	l := newLimits()
	now := time.Now()
	ip := netip.MustParseAddr("203.0.113.1")
	for i := 0; i < requestsPerMinute; i++ {
		if _, ok := l.admit(ip, false, now); !ok {
			t.Fatal("early rate refusal")
		}
	}
	if _, ok := l.admit(ip, false, now); ok {
		t.Fatal("rate limit exceeded")
	}
	if _, ok := l.admit(ip, false, now.Add(time.Second)); !ok {
		t.Fatal("token not refilled")
	}
	for i := 1; len(l.peers) < maxPeers; i++ {
		address := netip.AddrFrom4([4]byte{10, byte(i >> 16), byte(i >> 8), byte(i)})
		if _, ok := l.admit(address, false, now); !ok {
			t.Fatal("peer allocation failed")
		}
	}
	if _, ok := l.admit(netip.MustParseAddr("192.0.2.1"), false, now); ok {
		t.Fatal("peer ledger exceeded capacity")
	}
	if _, ok := l.admit(netip.MustParseAddr("192.0.2.1"), false, now.Add(6*time.Minute)); !ok {
		t.Fatal("idle peers not reclaimed")
	}
}

func TestConcurrentAdmissionsAndReleases(t *testing.T) {
	l := newLimits()
	var group sync.WaitGroup
	for i := 1; i <= 20; i++ {
		group.Add(1)
		go func(i int) {
			defer group.Done()
			ip := netip.AddrFrom4([4]byte{203, 0, 113, byte(i)})
			for j := 0; j < 100; j++ {
				if release, ok := l.admit(ip, true, time.Now()); ok {
					release()
					release()
				}
			}
		}(i)
	}
	group.Wait()
	if l.viewers != 0 {
		t.Fatal("concurrent admissions leaked slots")
	}
}
