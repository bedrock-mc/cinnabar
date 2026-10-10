package catalog

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"github.com/sandertv/gophertunnel/minecraft/realms"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"golang.org/x/oauth2"
)

type roundTripFunc func(*http.Request) (*http.Response, error)

func (f roundTripFunc) RoundTrip(request *http.Request) (*http.Response, error) {
	return f(request)
}

func tlsTransport(t *testing.T, servers ...*httptest.Server) *http.Transport {
	t.Helper()
	roots := x509.NewCertPool()
	for _, server := range servers {
		roots.AddCert(server.Certificate())
	}
	transport := &http.Transport{TLSClientConfig: &tls.Config{RootCAs: roots}}
	t.Cleanup(transport.CloseIdleConnections)
	return transport
}

func assertEmptyDirectory(t *testing.T, directory string) {
	t.Helper()
	entries, err := os.ReadDir(directory)
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 0 {
		t.Fatalf("download failure published files: %v", entries)
	}
}

type fixedRealmsXSTS struct{}

func (fixedRealmsXSTS) Token() (*oauth2.Token, error) { return nil, io.EOF }

func (fixedRealmsXSTS) XSTSToken(context.Context, string) (*xsts.Token, error) {
	token := new(xsts.Token)
	err := json.Unmarshal([]byte(`{"Token":"synthetic","NotAfter":"`+time.Now().Add(time.Hour).Format(time.RFC3339)+
		`","DisplayClaims":{"xui":[{"uhs":"hash"}]}}`), token)
	return token, err
}

// Listing Realms never joins one; the game joins only when the player picks a Realm.
func TestRealmsListingDoesNotJoin(t *testing.T) {
	var joins []string
	client := realms.NewClient(fixedRealmsXSTS{}, &http.Client{Transport: roundTripFunc(func(req *http.Request) (*http.Response, error) {
		body := `{"servers":[{"id":7,"name":"R","state":"OPEN"}]}`
		if req.Method != http.MethodGet {
			joins = append(joins, req.Method+" "+req.URL.Path)
			body = `{"address":"1.2.3.4:19132","networkProtocol":"DEFAULT"}`
		}
		return &http.Response{StatusCode: http.StatusOK, Header: make(http.Header), Body: io.NopCloser(strings.NewReader(body))}, nil
	})})
	list, err := listRealms(context.Background(), client)
	if err != nil || len(list) != 1 || list[0].Target != "realm_id/7" {
		t.Fatalf("realms = %+v, err = %v", list, err)
	}
	if len(joins) != 0 {
		t.Fatalf("listing sent %v", joins)
	}
}

// The account's Realms client is built once, on the discovered endpoint, so its version
// negotiation and token cache last across calls.
func TestRealmsClientIsSharedOnTheDiscoveredEndpoint(t *testing.T) {
	var paths []string
	server := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		paths = append(paths, r.Host+r.URL.Path)
		_, _ = io.WriteString(w, `{"servers":[]}`)
	}))
	defer server.Close()
	discovery := &service.Discovery{ServiceEnvironments: map[string]map[string]json.RawMessage{
		"realms_frontend_bedrock_legacy": {"prod": json.RawMessage(`{"serviceUri":"` + server.URL + `"}`)},
	}}
	var cache realmsClients
	first, err := cache.get(discovery, fixedRealmsXSTS{}, server.Client())
	if err != nil {
		t.Fatal(err)
	}
	if second, _ := cache.get(discovery, fixedRealmsXSTS{}, server.Client()); second != first {
		t.Fatal("a second call built another Realms client")
	}
	if _, err := listRealms(context.Background(), first); err != nil || len(paths) != 1 || paths[0] != strings.TrimPrefix(server.URL, "https://")+"/worlds" {
		t.Fatalf("paths = %v err = %v", paths, err)
	}
}

// A discovered Realms endpoint that is not absolute https is refused rather than used.
func TestRealmsClientRefusesAnInsecureEndpoint(t *testing.T) {
	discovery := &service.Discovery{ServiceEnvironments: map[string]map[string]json.RawMessage{
		"realms_frontend_bedrock_legacy": {"prod": json.RawMessage(`{"serviceUri":"http://realms.example.test"}`)},
	}}
	var cache realmsClients
	if client, err := cache.get(discovery, fixedRealmsXSTS{}, nil); err == nil || client != nil {
		t.Fatalf("client = %v err = %v, want a refusal", client, err)
	}
}
