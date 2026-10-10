// Package launcher implements the control-channel launcher services: catalog
// listings, upstream selection, and sign-out.
package launcher

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"os"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/authflow"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
	"github.com/hashimthearab/rust-mcbe/core/xboxpresence"
	"github.com/sandertv/gophertunnel/minecraft/realms"
	"github.com/sandertv/gophertunnel/minecraft/service/gatherings"
	"golang.org/x/oauth2"
)

// Config wires a Service. A nil Account means the core runs without one.
type Config struct {
	Account    *authcache.Account // shared per-account runtime; sign-out closes it
	AuthCache  string             // token cache path; sign-out deletes it and its derived cache
	Store      *control.Store
	Selector   *proxy.UpstreamSelector
	Transfers  *proxy.TransferState
	ArtworkDir string // screen artwork cache; empty skips caching
	CacheFile  string // last good catalog; empty keeps it in memory only
	Logger     *slog.Logger
	Language   string // active UI locale used by messaging
	// StoreImageDir holds cached Marketplace images; empty disables them.
	StoreImageDir string

	// Injectable for tests; nil selects the real implementation.
	RealmMembership func(context.Context, *authcache.Account, string, bool) (catalog.Realm, error)
	Realms          func(context.Context, *authcache.Account) ([]catalog.Realm, error)
	Friends         func(context.Context, *authcache.Account) ([]catalog.Friend, error)
	People          func(context.Context, *authcache.Account) ([]catalog.Person, error)
	Gamertag        func(context.Context, *authcache.Account) (string, error)
	Remove          func(path string) error

	Featured                  func(context.Context, *authcache.Account) ([]catalog.FeaturedServer, error)
	ExperienceCounts          func(context.Context, *authcache.Account) ([]gatherings.ExperiencePlayerCount, error)
	Profile                   func(context.Context, *authcache.Account) (catalog.Profile, error)
	ProfileFeaturedScreenshot func(context.Context, *authcache.Account, string) (catalog.Image, error)
	ProfileAvatar             func(context.Context, *authcache.Account, string, string) (catalog.Image, error)
	CacheArt                  func(ctx context.Context, directory string, images []*catalog.Image)
	Ping                      func(ctx context.Context, addresses []string) []catalog.PingResult
	Home                      func(ctx context.Context, src *authcache.Account, session *catalog.MessagingSession, artworkDir string) (catalog.Home, error)
	Report                    func(ctx context.Context, src *authcache.Account, session *catalog.MessagingSession, event catalog.MessageEvent) error
	JoinGathering             func(context.Context, *authcache.Account, uuid.UUID) (*gatherings.Address, error)
}

// Service implements control.Services.
type Service struct {
	cfg       Config
	logger    *slog.Logger
	presence  *xboxpresence.Worker
	signedOut atomic.Bool
	messaging *catalog.MessagingSession

	mu           sync.Mutex
	snap         snapshot
	flights      [2]*flight
	attempted    [2]time.Time
	profileLogMu sync.Mutex
	profileLogs  map[string]time.Time
	profileArt   []string   // current avatar and achievement art pruning must keep
	gamerpic     string     // profile artwork pruning must keep
	disk         sync.Mutex // orders cache rewrites
}

// New returns a Service; it fills unset injectables with the real implementations.
func New(cfg Config) *Service {
	if cfg.RealmMembership == nil {
		cfg.RealmMembership = catalog.RealmMembership
	}
	if cfg.Realms == nil {
		cfg.Realms = catalog.Realms
	}
	if cfg.Friends == nil {
		cfg.Friends = catalog.Friends
	}
	if cfg.People == nil {
		cfg.People = catalog.People
	}
	if cfg.Gamertag == nil {
		cfg.Gamertag = catalog.Gamertag
	}
	if cfg.Remove == nil {
		cfg.Remove = os.Remove
	}
	if cfg.Featured == nil {
		cfg.Featured = catalog.FeaturedServers
	}
	if cfg.ExperienceCounts == nil {
		cfg.ExperienceCounts = new(catalog.ExperienceCounts).Counts
	}
	if cfg.Profile == nil {
		cfg.Profile = catalog.AccountProfile
	}
	if cfg.ProfileFeaturedScreenshot == nil {
		cfg.ProfileFeaturedScreenshot = catalog.ProfileFeaturedScreenshot
	}
	if cfg.ProfileAvatar == nil {
		cfg.ProfileAvatar = catalog.ProfileAvatar
	}
	if cfg.CacheArt == nil {
		cfg.CacheArt = catalog.CacheImages
	}
	if cfg.Ping == nil {
		cfg.Ping = catalog.PingServers
	}
	if cfg.Home == nil {
		cfg.Home = catalog.HomeFeed
	}
	if cfg.Report == nil {
		cfg.Report = catalog.ReportMessageEvent
	}
	if cfg.JoinGathering == nil {
		cfg.JoinGathering = catalog.JoinGathering
	}
	s := &Service{cfg: cfg, logger: cfg.Logger, messaging: catalog.NewMessagingSession(cfg.Language)}
	if s.logger == nil {
		s.logger = slog.New(slog.DiscardHandler)
	}
	s.load()
	return s
}

func (s *Service) source() (*authcache.Account, error) {
	if s.cfg.Account == nil || s.signedOut.Load() || s.cfg.Account.Closed() {
		return nil, control.ErrSignedOut
	}
	return s.cfg.Account, nil
}

// Realms lists the account's Realms.
func (s *Service) Realms(ctx context.Context) ([]catalog.Realm, error) {
	src, err := s.source()
	if err != nil {
		return nil, err
	}
	return s.cfg.Realms(ctx, src)
}

// Friends lists joinable friend worlds.
func (s *Service) Friends(ctx context.Context) ([]catalog.Friend, error) {
	src, err := s.source()
	if err != nil {
		return nil, err
	}
	return s.cfg.Friends(ctx, src)
}

// FeaturedServers lists the featured servers with their artwork cached, from the last good fetch.
func (s *Service) FeaturedServers(ctx context.Context) ([]catalog.FeaturedServer, error) {
	return cached(ctx, s, featuredFeed)
}

// FeaturedServersWithCounts adds live populations while experience details are visible.
func (s *Service) FeaturedServersWithCounts(ctx context.Context) ([]catalog.FeaturedServer, error) {
	servers, err := s.FeaturedServers(ctx)
	if err != nil || !hasExperiences(servers) {
		return servers, err
	}
	src, err := s.source()
	if err != nil {
		return nil, err
	}
	counts, countErr := s.cfg.ExperienceCounts(ctx, src)
	if countErr != nil {
		s.logger.Warn("experience counts unavailable", "error", control.RedactError(countErr))
	}
	if _, err := s.source(); err != nil {
		return nil, err
	}
	return withExperienceCounts(servers, counts), nil
}

// Home returns the start screen's service data with its artwork cached, from the last good fetch.
func (s *Service) Home(ctx context.Context) (catalog.Home, error) {
	return cached(ctx, s, homeFeed)
}

// ReportMessage posts one messaging report for the signed-in session.
func (s *Service) ReportMessage(ctx context.Context, event catalog.MessageEvent) error {
	src, err := s.source()
	if err != nil {
		return err
	}
	return s.cfg.Report(ctx, src, s.messaging, event)
}

// Ping pings servers for their player counts and round trip; it needs no account.
func (s *Service) Ping(ctx context.Context, addresses []string) []catalog.PingResult {
	return s.cfg.Ping(ctx, addresses)
}

func (s *Service) cacheArt(ctx context.Context, images []*catalog.Image) {
	if s.cfg.ArtworkDir != "" {
		s.cfg.CacheArt(ctx, s.cfg.ArtworkDir, images)
	}
}

// Connect selects the upstream for the next client connection and drops any pending transfer.
// A gathering is joined now, so its server assignment is fresh.
func (s *Service) Connect(ctx context.Context, kind, value string) error {
	target, err := upstreamTarget(kind, value)
	if err != nil {
		return err
	}
	if kind != control.TargetRakNet {
		account, err := s.source()
		if err != nil {
			return err
		}
		if kind == control.TargetGathering {
			if target, err = s.joinGathering(ctx, account, uuid.MustParse(target)); err != nil {
				return err
			}
		}
	}
	if s.cfg.Selector != nil {
		s.cfg.Selector.Set(target)
	}
	if s.cfg.Transfers != nil {
		s.cfg.Transfers.Clear()
	}
	if s.cfg.Store != nil {
		s.cfg.Store.ClearTransfer()
	}
	return nil
}

// upstreamTarget maps a connect.v1 target to the proxy's target syntax.
func upstreamTarget(kind, value string) (string, error) {
	value = strings.TrimSpace(value)
	switch kind {
	case control.TargetRakNet:
		host, port, err := net.SplitHostPort(value)
		number, portErr := strconv.ParseUint(port, 10, 16)
		if err != nil || portErr != nil || number == 0 || host == "" || strings.ContainsAny(host, " \t/\\") {
			return "", control.ErrInvalidTarget
		}
		return net.JoinHostPort(host, port), nil
	case control.TargetRealm:
		if id, err := strconv.ParseUint(value, 10, 31); err != nil || id == 0 {
			return "", control.ErrInvalidTarget
		}
		return "realm_id/" + value, nil
	case control.TargetFriend:
		if _, err := strconv.ParseUint(value, 10, 64); err != nil {
			return "", control.ErrInvalidTarget
		}
		return "friend_xuid/" + value, nil
	case control.TargetGathering:
		id, err := uuid.Parse(value)
		if err != nil || id == uuid.Nil {
			return "", control.ErrInvalidTarget
		}
		return id.String(), nil
	}
	return "", control.ErrInvalidTarget
}

// joinGathering joins the experience and maps its typed assignment to the proxy target syntax.
func (s *Service) joinGathering(ctx context.Context, account *authcache.Account, id uuid.UUID) (string, error) {
	address, err := s.cfg.JoinGathering(ctx, account, id)
	if err != nil {
		return "", fmt.Errorf("launcher: join gathering: %w", err)
	}
	target, err := gatheringTarget(address)
	if err != nil {
		return "", err
	}
	info := address.DestinationInfo
	s.logger.Info("gathering joined", "experience", id, "protocol", address.NetworkProtocol,
		"server_id", info.ServerID, "world_id", info.WorldID, "scenario_id", info.ScenarioID)
	return target, nil
}

// gatheringTarget names the transport the assignment selects: host:port for RakNet, or the
// NetherNet ID with its signaling dialect.
func gatheringTarget(address *gatherings.Address) (string, error) {
	if address == nil {
		return "", errors.New("launcher: gathering returned no address")
	}
	dial, err := address.DialAddress()
	if err != nil {
		return "", err
	}
	switch realms.ParseNetworkProtocol(string(address.NetworkProtocol)) {
	case realms.NetworkProtocolNetherNet:
		return "nethernet/websocket/" + dial, nil
	case realms.NetworkProtocolNetherNetJSONRPC:
		return "nethernet/jsonrpc/" + dial, nil
	}
	return dial, nil
}

// SignOut deletes the cached Microsoft tokens and reports the signed-out state. The running
// process stops using the account; a new sign-in needs the device-code flow and a core restart.
func (s *Service) SignOut() error {
	if s.cfg.Account == nil {
		return control.ErrSignedOut
	}
	s.mu.Lock()
	s.signedOut.Store(true)
	s.snap = snapshot{}
	s.mu.Unlock()
	if s.presence != nil {
		s.presence.Close()
	}
	_ = s.cfg.Account.Close()
	s.disk.Lock()
	wait, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	failed := authcache.Remove(wait, s.cfg.AuthCache, s.cfg.Remove) != nil
	cancel()
	if s.cfg.CacheFile != "" {
		if err := s.cfg.Remove(s.cfg.CacheFile); err != nil && !errors.Is(err, os.ErrNotExist) {
			failed = true
		}
	}
	s.disk.Unlock()
	if s.cfg.Selector != nil {
		s.cfg.Selector.Set("")
	}
	if s.cfg.Store != nil {
		s.cfg.Store.SetAuth(control.AuthV1{State: control.AuthSignedOut})
	}
	if failed {
		return errors.New("launcher: remove cached tokens")
	}
	return nil
}

// PublishSignedIn reports the signed-in state with the gamertag when it can be read.
func (s *Service) PublishSignedIn(ctx context.Context) {
	if s.cfg.Store == nil || s.cfg.Account == nil {
		return
	}
	state := control.AuthV1{State: control.AuthSignedIn}
	if tag, err := s.cfg.Gamertag(ctx, s.cfg.Account); err == nil {
		state.Gamertag = tag
	}
	if _, err := s.source(); err != nil {
		return
	}
	s.cfg.Store.SetAuth(state)
}

// DeviceRequest is an authcache.Config.Request that publishes the device code to store and
// writes the standard prompt line to w; it publishes a sanitized failure reason on error.
func DeviceRequest(store *control.Store) func(context.Context, io.Writer) (*oauth2.Token, error) {
	return func(ctx context.Context, w io.Writer) (*oauth2.Token, error) {
		token, err := (authflow.DeviceFlow{}).Request(ctx, func(device *oauth2.DeviceAuthResponse) error {
			if store != nil {
				store.SetAuth(control.AuthV1{
					State: control.AuthAwaitingCode, VerificationURI: device.VerificationURI, UserCode: device.UserCode,
				})
			}
			_, err := fmt.Fprintf(w, "Authenticate at %v using the code %v.\n", device.VerificationURI, device.UserCode)
			return err
		})
		if err != nil {
			if store != nil {
				store.SetAuth(control.AuthV1{State: control.AuthFailed, Reason: "Microsoft sign-in did not complete."})
			}
			return nil, err
		}
		_, _ = w.Write([]byte("Authentication successful.\n"))
		return token, nil
	}
}
