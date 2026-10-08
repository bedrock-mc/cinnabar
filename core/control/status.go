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
	Transfer      *TransferV1                         `json:"transfer,omitempty"`
}

// TransferV1 announces that the server transferred the session; the client should
// reconnect to this same endpoint, which dials the new host.
type TransferV1 struct {
	Host     string `json:"host"`
	Port     uint16 `json:"port"`
	Sequence uint64 `json:"sequence"` // increases with every transfer
}

// Store retains only the newest resource-pack admission attempt.
type Store struct {
	mu        sync.RWMutex
	lifecycle Lifecycle
	latest    proxy.ResourcePackAdmissionSnapshot
	applied   uint64 // attempt whose packs the client confirmed applying
	transfer  *TransferV1
	transfers uint64
	connect   *proxy.ConnectProgress // live while the newest attempt prepares a join

	auth        AuthV1
	disconnect  *DisconnectV1
	disconnects uint64

	trustPrompt *proxy.ServerTrustPrompt           // the pending trust question, if any
	trustAnswer func(id uint64, trusted bool) bool // resolves a trust question
}

func NewStore() *Store {
	return &Store{
		lifecycle: LifecycleStarting,
		auth:      AuthV1{State: AuthSignedOut},
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
		if snapshot.AttemptID > store.latest.AttemptID {
			store.transfer = nil // the client reconnected
			store.disconnect = nil
		}
		store.latest = snapshot
		store.connect = nil // a reset or final snapshot ends the join's core stages
	}
	store.mu.Unlock()
}

// ObserveConnectProgress publishes the preparing join's stage; a zero stage withdraws it.
func (store *Store) ObserveConnectProgress(progress proxy.ConnectProgress) {
	store.mu.Lock()
	if progress.Stage == "" {
		store.connect = nil
	} else {
		store.connect = &progress
	}
	store.mu.Unlock()
}

// ObserveTransfer publishes a pending transfer until the next admission attempt begins.
func (store *Store) ObserveTransfer(target proxy.TransferTarget) {
	store.mu.Lock()
	store.transfers++
	store.transfer = &TransferV1{Host: target.Host, Port: target.Port, Sequence: store.transfers}
	store.mu.Unlock()
}

// ClearTransfer withdraws a pending transfer the client no longer needs to follow.
func (store *Store) ClearTransfer() {
	store.mu.Lock()
	store.transfer = nil
	store.mu.Unlock()
}

// SetApplied records the client's confirmation that it applied (or reverted)
// the packs handed off for attempt id; it only counts for the newest attempt.
func (store *Store) SetApplied(attemptID uint64, applied bool) {
	store.mu.Lock()
	defer store.mu.Unlock()
	switch {
	case applied && attemptID != 0 && attemptID == store.latest.AttemptID &&
		(store.latest.DownstreamOutcome == proxy.ResourcePackDownstreamHandedOffOptional ||
			store.latest.DownstreamOutcome == proxy.ResourcePackDownstreamHandedOffRequired):
		store.applied = attemptID
	case !applied && store.applied == attemptID:
		store.applied = 0
	}
}

func (store *Store) Status() StatusV1 {
	store.mu.RLock()
	status := StatusV1{SchemaVersion: 1, Lifecycle: store.lifecycle, PackAdmission: store.latest}
	if store.transfer != nil {
		pending := *store.transfer
		status.Transfer = &pending
	}
	if store.applied != 0 && store.applied == store.latest.AttemptID {
		status.PackAdmission.Application = proxy.ResourcePackApplicationApplied
	}
	store.mu.RUnlock()
	return status
}
