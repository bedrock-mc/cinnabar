package proxy

import (
	"context"
	"fmt"
	"log/slog"
	"net"
	"strconv"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/df-mc/go-nethernet/discovery"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/p2p"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

const localLANDiscoveryTimeout = 8 * time.Second

func resolveLocalLANTarget(ctx context.Context, target localworld.ConnectionTarget) (*resolvedUpstreamTarget, error) {
	host, port, err := net.SplitHostPort(target.LANAddress)
	ip := net.ParseIP(host)
	portNumber, portErr := strconv.ParseUint(port, 10, 16)
	if err != nil || portErr != nil || ip == nil || ip.To4() == nil || !ip.IsLoopback() || portNumber == 0 || target.LevelName == "" {
		return nil, fmt.Errorf("local BDS LAN target must name a selected level and loopback discovery endpoint: %q", target.LANAddress)
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	remote := &net.UDPAddr{IP: ip, Port: int(portNumber)}
	signaling, err := (discovery.ListenConfig{BroadcastAddress: remote, Log: secretSafeResourcePackLogger()}).Listen(net.JoinHostPort(remote.IP.String(), "0"))
	if err != nil {
		return nil, fmt.Errorf("local BDS LAN discovery: %w", err)
	}
	owned := true
	defer func() {
		if owned {
			_ = signaling.Close()
		}
	}()
	discoveryCtx, cancel := context.WithTimeout(ctx, localLANDiscoveryTimeout)
	defer cancel()
	id, data, err := findLocalLANPeer(discoveryCtx, signaling, target.LevelName)
	if err != nil {
		return nil, err
	}
	owned = false
	return &resolvedUpstreamTarget{
		address: strconv.FormatUint(id, 10), local: signaling, offline: true,
		// BDS explicitly advertises offline LAN login and returns an identityless
		// LAN answer. HTTP, external endpoints and online identities stay strict.
		network:    minecraft.NetherNet{Signaling: signaling, Dialer: nethernet.Dialer{AllowIdentitylessServer: true}},
		clientData: func(client *login.ClientData) { client.Nonce = data.Nonce },
	}, nil
}

type localLANResponses interface {
	Responses() map[uint64][]byte
	Context() context.Context
}

func findLocalLANPeer(ctx context.Context, signaling localLANResponses, level string) (uint64, discovery.ServerData, error) {
	var skipped uint64
	defer func() {
		if skipped != 0 {
			slog.Warn("skipped unsupported local LAN metadata", "count", skipped)
		}
	}()
	ticker := time.NewTicker(50 * time.Millisecond)
	defer ticker.Stop()
	for {
		for id, response := range signaling.Responses() {
			var data discovery.ServerData
			if err := data.UnmarshalBinary(response); err != nil {
				skipped++
				continue
			}
			if data.LevelName != level {
				continue
			}
			if !data.AcceptsSelfSignedAuth {
				return 0, data, fmt.Errorf("local BDS does not advertise offline LAN admission")
			}
			if data.ConnectionType != p2p.ConnectionTypeSignalingOverLAN || data.Nonce == "" {
				skipped++
				continue
			}
			return id, data, nil
		}
		select {
		case <-ctx.Done():
			return 0, discovery.ServerData{}, fmt.Errorf("local BDS LAN discovery: %w", ctx.Err())
		case <-signaling.Context().Done():
			return 0, discovery.ServerData{}, fmt.Errorf("local BDS LAN discovery closed: %w", context.Cause(signaling.Context()))
		case <-ticker.C:
		}
	}
}
