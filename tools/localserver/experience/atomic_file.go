package experience

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
)

// writeJSONFile encodes a durable record without exposing partial JSON at its path.
func writeJSONFile(path string, value any) error {
	return writeAtomicFile(path, func(w io.Writer) error {
		return json.NewEncoder(w).Encode(value)
	})
}

// writeAtomicFile replaces path only after write succeeds and the temporary file is synced.
// Callers serialize writes to the same path. Failed writes preserve the old record;
// interrupted temporary files are never read as records.
func writeAtomicFile(path string, write func(io.Writer) error) error {
	f, err := os.OpenFile(path+".tmp", os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0o600)
	if err != nil {
		return fmt.Errorf("write %s: %w", path, err)
	}
	defer os.Remove(f.Name())
	err = write(f)
	if err == nil {
		err = f.Sync()
	}
	err = errors.Join(err, f.Close())
	if err == nil {
		err = commitAtomicFile(f.Name(), path)
	}
	if err != nil {
		return fmt.Errorf("write %s: %w", path, err)
	}
	return nil
}
