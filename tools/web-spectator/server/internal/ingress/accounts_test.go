package ingress

import (
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"testing"
)

func TestAccountGateRejectsForgedCookiesAndFailsClosed(t *testing.T) {
	account := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		cookie, _ := r.Cookie("zeno_account")
		if cookie != nil && cookie.Value == "valid" {
			_, _ = w.Write([]byte(`{"account":{"uuid":"player-id","name":"Player"}}`))
			return
		}
		if cookie != nil && cookie.Value == "unavailable" {
			w.WriteHeader(503)
			return
		}
		_, _ = w.Write([]byte(`{"account":null}`))
	}))
	defer account.Close()
	upstream, _ := url.Parse(account.URL)
	h := &Handler{}
	secret := strings.Repeat("x", 32)
	h.ProtectAccounts(upstream, secret)
	for _, test := range []struct {
		cookie, bearer string
		replay         bool
		status         int
	}{
		{status: 401}, {cookie: "forged", status: 401}, {cookie: "valid", status: 200}, {cookie: "unavailable", status: 503},
		{bearer: "Bearer " + secret, replay: true, status: 200}, {bearer: secret, replay: true, status: 401}, {bearer: "Bearer " + secret, status: 401},
	} {
		request := httptest.NewRequest("GET", "/api/replays", nil)
		if test.cookie != "" {
			request.AddCookie(&http.Cookie{Name: "zeno_account", Value: test.cookie})
		}
		request.Header.Set("Authorization", test.bearer)
		response := httptest.NewRecorder()
		allowed := h.accounts.authorize(response, request, test.replay)
		if allowed != (test.status == 200) || response.Code != test.status {
			t.Fatalf("cookie %q replay=%v: allowed=%v status=%d wanted=%d", test.cookie, test.replay, allowed, response.Code, test.status)
		}
	}
}
