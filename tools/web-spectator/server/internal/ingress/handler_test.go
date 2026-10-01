package ingress

import (
	"bufio"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"net/url"
	"strings"
	"testing"
	"time"

	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/spectator"
)

func testHandler(t *testing.T) (*Handler, *spectator.Store) {
	t.Helper()
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { _, _ = io.WriteString(w, "website") }))
	t.Cleanup(upstream.Close)
	parsed, _ := url.Parse(upstream.URL)
	origin, _ := url.Parse("https://dev.zenomc.org")
	s := spectator.NewStore()
	now := time.Now()
	arena := spectator.ArenaPart{Version: spectator.Version, Arena: spectator.Arena{ID: "arena", Name: "Test", Palette: []spectator.PaletteEntry{{Name: "minecraft:air", States: map[string]any{}}, {Name: "minecraft:stone", States: map[string]any{}}}, Bounds: [6]int32{0, 0, 0, 15, 15, 15}, Blocks: [][4]int32{{0, 0, 0, 1}}}, Parts: 1}
	frame := spectator.Frame{Version: spectator.Version, ID: "match", ArenaID: "arena", Mode: "boxing", UpdatedAt: now, Players: []spectator.Player{{ID: "one", Name: "One", MaxHealth: 20, Health: 20}, {ID: "two", Name: "Two", Team: 1, MaxHealth: 20, Health: 20}}, TeamWins: []int{0, 0}}
	for _, message := range []struct {
		subject string
		value   any
	}{{spectator.ArenaSubject, arena}, {spectator.FrameSubject, frame}} {
		data, err := json.Marshal(message.value)
		if err != nil {
			t.Fatal(err)
		}
		if err := s.Accept(message.subject, data, now); err != nil {
			t.Fatal(err)
		}
	}
	return New(s, parsed, origin, []netip.Prefix{netip.MustParsePrefix("127.0.0.1/32")}), s
}

func TestReadOnlySurfaceAndOrigin(t *testing.T) {
	h, _ := testHandler(t)
	for _, path := range []string{"/api/spectator/duels", "/api/spectator/duels/match/arena", "/api/spectator/duels/match/events", "/api/sign-in", "/api/checkout", "/"} {
		response := httptest.NewRecorder()
		h.ServeHTTP(response, httptest.NewRequest(http.MethodPost, path, strings.NewReader(`{"command":"attack"}`)))
		if response.Code != http.StatusMethodNotAllowed {
			t.Fatalf("mutation escaped read-only guard at %s: %d", path, response.Code)
		}
	}
	request := httptest.NewRequest(http.MethodGet, apiPrefix, nil)
	request.Header.Set("Origin", "https://attacker.example")
	response := httptest.NewRecorder()
	h.ServeHTTP(response, request)
	if response.Code != http.StatusForbidden {
		t.Fatal("cross-origin spectator request accepted")
	}
	request = httptest.NewRequest(http.MethodGet, apiPrefix, nil)
	request.Header.Set("Sec-Fetch-Site", "cross-site")
	response = httptest.NewRecorder()
	h.ServeHTTP(response, request)
	if response.Code != http.StatusForbidden {
		t.Fatal("cross-site spectator request accepted")
	}
}

func TestCatalogGeometryAndMissingMatches(t *testing.T) {
	h, store := testHandler(t)
	for _, path := range []string{apiPrefix, apiPrefix + "/match/arena"} {
		response := httptest.NewRecorder()
		h.ServeHTTP(response, httptest.NewRequest(http.MethodGet, path, nil))
		if response.Code != http.StatusOK || response.Header().Get("Cache-Control") != "no-store" {
			t.Fatalf("bad public response: %d %v", response.Code, response.Header())
		}
		if !json.Valid(response.Body.Bytes()) {
			t.Fatal("invalid response JSON")
		}
	}
	response := httptest.NewRecorder()
	h.ServeHTTP(response, httptest.NewRequest(http.MethodHead, apiPrefix+"/match/arena", nil))
	if response.Code != http.StatusOK || response.Body.Len() != 0 {
		t.Fatal("HEAD returned body or failed")
	}
	for _, path := range []string{apiPrefix + "/../arena", apiPrefix + "/missing/arena", apiPrefix + "/match/control"} {
		response := httptest.NewRecorder()
		h.ServeHTTP(response, httptest.NewRequest(http.MethodGet, path, nil))
		if response.Code != http.StatusNotFound {
			t.Fatalf("bad route accepted: %s %d", path, response.Code)
		}
	}
	store.Close("match", time.Now())
	response = httptest.NewRecorder()
	h.ServeHTTP(response, httptest.NewRequest(http.MethodGet, apiPrefix+"/match/arena", nil))
	if response.Code != http.StatusNotFound {
		t.Fatal("closed geometry remains public")
	}
	response = httptest.NewRecorder()
	h.ServeHTTP(response, httptest.NewRequest(http.MethodGet, apiPrefix, nil))
	if strings.Contains(response.Body.String(), "One") || !strings.Contains(response.Body.String(), `"duels":[]`) {
		t.Fatal("closed fighter exposed in catalog")
	}
}

func TestForwardedIPRequiresTrustedPeer(t *testing.T) {
	h, _ := testHandler(t)
	request := httptest.NewRequest(http.MethodGet, apiPrefix, nil)
	request.RemoteAddr = "192.0.2.1:1234"
	request.Header.Set("X-Real-IP", "203.0.113.10")
	if actual := h.visitorIP(request).String(); actual != "192.0.2.1" {
		t.Fatalf("spoofed visitor identity accepted: %s", actual)
	}
	request.RemoteAddr = "127.0.0.1:1234"
	if actual := h.visitorIP(request).String(); actual != "203.0.113.10" {
		t.Fatalf("trusted visitor identity lost: %s", actual)
	}
	request.Header.Set("X-Real-IP", "invalid")
	if actual := h.visitorIP(request).String(); actual != "127.0.0.1" {
		t.Fatal("invalid forwarded IP accepted")
	}
}

func TestNativeHTTPStreamClosesOnConsentAndStaleness(t *testing.T) {
	for _, reason := range []string{"consent", "stale"} {
		t.Run(reason, func(t *testing.T) {
			h, store := testHandler(t)
			server := httptest.NewServer(h)
			defer server.Close()
			client := &http.Client{Timeout: 3 * time.Second}
			response, err := client.Get(server.URL + apiPrefix + "/match/events")
			if err != nil {
				t.Fatal(err)
			}
			defer response.Body.Close()
			if response.StatusCode != http.StatusOK || response.Header.Get("Content-Type") != "text/event-stream" {
				t.Fatal("SSE request failed")
			}
			reader := bufio.NewReader(response.Body)
			initial := readEvent(t, reader)
			if !strings.HasPrefix(initial, "event: frame\n") || !strings.Contains(initial, `"name":"One"`) {
				t.Fatalf("initial frame missing: %s", initial)
			}
			if reason == "consent" {
				store.Close("match", time.Now())
			} else {
				store.Sweep(time.Now().Add(spectator.Freshness + time.Second))
			}
			closed := readEvent(t, reader)
			if !strings.HasPrefix(closed, "event: closed\n") || strings.Contains(closed, "One") {
				t.Fatalf("unexpected closure event: %s", closed)
			}
			if _, err := reader.ReadByte(); err != io.EOF {
				t.Fatalf("closed stream retained data: %v", err)
			}
		})
	}
}

func readEvent(t *testing.T, reader *bufio.Reader) string {
	t.Helper()
	var result strings.Builder
	for {
		line, err := reader.ReadString('\n')
		if err != nil {
			t.Fatal(err)
		}
		result.WriteString(line)
		if line == "\n" {
			return result.String()
		}
	}
}
