package ingress

import (
	"net/netip"
	"sync"
	"time"
)

const maxViewers = 64
const maxIPViewers = 4
const maxPeers = 4096
const requestsPerMinute = 60

type peer struct {
	tokens  float64
	updated time.Time
	viewers int
}

type limits struct {
	mu      sync.Mutex
	peers   map[netip.Addr]*peer
	viewers int
}

func newLimits() *limits { return &limits{peers: make(map[netip.Addr]*peer)} }

func (l *limits) admit(ip netip.Addr, stream bool, now time.Time) (func(), bool) {
	l.mu.Lock()
	defer l.mu.Unlock()
	p := l.peers[ip]
	if p == nil {
		if len(l.peers) >= maxPeers {
			for address, existing := range l.peers {
				if existing.viewers == 0 && now.Sub(existing.updated) > 5*time.Minute {
					delete(l.peers, address)
				}
			}
			if len(l.peers) >= maxPeers {
				return nil, false
			}
		}
		p = &peer{tokens: requestsPerMinute, updated: now}
		l.peers[ip] = p
	}
	elapsed := now.Sub(p.updated).Seconds()
	if elapsed > 0 {
		p.tokens = min(requestsPerMinute, p.tokens+elapsed)
		p.updated = now
	}
	if p.tokens < 1 || (stream && (p.viewers >= maxIPViewers || l.viewers >= maxViewers)) {
		return nil, false
	}
	p.tokens--
	if !stream {
		return func() {}, true
	}
	p.viewers++
	l.viewers++
	var once sync.Once
	return func() {
		once.Do(func() {
			l.mu.Lock()
			defer l.mu.Unlock()
			p.viewers--
			l.viewers--
		})
	}, true
}
