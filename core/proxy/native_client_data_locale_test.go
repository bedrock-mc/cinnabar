package proxy

import (
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"testing"

	"golang.org/x/text/language"
)

// nativeClientDataLocale emits only membership data used by the private acceptance predicate.
func nativeClientDataLocale(t *testing.T, client nativeSchemaSource, locale *nativeListedModule) []byte {
	t.Helper()
	out := nativeClientDataSemanticHeader(client, locale)
	letters := "abcdefghijklmnopqrstuvwxyz"
	nativeClientDataBits(out, "BASE_TWO", 2, func(s string) bool {
		_, err := language.ParseBase(s)
		return err == nil
	}, letters)
	nativeClientDataBits(out, "BASE_THREE", 3, func(s string) bool {
		_, err := language.ParseBase(s)
		return err == nil
	}, letters)
	nativeClientDataBits(out, "REGION_TWO", 2, func(s string) bool {
		_, err := language.ParseRegion(s)
		return err == nil
	}, letters)
	nativeClientDataBits(out, "REGION_NUMBER", 3, func(s string) bool {
		_, err := language.ParseRegion(s)
		return err == nil
	}, "0123456789")
	out.WriteString("#[rustfmt::skip]\npub(super) const SCRIPTS: &[&str] = &[\n")
	nativeClientDataEach(4, letters, func(word string) {
		if _, err := language.ParseScript(word); err == nil {
			fmt.Fprintf(out, "    %s,\n", strconv.Quote(word))
		}
	})
	out.WriteString("];\n\n")

	tables := filepath.Join(locale.Dir, "internal/language/tables.go")
	variants := nativeClientDataMap(t, tables, "variantIndex", func(value ast.Expr) bool {
		key, ok := value.(*ast.Ident)
		return ok && key.Name == "string"
	}, "uint8")
	rows := make(map[string]uint64, len(variants.Elts))
	ids := map[uint64]bool{}
	for _, entry := range variants.Elts {
		pair, ok := entry.(*ast.KeyValueExpr)
		if !ok {
			t.Fatal("unexpected variant map entry")
		}
		key := nativeClientDataLiteral(t, pair.Key, token.STRING)
		if _, err := language.ParseVariant(key); err != nil {
			t.Fatal("source variant is not admitted by the actual selected parser")
		}
		number, ok := pair.Value.(*ast.BasicLit)
		if !ok || number.Kind != token.INT {
			t.Fatal("unexpected variant ordinal")
		}
		id, err := strconv.ParseUint(number.Value, 0, 8)
		if err != nil || rows[key] != 0 || ids[id] || key == "" {
			t.Fatal("unexpected variant key or ordinal")
		}
		rows[key], ids[id] = id+1, true
	}
	keys := make([]string, 0, len(rows))
	for key := range rows {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	out.WriteString("#[rustfmt::skip]\npub(super) const VARIANTS: &[(&str, u8)] = &[\n")
	for _, key := range keys {
		fmt.Fprintf(out, "    (%s, %d),\n", strconv.Quote(key), rows[key]-1)
	}
	out.WriteString("];\n\n")

	lookup := filepath.Join(locale.Dir, "internal/language/lookup.go")
	grandfathered := nativeClientDataMap(t, lookup, "grandfatheredMap", func(value ast.Expr) bool {
		array, ok := value.(*ast.ArrayType)
		if !ok {
			return false
		}
		length, fixed := array.Len.(*ast.Ident)
		element, character := array.Elt.(*ast.Ident)
		return fixed && length.Name == "maxLen" && character && element.Name == "byte"
	}, "int16")
	keys = keys[:0]
	seen := map[string]bool{}
	for _, entry := range grandfathered.Elts {
		pair, ok := entry.(*ast.KeyValueExpr)
		if !ok {
			t.Fatal("unexpected grandfathered map entry")
		}
		array, ok := pair.Key.(*ast.CompositeLit)
		if !ok {
			t.Fatal("unexpected grandfathered tag encoding")
		}
		kind, ok := array.Type.(*ast.ArrayType)
		if !ok {
			t.Fatal("unexpected grandfathered tag type")
		}
		length, fixed := kind.Len.(*ast.Ident)
		element, character := kind.Elt.(*ast.Ident)
		if !fixed || length.Name != "maxLen" || !character || element.Name != "byte" {
			t.Fatal("unexpected grandfathered tag array")
		}
		var key strings.Builder
		for _, value := range array.Elts {
			character := nativeClientDataLiteral(t, value, token.CHAR)
			if len(character) != 1 || character[0] >= 0x80 {
				t.Fatal("unexpected grandfathered tag byte")
			}
			key.WriteString(character)
		}
		if key.Len() == 0 || seen[key.String()] {
			t.Fatal("empty or duplicate grandfathered tag")
		}
		if _, err := language.Parse(key.String()); err != nil {
			t.Fatal("source grandfathered key is not admitted by the actual selected parser")
		}
		seen[key.String()] = true
		keys = append(keys, key.String())
	}
	sort.Strings(keys)
	out.WriteString("#[rustfmt::skip]\npub(super) const GRANDFATHERED: &[&str] = &[\n")
	for _, key := range keys {
		fmt.Fprintf(out, "    %s,\n", strconv.Quote(key))
	}
	out.WriteString("];\n")
	t.Logf("semantic locale data: %d variants, %d grandfathered tags", len(rows), len(keys))
	return []byte(out.String())
}

// nativeClientDataEach enumerates a finite ASCII subtag domain in lexical order.
func nativeClientDataEach(length int, alphabet string, visit func(string)) {
	count := 1
	for range length {
		count *= len(alphabet)
	}
	word := make([]byte, length)
	for index := range count {
		remaining := index
		for at := length - 1; at >= 0; at-- {
			word[at] = alphabet[remaining%len(alphabet)]
			remaining /= len(alphabet)
		}
		visit(string(word))
	}
}

// nativeClientDataMap reads one identified finite source declaration and rejects unfamiliar types.
func nativeClientDataMap(t *testing.T, path, name string, keyType func(ast.Expr) bool, valueType string) *ast.CompositeLit {
	t.Helper()
	file, err := parser.ParseFile(token.NewFileSet(), path, nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	var found *ast.CompositeLit
	for _, declaration := range file.Decls {
		general, ok := declaration.(*ast.GenDecl)
		if !ok || general.Tok != token.VAR {
			continue
		}
		for _, specification := range general.Specs {
			value, ok := specification.(*ast.ValueSpec)
			if !ok || len(value.Names) != 1 || value.Names[0].Name != name {
				continue
			}
			if found != nil || len(value.Values) != 1 {
				t.Fatal("unexpected duplicate or uninitialized semantic source map")
			}
			literal, ok := value.Values[0].(*ast.CompositeLit)
			if !ok || len(literal.Elts) == 0 {
				t.Fatal("unexpected empty semantic source map")
			}
			mapping, ok := literal.Type.(*ast.MapType)
			if !ok || !keyType(mapping.Key) {
				t.Fatal("unexpected semantic source map key type")
			}
			element, ok := mapping.Value.(*ast.Ident)
			if !ok || element.Name != valueType {
				t.Fatal("unexpected semantic source map value type")
			}
			found = literal
		}
	}
	if found == nil {
		t.Fatalf("missing identified %s declaration in locale source", name)
	}
	return found
}

// nativeClientDataLiteral admits only the expected already parsed literal form from the selected module.
func nativeClientDataLiteral(t *testing.T, expression ast.Expr, kind token.Token) string {
	t.Helper()
	literal, ok := expression.(*ast.BasicLit)
	if !ok || literal.Kind != kind {
		t.Fatal("unexpected semantic source literal")
	}
	value, err := strconv.Unquote(literal.Value)
	if err != nil {
		t.Fatal(err)
	}
	return value
}
