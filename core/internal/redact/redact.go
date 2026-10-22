// Package redact keeps credentials and private data out of what the core logs.
//
// Text applies the rule file shared with the Rust client, embedded here as rules.json: a
// byte-for-byte copy of crates/crash-report/data/redaction-rules.json that a crash-report test
// keeps current. Its conformance vectors run in both languages. Text costs regular expressions,
// so it suits errors and other cold paths; hot log sites use the Secret and Private values,
// which cost nothing unless a record is written.
package redact

import (
	_ "embed"
	"encoding/json"
	"fmt"
	"net/netip"
	"regexp"
	"strings"
	"unicode/utf8"
)

//go:embed rules.json
var rulesJSON []byte

// maxLoggedError bounds a logged error message in bytes.
const maxLoggedError = 400

type ruleFile struct {
	Placeholder string `json:"placeholder"`
	Fields      struct {
		Credential []string `json:"credential"`
		Private    []string `json:"private"`
	} `json:"fields"`
	PackIdentityFields []string `json:"pack_identity_fields"`
	SourceLocation     string   `json:"source_location"`
	Timestamp          string   `json:"timestamp"`
	FileExtensions     []string `json:"file_extensions"`
	Rules              []struct {
		Name    string `json:"name"`
		Pattern string `json:"pattern"`
		Fields  string `json:"fields"`
		Check   string `json:"check"`
	} `json:"rules"`
}

// rule replaces every participating capture group of a match, or, without groups, the whole
// match as narrowed by check: "" (none), "ipv6" or "host".
type rule struct {
	pattern *regexp.Regexp
	check   string
}

type ruleSet struct {
	placeholder    string
	rules          []rule
	packIdentity   []string
	sourceLocation *regexp.Regexp
	timestamp      *regexp.Regexp
	extensions     []string
}

var rules = mustCompile(rulesJSON)

func mustCompile(data []byte) *ruleSet {
	set, err := compile(data)
	if err != nil {
		panic(fmt.Sprintf("redact: embedded rules: %v", err))
	}
	return set
}

// compile parses a rule file, substituting each rule's field list for {fields}.
func compile(data []byte) (*ruleSet, error) {
	var file ruleFile
	if err := json.Unmarshal(data, &file); err != nil {
		return nil, err
	}
	lists := map[string]string{
		"credential": strings.Join(file.Fields.Credential, "|"),
		"private":    strings.Join(file.Fields.Private, "|"),
	}
	set := &ruleSet{placeholder: file.Placeholder, packIdentity: file.PackIdentityFields, extensions: file.FileExtensions}
	for _, spec := range file.Rules {
		pattern := spec.Pattern
		if spec.Fields != "" {
			list, ok := lists[spec.Fields]
			if !ok {
				return nil, fmt.Errorf("rule %q: unknown field list %q", spec.Name, spec.Fields)
			}
			pattern = strings.ReplaceAll(pattern, "{fields}", list)
		}
		switch spec.Check {
		case "", "ipv6", "host":
		default:
			return nil, fmt.Errorf("rule %q: unknown check %q", spec.Name, spec.Check)
		}
		compiled, err := regexp.Compile(pattern)
		if err != nil {
			return nil, fmt.Errorf("rule %q: %w", spec.Name, err)
		}
		set.rules = append(set.rules, rule{pattern: compiled, check: spec.Check})
	}
	location, err := regexp.Compile(file.SourceLocation)
	if err != nil {
		return nil, fmt.Errorf("source location: %w", err)
	}
	set.sourceLocation = location
	timestamp, err := regexp.Compile(file.Timestamp)
	if err != nil {
		return nil, fmt.Errorf("timestamp: %w", err)
	}
	set.timestamp = timestamp
	return set, nil
}

// Text is text with every credential and private value the shared rules find replaced by the
// placeholder; the Rust client's crash_report::redact::patterns gives the same result.
func Text(text string) string {
	for _, rule := range rules.rules {
		text = rules.apply(rule, text, false)
	}
	return text
}

// FileName applies the shared rules to a known module filename without mistaking it for a host.
func FileName(text string) string {
	for _, rule := range rules.rules {
		text = rules.apply(rule, text, true)
	}
	return text
}

// Error is err's message passed through Text and bounded, safe to write to the core log; nil
// is empty.
func Error(err error) string {
	if err == nil {
		return ""
	}
	message := Text(err.Error())
	if len(message) <= maxLoggedError {
		return message
	}
	cut := maxLoggedError
	for cut > 0 && !utf8.RuneStart(message[cut]) {
		cut--
	}
	return message[:cut] + "…"
}

// PackIdentityKey reports whether a log attribute key names a resource pack's identity or
// location: it equals, or ends in "_" and, one of the rule file's pack identity fields.
func PackIdentityKey(key string) bool {
	key = strings.ToLower(key)
	for _, identity := range rules.packIdentity {
		if key == identity || strings.HasSuffix(key, "_"+identity) {
			return true
		}
	}
	return false
}

func (set *ruleSet) apply(rule rule, text string, fileName bool) string {
	matches := rule.pattern.FindAllStringSubmatchIndex(text, -1)
	if matches == nil {
		return text
	}
	var sources [][]int
	if rule.check == "host" {
		sources = set.sourceLocations(text)
	}
	var out strings.Builder
	last := 0
	replace := func(start, end int) {
		out.WriteString(text[last:start])
		out.WriteString(set.placeholder)
		last = end
	}
	for _, match := range matches {
		if len(match) > 2 {
			for group := 2; group < len(match); group += 2 {
				if match[group] >= 0 {
					replace(match[group], match[group+1])
				}
			}
			continue
		}
		start, end := match[0], match[1]
		switch rule.check {
		case "":
			replace(start, end)
		case "ipv6":
			if stop, ok := ipv6End(text, start, end); ok {
				replace(start, stop)
			}
		case "host":
			if fileName && !strings.ContainsAny(text, ":/\\") {
				continue
			}
			if !set.keepsHost(text[start:end], start, end, sources) {
				replace(start, end)
			}
		}
	}
	if last == 0 {
		return text
	}
	out.WriteString(text[last:])
	return out.String()
}

// sourceLocations preserves file locations with a path separator or column, ISO timestamps
// and line-leading log times, whose colons and digits do not name servers.
func (set *ruleSet) sourceLocations(text string) [][]int {
	var spans [][]int
	for _, match := range set.sourceLocation.FindAllStringSubmatchIndex(text, -1) {
		if match[2] >= 0 || (match[0] > 0 && (text[match[0]-1] == '/' || text[match[0]-1] == '\\')) {
			spans = append(spans, match[:2])
		}
	}
	spans = append(spans, set.timestamp.FindAllStringIndex(text, -1)...)
	return spans
}

// keepsHost reports whether a host match is a source location or a file name, not a server.
func (set *ruleSet) keepsHost(host string, start, end int, sources [][]int) bool {
	for _, source := range sources {
		if source[0] <= start && end <= source[1] {
			return true
		}
	}
	if colon := strings.LastIndexByte(host, ':'); colon >= 0 && allDigits(host[colon+1:]) {
		return false
	}
	dot := strings.LastIndexByte(host, '.')
	if dot < 0 {
		return false
	}
	for _, extension := range set.extensions {
		if strings.EqualFold(extension, host[dot+1:]) {
			return true
		}
	}
	return false
}

// ipv6End is where the address in an IPv6 candidate match ends, when it is a standalone
// address: a trailing dot or prose colon is not part of it, and a Rust path such as
// `<u32 as Add>::add` is not one.
func ipv6End(text string, start, end int) (int, bool) {
	address := strings.TrimRight(text[start:end], ".")
	if !ipv6Address(address) && strings.HasSuffix(address, ":") && !strings.HasSuffix(address, "::") {
		address = address[:len(address)-1]
	}
	stop := start + len(address)
	standalone := !(start > 0 && pathByte(text[start-1])) && !(stop < len(text) && identifierByte(text[stop]))
	return stop, standalone && ipv6Address(address)
}

// ipv6Address reports whether a candidate, with optional brackets, zone and port, is an IPv6
// address with at least one hex group.
func ipv6Address(candidate string) bool {
	var addresses []string
	if rest, bracketed := strings.CutPrefix(candidate, "["); bracketed {
		if address, _, closed := strings.Cut(rest, "]"); closed {
			rest = address
		}
		addresses = append(addresses, rest)
	} else {
		addresses = append(addresses, candidate)
		if colon := strings.LastIndexByte(candidate, ':'); colon >= 0 && allDigits(candidate[colon+1:]) {
			addresses = append(addresses, candidate[:colon])
		}
	}
	for _, address := range addresses {
		if zone := strings.IndexByte(address, '%'); zone >= 0 {
			address = address[:zone]
		}
		parsed, err := netip.ParseAddr(address)
		if err == nil && parsed.Is6() && strings.Trim(address, ":") != "" {
			return true
		}
	}
	return false
}

// allDigits reports whether text is a non-empty run of ASCII digits.
func allDigits(text string) bool {
	if text == "" {
		return false
	}
	for i := range len(text) {
		if text[i] < '0' || text[i] > '9' {
			return false
		}
	}
	return true
}

func identifierByte(b byte) bool {
	return b == '_' || '0' <= b && b <= '9' || 'a' <= b && b <= 'z' || 'A' <= b && b <= 'Z'
}

// pathByte is a byte that ends a Rust path segment, as in `{{closure}}::fade`.
func pathByte(b byte) bool {
	return identifierByte(b) || b == '>' || b == '}' || b == ')' || b == ':'
}
