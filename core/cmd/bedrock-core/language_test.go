package main

import (
	"io"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/locale"
)

// The client can pass its active locale without changing the default for standalone launches.
func TestLanguageFlag(t *testing.T) {
	for _, test := range []struct {
		args []string
		want string
	}{
		{nil, locale.Default},
		{[]string{"-language", "fr-FR"}, "fr-FR"},
	} {
		options, err := parseFlags(test.args, io.Discard)
		if err != nil {
			t.Fatal(err)
		}
		if options.language != test.want {
			t.Fatalf("language = %q, want %q", options.language, test.want)
		}
	}
}
