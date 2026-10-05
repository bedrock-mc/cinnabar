package proxy

import (
	"context"
	"errors"
	"log/slog"
	"net"
	"strconv"
	"strings"
	"sync"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// TransferTarget is a server-directed transfer destination.
type TransferTarget struct {
	Host string
	Port uint16
}

// TransferState holds the upstream that the next local client connection dials
// after a server-directed transfer. The zero value is ready to use.
type TransferState struct {
	mu   sync.Mutex
	next string
	// OnTransfer, when set, is called after each recorded transfer; it must not block.
	OnTransfer func(TransferTarget)
}

// Record makes target the next upstream. Unusable targets return an error and change nothing.
func (s *TransferState) Record(target TransferTarget) error {
	address, err := transferAddress(target.Host, target.Port)
	if err != nil {
		return err
	}
	s.mu.Lock()
	s.next = address
	callback := s.OnTransfer
	s.mu.Unlock()
	if callback != nil {
		host, port, _ := net.SplitHostPort(address)
		parsed, _ := strconv.ParseUint(port, 10, 16)
		callback(TransferTarget{Host: host, Port: uint16(parsed)})
	}
	return nil
}

// Upstream returns the recorded transfer address, or initial if none was recorded.
func (s *TransferState) Upstream(initial string) string {
	if address, ok := s.Pending(); ok {
		return address
	}
	return initial
}

// Pending returns the recorded transfer address; ok is false before any transfer.
func (s *TransferState) Pending() (address string, ok bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.next, s.next != ""
}

// Clear drops any recorded transfer.
func (s *TransferState) Clear() {
	s.mu.Lock()
	s.next = ""
	s.mu.Unlock()
}

// clearIf drops the recorded transfer only when it is still address, so a newer
// transfer recorded mid-dial survives.
func (s *TransferState) clearIf(address string) {
	s.mu.Lock()
	if strings.EqualFold(s.next, address) {
		s.next = ""
	}
	s.mu.Unlock()
}

// consumeTransferOnDial clears the pending transfer once a dial to it succeeds.
func consumeTransferOnDial(
	inner func(context.Context, *resolvedUpstreamTarget, minecraft.Dialer) (upstreamSession, error),
	transfers *TransferState,
) func(context.Context, *resolvedUpstreamTarget, minecraft.Dialer) (upstreamSession, error) {
	return func(ctx context.Context, target *resolvedUpstreamTarget, dialer minecraft.Dialer) (upstreamSession, error) {
		pending, hadPending := transfers.Pending()
		upstream, err := inner(ctx, target, dialer)
		if err != nil || upstream == nil {
			return upstream, err
		}
		if hadPending && strings.EqualFold(target.address, pending) {
			transfers.clearIf(pending)
		}
		return upstream, nil
	}
}

// withPendingTransfer dials a recorded server transfer ahead of next, since a Transfer
// packet is an explicit instruction that outranks the client's own target selection.
func withPendingTransfer(
	transfers *TransferState,
	dial func(context.Context, string) (*resolvedUpstreamTarget, error),
	next func(context.Context) (*resolvedUpstreamTarget, error),
) func(context.Context) (*resolvedUpstreamTarget, error) {
	if transfers == nil {
		return next
	}
	return func(ctx context.Context) (*resolvedUpstreamTarget, error) {
		if address, ok := transfers.Pending(); ok {
			return dial(ctx, address)
		}
		return next(ctx)
	}
}

// transferAddress joins a transfer host and port into a dialable address.
func transferAddress(host string, port uint16) (string, error) {
	host = strings.TrimSpace(host)
	if strings.HasPrefix(host, "[") && strings.HasSuffix(host, "]") {
		host = strings.TrimSpace(host[1 : len(host)-1])
	}
	if host == "" {
		return "", errors.New("proxy: invalid transfer: empty address")
	}
	if port == 0 {
		return "", errors.New("proxy: invalid transfer: zero port")
	}
	return net.JoinHostPort(host, strconv.Itoa(int(port))), nil
}

// transferObservingSession records Transfer packets read from upstream and still relays them.
type transferObservingSession struct {
	upstreamSession
	state  *TransferState
	logger *slog.Logger
}

func observeTransfers(upstream upstreamSession, state *TransferState, logger *slog.Logger) upstreamSession {
	if state == nil {
		return upstream
	}
	return &transferObservingSession{upstreamSession: upstream, state: state, logger: logger}
}

func (s *transferObservingSession) ReadBatchRaw(decode func(uint32) bool) ([]minecraft.RawPacket, error) {
	batch, err := s.upstreamSession.ReadBatchRaw(func(id uint32) bool {
		return id == packet.IDTransfer || decode != nil && decode(id)
	})
	for _, raw := range batch {
		for _, value := range raw.Decoded {
			transfer, ok := value.(*packet.Transfer)
			if !ok {
				continue
			}
			if recordErr := s.state.Record(TransferTarget{Host: transfer.Address, Port: transfer.Port}); recordErr != nil && s.logger != nil {
				s.logger.Warn("ignoring unusable server transfer", "error", recordErr)
			}
		}
	}
	return batch, err
}

// DisconnectInfo is the server's own reason for ending a session.
type DisconnectInfo struct {
	Reason  int32
	Message string
}

const maxDisconnectMessageBytes = 512

func disconnectInfoFrom(err error) (DisconnectInfo, bool) {
	var disconnect *minecraft.DisconnectPacketError
	if !errors.As(err, &disconnect) || disconnect == nil {
		return DisconnectInfo{}, false
	}
	message := disconnect.Error()
	if len(message) > maxDisconnectMessageBytes {
		message = strings.ToValidUTF8(message[:maxDisconnectMessageBytes], "")
	}
	return DisconnectInfo{Reason: disconnect.Reason, Message: message}, true
}

func reportDisconnect(callback func(DisconnectInfo), err error) {
	if callback == nil {
		return
	}
	if info, ok := disconnectInfoFrom(err); ok {
		callback(info)
	}
}

// disconnectObservingSession reports a server disconnect read from upstream and still returns the error.
type disconnectObservingSession struct {
	upstreamSession
	callback func(DisconnectInfo)
}

func observeDisconnects(upstream upstreamSession, callback func(DisconnectInfo)) upstreamSession {
	if callback == nil {
		return upstream
	}
	return &disconnectObservingSession{upstreamSession: upstream, callback: callback}
}

func (s *disconnectObservingSession) ReadBatchRaw(decode func(uint32) bool) ([]minecraft.RawPacket, error) {
	batch, err := s.upstreamSession.ReadBatchRaw(decode)
	if err != nil {
		reportDisconnect(s.callback, err)
	}
	return batch, err
}

// UpstreamSelector holds the client-chosen upstream for later connections. The zero value is ready.
type UpstreamSelector struct {
	mu          sync.Mutex
	target      string
	preparation *selectedTransport
}

// Set selects target ("host:port", "realm_id/N" or "friend_xuid/X"); "" clears.
func (s *UpstreamSelector) Set(target string) {
	s.mu.Lock()
	s.target = target
	if s.preparation != nil {
		s.preparation.keep(target)
	}
	s.mu.Unlock()
}

// Target returns the selected upstream; ok is false when none is selected.
func (s *UpstreamSelector) Target() (target string, ok bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.target, s.target != ""
}

// withSelectedTarget dials the client-selected upstream ahead of next.
func withSelectedTarget(
	selector *UpstreamSelector,
	dial func(context.Context, string) (*resolvedUpstreamTarget, error),
	next func(context.Context) (*resolvedUpstreamTarget, error),
) func(context.Context) (*resolvedUpstreamTarget, error) {
	if selector == nil {
		return next
	}
	return func(ctx context.Context) (*resolvedUpstreamTarget, error) {
		if target, ok := selector.Target(); ok {
			return dial(ctx, target)
		}
		return next(ctx)
	}
}
