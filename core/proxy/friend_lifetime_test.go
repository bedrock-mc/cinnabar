package proxy

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/coder/websocket"
	"github.com/coder/websocket/wsjson"
	"github.com/df-mc/go-xsapi/v2"
	"github.com/df-mc/go-xsapi/v2/xal/xasd"
	"github.com/df-mc/go-xsapi/v2/xal/xasu"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/p2p"
)

type friendTokens struct{ key *ecdsa.PrivateKey }

// XSTSToken provides a synthetic identity used only by the local service fixture.
func (friendTokens) XSTSToken(context.Context, string) (*xsts.Token, error) {
	return &xsts.Token{Token: "fixture", NotAfter: time.Now().Add(time.Hour), DisplayClaims: xsts.DisplayClaims{UserInfo: []xsts.UserInfo{{UserInfo: xasu.UserInfo{UserHash: "fixture"}, XUID: "123"}}}}, nil
}

// DeviceToken refuses any unexpected authentication request.
func (friendTokens) DeviceToken(context.Context) (*xasd.Token, error) {
	return nil, errors.New("unexpected device authentication")
}

// ProofKey signs fixture NSAL requests without using account credentials.
func (s friendTokens) ProofKey() *ecdsa.PrivateKey { return s.key }

type friendTransport struct{ local *url.URL }

// RoundTrip routes all Xbox HTTP and RTA traffic into the local fixture.
func (s friendTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	clone := req.Clone(req.Context())
	clone.URL.Scheme, clone.URL.Host = s.local.Scheme, s.local.Host
	return http.DefaultTransport.RoundTrip(clone)
}

type friendService struct {
	mu     sync.Mutex
	events []string
	world  p2p.World
	fail   bool
}

// record stores lifecycle events in service-observed order.
func (s *friendService) record(event string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.events = append(s.events, event)
}

// observed returns a stable copy of the service lifecycle.
func (s *friendService) observed() []string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]string(nil), s.events...)
}

// ServeHTTP implements the bounded listing, join, leave, and RTA fixture.
func (s *friendService) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	if strings.EqualFold(r.Header.Get("Upgrade"), "websocket") {
		s.serveRTA(w, r)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	switch {
	case strings.Contains(r.URL.Path, "/titles/"):
		_, _ = io.WriteString(w, `{"EndPoints":[{"Protocol":"https","Host":"*.xboxlive.com","HostType":"wildcard","RelyingParty":"http://xboxlive.com","TokenType":"JWT"}]}`)
	case r.URL.Path == "/handles/query":
		if s.fail {
			http.Error(w, "fixture failed", http.StatusBadRequest)
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"results": []any{map[string]any{"id": uuid.New(), "relatedInfo": map[string]bool{"closed": false}, "customProperties": s.world}}})
	case r.URL.Path == "/handles" && r.Method == http.MethodPost:
		w.WriteHeader(http.StatusCreated)
	case r.Method == http.MethodPut:
		var body struct {
			Members map[string]json.RawMessage `json:"members"`
		}
		_ = json.NewDecoder(r.Body).Decode(&body)
		if string(body.Members["me"]) == "null" {
			s.record("leave")
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if strings.HasPrefix(r.URL.Path, "/handles/") {
			s.record("join")
		}
		w.Header().Set("Content-Location", "/serviceconfigs/00000000-0000-0000-0000-000000000001/sessiontemplates/fixture/sessions/fixture")
		w.Header().Set("ETag", `"fixture"`)
		_ = json.NewEncoder(w).Encode(map[string]any{"properties": map[string]any{"custom": s.world}, "members": map[string]any{"me": map[string]any{"constants": map[string]any{"system": map[string]string{"xuid": "123"}}}}})
	default:
		http.Error(w, "unexpected fixture request: "+r.URL.Path, http.StatusBadRequest)
	}
}

// serveRTA acknowledges subscription operations and records when they stop.
func (s *friendService) serveRTA(w http.ResponseWriter, r *http.Request) {
	conn, err := websocket.Accept(w, r, &websocket.AcceptOptions{Subprotocols: []string{"rta.xboxlive.com"}})
	if err != nil {
		return
	}
	defer conn.CloseNow()
	for {
		var request []json.RawMessage
		if wsjson.Read(r.Context(), conn, &request) != nil {
			return
		}
		var operation, sequence int
		_ = json.Unmarshal(request[0], &operation)
		_ = json.Unmarshal(request[1], &sequence)
		response := []any{operation, sequence, 0}
		if operation == 1 {
			s.record("subscribe")
			response = append(response, 1, map[string]string{"ConnectionId": uuid.NewString()})
		} else {
			s.record("unsubscribe")
		}
		if wsjson.Write(r.Context(), conn, response) != nil {
			return
		}
	}
}

func TestFriendTargetRetainsXboxUntilAfterLeaving(t *testing.T) {
	for _, fail := range []bool{false, true} {
		t.Run(fmt.Sprint(fail), func(t *testing.T) {
			service := &friendService{fail: fail, world: p2p.World{OwnerID: "456", HostName: "Host", MemberCount: 1, BroadcastSetting: p2p.BroadcastSettingFriendsOfFriends, TransportLayer: p2p.TransportLayerNetherNet, Joinability: p2p.JoinabilityFriends, Nonces: map[string]string{"123": "fixture-nonce"}, SupportedConnections: []p2p.Connection{{Type: p2p.ConnectionTypeSignalingOverJSONRPC, PlayerMessagingID: uuid.New(), NetherNetID: p2p.NetherNetID(uuid.NewString())}}}}
			server := httptest.NewServer(service)
			defer server.Close()
			local, _ := url.Parse(server.URL)
			key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
			if err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			xbl, err := (xsapi.ClientConfig{RTAMode: xsapi.RTALazy, HTTPClient: &http.Client{Transport: friendTransport{local: local}}, Logger: slog.New(slog.DiscardHandler)}).New(ctx, friendTokens{key: key})
			if err != nil {
				t.Fatal(err)
			}
			defer xbl.Close()
			target, err := resolveFriendWorld(ctx, "456", xbl, nil, slog.New(slog.DiscardHandler))
			if fail {
				if err == nil {
					t.Fatal("failed listing returned a target")
				}
			} else {
				if err != nil {
					t.Fatal(err)
				}
				if got := fmt.Sprint(service.observed()); got != "[subscribe join]" {
					t.Fatalf("resolved lifecycle = %s", got)
				}
				if err := target.close(); err != nil {
					t.Fatal(err)
				}
				if got := fmt.Sprint(service.observed()); got != "[subscribe join leave unsubscribe]" {
					t.Fatalf("closed lifecycle = %s", got)
				}
			}
			request, _ := http.NewRequestWithContext(ctx, http.MethodGet, "https://sessiondirectory.xboxlive.com/closed", nil)
			if _, err := xbl.HTTPClient().Do(request); !errors.Is(err, net.ErrClosed) {
				t.Fatalf("Xbox client still open: %v", err)
			}
		})
	}
}
