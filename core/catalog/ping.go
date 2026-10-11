package catalog

import (
	"context"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"net"
	"strings"
	"sync"
	"time"

	"github.com/sandertv/go-raknet"
	"github.com/sandertv/gophertunnel/minecraft"
)

// PingResult is one server's RakNet pong as the server rows show it.
type PingResult struct {
	Address    string `json:"address"`
	Online     bool   `json:"online"`
	Players    int    `json:"players"`
	MaxPlayers int    `json:"max_players"`
	PingMillis int64  `json:"ping_ms"`
	MOTD       string `json:"motd,omitempty"`
}

const (
	// MaxPingTargets bounds one ping request.
	MaxPingTargets = bridgecontract.MaxPingTargets
	pingTimeout    = 2 * time.Second
	pingWorkers    = 8
	defaultPort    = "19132"
)

// PingServers pings each address concurrently; unreachable servers come back offline.
func PingServers(ctx context.Context, addresses []string) []PingResult {
	return pingWith(ctx, addresses, raknet.PingContext)
}

func pingWith(ctx context.Context, addresses []string, ping func(context.Context, string) ([]byte, error)) []PingResult {
	if len(addresses) > MaxPingTargets {
		addresses = addresses[:MaxPingTargets]
	}
	results := make([]PingResult, len(addresses))
	jobs := make(chan int)
	var wait sync.WaitGroup
	for range min(pingWorkers, len(addresses)) {
		wait.Add(1)
		go func() {
			defer wait.Done()
			for index := range jobs {
				results[index] = pingOne(ctx, addresses[index], ping)
			}
		}()
	}
	for index := range addresses {
		jobs <- index
	}
	close(jobs)
	wait.Wait()
	return results
}

func pingOne(ctx context.Context, address string, ping func(context.Context, string) ([]byte, error)) PingResult {
	result := PingResult{Address: address}
	target := withDefaultPort(strings.TrimSpace(address))
	if target == "" {
		return result
	}
	pingContext, cancel := context.WithTimeout(ctx, pingTimeout)
	defer cancel()
	started := time.Now()
	data, err := ping(pingContext, target)
	if err != nil {
		return result
	}
	result.PingMillis = time.Since(started).Milliseconds()
	status, err := minecraft.ParsePong(data)
	if err != nil {
		return PingResult{Address: address}
	}
	result.MOTD, result.Online = strings.TrimSpace(status.ServerName), true
	result.Players, result.MaxPlayers = max(status.PlayerCount, 0), max(status.MaxPlayers, 0)
	return result
}

func withDefaultPort(address string) string {
	if address == "" {
		return ""
	}
	if _, _, err := net.SplitHostPort(address); err == nil {
		return address
	}
	return net.JoinHostPort(strings.Trim(address, "[]"), defaultPort)
}
