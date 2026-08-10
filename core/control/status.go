package control

import (
	"sync"

	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

// Lifecycle is the process lifecycle exposed by Status v1.
type Lifecycle string

const (
	LifecycleStarting Lifecycle = "starting"
	LifecycleRunning  Lifecycle = "running"
	LifecycleStopping Lifecycle = "stopping"
)

// StatusV1 is the complete secret-safe Status v1 result.
type StatusV1 struct {
	SchemaVersion uint32                              `json:"schema_version"`
	Lifecycle     Lifecycle                           `json:"lifecycle"`
	PackAdmission proxy.ResourcePackAdmissionSnapshot `json:"pack_admission"`
}

// Store retains only the newest resource-pack admission attempt.
type Store struct {
	mu        sync.RWMutex
	lifecycle Lifecycle
	latest    proxy.ResourcePackAdmissionSnapshot
}

func NewStore() *Store {
	return &Store{
		lifecycle: LifecycleStarting,
		latest: proxy.ResourcePackAdmissionSnapshot{
			Offer:             proxy.ResourcePackOfferNone,
			Acquisition:       proxy.ResourcePackAcquisitionNone,
			DownstreamOutcome: proxy.ResourcePackDownstreamNone,
			Application:       proxy.ResourcePackApplicationUnavailable,
		},
	}
}

func (store *Store) SetLifecycle(lifecycle Lifecycle) {
	store.mu.Lock()
	store.lifecycle = lifecycle
	store.mu.Unlock()
}

// Observe applies an attempt reset or final update. Older attempts cannot
// overwrite a newer attempt that finished first.
func (store *Store) Observe(snapshot proxy.ResourcePackAdmissionSnapshot) {
	snapshot.Application = proxy.ResourcePackApplicationUnavailable
	store.mu.Lock()
	if snapshot.AttemptID >= store.latest.AttemptID {
		store.latest = snapshot
	}
	store.mu.Unlock()
}

func (store *Store) Status() StatusV1 {
	store.mu.RLock()
	status := StatusV1{SchemaVersion: 1, Lifecycle: store.lifecycle, PackAdmission: store.latest}
	store.mu.RUnlock()
	return status
}
