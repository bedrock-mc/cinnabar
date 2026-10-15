package redact

import (
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"log/slog"
	"strings"
)

// Secret is a credential such as a token or ticket. It logs and formats as the placeholder,
// so a record or %v never carries the value; slog resolves it only when a record is written.
type Secret string

// LogValue is the placeholder.
func (Secret) LogValue() slog.Value { return slog.StringValue(rules.placeholder) }

// String is the placeholder.
func (Secret) String() string { return rules.placeholder }

// Private is an identifier such as a server address or XUID. It logs and formats as
// "[redacted:<8 hex digits>]", a keyed hash of the case-folded value that is equal for equal
// values within this process and unlinkable across processes, so records about one server
// still correlate while the log never names it. The share-time rules leave that form intact.
type Private string

// processKey keys Private's hash; it never leaves the process.
var processKey = func() []byte {
	key := make([]byte, sha256.Size)
	_, _ = rand.Read(key) // crypto/rand.Read never fails; it aborts the process instead.
	return key
}()

// LogValue is the keyed hash form.
func (p Private) LogValue() slog.Value { return slog.StringValue(p.String()) }

// String is the keyed hash form.
func (p Private) String() string {
	mac := hmac.New(sha256.New, processKey)
	mac.Write([]byte(strings.ToLower(string(p))))
	return strings.TrimSuffix(rules.placeholder, "]") + ":" + hex.EncodeToString(mac.Sum(nil)[:4]) + "]"
}
