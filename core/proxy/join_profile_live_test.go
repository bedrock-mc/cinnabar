package proxy

import (
	"context"
	"crypto/ecdsa"
	"errors"
	"io"
	"log/slog"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/packcache"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"golang.org/x/oauth2"
)

type joinProfileTokenSource struct {
	*authcache.Account
	report func(string, time.Duration)
}

func (source joinProfileTokenSource) MultiplayerToken(ctx context.Context, key *ecdsa.PublicKey) (string, error) {
	started := time.Now()
	token, err := source.Account.MultiplayerToken(ctx, key)
	source.report("multiplayer_token", time.Since(started))
	return token, err
}

// TestJoinProfileLive measures authenticated StartGame arrival, before terrain or presentation readiness.
// It copies credentials into a private scratch cache and never outputs packet payloads or tokens.
func TestJoinProfileLive(t *testing.T) {
	joinProfileLive(t, false, false)
}

func TestJoinProfileLiveWarmup(t *testing.T) {
	joinProfileLive(t, true, false)
}

func TestJoinProfileLiveHeadStart(t *testing.T) {
	joinProfileLive(t, true, true)
}

func joinProfileLive(t *testing.T, warmAuthentication, selectedHeadStart bool) {
	server := os.Getenv("CINNABAR_JOIN_PROFILE_SERVER")
	if server == "" {
		t.Skip("missing fixture: CINNABAR_JOIN_PROFILE_SERVER")
	}
	authPath := os.Getenv("CINNABAR_JOIN_PROFILE_AUTH_CACHE")
	if authPath == "" {
		t.Skip("missing fixture: CINNABAR_JOIN_PROFILE_AUTH_CACHE")
	}
	scratch := t.TempDir()
	copied := filepath.Join(scratch, "account.json")
	for _, paths := range [][2]string{{authPath, copied}, {authcache.DerivedCachePath(authPath), authcache.DerivedCachePath(copied)}} {
		data, err := os.ReadFile(paths[0])
		if err != nil && paths[0] != authPath {
			continue
		}
		if err != nil {
			t.Fatal("read authentication fixture")
		}
		if err := os.WriteFile(paths[1], data, 0o600); err != nil {
			t.Fatal("copy authentication fixture")
		}
	}
	ctx, cancel := context.WithTimeout(t.Context(), 90*time.Second)
	defer cancel()
	source, err := authcache.Source(ctx, authcache.Config{
		Path: copied, Writer: io.Discard,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			return nil, errors.New("join profile requires a reusable authentication fixture")
		},
	})
	if err != nil {
		t.Fatal("validate cached authentication fixture")
	}
	account := authcache.NewAccount(ctx, authcache.DerivedCachePath(copied), source, io.Discard)
	defer account.Close()
	cache, err := packcache.New(filepath.Join(scratch, "packs"))
	if err != nil {
		t.Fatal("open scratch pack cache")
	}
	defer cache.Close()
	if warmAuthentication {
		started := time.Now()
		warmup := warmUpstreamAuthentication(ctx, account, slog.New(slog.DiscardHandler))
		<-warmup.done
		warmup.stop()
		t.Logf("JOIN_PROFILE authentication_warmup duration_ms=%.3f success=%t", time.Since(started).Seconds()*1000, !warmup.failed.Load())
		if warmup.failed.Load() {
			t.Fatal("authentication warmup failed")
		}
	}
	reportJoinProfileServer(t, ctx, server)
	t.Log("JOIN_PROFILE measured_boundary=initial_endpoint_start_game final_session_ready=unmeasured")
	const attempts = 13
	for attempt := range attempts {
		overlap := attempt > 0 && attempt%2 == 0
		headStart := selectedHeadStart && overlap
		var selector *UpstreamSelector
		if headStart {
			selector = new(UpstreamSelector)
			selector.startTransportPreparation(ctx)
			selector.PrepareTransport(server)
			time.Sleep(250 * time.Millisecond)
			selector.Set(server)
			t.Logf("JOIN_PROFILE attempt=%d selected_transport_lead_ms=250", attempt)
		}
		started := time.Now()
		report := func(phase string, duration time.Duration) {
			t.Logf("JOIN_PROFILE attempt=%d overlap=%t head_start=%t phase=%s elapsed_ms=%.3f duration_ms=%.3f", attempt, overlap, headStart, phase, time.Since(started).Seconds()*1000, duration.Seconds()*1000)
		}
		dialCtx, cancelDial := context.WithCancelCause(ctx)
		budget := newResourcePackAcquisitionBudget(minecraft.DefaultProtocol, cancelDial)
		dialer := newUpstreamDialerForAdmission(dialerTestDownstream{protocol: minecraft.DefaultProtocol}, joinProfileTokenSource{Account: account, report: report}, cache, nil, false)
		dialer = withResourcePackAcquisitionBudget(dialer, budget)
		observePacket, observePack := dialer.PacketFunc, dialer.ResourcePackProgress
		dialer.PacketFunc = func(header packet.Header, payload []byte, source, destination net.Addr) {
			observePacket(header, payload, source, destination)
			if phase := joinProfilePacketPhase(header.PacketID); phase != "" {
				report(phase, 0)
			}
		}
		dialer.ResourcePackProgress = func(event minecraft.ResourcePackEvent) {
			observePack(event)
			if event.Kind == minecraft.ResourcePackStarted || event.Kind == minecraft.ResourcePackFinished {
				t.Logf("JOIN_PROFILE attempt=%d pack_event=%d pack_source=%d bytes=%d elapsed_ms=%.3f", attempt, event.Kind, event.Source, event.Size, time.Since(started).Seconds()*1000)
			}
		}
		transfers := newProfilePackTransfers(minecraft.DefaultProtocol)
		dialer.PacketFunc = transfers.wrap(dialer.PacketFunc)
		var conn *minecraft.Conn
		if headStart {
			conn, err = dialWithSelectedTransport(dialCtx, selector, minecraft.RakNet{}, server, dialer.DialContextNetwork)
			selector.stopTransportPreparation()
		} else if overlap || selectedHeadStart {
			conn, err = dialWithPreparedTransport(dialCtx, minecraft.RakNet{}, server, dialer.DialContextNetwork)
		} else {
			conn, err = dialer.DialContextNetwork(dialCtx, minecraft.RakNet{}, server)
		}
		budget.finish()
		cancelDial(nil)
		report("start_game_arrived", 0)
		transfers.report(t, attempt)
		if err == nil && overlap && attempt == attempts-1 {
			assertJoinProfileRetainedConnection(t, conn)
		}
		if conn != nil {
			_ = conn.Abort()
			_ = conn.Close()
		}
		if err != nil {
			t.Fatalf("join profile attempt %d failed (error type %T)", attempt, err)
		}
		time.Sleep(250 * time.Millisecond)
	}
}

func reportJoinProfileServer(t *testing.T, ctx context.Context, server string) {
	t.Helper()
	host, port, err := net.SplitHostPort(server)
	if err != nil {
		t.Fatal("invalid join profile server endpoint")
	}
	resolved, err := net.DefaultResolver.LookupHost(ctx, host)
	if err == nil {
		for _, address := range resolved {
			t.Logf("JOIN_PROFILE resolved_endpoint=%s", net.JoinHostPort(address, port))
		}
	}
	pingCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	advertisement, err := (minecraft.RakNet{}).PingContext(pingCtx, server)
	if err != nil {
		t.Log("JOIN_PROFILE server_advertisement=unavailable")
		return
	}
	fields := strings.Split(string(advertisement), ";")
	if len(fields) > 3 {
		t.Logf("JOIN_PROFILE endpoint=%s server_protocol=%s server_version=%s", server, fields[2], fields[3])
	}
}

func assertJoinProfileRetainedConnection(t *testing.T, conn *minecraft.Conn) {
	t.Helper()
	started := time.Now()
	if err := conn.SetReadDeadline(started.Add(1100 * time.Millisecond)); err != nil {
		t.Fatal("set retained connection smoke deadline")
	}
	var packets, controls uint64
	for time.Since(started) < time.Second {
		batch, err := conn.ReadBatchRaw(nil)
		if err != nil {
			var networkError net.Error
			if errors.As(err, &networkError) && networkError.Timeout() {
				break
			}
			t.Fatalf("retained connection read failed (error type %T)", err)
		}
		packets += uint64(len(batch))
		for _, raw := range batch {
			switch raw.ID {
			case packet.IDStartGame, packet.IDLevelChunk, packet.IDSubChunk, packet.IDSetTime, packet.IDNetworkStackLatency, packet.IDPlayStatus:
				controls++
			}
		}
	}
	t.Logf("JOIN_PROFILE retained_connection elapsed_ms=%.3f packets=%d startup_or_control_packets=%d connected=%t", time.Since(started).Seconds()*1000, packets, controls, conn.Context().Err() == nil)
	if conn.Context().Err() != nil || packets == 0 || controls == 0 {
		t.Fatal("retained connection did not remain open with startup/control delivery")
	}
}

func joinProfilePacketPhase(id uint32) string {
	if id == packet.IDClientToServerHandshake {
		return "client_handshake"
	}
	phase, _ := joinPacketPhase(id)
	return phase
}
