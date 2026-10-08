// Command release-manifest generates update signing keys and signs release manifests (CI only).
package main

import (
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"strings"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/update"
)

const keyEnv = "CINNABAR_UPDATE_SIGNING_KEY"

type artifactFlags []string

func (a *artifactFlags) String() string     { return strings.Join(*a, ",") }
func (a *artifactFlags) Set(v string) error { *a = append(*a, v); return nil }

func main() {
	if err := run(os.Args[1:], os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, "release-manifest:", err)
		os.Exit(1)
	}
}

func run(args []string, stdout io.Writer) error {
	if len(args) == 0 {
		return errors.New("usage: release-manifest keygen | sign [flags]")
	}
	switch args[0] {
	case "keygen":
		pub, priv, err := ed25519.GenerateKey(rand.Reader)
		if err != nil {
			return err
		}
		fmt.Fprintf(stdout, "public (commit as k1:<this> in core/update/trusted-keys.txt): %s\n", base64.StdEncoding.EncodeToString(pub))
		fmt.Fprintf(stdout, "private (store as %s, never commit): %s\n", keyEnv, base64.StdEncoding.EncodeToString(priv.Seed()))
		return nil
	case "sign":
		return sign(args[1:], stdout)
	}
	return fmt.Errorf("unknown command %q", args[0])
}

// sign requires the release seed to match the client's committed trust store.
func sign(args []string, stdout io.Writer) error {
	keys, err := update.TrustedKeys()
	if err != nil {
		return err
	}
	return signWithKeys(args, stdout, keys)
}

// signWithKeys builds an envelope only for a key trusted by the release binary.
func signWithKeys(args []string, stdout io.Writer, keys map[string]ed25519.PublicKey) error {
	flags := flag.NewFlagSet("sign", flag.ContinueOnError)
	version := flags.String("version", "", "release version, e.g. 1.2.3")
	channel := flags.String("channel", "stable", "release channel")
	keyID := flags.String("key-id", "k1", "signing key id")
	notes := flags.String("notes-url", "", "release notes URL")
	validity := flags.Duration("validity", 30*24*time.Hour, "manifest lifetime before clients reject it")
	var artifacts artifactFlags
	flags.Var(&artifacts, "artifact", "platform=url=localfile (repeatable)")
	if err := flags.Parse(args); err != nil {
		return err
	}
	seed, err := base64.StdEncoding.DecodeString(os.Getenv(keyEnv))
	if err != nil || len(seed) != ed25519.SeedSize {
		return fmt.Errorf("%s must hold a base64 %d-byte seed", keyEnv, ed25519.SeedSize)
	}
	private := ed25519.NewKeyFromSeed(seed)
	trusted, ok := keys[*keyID]
	if !ok || !trusted.Equal(private.Public()) {
		return errors.New("signing seed does not match the committed public key")
	}
	manifest := update.Manifest{
		Schema: 1, Channel: *channel, Version: *version, NotesURL: *notes,
		Expires: time.Now().Add(*validity).UTC(), Artifacts: map[string]update.Artifact{},
	}
	for _, spec := range artifacts {
		platform, artifact, err := describe(spec)
		if err != nil {
			return err
		}
		manifest.Artifacts[platform] = artifact
	}
	if len(manifest.Artifacts) == 0 {
		return errors.New("at least one -artifact is required")
	}
	body, err := update.Sign(manifest, *keyID, private)
	if err != nil {
		return err
	}
	_, err = stdout.Write(append(body, '\n'))
	return err
}

func describe(spec string) (string, update.Artifact, error) {
	parts := strings.SplitN(spec, "=", 3)
	if len(parts) != 3 {
		return "", update.Artifact{}, fmt.Errorf("artifact %q must be platform=url=localfile", spec)
	}
	data, err := os.ReadFile(parts[2])
	if err != nil {
		return "", update.Artifact{}, err
	}
	sum := sha256.Sum256(data)
	return parts[0], update.Artifact{URL: parts[1], SHA256: hex.EncodeToString(sum[:]), Size: int64(len(data))}, nil
}
