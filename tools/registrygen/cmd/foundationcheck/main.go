package main

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/hashimthearab/rust-mcbe/tools/registrygen/internal/foundation"
)

func main() {
	os.Exit(run(os.Args[1:], os.Stdout, os.Stderr))
}

func run(arguments []string, stdout, stderr io.Writer) int {
	flags := flag.NewFlagSet("foundationcheck", flag.ContinueOnError)
	flags.SetOutput(stderr)
	manifestPath := flags.String("manifest", "", "registry foundation manifest to validate")
	expectBlocked := flags.Bool("expect-blocked", false, "succeed only for a valid blocked foundation")
	if err := flags.Parse(arguments); err != nil {
		return 1
	}
	if flags.NArg() != 0 {
		fmt.Fprintln(stderr, "registry-foundation: positional arguments are not accepted")
		return 1
	}
	status, missing, err := validateFile(*manifestPath)
	if err != nil {
		fmt.Fprintln(stderr, "registry-foundation:", err)
		return 1
	}
	if status == "blocked" {
		fmt.Fprintf(stdout, "registry-foundation: status=blocked missing=%s\n", strings.Join(missing, ","))
		if *expectBlocked {
			return 0
		}
		return 2
	}
	if *expectBlocked {
		fmt.Fprintln(stderr, "registry-foundation: expected blocked status, got ready")
		return 1
	}
	fmt.Fprintln(stdout, "registry-foundation: status=ready")
	return 0
}

func validateFile(path string) (string, []string, error) {
	if path == "" {
		return "", nil, errors.New("-manifest is required")
	}
	file, err := os.Open(path)
	if err != nil {
		return "", nil, fmt.Errorf("open manifest: %w", err)
	}
	defer file.Close()
	result, err := foundation.ValidateRegistryFoundation(file)
	if err != nil {
		return "", nil, err
	}
	return string(result.Status), result.MissingStrings(), nil
}
