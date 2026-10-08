package proxy

import (
	"context"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"log/slog"
	"maps"
	"strings"
	"sync"
	"time"

	"github.com/df-mc/go-xsapi/v2/mpsd"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"github.com/sandertv/gophertunnel/minecraft/room"
)

// friendSessionTemplate is the MPSD template vanilla publishes hosted worlds under.
const friendSessionTemplate = "MinecraftLobby"

// friendSessionWriteTimeout bounds one custom-property update.
const friendSessionWriteTimeout = 15 * time.Second

// friendSessionProperties is the custom-property document vanilla clients read: the world
// card plus the per-member nonces a join waits for.
type friendSessionProperties struct {
	room.Status
	Nonces map[string]string `json:"nonces"`
}

// friendSession advertises a hosted world in the Xbox Live session directory and issues each
// joining member the nonce its login must carry.
type friendSession struct {
	session *mpsd.Session
	host    string
	log     *slog.Logger
	wake    chan struct{}
	stop    context.CancelFunc
	done    chan struct{}

	mu         sync.Mutex
	properties friendSessionProperties
}

func publishFriendSession(ctx context.Context, client *mpsd.Client, host string, status room.Status, log *slog.Logger) (*friendSession, error) {
	properties := friendSessionProperties{Status: status, Nonces: map[string]string{}}
	custom, err := json.Marshal(properties)
	if err != nil {
		return nil, fmt.Errorf("encode session: %w", err)
	}
	session, err := client.Publish(ctx, mpsd.SessionReference{
		ServiceConfigID: auth.ServiceConfigID,
		TemplateName:    friendSessionTemplate,
		Name:            strings.ToUpper(uuid.NewString()),
	}, mpsd.PublishConfig{
		CustomProperties: custom,
		JoinRestriction:  status.BroadcastSetting.JoinRestriction(),
		ReadRestriction:  status.BroadcastSetting.ReadRestriction(),
	})
	if err != nil {
		return nil, fmt.Errorf("publish session: %w", err)
	}
	runCtx, stop := context.WithCancel(context.Background())
	s := &friendSession{
		session:    session,
		host:       host,
		log:        log,
		wake:       make(chan struct{}, 1),
		stop:       stop,
		done:       make(chan struct{}),
		properties: properties,
	}
	session.Handle(s)
	go s.run(runCtx)
	s.HandleSessionChange(session)
	return s, nil
}

// HandleSessionChange runs on the directory's notification goroutine, so writes happen in run.
func (s *friendSession) HandleSessionChange(session *mpsd.Session) {
	members := make([]string, 0)
	for _, member := range session.Members() {
		if member.Constants != nil && member.Constants.System != nil {
			members = append(members, member.Constants.System.XUID)
		}
	}
	s.mu.Lock()
	changed := reconcileNonces(s.properties.Nonces, members, s.host)
	s.mu.Unlock()
	if changed {
		s.signal()
	}
}

// nonce reports the nonce issued to xuid, if any.
func (s *friendSession) nonce(xuid string) (string, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	nonce, ok := s.properties.Nonces[xuid]
	return nonce, ok
}

// setMembers publishes the number of players in the world.
func (s *friendSession) setMembers(count int) {
	s.mu.Lock()
	changed := s.properties.MemberCount != count
	s.properties.MemberCount = count
	s.mu.Unlock()
	if changed {
		s.signal()
	}
}

func (s *friendSession) invite(ctx context.Context, xuid string) error {
	_, err := s.session.Invite(ctx, xuid, inviteTitleID)
	return err
}

func (s *friendSession) signal() {
	select {
	case s.wake <- struct{}{}:
	default:
	}
}

func (s *friendSession) run(ctx context.Context) {
	defer close(s.done)
	for {
		select {
		case <-ctx.Done():
			return
		case <-s.wake:
		}
		s.mu.Lock()
		properties := s.properties
		properties.Nonces = maps.Clone(s.properties.Nonces)
		s.mu.Unlock()
		custom, err := json.Marshal(properties)
		if err != nil {
			s.log.Error("friends: encode session", "error", err)
			continue
		}
		writeCtx, cancel := context.WithTimeout(ctx, friendSessionWriteTimeout)
		if err := s.session.SetCustomProperties(writeCtx, custom); err != nil && ctx.Err() == nil {
			s.log.Warn("friends: update session", "error", err)
		}
		cancel()
	}
}

func (s *friendSession) Close() error {
	s.stop()
	<-s.done
	return s.session.Close()
}

// reconcileNonces gives every member except the host a fresh nonce and forgets members who
// left, so a departed player's nonce cannot be replayed. It reports whether nonces changed.
func reconcileNonces(nonces map[string]string, members []string, host string) bool {
	present := make(map[string]bool, len(members))
	changed := false
	for _, xuid := range members {
		if xuid == "" || xuid == host {
			continue
		}
		present[xuid] = true
		if _, ok := nonces[xuid]; !ok {
			nonces[xuid] = newNonce()
			changed = true
		}
	}
	for xuid := range nonces {
		if !present[xuid] {
			delete(nonces, xuid)
			changed = true
		}
	}
	return changed
}

func newNonce() string {
	var b [16]byte
	_, _ = rand.Read(b[:])
	return base64.RawURLEncoding.EncodeToString(b[:])
}
