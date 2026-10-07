package catalog

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/locale"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/playermessaging"
)

type messagingTokens struct{}

// ServiceToken supplies synthetic credentials without an account cache.
func (messagingTokens) ServiceToken(context.Context) (*service.Token, error) {
	return &service.Token{AuthorizationHeader: "MCToken fixture", ValidUntil: time.Now().Add(time.Hour)}, nil
}

// The session's client keeps the request shape and continuation while both calls carry the UI locale.
func TestMessagingRequestsCarryLanguage(t *testing.T) {
	var bodies []map[string]any
	transport := roundTripFunc(func(request *http.Request) (*http.Response, error) {
		if request.Header.Get("Accept-Language") != "fr-FR" {
			t.Fatalf("Accept-Language = %q", request.Header.Get("Accept-Language"))
		}
		if request.Method != http.MethodPost || request.URL.RawQuery != "" {
			t.Fatalf("request = %s %s", request.Method, request.URL)
		}
		if request.Header.Get("Authorization") != "MCToken fixture" || request.Header.Get("Content-Type") != "application/json" {
			t.Fatal("messaging authorization or content type changed")
		}
		var body map[string]any
		if err := json.NewDecoder(request.Body).Decode(&body); err != nil {
			t.Fatal(err)
		}
		bodies = append(bodies, body)
		data := `{"result":{"continuationToken":"next","messages":[]}}`
		if request.URL.Path == "/api/v1.0/messages/event" {
			data = `{}`
		} else if request.URL.Path != "/api/v1.0/session/refresh" {
			t.Fatalf("path = %q", request.URL.Path)
		}
		return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(data)), Header: http.Header{}}, nil
	})
	previous := http.DefaultClient.Transport
	http.DefaultClient.Transport = transport
	t.Cleanup(func() { http.DefaultClient.Transport = previous })
	discovery := &service.Discovery{ServiceEnvironments: map[string]map[string]json.RawMessage{
		"messaging": {"prod": json.RawMessage(`{"serviceUri":"https://messaging.fixture.test"}`)},
	}}
	client, err := NewMessagingSession("fr-FR").get(discovery, messagingTokens{})
	if err != nil {
		t.Fatal(err)
	}
	for range 2 {
		if _, err := client.Refresh(context.Background()); err != nil {
			t.Fatal(err)
		}
	}
	if err := client.ReportEvents(context.Background(), playermessaging.Event{
		Type: playermessaging.EventImpression, InstanceID: "fixture-instance", ReportID: "fixture-report",
	}); err != nil {
		t.Fatal(err)
	}
	if len(bodies[0]) != 2 || bodies[0]["sessionId"] != client.SessionID() || bodies[0]["continuationToken"] != "" || bodies[1]["continuationToken"] != "next" {
		t.Fatalf("refresh bodies = %+v", bodies[:2])
	}
	if bodies[2]["SessionId"] != client.SessionID() || bodies[2]["continuationToken"] != "next" {
		t.Fatalf("event body = %+v", bodies[2])
	}
	events := bodies[2]["events"].([]any)
	event := events[0].(map[string]any)
	if event["eventType"] != "Impression" || event["instanceId"] != "fixture-instance" || event["reportId"] != "fixture-report" || event["sessionId"] != client.SessionID() {
		t.Fatalf("event = %+v", event)
	}
}

// A session opened before the UI language is known asks in the default locale.
func TestMessagingDefaultsToTheDefaultLocale(t *testing.T) {
	var language string
	previous := http.DefaultClient.Transport
	http.DefaultClient.Transport = roundTripFunc(func(request *http.Request) (*http.Response, error) {
		language = request.Header.Get("Accept-Language")
		return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(`{"result":{"messages":[]}}`)), Header: http.Header{}}, nil
	})
	t.Cleanup(func() { http.DefaultClient.Transport = previous })
	discovery := &service.Discovery{ServiceEnvironments: map[string]map[string]json.RawMessage{
		"messaging": {"prod": json.RawMessage(`{"serviceUri":"https://messaging.fixture.test"}`)},
	}}
	client, err := NewMessagingSession("").get(discovery, messagingTokens{})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := client.Refresh(context.Background()); err != nil || language != locale.Default {
		t.Fatalf("Accept-Language = %q err = %v", language, err)
	}
}
