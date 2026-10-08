package catalog

import (
	"bytes"
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"sync"
)

// messageArt keeps ButtonArt fields not yet exposed by the messaging library.
type messageArt struct {
	mu         sync.Mutex
	byInstance map[string]buttonArtFields
}

type buttonArtFields struct {
	InstanceID string `json:"instanceId"`
	Text       struct {
		Banner string `json:"bannerText"`
	} `json:"messageText"`
	Colors map[string]struct {
		RGB struct{ R, G, B uint8 } `json:"hexColor"`
	} `json:"colors"`
}

type messageArtTransport struct {
	http.RoundTripper
	art *messageArt
}

// RoundTrip observes only refresh response artwork and preserves the original response bytes.
func (m messageArtTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	response, err := m.RoundTripper.RoundTrip(request)
	if err != nil || response == nil || response.Body == nil || !strings.HasSuffix(request.URL.Path, "/session/refresh") {
		return response, err
	}
	const limit = 4 << 20
	body, readErr := io.ReadAll(io.LimitReader(response.Body, limit+1))
	response.Body = struct {
		io.Reader
		io.Closer
	}{io.MultiReader(bytes.NewReader(body), response.Body), response.Body}
	if readErr == nil && len(body) <= limit {
		m.art.observe(body)
	}
	return response, nil
}

// observe retains only the known ButtonArt fields from a successful session envelope.
func (m *messageArt) observe(body []byte) {
	var envelope struct {
		Result struct {
			Messages []buttonArtFields `json:"messages"`
		} `json:"result"`
	}
	if json.Unmarshal(body, &envelope) != nil {
		return
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	m.byInstance = make(map[string]buttonArtFields)
	for _, message := range envelope.Result.Messages {
		m.byInstance[message.InstanceID] = message
	}
}

// apply adds ribbon text and native RGB colors after the ordinary message decode.
func (m *messageArt) apply(messages []Message) {
	m.mu.Lock()
	defer m.mu.Unlock()
	for i := range messages {
		extra, ok := m.byInstance[messages[i].InstanceID]
		if !ok {
			continue
		}
		if extra.Text.Banner != "" {
			messages[i].Banner = extra.Text.Banner
		}
		messages[i].Colors = make(map[string][3]uint8)
		for name, color := range extra.Colors {
			messages[i].Colors[name] = [3]uint8{color.RGB.R, color.RGB.G, color.RGB.B}
		}
	}
}
