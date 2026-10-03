package ingress

import (
	"encoding/json"
	"net"
	"net/http"
	"net/http/httputil"
	"net/netip"
	"net/url"
	"strings"
	"time"

	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/spectator"
)

const apiPrefix = "/api/spectator/duels"
const writeTimeout = 3 * time.Second

type Handler struct {
	audio     *Audio
	store     *spectator.Store
	proxy     *httputil.ReverseProxy
	origin    *url.URL
	trusted   []netip.Prefix
	limits    *limits
	downloads chan struct{}
	assets    *Assets
	replays   http.Handler
	accounts  *accountGate
}

func New(store *spectator.Store, upstream, origin *url.URL, trusted []netip.Prefix, assets ...*Assets) *Handler {
	h := &Handler{store: store, origin: origin, trusted: trusted, limits: newLimits(), downloads: make(chan struct{}, 2)}
	if len(assets) > 0 {
		h.assets = assets[0]
	}
	h.proxy = &httputil.ReverseProxy{
		Rewrite: func(r *httputil.ProxyRequest) {
			r.SetURL(upstream)
			ip := h.visitorIP(r.In)
			r.Out.Header.Set("X-Real-IP", ip.String())
			r.Out.Header.Set("X-Forwarded-For", ip.String())
			r.Out.Header.Set("X-Forwarded-Host", origin.Host)
			r.Out.Header.Set("X-Forwarded-Proto", origin.Scheme)
		},
		Transport: &http.Transport{Proxy: http.ProxyFromEnvironment, DialContext: (&net.Dialer{Timeout: 3 * time.Second, KeepAlive: 30 * time.Second}).DialContext, MaxIdleConns: 64, MaxIdleConnsPerHost: 32, IdleConnTimeout: 60 * time.Second, ResponseHeaderTimeout: 15 * time.Second},
		ErrorHandler: func(w http.ResponseWriter, _ *http.Request, _ error) {
			failure(w, http.StatusBadGateway, "The Zeno preview is temporarily unavailable.")
		},
	}
	return h
}

func (h *Handler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	// The entire dev preview is read-only, including proxied website account/store routes.
	if r.Method != http.MethodGet && r.Method != http.MethodHead {
		methodDenied(w)
		return
	}
	if r.URL.Path == "/healthz" {
		if r.Method != http.MethodGet && r.Method != http.MethodHead {
			methodDenied(w)
			return
		}
		writeJSON(w, r, map[string]bool{"ok": true})
		return
	}
	isReplay := r.URL.Path == "/api/replays" || strings.HasPrefix(r.URL.Path, "/api/replays/")
	if !isReplay && !strings.HasPrefix(r.URL.Path, "/api/spectator") {
		h.proxy.ServeHTTP(w, r)
		return
	}
	w.Header().Set("Cache-Control", "no-store")
	w.Header().Set("X-Content-Type-Options", "nosniff")
	if r.Method != http.MethodGet && r.Method != http.MethodHead {
		methodDenied(w)
		return
	}
	if supplied := r.Header.Get("Origin"); supplied != "" && supplied != h.origin.String() {
		failure(w, http.StatusForbidden, "Use the Zeno preview to watch a duel.")
		return
	}
	if r.Header.Get("Sec-Fetch-Site") == "cross-site" {
		failure(w, http.StatusForbidden, "Use the Zeno preview to watch a duel.")
		return
	}
	if !isReplay && r.URL.RawQuery != "" {
		failure(w, http.StatusBadRequest, "This spectator route does not accept parameters.")
		return
	}
	stream := strings.HasSuffix(r.URL.Path, "/events") && r.Method == http.MethodGet
	release, allowed := h.limits.admit(h.visitorIP(r), stream, time.Now())
	if !allowed {
		w.Header().Set("Retry-After", "5")
		failure(w, http.StatusTooManyRequests, "Too many spectator requests. Try again shortly.")
		return
	}
	defer release()
	if h.accounts != nil && !h.accounts.authorize(w, r, isReplay) {
		return
	}
	if isReplay {
		if h.replays == nil {
			failure(w, 503, "Replays are not available.")
			return
		}
		h.replays.ServeHTTP(w, r)
		return
	}
	if strings.HasPrefix(r.URL.Path, "/api/spectator/audio/") {
		h.audioRoute(w, r)
		return
	}
	if r.URL.Path == "/api/spectator/assets" || strings.HasPrefix(r.URL.Path, "/api/spectator/assets/") {
		h.assetsRoute(w, r)
		return
	}
	if r.URL.Path == apiPrefix {
		_ = http.NewResponseController(w).SetWriteDeadline(time.Now().Add(writeTimeout))
		_ = h.store.WithList(time.Now(), func(frames []spectator.Frame) error {
			w.Header().Set("Content-Type", "application/json")
			if r.Method == http.MethodHead {
				w.WriteHeader(http.StatusOK)
				return nil
			}
			return json.NewEncoder(w).Encode(struct {
				Duels []spectator.Frame `json:"duels"`
			}{frames})
		})
		return
	}
	path, ok := strings.CutPrefix(r.URL.Path, apiPrefix+"/")
	if !ok {
		failure(w, http.StatusNotFound, "Spectator route not found.")
		return
	}
	id, operation, found := strings.Cut(path, "/")
	if !found || !spectator.ValidID(id) || (strings.Contains(operation, "/") && !strings.HasPrefix(operation, "skins/")) {
		failure(w, http.StatusNotFound, "Spectator route not found.")
		return
	}
	live := h.store.Lookup(id)
	if live == nil {
		failure(w, http.StatusNotFound, "This duel is no longer available to watch.")
		return
	}
	if hash, ok := strings.CutPrefix(operation, "skins/"); ok {
		h.skin(w, r, id, hash, live)
		return
	}
	switch operation {
	case "arena":
		h.arena(w, r, live)
	case "events":
		h.events(w, r, id, live)
	default:
		failure(w, http.StatusNotFound, "Spectator route not found.")
	}
}

func (h *Handler) visitorIP(r *http.Request) netip.Addr {
	host, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		host = r.RemoteAddr
	}
	ip, err := netip.ParseAddr(host)
	if err != nil {
		return netip.IPv4Unspecified()
	}
	ip = ip.Unmap()
	for _, prefix := range h.trusted {
		if prefix.Contains(ip) {
			if forwarded, err := netip.ParseAddr(r.Header.Get("X-Real-IP")); err == nil && forwarded.Zone() == "" {
				return forwarded.Unmap()
			}
			break
		}
	}
	return ip
}

func (h *Handler) arena(w http.ResponseWriter, r *http.Request, live *spectator.Live) {
	if r.Method == http.MethodGet {
		select {
		case h.downloads <- struct{}{}:
			defer func() { <-h.downloads }()
		default:
			w.Header().Set("Retry-After", "2")
			failure(w, http.StatusTooManyRequests, "Arena downloads are busy. Try again shortly.")
			return
		}
	}
	controller := http.NewResponseController(w)
	_ = controller.SetWriteDeadline(time.Now().Add(writeTimeout))
	valid, _ := live.WithCurrent(time.Now(), func(_ spectator.Frame, arena *spectator.Arena) error {
		w.Header().Set("Content-Type", "application/json")
		if r.Method == http.MethodHead {
			w.WriteHeader(http.StatusOK)
			return nil
		}
		return json.NewEncoder(w).Encode(arena)
	})
	if !valid {
		failure(w, http.StatusNotFound, "This duel is no longer available to watch.")
	}
}

func writeJSON(w http.ResponseWriter, r *http.Request, value any) {
	w.Header().Set("Content-Type", "application/json")
	if r.Method == http.MethodHead {
		w.WriteHeader(http.StatusOK)
		return
	}
	_ = json.NewEncoder(w).Encode(value)
}

func methodDenied(w http.ResponseWriter) {
	w.Header().Set("Allow", "GET, HEAD")
	failure(w, http.StatusMethodNotAllowed, "Spectator routes are read-only.")
}
func failure(w http.ResponseWriter, status int, message string) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Cache-Control", "no-store")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(map[string]string{"error": message})
}
