package control

import "sync"

// Lifecycle is the process lifecycle exposed by Status v1.
type Lifecycle string

const (
	LifecycleStarting Lifecycle = "starting"
	LifecycleRunning  Lifecycle = "running"
	LifecycleStopping Lifecycle = "stopping"
)

// StatusV1 is the complete secret-safe Status v1 result.
type StatusV1 struct {
	SchemaVersion uint32        `json:"schema_version"`
	Lifecycle     Lifecycle     `json:"lifecycle"`
	PackAdmission PackAdmission `json:"pack_admission"`
	Transfer      *TransferV1   `json:"transfer,omitempty"`
	Network       *NetworkV1    `json:"network,omitempty"`
}

// PackAdmission is the newest resource-pack admission attempt; it names no pack identity,
// version, source, content key, digest or filesystem location.
type PackAdmission struct {
	AttemptID         uint64 `json:"attempt_id"`
	Offer             string `json:"offer"`
	PackCount         uint32 `json:"pack_count"`
	TotalBytes        uint64 `json:"total_bytes"`
	Acquisition       string `json:"acquisition"`
	CacheLoads        uint64 `json:"cache_loads"`
	CacheHits         uint64 `json:"cache_hits"`
	CacheMisses       uint64 `json:"cache_misses"`
	CacheStores       uint64 `json:"cache_stores"`
	CacheErrors       uint64 `json:"cache_errors"`
	DownstreamOutcome string `json:"downstream_outcome"`
	Application       string `json:"application"`
}

// Pack admission values the store reasons about; the relay supplies the others verbatim.
const (
	packNone                   = "none" // offer, acquisition and downstream outcome before any attempt
	packHandedOffOptional      = "handed_off_optional"
	packHandedOffRequired      = "handed_off_required"
	packApplicationUnavailable = "unavailable"
	packApplicationApplied     = "applied" // the client confirmed applying the handed-off packs
)

// NetworkV1 is the current upstream transport's round-trip estimate.
type NetworkV1 struct {
	AttemptID       uint64  `json:"attempt_id"`
	LocalRuntimeID  uint64  `json:"local_runtime_id"`
	RoundTripMillis *uint32 `json:"round_trip_ms"`
}

// NetworkObservation is the control plane's transport-neutral retained RTT sampler.
// A nil sampler withdraws an attempt; a nil sample means no measured value.
type NetworkObservation struct {
	AttemptID       uint64
	LocalRuntimeID  uint64
	RoundTripMillis func() *uint32
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
	latest    PackAdmission
	applied   uint64 // attempt whose packs the client confirmed applying
	transfer  *TransferV1
	transfers uint64
	connect   *ConnectProgress // live while the newest attempt prepares a join
	network   *NetworkObservation

	auth        AuthV1
	disconnect  *DisconnectV1
	disconnects uint64

	trustPrompt *ServerTrustPrompt                 // the pending trust question, if any
	trustAnswer func(id uint64, trusted bool) bool // resolves a trust question
}

func NewStore() *Store {
	return &Store{
		lifecycle: LifecycleStarting,
		auth:      AuthV1{State: AuthSignedOut},
		latest: PackAdmission{
			Offer:             packNone,
			Acquisition:       packNone,
			DownstreamOutcome: packNone,
			Application:       packApplicationUnavailable,
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
func (store *Store) Observe(snapshot PackAdmission) {
	snapshot.Application = packApplicationUnavailable
	store.mu.Lock()
	if snapshot.AttemptID >= store.latest.AttemptID {
		if snapshot.AttemptID > store.latest.AttemptID {
			store.transfer = nil // the client reconnected
			store.disconnect = nil
			store.network = nil
		}
		store.latest = snapshot
		store.connect = nil // a reset or final snapshot ends the join's core stages
	}
	store.mu.Unlock()
}

// ObserveConnectProgress publishes the preparing join's stage; a zero stage withdraws it.
func (store *Store) ObserveConnectProgress(progress ConnectProgress) {
	store.mu.Lock()
	if progress.Stage == "" {
		store.connect = nil
	} else {
		store.connect = &progress
	}
	store.mu.Unlock()
}

// ObserveTransfer publishes a pending transfer until the next admission attempt begins.
func (store *Store) ObserveTransfer(host string, port uint16) {
	store.mu.Lock()
	store.transfers++
	store.transfer = &TransferV1{Host: host, Port: port, Sequence: store.transfers}
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
		(store.latest.DownstreamOutcome == packHandedOffOptional ||
			store.latest.DownstreamOutcome == packHandedOffRequired):
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
		status.PackAdmission.Application = packApplicationApplied
	}
	if store.network != nil {
		source := *store.network
		status.Network = &NetworkV1{AttemptID: source.AttemptID, LocalRuntimeID: source.LocalRuntimeID, RoundTripMillis: source.RoundTripMillis()}
	}
	store.mu.RUnlock()
	return status
}

// ObserveNetwork retains only the newest attempt's live estimator and ignores stale withdrawals.
func (store *Store) ObserveNetwork(source NetworkObservation) {
	store.mu.Lock()
	defer store.mu.Unlock()
	if source.AttemptID < store.latest.AttemptID || (store.network != nil && source.AttemptID < store.network.AttemptID) {
		return
	}
	if source.RoundTripMillis == nil {
		if store.network != nil && store.network.AttemptID == source.AttemptID {
			store.network = nil
		}
	} else {
		store.network = &source
	}
}
