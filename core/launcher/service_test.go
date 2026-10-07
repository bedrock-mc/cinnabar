package launcher

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
	"github.com/sandertv/gophertunnel/minecraft/realms"
	"github.com/sandertv/gophertunnel/minecraft/service/gatherings"
	"golang.org/x/oauth2"
)

// testAccount is an account runtime without persistence; injected fetchers never touch it.
func testAccount() *authcache.Account {
	return authcache.NewAccount(context.Background(), "", oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "x"}), nil)
}

type fixture struct {
	service  *Service
	store    *control.Store
	selector *proxy.UpstreamSelector
	removed  []string
}

func newFixture(t *testing.T, source *authcache.Account) *fixture {
	t.Helper()
	f := &fixture{store: control.NewStore(), selector: new(proxy.UpstreamSelector)}
	f.service = New(Config{
		Account: source, AuthCache: filepath.Join(t.TempDir(), "token.json"),
		Store: f.store, Selector: f.selector, Transfers: new(proxy.TransferState),
		Realms: func(context.Context, *authcache.Account) ([]catalog.Realm, error) {
			return []catalog.Realm{{Name: "R", Target: "realm_id/1"}}, nil
		},
		Friends: func(context.Context, *authcache.Account) ([]catalog.Friend, error) {
			return []catalog.Friend{{XUID: "9"}}, nil
		},
		Gamertag: func(context.Context, *authcache.Account) (string, error) { return "Steve", nil },
		Remove:   func(path string) error { f.removed = append(f.removed, path); return nil },
	})
	return f
}

func TestConnectMapsTargetsToProxySyntax(t *testing.T) {
	f := newFixture(t, testAccount())
	for _, test := range []struct{ kind, value, want string }{
		{control.TargetRakNet, " play.example.net:19132 ", "play.example.net:19132"},
		{control.TargetRakNet, "[::1]:19132", "[::1]:19132"},
		{control.TargetRealm, "12345", "realm_id/12345"},
		{control.TargetFriend, "2535428000000000", "friend_xuid/2535428000000000"},
	} {
		if err := f.service.Connect(context.Background(), test.kind, test.value); err != nil {
			t.Fatalf("Connect(%s, %q) = %v", test.kind, test.value, err)
		}
		if got, _ := f.selector.Target(); got != test.want {
			t.Fatalf("selected %q, want %q", got, test.want)
		}
	}
}

func TestConnectRejectsMalformedTargetsWithoutChangingSelection(t *testing.T) {
	f := newFixture(t, testAccount())
	_ = f.service.Connect(context.Background(), control.TargetRakNet, "keep.example:1")
	for _, test := range []struct{ kind, value string }{
		{control.TargetRakNet, "no-port"}, {control.TargetRakNet, "host:0"}, {control.TargetRakNet, "host:99999"},
		{control.TargetRakNet, "a b:1"}, {control.TargetRakNet, ":19132"}, {control.TargetRealm, "0"},
		{control.TargetRealm, "abc"}, {control.TargetFriend, "gamertag"}, {"other", "x"},
	} {
		if err := f.service.Connect(context.Background(), test.kind, test.value); !errors.Is(err, control.ErrInvalidTarget) {
			t.Fatalf("Connect(%s, %q) = %v, want invalid target", test.kind, test.value, err)
		}
	}
	if got, _ := f.selector.Target(); got != "keep.example:1" {
		t.Fatalf("selection changed to %q", got)
	}
}

func TestConnectClearsPendingTransfer(t *testing.T) {
	f := newFixture(t, testAccount())
	f.store.ObserveTransfer(proxy.TransferTarget{Host: "next", Port: 1})
	if err := f.service.Connect(context.Background(), control.TargetRakNet, "a.example:1"); err != nil {
		t.Fatal(err)
	}
	if f.store.Status().Transfer != nil {
		t.Fatal("explicit connect left the transfer pending")
	}
}

func TestAccountBoundOperationsNeedASession(t *testing.T) {
	f := newFixture(t, nil)
	if _, err := f.service.Realms(context.Background()); !errors.Is(err, control.ErrSignedOut) {
		t.Fatalf("Realms() = %v", err)
	}
	if _, err := f.service.Friends(context.Background()); !errors.Is(err, control.ErrSignedOut) {
		t.Fatalf("Friends() = %v", err)
	}
	if err := f.service.Connect(context.Background(), control.TargetRealm, "5"); !errors.Is(err, control.ErrSignedOut) {
		t.Fatalf("realm Connect() = %v", err)
	}
	if err := f.service.Connect(context.Background(), control.TargetRakNet, "a.example:1"); err != nil {
		t.Fatalf("raknet Connect() without account = %v", err)
	}
}

func TestSignOutRemovesCachesAndBlocksAccountCalls(t *testing.T) {
	f := newFixture(t, testAccount())
	f.store.SetAuth(control.AuthV1{State: control.AuthSignedIn, Gamertag: "Steve"})
	_ = f.service.Connect(context.Background(), control.TargetRealm, "5")
	if err := f.service.SignOut(); err != nil {
		t.Fatal(err)
	}
	if len(f.removed) != 2 || f.removed[0] != f.service.cfg.AuthCache || f.removed[1] == f.removed[0] {
		t.Fatalf("removed %v, want the token cache and its derived cache", f.removed)
	}
	if got := f.store.Auth(); got.State != control.AuthSignedOut || got.Gamertag != "" {
		t.Fatalf("auth = %+v", got)
	}
	if _, ok := f.selector.Target(); ok {
		t.Fatal("sign-out kept the account-bound selection")
	}
	if _, err := f.service.Realms(context.Background()); !errors.Is(err, control.ErrSignedOut) {
		t.Fatalf("Realms() after sign-out = %v", err)
	}
	if _, err := f.service.cfg.Account.PlayFab(context.Background()); !errors.Is(err, authcache.ErrAccountClosed) {
		t.Fatalf("sign-out left the account runtime open: %v", err)
	}
	f.service.PublishSignedIn(context.Background())
	if f.store.Auth().State != control.AuthSignedOut {
		t.Fatal("late sign-in publication overwrote the signed-out state")
	}
}

func TestSignOutToleratesMissingFilesAndReportsRealFailures(t *testing.T) {
	f := newFixture(t, testAccount())
	f.service.cfg.Remove = func(string) error { return os.ErrNotExist }
	if err := f.service.SignOut(); err != nil {
		t.Fatalf("missing cache files: %v", err)
	}
	f = newFixture(t, testAccount())
	f.service.cfg.Remove = func(string) error { return errors.New("/secret/path: permission denied") }
	if err := f.service.SignOut(); err == nil || strings.Contains(err.Error(), "/secret") {
		t.Fatalf("SignOut() = %v, want a failure", err)
	}
	if f.store.Auth().State != control.AuthSignedOut {
		t.Fatal("failed removal must still report signed out")
	}
}

func TestPublishSignedInIncludesGamertag(t *testing.T) {
	f := newFixture(t, testAccount())
	f.service.PublishSignedIn(context.Background())
	if got := f.store.Auth(); got.State != control.AuthSignedIn || got.Gamertag != "Steve" {
		t.Fatalf("auth = %+v", got)
	}
}

func TestPublishSignedInDoesNotOutliveAccount(t *testing.T) {
	accountCtx, cancel := context.WithCancel(context.Background())
	defer cancel()
	account := authcache.NewAccount(accountCtx, "", oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "x"}), nil)
	t.Cleanup(func() { _ = account.Close() })
	f := newFixture(t, account)
	f.store.SetAuth(control.AuthV1{State: control.AuthSignedOut})
	f.service.cfg.Gamertag = func(context.Context, *authcache.Account) (string, error) {
		cancel()
		return "Steve", nil
	}
	f.service.PublishSignedIn(context.Background())
	if got := f.store.Auth(); got.State != control.AuthSignedOut || got.Gamertag != "" {
		t.Fatalf("ended account published auth = %+v", got)
	}
}

func TestScreenFeedsCacheArtworkAndNeedAnAccount(t *testing.T) {
	var cached []string
	service := New(Config{
		Account: testAccount(), ArtworkDir: "/art",
		Featured: func(context.Context, *authcache.Account) ([]catalog.FeaturedServer, error) {
			return []catalog.FeaturedServer{{Name: "S", Logo: catalog.Image{URL: "https://a.test/l.png"}}}, nil
		},
		Profile: func(context.Context, *authcache.Account) (catalog.Profile, error) {
			return catalog.Profile{Gamertag: "Steve"}, nil
		},
		CacheArt: func(_ context.Context, directory string, images []*catalog.Image) {
			for _, image := range images {
				if image.URL != "" {
					image.Path = directory + "/cached"
					cached = append(cached, image.URL)
				}
			}
		},
	})
	servers, err := service.FeaturedServers(context.Background())
	if err != nil || len(servers) != 1 || servers[0].Logo.Path != "/art/cached" || len(cached) != 1 {
		t.Fatalf("servers = %+v, err = %v, cached = %v", servers, err, cached)
	}
	if profile, err := service.Profile(context.Background()); err != nil || profile.Gamertag != "Steve" {
		t.Fatalf("profile = %+v, err = %v", profile, err)
	}
	offline := New(Config{})
	if _, err := offline.FeaturedServers(context.Background()); !errors.Is(err, control.ErrSignedOut) {
		t.Fatalf("offline featured servers err = %v", err)
	}
}

func TestHomeCachesMessageArtwork(t *testing.T) {
	service := New(Config{
		Account: testAccount(), ArtworkDir: "/art",
		Home: func(context.Context, *authcache.Account, *catalog.MessagingSession, string) (catalog.Home, error) {
			return catalog.Home{
				Messages: []catalog.Message{{ID: "m", Images: []catalog.MessageImage{
					{ID: "tile", Image: catalog.Image{URL: "https://a.test/t.png"}},
				}}},
			}, nil
		},
		CacheArt: func(_ context.Context, directory string, images []*catalog.Image) {
			for _, image := range images {
				if image.URL != "" {
					image.Path = directory + "/cached"
				}
			}
		},
	})
	home, err := service.Home(context.Background())
	if err != nil || home.Messages[0].Images[0].Path != "/art/cached" {
		t.Fatalf("home = %+v, err = %v", home, err)
	}
}

// A gathering is joined at connect time and its typed assignment picks the transport.
func TestConnectJoinsGatheringsAtConnectTime(t *testing.T) {
	id := uuid.MustParse("5b0f2bd4-8a8e-4a6e-9d3c-0a1b2c3d4e5f")
	for _, test := range []struct {
		address gatherings.Address
		want    string
	}{
		{gatherings.Address{NetworkProtocol: gatherings.NetworkProtocolDefault, IPv4Address: "203.0.113.7", Port: 19132}, "203.0.113.7:19132"},
		{gatherings.Address{NetworkProtocol: realms.NetworkProtocolNetherNetJSONRPC, NetherNetID: "1234"}, "nethernet/jsonrpc/1234"},
		{gatherings.Address{NetworkProtocol: realms.NetworkProtocolNetherNet, NetherNetID: "1234"}, "nethernet/websocket/1234"},
	} {
		f := newFixture(t, testAccount())
		var joined []uuid.UUID
		f.service.cfg.JoinGathering = func(_ context.Context, _ *authcache.Account, got uuid.UUID) (*gatherings.Address, error) {
			joined = append(joined, got)
			address := test.address
			return &address, nil
		}
		if err := f.service.Connect(context.Background(), control.TargetGathering, id.String()); err != nil {
			t.Fatal(err)
		}
		if got, _ := f.selector.Target(); got != test.want || len(joined) != 1 || joined[0] != id {
			t.Fatalf("target = %q joined = %v, want %q", got, joined, test.want)
		}
	}
	f := newFixture(t, testAccount())
	f.service.cfg.JoinGathering = func(context.Context, *authcache.Account, uuid.UUID) (*gatherings.Address, error) {
		return &gatherings.Address{NetworkProtocol: gatherings.NetworkProtocolDefault}, nil
	}
	if err := f.service.Connect(context.Background(), control.TargetGathering, id.String()); err == nil {
		t.Fatal("an assignment without a host was selected")
	}
	if err := f.service.Connect(context.Background(), control.TargetGathering, "not-a-uuid"); !errors.Is(err, control.ErrInvalidTarget) {
		t.Fatalf("malformed experience ID err = %v", err)
	}
}

func TestSignOutReportsLeaseFailureWithoutRemovingUnlockedCredentials(t *testing.T) {
	f := newFixture(t, testAccount())
	blocked := filepath.Join(t.TempDir(), "not-a-directory")
	if err := os.WriteFile(blocked, []byte("synthetic"), 0o600); err != nil {
		t.Fatal(err)
	}
	f.service.cfg.AuthCache = filepath.Join(blocked, "token.json")
	if err := f.service.SignOut(); err == nil || strings.Contains(err.Error(), blocked) {
		t.Fatalf("lease failure was hidden or exposed a path: %v", err)
	}
	if len(f.removed) != 0 {
		t.Fatalf("removed credentials without their leases: %v", f.removed)
	}
	if f.store.Auth().State != control.AuthSignedOut {
		t.Fatal("lease failure left the local account signed in")
	}
}

// TestProfileCarriesTheRenderedAvatar uses an injected fixture rather than any account service.
func TestProfileCarriesTheRenderedAvatar(t *testing.T) {
	calls := 0
	service := New(Config{
		Account: testAccount(), ArtworkDir: t.TempDir(),
		Profile: func(context.Context, *authcache.Account) (catalog.Profile, error) {
			return catalog.Profile{Gamertag: "Steve", XUID: "123"}, nil
		},
		ProfileFeaturedScreenshot: func(context.Context, *authcache.Account, string) (catalog.Image, error) { return catalog.Image{}, nil },
		ProfileAvatar: func(_ context.Context, _ *authcache.Account, xuid, directory string) (catalog.Image, error) {
			calls++
			if xuid != "123" || directory == "" {
				t.Fatal("avatar did not use profile identity/cache")
			}
			return catalog.Image{Path: directory + "/avatar.img"}, nil
		},
		CacheArt: func(context.Context, string, []*catalog.Image) {},
	})
	profile, err := service.Profile(context.Background())
	if err != nil || calls != 1 || profile.Avatar.Path == "" || profile.AvatarError {
		t.Fatalf("profile avatar: %+v %v", profile, err)
	}
	service.cfg.ProfileAvatar = func(context.Context, *authcache.Account, string, string) (catalog.Image, error) {
		return catalog.Image{}, errors.New("fixture unavailable")
	}
	profile, err = service.Profile(context.Background())
	if err != nil || !profile.AvatarError || profile.Gamertag != "Steve" {
		t.Fatalf("failed avatar lost available profile: %+v %v", profile, err)
	}
}
