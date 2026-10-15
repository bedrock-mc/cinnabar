package cinnabarapi

import (
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/coder/websocket"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

const reconnectSpread = 30 * time.Second
const credentialMargin = 2 * time.Minute
const rejectionLimit = 3

// parameters holds the current server's optional operating limits.
type parameters struct {
	LookupBatchMax       int   `json:"lookup_batch_max"`
	LookupBucketCapacity int   `json:"lookup_bucket_capacity"`
	LookupRefillPerMin   int   `json:"lookup_refill_per_min"`
	CacheTTLMS           int64 `json:"cache_ttl_ms"`
	PingIntervalMS       int64 `json:"ping_interval_ms"`
	ReconnectMaxMS       int64 `json:"reconnect_max_ms"`
}

// defaults supplies limits for older v1 servers that omit operating parameters.
func defaults() parameters {
	return parameters{
		LookupBatchMax:       bridgecontract.CinnabarAPIMaxLookupXUIDs,
		LookupBucketCapacity: bridgecontract.CinnabarAPILookupBucketCapacity,
		LookupRefillPerMin:   bridgecontract.CinnabarAPILookupRefillPerMin,
		CacheTTLMS:           bridgecontract.CinnabarAPICacheTTLMS,
		PingIntervalMS:       bridgecontract.CinnabarAPIPingIntervalMS,
		ReconnectMaxMS:       bridgecontract.CinnabarAPIReconnectMaxMS,
	}
}

// normalized ignores unusable extensions, including durations that would overflow.
func (p parameters) normalized() parameters {
	d := defaults()
	if p.LookupBatchMax > 0 {
		d.LookupBatchMax = min(p.LookupBatchMax, d.LookupBatchMax)
	}
	if p.LookupBucketCapacity > 0 {
		d.LookupBucketCapacity = p.LookupBucketCapacity
	}
	if p.LookupRefillPerMin > 0 {
		d.LookupRefillPerMin = p.LookupRefillPerMin
	}
	for _, pair := range [][2]*int64{{&d.CacheTTLMS, &p.CacheTTLMS}, {&d.PingIntervalMS, &p.PingIntervalMS}, {&d.ReconnectMaxMS, &p.ReconnectMaxMS}} {
		if *pair[1] > 0 && *pair[1] <= int64((1<<63-1)/(3*time.Millisecond)) {
			*pair[0] = *pair[1]
		}
	}
	return d
}

// tokenExpiry reads only the lifetime of an issuer-provided token; the server verifies its signature.
func tokenExpiry(token string) time.Time {
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		return time.Time{}
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return time.Time{}
	}
	var claims struct {
		Exp int64 `json:"exp"`
	}
	if json.Unmarshal(payload, &claims) != nil || claims.Exp <= 0 {
		return time.Time{}
	}
	return time.Unix(claims.Exp, 0)
}

// retryError retains a server-directed minimum while preserving the transport error's close code.
type retryError struct {
	error
	after time.Duration
}

// Unwrap exposes the underlying WebSocket or HTTP failure.
func (e retryError) Unwrap() error { return e.error }

// retryAfter accepts HTTP delta seconds, an HTTP date, or the 4429 reason's delta seconds.
func retryAfter(value string, now time.Time) time.Duration {
	value = strings.TrimSpace(strings.TrimPrefix(value, "retry_after="))
	if seconds, err := strconv.ParseUint(value, 10, 64); err == nil {
		limit := uint64(((1<<63 - 1) - reconnectSpread) / time.Second)
		return time.Duration(min(seconds, limit)) * time.Second
	}
	if at, err := http.ParseTime(value); err == nil {
		return min(max(0, at.Sub(now)), (1<<63-1)-reconnectSpread)
	}
	return 0
}

// retryDelay spreads every retry and keeps server minima outside the exponential cap.
func retryDelay(err error, stage string, delay, cap time.Duration, now time.Time, random func(int64) int64) time.Duration {
	if delay > cap {
		delay = cap/2 + time.Duration(random(int64(cap-cap/2)+1))
	}
	var minimum time.Duration
	var refused retryError
	if errors.As(err, &refused) {
		minimum = refused.after
	}
	var closed websocket.CloseError
	if errors.As(err, &closed) && closed.Code == 4429 {
		minimum = retryAfter(closed.Reason, now)
	}
	delay = max(delay, minimum)
	if minimum > 0 || (stage == "session" || stage == "handshake") || websocket.CloseStatus(err) >= 1000 {
		delay += time.Duration(random(int64(reconnectSpread) + 1))
	}
	return delay
}

// lookupBudget counts XUIDs across reconnects, refilling from the caller's clock.
type lookupBudget struct {
	tokens      float64
	at          time.Time
	initialized bool
}

// take reserves one batch without allowing a reconnect or new welcome to reset the account budget.
func (b *lookupBudget) take(now time.Time, count int, p parameters) bool {
	if !b.initialized {
		b.tokens = float64(p.LookupBucketCapacity)
		b.initialized = true
	}
	if !b.at.IsZero() {
		b.tokens += max(0, now.Sub(b.at).Minutes()) * float64(p.LookupRefillPerMin)
	}
	b.tokens = min(b.tokens, float64(p.LookupBucketCapacity))
	b.at = now
	if b.tokens < float64(count) {
		return false
	}
	b.tokens -= float64(count)
	return true
}
