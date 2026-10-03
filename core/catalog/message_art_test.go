package catalog

import (
	"io"
	"net/http"
	"strings"
	"testing"
)

type artRoundTripper struct{ body string }

// RoundTrip returns an authored service envelope without using the network.
func (r artRoundTripper) RoundTrip(*http.Request) (*http.Response, error) {
	return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(r.body))}, nil
}

func TestMessageArtPreservesEnvelopeAndFields(t *testing.T) {
	const body = `{"result":{"messages":[{"instanceId":"i","messageText":{"bannerText":"Add-ons!"},"colors":{"BannerTextColor":{"hexColor":{"r":10,"g":20,"b":30}}}}]}}`
	for _, path := range []string{"/session/refresh", "/unrelated"} {
		art := &messageArt{}
		transport := messageArtTransport{RoundTripper: artRoundTripper{body}, art: art}
		request, _ := http.NewRequest(http.MethodPost, "https://example.invalid"+path, nil)
		response, err := transport.RoundTrip(request)
		if err != nil {
			t.Fatal(err)
		}
		received, err := io.ReadAll(response.Body)
		response.Body.Close()
		if err != nil || string(received) != body {
			t.Fatalf("envelope changed: %q, %v", received, err)
		}
		messages := []Message{{InstanceID: "i"}}
		art.apply(messages)
		if path == "/session/refresh" {
			if messages[0].Banner != "Add-ons!" || messages[0].Colors["BannerTextColor"] != [3]uint8{10, 20, 30} {
				t.Fatalf("art fields lost: %+v", messages[0])
			}
		} else if messages[0].Banner != "" {
			t.Fatal("observed unrelated response")
		}
	}
}
