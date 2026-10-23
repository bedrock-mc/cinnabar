package cinnabarext

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"time"
)

// cappedOutput stops helper output from allocating beyond its small metadata contract.
type cappedOutput struct {
	buffer bytes.Buffer
	limit  int
}

// Write refuses output beyond the configured limit.
func (b *cappedOutput) Write(data []byte) (int, error) {
	if len(data) > b.limit-b.buffer.Len() {
		return 0, errors.New("bundle verifier output limit exceeded")
	}
	return b.buffer.Write(data)
}

// verifierBinary finds the configured CLI, a sibling tool, or the installed CLI on PATH.
func verifierBinary() (string, error) {
	if path := os.Getenv("CINNABAR_CXB_BIN"); path != "" {
		return path, nil
	}
	if self, err := os.Executable(); err == nil {
		suffix := filepath.Ext(self)
		if suffix != ".exe" {
			suffix = ""
		}
		path := filepath.Join(filepath.Dir(self), "cinnabar-cxb"+suffix)
		if info, err := os.Stat(path); err == nil && info.Mode().IsRegular() {
			return path, nil
		}
	}
	path, err := exec.LookPath("cinnabar-cxb")
	if err != nil {
		return "", fmt.Errorf("cinnabar-cxb is required to validate bundles; build it with make local-server or set CINNABAR_CXB_BIN: %w", err)
	}
	return path, nil
}

// verifyBundle sends the already bounded archive snapshot to the offline Rust verifier.
func verifyBundle(data []byte) (Bundle, error) {
	binary, err := verifierBinary()
	if err != nil {
		return Bundle{}, err
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, binary, "verify", "--stdin")
	cmd.WaitDelay = time.Second
	cmd.Stdin = bytes.NewReader(data)
	out := cappedOutput{limit: MaxMarkerBytes}
	diagnostic := cappedOutput{limit: 4096}
	cmd.Stdout = &out
	cmd.Stderr = &diagnostic
	if err := cmd.Run(); err != nil {
		return Bundle{}, fmt.Errorf("offline bundle validation: %w: %s", err, diagnostic.buffer.String())
	}
	return verifiedMetadata(data, out.buffer.Bytes())
}

// verifiedMetadata accepts metadata only for the exact archive snapshot passed to the CLI.
func verifiedMetadata(data, metadata []byte) (Bundle, error) {
	var result struct {
		SHA256   string          `json:"sha256"`
		Bytes    uint64          `json:"bytes"`
		Manifest json.RawMessage `json:"manifest"`
	}
	decoder := json.NewDecoder(bytes.NewReader(metadata))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&result); err != nil {
		return Bundle{}, fmt.Errorf("bundle verifier metadata: %w", err)
	}
	var trailing any
	if err := decoder.Decode(&trailing); err != io.EOF {
		return Bundle{}, errors.New("trailing bundle verifier metadata")
	}
	if result.SHA256 != Digest(data) || result.Bytes != uint64(len(data)) {
		return Bundle{}, errors.New("bundle verifier metadata names different archive bytes")
	}
	var manifest Manifest
	if err := Decode(result.Manifest, &manifest); err != nil {
		return Bundle{}, fmt.Errorf("bundle verifier manifest: %w", err)
	}
	return Bundle{Manifest: manifest, Digest: result.SHA256, Bytes: result.Bytes}, nil
}
