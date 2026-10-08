# Optional acceptance plugin and diagnostics

Step 5 moves acceptance state, world-ready and mutation gates, teleport and remesh
proofs, witness request files, Phase 2/3 evidence, bounded audio wire observations
and terminal evidence into `acceptance`. The app enables it through the default
`acceptance` feature; `--no-default-features` leaves it out of the app's production
dependency graph. Evidence-only CLI options fail before startup when that feature is absent.

The existing gameplay adapter still requests an `AcceptanceRun` deadline resource.
To leave step 4's code untouched, the app supplies an empty deadline observation
only when the feature is disabled. It always reports no deadline and owns no
evidence state. The enabled build uses the real acceptance resource. This host
compatibility seam can disappear when gameplay accepts its own deadline observation.
The no-feature production build compiles and excludes the acceptance dependency.
It currently reports five dead-code warnings for evidence-only accessors in the
unchanged gameplay/session files. The existing app test suite assumes the default
feature; it is compiled and run in that configuration.

The always-on `diagnostics` crate owns frame/asset/pipeline metrics, shared marker
identifiers and bounded file reads. Production telemetry and startup still need
these services when acceptance is disabled. This is the original metrics schema
and marker vocabulary, with no additional authority or acceptance state.

## Observations and commands

`WorldReadyObservation` captures one frame's committed cohort, stream counters,
transport fence, visible/rendered counts, mutation target facts and player identity.
The optional crate does not own `ClientWorld`, `NetworkHandle` or gameplay state.
A narrow `WorldReadyCommands` adapter permits the existing synchronous remesh,
manifest-state inspection and timed-session start operations. This retains the
same exact manifest and ingress fence without adding queue latency.

The model witness receives only `ModelWitnessObservation::committed_cohort` and
the renderer's existing witness resources. It cannot mutate the client's world.
The runtime's Phase 3 adapter drains completed movement records every frame,
including ordinary play with evidence disabled, so the bounded movement queue
cannot fill. With the feature enabled, it converts those records to independent
`Phase3EvidenceFrame` values and correction/fault observations. Gameplay retains
its tick queue, authority faults, collision registries and reconciliation.

Terminal evidence consumes a `TerminalMovementObservation` and returns an
ordinary `AppExit` request. The app then shuts down its network handle and writes
the exit message. Acceptance never acquires or closes a transport directly.

Audio wire observations run before catalog admission at the existing committed
FIFO observation point. They remain bounded to four rows per session and record
only the fixed critical-hit sound's decoded fields. They make no audibility claim.

## Ordering and behavior

The runtime keeps explicit system ordering. Readiness runs after the world and
render publication observations; model witnesses run after readiness; terminal
evidence follows network send, UI publication, metrics and menu recovery. The
optional plugin installs only its owned resources and the runtime schedules its
systems at those existing boundaries.

This is a behavior-preserving extraction, not a new vanilla parity claim. Existing
world-ready, teleport, remesh, witness, metrics, Phase 3 and composed audio tests
remain the acceptance witnesses. No live server, remote machine or installed asset
write is required by the extraction. The architecture policy rejects reverse
app/session/gameplay dependencies and source access from the extracted crates.
