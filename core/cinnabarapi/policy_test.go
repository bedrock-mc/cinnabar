package cinnabarapi

import (
	"context"
	"crypto/ecdsa"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"sync/atomic"
	"testing"
	"time"

	"github.com/coder/websocket"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
)

// TestServerMinimumAndReconnectSpread covers all retry classes without sleeping.
func TestServerMinimumAndReconnectSpread(t *testing.T) {
	now := time.Date(2026, 10, 15, 0, 0, 0, 0, time.UTC)
	low := func(int64) int64 { return 0 }
	high := func(n int64) int64 { return n - 1 }
	cases := []struct {
		name, stage string
		err         error
		minimum     time.Duration
	}{
		{"dial", "dial", errors.New("dial"), time.Second},
		{"429", "dial", retryError{errors.New("429"), time.Minute}, time.Minute},
		{"stay away", "session", websocket.CloseError{Code: 4429, Reason: "retry_after=90"}, 90 * time.Second},
		{"abnormal", "session", errors.New("EOF"), time.Second},
		{"server close", "session", websocket.CloseError{Code: 1001}, time.Second},
		{"bad client", "session", websocket.CloseError{Code: 1008}, time.Second},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := retryDelay(tc.err, tc.stage, time.Second, maxBackoff, now, low); got != tc.minimum {
				t.Fatal(got)
			}
			got := retryDelay(tc.err, tc.stage, time.Second, maxBackoff, now, high)
			if tc.stage == "session" || tc.minimum > time.Second {
				if got != tc.minimum+reconnectSpread {
					t.Fatal(got)
				}
			}
		})
	}
	for _, value := range []string{"90", now.Add(90 * time.Second).Format(http.TimeFormat), "retry_after=90"} {
		if got := retryAfter(value, now); got != 90*time.Second {
			t.Fatal(value, got)
		}
	}
	for _, value := range []string{"", "-1", "nonsense"} {
		if retryAfter(value, now) != 0 {
			t.Fatal(value)
		}
	}
}

// A welcome can lower or raise the exponential cap, while retaining jitter at either bound.
func TestAdvertisedReconnectCapReplacesTheFallback(t *testing.T) {
	for _, cap := range []time.Duration{time.Millisecond, 10 * time.Minute} {
		if got := backoff(40, cap, func(int64) int64 { return 0 }); got != cap/2 {
			t.Fatal(got)
		}
		if got := backoff(40, cap, func(n int64) int64 { return n - 1 }); got != cap {
			t.Fatal(got)
		}
	}
}

// TestCredentialExpiry rotates only near expiry and keeps nonce proofs independent of credential reuse.
func TestCredentialExpiry(t *testing.T) {
	now := time.Unix(1000, 0)
	var minted int
	source := func(_ context.Context, _ *ecdsa.PublicKey) (string, error) {
		minted++
		payload, _ := json.Marshal(map[string]int64{"exp": now.Add(time.Hour).Unix()})
		return "header." + base64.RawURLEncoding.EncodeToString(payload) + ".signature", nil
	}
	var pair credential
	if err := pair.refresh(context.Background(), source, now); err != nil {
		t.Fatal(err)
	}
	first := pair.key
	if err := pair.refresh(context.Background(), source, now.Add(time.Minute)); err != nil {
		t.Fatal(err)
	}
	if minted != 1 || first != pair.key {
		t.Fatal("early rotation")
	}
	if err := pair.refresh(context.Background(), source, now.Add(time.Hour-credentialMargin)); err != nil {
		t.Fatal(err)
	}
	if minted != 2 || first == pair.key {
		t.Fatal("expired pair reused")
	}
	signature, _ := sign(pair.key, "nonce-one")
	if !verify(&pair.key.PublicKey, "nonce-one", signature) || verify(&pair.key.PublicKey, "nonce-two", signature) {
		t.Fatal("nonce binding")
	}
}

// TestNegotiatedParametersAndBudget applies overrides and preserves the spent XUID budget.
func TestNegotiatedParametersAndBudget(t *testing.T) {
	var msg inbound
	if err := json.Unmarshal([]byte(`{"type":"welcome","lookup_batch_max":2,"lookup_bucket_capacity":4,"lookup_refill_per_min":2,"cache_ttl_ms":500,"ping_interval_ms":1000,"reconnect_max_ms":2000,"future":true}`), &msg); err != nil {
		t.Fatal(err)
	}
	p := msg.parameters.normalized()
	if p.LookupBatchMax != 2 || p.CacheTTLMS != 500 || p.PingIntervalMS != 1000 || p.ReconnectMaxMS != 2000 {
		t.Fatal(p)
	}
	if (parameters{}).normalized() != defaults() {
		t.Fatal("missing defaults")
	}
	now := time.Unix(1000, 0)
	var budget lookupBudget
	if !budget.take(now, 4, p) || budget.take(now, 1, p) {
		t.Fatal("capacity")
	}
	// A new welcome, including a larger capacity, cannot restore spent tokens.
	p.LookupBucketCapacity = 8
	if budget.take(now, 1, p) || !budget.take(now.Add(time.Minute), 2, p) {
		t.Fatal("refill or reconnect reset")
	}
}

// TestRepeatedFrameRejectionsParkUntilRestart verifies the loop exits after a bounded number of rejections.
func TestRepeatedFrameRejectionsParkUntilRestart(t *testing.T) {
	for _, code := range []websocket.StatusCode{1007, 1008, 1009} {
		t.Run(fmt.Sprint(code), func(t *testing.T) {
			api := newFakeAPI(t)
			client := newTestClient(t, api, nil)
			client.Configure(testConfig)
			for n := 0; n < rejectionLimit; n++ {
				session := receive(t, api.sessions)
				_ = session.conn.Close(code, "")
				close(session.done)
			}
			receive(t, client.done)
			if api.minted.Load() != 1 {
				t.Fatal("rejection retries minted new tokens")
			}
		})
	}
}

// TestClientBuildAndLookupSplitting exercises negotiated batch limits on the wire.
func TestClientBuildAndLookupSplitting(t *testing.T) {
	api := newFakeAPI(t)
	api.welcome = parameters{LookupBatchMax: 2, CacheTTLMS: 500}
	client := newTestClient(t, api, nil)
	config := testConfig
	config.ClientBuild = 42
	client.Configure(config)
	session := receive(t, api.sessions)
	if session.hello.ClientBuild != 42 {
		t.Fatal("build omitted")
	}
	testwaitReady(t, client)
	if client.CacheTTLMS() != 500 {
		t.Fatal("server lifetime ignored")
	}
	done := make(chan error, 1)
	go func() { _, err := client.Lookup(context.Background(), []string{"1", "2", "3"}, nil); done <- err }()
	var count atomic.Int64
	for _, size := range []int{2, 1} {
		var lookup lookupMessage
		session.receive(t, &lookup)
		if len(lookup.XUIDs) != size {
			t.Fatal(lookup)
		}
		count.Add(int64(size))
		session.send(t, map[string]any{"type": typeLookupResult, "id": lookup.ID, "cinnabar": lookup.XUIDs})
	}
	if err := receive(t, done); err != nil || count.Load() != 3 {
		t.Fatal(err)
	}
}

// testwaitReady waits for the client to publish its authenticated session.
func testwaitReady(t *testing.T, c *Client) {
	t.Helper()
	testwait.Eventually(t, testwait.DefaultTimeout, "authenticated client", func() bool { c.mu.Lock(); defer c.mu.Unlock(); return c.live != nil })
}
