package ingress

import (
	"crypto/subtle"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

type accountGate struct {
	sessionURL string
	gameSecret string
	client     *http.Client
	pending    chan struct{}
}

// ProtectAccounts validates real website sessions even when ingress is reached
// directly. Only the private game reader may use the replay service credential.
func (h *Handler) ProtectAccounts(upstream *url.URL, gameSecret string) {
	h.accounts = &accountGate{sessionURL: upstream.ResolveReference(&url.URL{Path: "/api/account/session"}).String(), gameSecret: gameSecret,
		client: &http.Client{Timeout: 3 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}, pending: make(chan struct{}, 4)}
}

func (h *Handler) SetReplays(handler http.Handler) { h.replays = handler }

func (g *accountGate) authorize(w http.ResponseWriter, r *http.Request, replay bool) bool {
	w.Header().Set("Cache-Control", "private, no-store")
	if replay && g.gameSecret != "" && strings.HasPrefix(r.Header.Get("Authorization"), "Bearer ") {
		token := strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")
		if subtle.ConstantTimeCompare([]byte(token), []byte(g.gameSecret)) == 1 {
			return true
		}
	}
	cookie, err := r.Cookie("zeno_account")
	if err != nil || cookie.Value == "" || len(cookie.Value) > 512 {
		failure(w, 401, "Log in to watch matches and replays.")
		return false
	}
	select {
	case g.pending <- struct{}{}:
		defer func() { <-g.pending }()
	default:
		failure(w, 503, "Account verification is busy. Try again shortly.")
		return false
	}
	request, err := http.NewRequestWithContext(r.Context(), http.MethodGet, g.sessionURL, nil)
	if err != nil {
		failure(w, 503, "Account verification is unavailable.")
		return false
	}
	request.AddCookie(cookie)
	response, err := g.client.Do(request)
	if err != nil {
		failure(w, 503, "Account verification is unavailable.")
		return false
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		failure(w, 503, "Account verification is unavailable.")
		return false
	}
	var result struct {
		Account *struct {
			UUID string `json:"uuid"`
			Name string `json:"name"`
		} `json:"account"`
	}
	if json.NewDecoder(io.LimitReader(response.Body, 16<<10)).Decode(&result) != nil {
		failure(w, 503, "Account verification is unavailable.")
		return false
	}
	if result.Account == nil || result.Account.UUID == "" || result.Account.Name == "" {
		failure(w, 401, "Log in to watch matches and replays.")
		return false
	}
	return true
}
