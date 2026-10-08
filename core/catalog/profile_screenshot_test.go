package catalog

import (
	"context"
	"io"
	"net/http"
	"net/url"
	"strings"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/service/gallery"
)

// TestProfileFeaturedScreenshot uses synthetic authorization and authored gallery metadata.
func TestProfileFeaturedScreenshot(t *testing.T) {
	endpoint, err := url.Parse("https://persona.fixture.test")
	if err != nil {
		t.Fatal(err)
	}
	for _, fixture := range []struct {
		body    string
		want    string
		failure bool
	}{
		{`{"result":{"showcasedImages":[{"id":"ordinary","url":"https://fixture.test/ordinary.png"},{"id":"featured","isFeatured":true,"url":"https://fixture.test/featured.png"}]}}`, "https://fixture.test/featured.png", false},
		{`{"result":{"showcasedImages":[]}}`, "", false},
		{`{"result":{"showcasedImages":[{"id":"bad","isFeatured":true,"url":"http://fixture.test/bad.png"}]}}`, "", true},
	} {
		env := &gallery.Environment{ServiceURI: endpoint, HTTPClient: &http.Client{Transport: roundTripFunc(func(request *http.Request) (*http.Response, error) {
			if request.Method != http.MethodGet || request.URL.Host != endpoint.Host || request.URL.Path != "/api/v1.0/gallery/xuid/123" {
				t.Fatalf("unexpected gallery request: %s %s", request.Method, request.URL)
			}
			if request.Header.Get("Authorization") != "MCToken synthetic" {
				t.Fatal("wrong gallery authorization")
			}
			return &http.Response{StatusCode: http.StatusOK, Header: http.Header{}, Body: io.NopCloser(strings.NewReader(fixture.body))}, nil
		})}}
		got, err := profileFeaturedScreenshot(context.Background(), env, fixedTokens{}, "123")
		if (err != nil) != fixture.failure || got.URL != fixture.want || got.Path != "" {
			t.Fatalf("featured screenshot = %+v %v", got, err)
		}
	}
}
