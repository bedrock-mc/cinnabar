package extension

import (
	"errors"
	"fmt"
	"math"
	"slices"
	"strconv"
	"unicode/utf8"
)

// Message is a value whose encoding is the exact compact serde_json encoding of its Rust type.
type Message interface {
	appendJSON(dst []byte) ([]byte, error)
}

// Target is a pointer that Decode fills as strictly as serde_json fills its Rust type.
type Target interface {
	decodeJSON(r *reader) error
}

// Document is a pointer to a Message that Decode can fill; Verify needs both to check that signed
// bytes are canonical.
type Document interface {
	Message
	Target
}

// Encode returns the canonical bytes of v: Rust's field order, no whitespace, explicit nulls,
// sets in Rust's order and serde_json's string escaping. A value that Rust cannot hold, such as a
// string that is not UTF-8, does not encode.
func Encode(v Message) ([]byte, error) {
	return v.appendJSON(nil)
}

// Decode fills v from data, which holds exactly one JSON value. It refuses what serde_json
// refuses for the Rust type: unknown, duplicate or missing members (a missing Option is null),
// null for anything else, numbers that are not integers of the field's width, invalid UTF-8,
// lone surrogates and unknown enum variants. It reads only the object form of structs, never
// serde's positional array form.
func Decode(data []byte, v Target) error {
	r := reader{data: data}
	if err := v.decodeJSON(&r); err != nil {
		return err
	}
	r.space()
	if r.pos != len(r.data) {
		return r.errorf("trailing characters")
	}
	return nil
}

// errNotUTF8 refuses a Go string that no Rust String can hold.
var errNotUTF8 = errors.New("string is not valid UTF-8")

// appendString appends s as serde_json writes a string: quote, backslash and control characters
// escaped, everything else (DEL, U+2028, U+2029, HTML characters) as is.
func appendString(b []byte, s string) ([]byte, error) {
	if !utf8.ValidString(s) {
		return nil, errNotUTF8
	}
	const hex = "0123456789abcdef"
	b = append(b, '"')
	start := 0
	for i := range len(s) {
		c := s[i]
		if c >= 0x20 && c != '"' && c != '\\' {
			continue
		}
		b = append(b, s[start:i]...)
		switch c {
		case '"', '\\':
			b = append(b, '\\', c)
		case '\b':
			b = append(b, '\\', 'b')
		case '\f':
			b = append(b, '\\', 'f')
		case '\n':
			b = append(b, '\\', 'n')
		case '\r':
			b = append(b, '\\', 'r')
		case '\t':
			b = append(b, '\\', 't')
		default:
			b = append(b, '\\', 'u', '0', '0', hex[c>>4], hex[c&15])
		}
		start = i + 1
	}
	b = append(b, s[start:]...)
	return append(b, '"'), nil
}

// appendStrings appends values as a JSON array of strings, in order.
func appendStrings(b []byte, values []string) ([]byte, error) {
	b = append(b, '[')
	for i, value := range values {
		if i > 0 {
			b = append(b, ',')
		}
		var err error
		if b, err = appendString(b, value); err != nil {
			return nil, err
		}
	}
	return append(b, ']'), nil
}

// appendSet appends values as Rust serializes a BTreeSet<String>: sorted by byte order, each once.
// The caller's slice is not reordered.
func appendSet(b []byte, values []string) ([]byte, error) {
	for i := 1; i < len(values); i++ {
		if values[i-1] >= values[i] {
			values = slices.Compact(slices.Sorted(slices.Values(values)))
			break
		}
	}
	return appendStrings(b, values)
}

// list is a Rust Vec of messages.
type list[T Message] []T

func (l list[T]) appendJSON(b []byte) ([]byte, error) {
	b = append(b, '[')
	for i, item := range l {
		if i > 0 {
			b = append(b, ',')
		}
		var err error
		if b, err = item.appendJSON(b); err != nil {
			return nil, err
		}
	}
	return append(b, ']'), nil
}

// object writes one struct's members in Rust declaration order; names are their serde names.
type object struct {
	b     []byte
	names []string
	next  int
	// written counts the members written, which skipped members are not.
	written int
	err     error
}

// key starts the next member.
func (o *object) key() {
	if o.written == 0 {
		o.b = append(o.b, '{')
	} else {
		o.b = append(o.b, ',')
	}
	o.b = append(o.b, '"')
	o.b = append(o.b, o.names[o.next]...)
	o.b = append(o.b, '"', ':')
	o.next++
	o.written++
}

// add keeps the result of an append that may fail; the first failure wins.
func (o *object) add(b []byte, err error) {
	if err != nil {
		if o.err == nil {
			o.err = err
		}
		return
	}
	o.b = b
}

func (o *object) uint(v uint64) {
	o.key()
	o.b = strconv.AppendUint(o.b, v, 10)
}

func (o *object) int(v int64) {
	o.key()
	o.b = strconv.AppendInt(o.b, v, 10)
}

func (o *object) str(s string) {
	o.key()
	o.add(appendString(o.b, s))
}

func (o *object) value(v Message) {
	o.key()
	o.add(v.appendJSON(o.b))
}

// skip leaves out the next member: a Rust Option that serde skips when it is None.
func (o *object) skip() {
	o.next++
}

func (o *object) end() ([]byte, error) {
	if o.err != nil {
		return nil, o.err
	}
	if o.written == 0 {
		o.b = append(o.b, '{')
	}
	return append(o.b, '}'), nil
}

// maxDepth is serde_json's recursion limit.
const maxDepth = 128

// reader parses JSON with serde_json's grammar and strictness.
type reader struct {
	data  []byte
	pos   int
	depth int
}

func (r *reader) errorf(format string, args ...any) error {
	return fmt.Errorf("JSON at byte %d: %s", r.pos, fmt.Sprintf(format, args...))
}

// space skips JSON whitespace.
func (r *reader) space() {
	for r.pos < len(r.data) {
		switch r.data[r.pos] {
		case ' ', '\t', '\n', '\r':
			r.pos++
		default:
			return
		}
	}
}

// at reports whether the next byte, without skipping whitespace, is c.
func (r *reader) at(c byte) bool {
	return r.pos < len(r.data) && r.data[r.pos] == c
}

// peek skips whitespace and returns the next byte, or 0 at the end.
func (r *reader) peek() byte {
	r.space()
	if r.pos < len(r.data) {
		return r.data[r.pos]
	}
	return 0
}

// literal consumes word if it comes next.
func (r *reader) literal(word string) bool {
	r.space()
	if len(r.data)-r.pos >= len(word) && string(r.data[r.pos:r.pos+len(word)]) == word {
		r.pos += len(word)
		return true
	}
	return false
}

// null consumes a null if one comes next.
func (r *reader) null() bool {
	return r.literal("null")
}

func (r *reader) boolean() (bool, error) {
	switch {
	case r.literal("true"):
		return true, nil
	case r.literal("false"):
		return false, nil
	}
	return false, r.errorf("expected a boolean")
}

// enter counts one level of nesting and refuses more than serde_json allows.
func (r *reader) enter() error {
	r.depth++
	if r.depth > maxDepth {
		return r.errorf("recursion limit exceeded")
	}
	return nil
}

// object reads an object; member is called with each key and must read its value.
func (r *reader) object(member func(key string) error) error {
	if r.peek() != '{' {
		return r.errorf("expected an object")
	}
	r.pos++
	if err := r.enter(); err != nil {
		return err
	}
	defer func() { r.depth-- }()
	if r.peek() == '}' {
		r.pos++
		return nil
	}
	for {
		key, err := r.str()
		if err != nil {
			return err
		}
		if r.peek() != ':' {
			return r.errorf("expected ':'")
		}
		r.pos++
		if err := member(key); err != nil {
			return err
		}
		switch r.peek() {
		case ',':
			r.pos++
		case '}':
			r.pos++
			return nil
		default:
			return r.errorf("expected ',' or '}'")
		}
	}
}

// array reads an array; item is called once per element and must read it.
func (r *reader) array(item func() error) error {
	if r.peek() != '[' {
		return r.errorf("expected an array")
	}
	r.pos++
	if err := r.enter(); err != nil {
		return err
	}
	defer func() { r.depth-- }()
	if r.peek() == ']' {
		r.pos++
		return nil
	}
	for {
		if err := item(); err != nil {
			return err
		}
		switch r.peek() {
		case ',':
			r.pos++
		case ']':
			r.pos++
			return nil
		default:
			return r.errorf("expected ',' or ']'")
		}
	}
}

// fields reads an object into a struct whose serde names are names, as serde's derive does with
// deny_unknown_fields: members in any order, each at most once, no others, and every one present
// except the optional ones (Rust Options, which default to None).
func (r *reader) fields(names []string, member func(name string) error, optional ...string) error {
	var seen uint64
	err := r.object(func(key string) error {
		i := slices.Index(names, key)
		if i < 0 {
			return r.errorf("unknown field %q", key)
		}
		if seen&(1<<i) != 0 {
			return r.errorf("duplicate field %q", key)
		}
		seen |= 1 << i
		return member(key)
	})
	if err != nil {
		return err
	}
	for i, name := range names {
		if seen&(1<<i) == 0 && !slices.Contains(optional, name) {
			return r.errorf("missing field %q", name)
		}
	}
	return nil
}

// tagged scans one object for the string member tag, wherever it appears, as serde finds the
// tag of a tagged enum, and returns it with the object's bytes for the variant to be read from.
func (r *reader) tagged(tag string) (string, []byte, error) {
	r.space()
	start := r.pos
	var variant string
	found := false
	err := r.object(func(key string) error {
		if key != tag {
			return r.skip()
		}
		if found {
			return r.errorf("duplicate field %q", tag)
		}
		found = true
		var err error
		variant, err = r.str()
		return err
	})
	if err != nil {
		return "", nil, err
	}
	if !found {
		return "", nil, r.errorf("missing field %q", tag)
	}
	return variant, r.data[start:r.pos], nil
}

// within reads raw, the bytes of exactly one value, with read.
func within(raw []byte, read func(*reader) error) error {
	sub := reader{data: raw}
	if err := read(&sub); err != nil {
		return err
	}
	sub.space()
	if sub.pos != len(sub.data) {
		return sub.errorf("trailing characters")
	}
	return nil
}

// skip reads and discards one value of any type.
func (r *reader) skip() error {
	switch r.peek() {
	case '{':
		return r.object(func(string) error { return r.skip() })
	case '[':
		return r.array(r.skip)
	case '"':
		_, err := r.str()
		return err
	case 't', 'f':
		_, err := r.boolean()
		return err
	case 'n':
		if r.null() {
			return nil
		}
		return r.errorf("expected a value")
	}
	return r.number()
}

// number skips a JSON number of any form; the typed readers refuse what Rust integers refuse.
func (r *reader) number() error {
	if r.at('-') {
		r.pos++
	}
	if !r.digits() {
		return r.errorf("expected a value")
	}
	if r.at('.') {
		r.pos++
		if !r.digits() {
			return r.errorf("invalid number")
		}
	}
	if r.at('e') || r.at('E') {
		r.pos++
		if r.at('+') || r.at('-') {
			r.pos++
		}
		if !r.digits() {
			return r.errorf("invalid number")
		}
	}
	return nil
}

// digits consumes decimal digits and reports whether there was one.
func (r *reader) digits() bool {
	start := r.pos
	for r.pos < len(r.data) && '0' <= r.data[r.pos] && r.data[r.pos] <= '9' {
		r.pos++
	}
	return r.pos > start
}

// integer reads a JSON integer as serde_json reads one for an integer field: a fraction, an
// exponent, a leading zero or negative zero (which serde_json reads as a float) is refused.
func (r *reader) integer() (negative bool, magnitude uint64, err error) {
	r.space()
	negative = r.at('-')
	if negative {
		r.pos++
	}
	start := r.pos
	overflow := false
	for r.pos < len(r.data) && '0' <= r.data[r.pos] && r.data[r.pos] <= '9' {
		digit := uint64(r.data[r.pos] - '0')
		if magnitude > (math.MaxUint64-digit)/10 {
			overflow = true
		}
		magnitude = magnitude*10 + digit
		r.pos++
	}
	switch {
	case r.pos == start:
		return false, 0, r.errorf("expected an integer")
	case r.data[start] == '0' && r.pos-start > 1:
		return false, 0, r.errorf("invalid number")
	case r.at('.') || r.at('e') || r.at('E'):
		return false, 0, r.errorf("expected an integer, found a float")
	case negative && magnitude == 0:
		return false, 0, r.errorf("expected an integer, found negative zero")
	case overflow:
		return false, 0, r.errorf("integer out of range")
	}
	return negative, magnitude, nil
}

// readUint reads an unsigned integer that fits T.
func readUint[T uint8 | uint16 | uint32 | uint64](r *reader) (T, error) {
	negative, magnitude, err := r.integer()
	if err != nil {
		return 0, err
	}
	if negative || magnitude > uint64(^T(0)) {
		return 0, r.errorf("integer out of range")
	}
	return T(magnitude), nil
}

// int64 reads a signed 64-bit integer.
func (r *reader) int64() (int64, error) {
	negative, magnitude, err := r.integer()
	if err != nil {
		return 0, err
	}
	if negative {
		if magnitude > 1<<63 {
			return 0, r.errorf("integer out of range")
		}
		return int64(-magnitude), nil
	}
	if magnitude > math.MaxInt64 {
		return 0, r.errorf("integer out of range")
	}
	return int64(magnitude), nil
}

// str reads a string as serde_json reads one into a Rust String: raw control characters, invalid
// UTF-8, unknown escapes and unpaired surrogates are refused.
func (r *reader) str() (string, error) {
	if r.peek() != '"' {
		return "", r.errorf("expected a string")
	}
	r.pos++
	start := r.pos
	var escaped []byte
	for r.pos < len(r.data) {
		c := r.data[r.pos]
		switch {
		case c == '"':
			text := r.data[start:r.pos]
			if escaped != nil {
				text = append(escaped, text...)
			}
			if !utf8.Valid(text) {
				return "", r.errorf("invalid UTF-8 in a string")
			}
			r.pos++
			return string(text), nil
		case c == '\\':
			escaped = append(escaped, r.data[start:r.pos]...)
			r.pos++
			var err error
			if escaped, err = r.escape(escaped); err != nil {
				return "", err
			}
			start = r.pos
		case c < 0x20:
			return "", r.errorf("control character in a string")
		default:
			r.pos++
		}
	}
	return "", r.errorf("unterminated string")
}

// escape appends the character of the escape after a backslash.
func (r *reader) escape(b []byte) ([]byte, error) {
	if r.pos >= len(r.data) {
		return nil, r.errorf("unterminated string")
	}
	c := r.data[r.pos]
	r.pos++
	switch c {
	case '"', '\\', '/':
		return append(b, c), nil
	case 'b':
		return append(b, '\b'), nil
	case 'f':
		return append(b, '\f'), nil
	case 'n':
		return append(b, '\n'), nil
	case 'r':
		return append(b, '\r'), nil
	case 't':
		return append(b, '\t'), nil
	case 'u':
	default:
		return nil, r.errorf("invalid escape")
	}
	n, err := r.hex4()
	if err != nil {
		return nil, err
	}
	if 0xDC00 <= n && n <= 0xDFFF {
		return nil, r.errorf("lone trailing surrogate in a hex escape")
	}
	if 0xD800 <= n && n <= 0xDBFF {
		if !r.at('\\') || r.pos+1 >= len(r.data) || r.data[r.pos+1] != 'u' {
			return nil, r.errorf("lone leading surrogate in a hex escape")
		}
		r.pos += 2
		low, err := r.hex4()
		if err != nil {
			return nil, err
		}
		if low < 0xDC00 || low > 0xDFFF {
			return nil, r.errorf("lone leading surrogate in a hex escape")
		}
		n = (n-0xD800)<<10 | (low - 0xDC00) + 0x10000
	}
	return utf8.AppendRune(b, n), nil
}

// hex4 reads the four hex digits of a \u escape, in either case as serde_json accepts them.
func (r *reader) hex4() (rune, error) {
	if len(r.data)-r.pos < 4 {
		return 0, r.errorf("unterminated hex escape")
	}
	var n rune
	for _, c := range r.data[r.pos : r.pos+4] {
		switch {
		case '0' <= c && c <= '9':
			c -= '0'
		case 'a' <= c && c <= 'f':
			c -= 'a' - 10
		case 'A' <= c && c <= 'F':
			c -= 'A' - 10
		default:
			return 0, r.errorf("invalid hex escape")
		}
		n = n<<4 | rune(c)
	}
	r.pos += 4
	return n, nil
}

// readStrings reads an array of strings.
func readStrings(r *reader) ([]string, error) {
	var values []string
	err := r.array(func() error {
		value, err := r.str()
		values = append(values, value)
		return err
	})
	return values, err
}

// readSet reads a Rust BTreeSet<String>: any order, repeats merged, held sorted.
func readSet(r *reader) ([]string, error) {
	values, err := readStrings(r)
	slices.Sort(values)
	return slices.Compact(values), err
}

// readList reads a Rust Vec of T.
func readList[T any, P interface {
	*T
	Target
}](r *reader) ([]T, error) {
	var items []T
	err := r.array(func() error {
		var item T
		if err := P(&item).decodeJSON(r); err != nil {
			return err
		}
		items = append(items, item)
		return nil
	})
	return items, err
}
