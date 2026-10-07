package proxy

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"sync"
)

// ServerTrustFile persists trusted NetherNet server keys as {"keys":[...]}, oldest first, the shape
// vanilla stores them in.
type ServerTrustFile string

type serverTrustDocument struct {
	Keys []string `json:"keys"`
}

// LoadTrustedKeys returns no keys when the file does not exist yet.
func (path ServerTrustFile) LoadTrustedKeys() ([]string, error) {
	data, err := os.ReadFile(string(path))
	if errors.Is(err, fs.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var document serverTrustDocument
	if err := json.Unmarshal(data, &document); err != nil {
		return nil, fmt.Errorf("decode %s: %w", path, err)
	}
	return document.Keys, nil
}

// SaveTrustedKeys replaces the file atomically.
func (path ServerTrustFile) SaveTrustedKeys(keys []string) error {
	if keys == nil {
		keys = []string{}
	}
	data, err := json.Marshal(serverTrustDocument{Keys: keys})
	if err != nil {
		return err
	}
	dir := filepath.Dir(string(path))
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	temp, err := os.CreateTemp(dir, filepath.Base(string(path))+".*.tmp")
	if err != nil {
		return err
	}
	defer os.Remove(temp.Name())
	if _, err := temp.Write(data); err != nil {
		_ = temp.Close()
		return err
	}
	if err := temp.Close(); err != nil {
		return err
	}
	return os.Rename(temp.Name(), string(path))
}

// ServerTrustPrompt asks the player whether to trust the NetherNet server at URL.
type ServerTrustPrompt struct {
	ID  uint64 `json:"id"`
	URL string `json:"url"`
}

// ServerTrustPrompts hands trust questions to the client and waits for its answer. publish receives
// each prompt as pending, then again as not pending once it is answered or its join ends; it must
// return promptly.
type ServerTrustPrompts struct {
	publish func(prompt ServerTrustPrompt, pending bool)

	mu      sync.Mutex
	next    uint64
	pending map[uint64]chan bool
}

func NewServerTrustPrompts(publish func(prompt ServerTrustPrompt, pending bool)) *ServerTrustPrompts {
	return &ServerTrustPrompts{publish: publish, pending: make(map[uint64]chan bool)}
}

// Confirm asks about url and reports the answer; it gives up when ctx ends.
func (prompts *ServerTrustPrompts) Confirm(ctx context.Context, url string) (bool, error) {
	answer := make(chan bool, 1)
	// Publishing under the lock keeps publications in id order, so an older join's prompt never
	// lands after a newer one's.
	prompts.mu.Lock()
	prompts.next++
	prompt := ServerTrustPrompt{ID: prompts.next, URL: url}
	prompts.pending[prompt.ID] = answer
	prompts.publish(prompt, true)
	prompts.mu.Unlock()
	defer func() {
		prompts.mu.Lock()
		delete(prompts.pending, prompt.ID)
		prompts.publish(prompt, false)
		prompts.mu.Unlock()
	}()
	select {
	case trusted := <-answer:
		return trusted, nil
	case <-ctx.Done():
		return false, ctx.Err()
	}
}

// Answer resolves prompt id; it reports false when the prompt is no longer pending.
func (prompts *ServerTrustPrompts) Answer(id uint64, trusted bool) bool {
	prompts.mu.Lock()
	defer prompts.mu.Unlock()
	answer, ok := prompts.pending[id]
	if !ok {
		return false
	}
	delete(prompts.pending, id)
	answer <- trusted
	return true
}
