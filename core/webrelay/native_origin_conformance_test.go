package webrelay

import (
	"encoding/json"
	"net/http"
	"os"
	"testing"
)

// TestNativeOriginConformance checks the native policy corpus against the actual relay owner.
func TestNativeOriginConformance(t *testing.T) {
	data, err := os.ReadFile("../../web/gateway/tests/fixtures/origins.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		Name       string   `json:"name"`
		Origin     string   `json:"origin"`
		Host       string   `json:"host"`
		Configured []string `json:"configured"`
		Accepted   bool     `json:"accepted"`
	}
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	for _, test := range cases {
		t.Run(test.Name, func(t *testing.T) {
			relay := testRelay(t)
			relay.cfg.AllowedOrigins = test.Configured
			request := &http.Request{Host: test.Host, Header: make(http.Header)}
			request.Header.Set("Origin", test.Origin)
			if got := relay.checkOrigin(request); got != test.Accepted {
				t.Fatalf("origin policy = %t, want %t", got, test.Accepted)
			}
		})
	}
}
