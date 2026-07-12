# Zeqa Authenticated Join Smoke Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let `bedrock-core` authenticate with Microsoft and join `zeqa.net:19132` through an explicit, ignored, safely persisted token cache while preserving the current offline BDS path.

**Architecture:** A focused Go `authcache` package owns device-code acquisition, refresh, validation, and atomic persistence. The command constructs the optional token source once and passes it through `proxy.Config`; the proxy selects either the existing offline identity dial or gophertunnel `Dialer.TokenSource`. Rust remains credential-blind and continues using only the local game socket.

**Tech Stack:** Go 1.26.1, pinned lunar-branch gophertunnel, `golang.org/x/oauth2`, local streamnet socket boundary.

## Global Constraints

- Authentication, Microsoft/Xbox identity, upstream encryption, RakNet, and NetherNet remain entirely in Go.
- Omitting `-auth-cache` preserves the existing offline BDS dial behavior.
- Token JSON, access tokens, and refresh tokens are never logged or committed.
- The documented cache is `.local/auth/microsoft-token.json`; `.local/` remains ignored.
- Existing cache inputs are bounded to 64 KiB and must be regular, non-link files.
- Cache publication uses a mode-`0600` same-directory temporary file and atomic rename.
- Device acquisition is cancellable and writes only the Microsoft URL/code prompt to the configured writer.

---

### Task 1: Checked Microsoft token cache

**Files:**
- Create: `core/authcache/cache.go`
- Create: `core/authcache/cache_test.go`

**Interfaces:**
- Produces: `authcache.Config` and `authcache.Source(context.Context, Config) (oauth2.TokenSource, error)`.
- `Config` contains `Path string`, `Writer io.Writer`, `Request func(context.Context, io.Writer) (*oauth2.Token, error)`, and `Refresh func(*oauth2.Token, io.Writer) oauth2.TokenSource`; nil functions select pinned gophertunnel Android auth defaults.

- [ ] **Step 1: Write the failing cache tests**

  Add table-driven tests named `TestSourceMissingCacheRequestsAndPublishes`, `TestSourceValidCacheRefreshesAndPersistsRotation`, `TestSourceExpiredRefreshRequestsOnce`, `TestSourceRejectsMalformedOversizedAndLinkedCaches`, and `TestSourceCancellationDoesNotPublish`. Use injected request/refresh functions and sentinel tokens; never contact Microsoft.

- [ ] **Step 2: Run the RED gate**

  Run: `go test ./core/authcache -run TestSource -count=1`

  Expected: compile failure because `authcache.Config` and `authcache.Source` do not exist.

- [ ] **Step 3: Implement bounded load and atomic save**

  Implement `load(path)` with `os.Lstat`, regular/non-link checks, a 64 KiB size bound, `os.Open` plus `Stat`/`os.SameFile` verification, bounded JSON decode with trailing-data rejection, and required refresh-token validation. Implement `save(path, token)` with parent creation, same-directory `os.CreateTemp`, `Chmod(0600)`, JSON encode, `Sync`, `Close`, `Rename`, and cleanup on every failure.

- [ ] **Step 4: Implement source acquisition and rotation persistence**

  On exact absence call the injected/default cancellable request once. On valid cache construct the injected/default refresh source and validate it with `Token()`. If refresh fails, request once; other cache failures remain fail-closed. Wrap the source in a mutex-protected token source that atomically persists each successfully returned current token before returning it.

- [ ] **Step 5: Run GREEN and strict checks**

  Run: `go test ./core/authcache -count=1 && go vet ./core/authcache`

  Expected: all cache tests pass and vet is clean.

- [ ] **Step 6: Commit**

  Commit: `feat(core): add checked Microsoft auth cache`

---

### Task 2: Authenticated upstream dial mode

**Files:**
- Modify: `core/cmd/bedrock-core/main.go`
- Create: `core/cmd/bedrock-core/main_test.go`
- Modify: `core/proxy/proxy.go`
- Modify: `core/proxy/proxy_test.go`

**Interfaces:**
- Consumes: `authcache.Source` from Task 1.
- Extends: `proxy.Config` with `TokenSource oauth2.TokenSource`.
- Produces: optional CLI flag `-auth-cache <path>` and pure `newUpstreamDialer(downstream, tokenSource) minecraft.Dialer` selection.

- [ ] **Step 1: Write failing offline/authenticated mode tests**

  Add `TestNewUpstreamDialerOfflinePreservesIdentity`, `TestNewUpstreamDialerAuthenticatedUsesTokenAndOmitsOfflineIdentity`, `TestParseFlagsAuthCacheIsOptional`, and `TestRunAuthFailureDoesNotStartProxy`. Use a sentinel token source and injected `serve`/`source` functions so tests do not dial or authenticate.

- [ ] **Step 2: Run the RED gate**

  Run: `go test ./core/proxy ./core/cmd/bedrock-core -run 'Test(NewUpstreamDialer|ParseFlags|RunAuth)' -count=1`

  Expected: compile failure on the missing token-source config, flag, and dialer factory.

- [ ] **Step 3: Wire the optional CLI source**

  Parse `-auth-cache`; when non-empty, call `authcache.Source` before `proxy.Serve` using the signal context and stdout. Pass the resulting source through `proxy.Config`. Keep nil when the flag is absent. Return/cache errors before opening the local listener.

- [ ] **Step 4: Select the correct gophertunnel login mode**

  Build the common dialer with downstream `ClientData`, protocol, and error log. For nil token source, retain the current copied offline `IdentityData`. For non-nil source, set `Dialer.TokenSource` and leave `IdentityData` zero so Xbox identity drives login. Preserve cancellation, spawn barrier, relay, batching, and disconnect behavior.

- [ ] **Step 5: Run focused and broad Go verification**

  Run: `go test ./core/... -count=1 && go vet ./core/...`

  Expected: all Go tests and vet pass, including existing proxy lifecycle/race fixtures.

- [ ] **Step 6: Commit**

  Commit: `feat(core): authenticate remote RakNet joins`

---

### Task 3: Document and live-test Zeqa

**Files:**
- Modify: `README.md`
- Modify: `plan.md`

**Interfaces:**
- Documents the exact `bedrock-core` and `bedrock-client` commands; no credential contents enter documentation or metrics.

- [ ] **Step 1: Add command/documentation assertions**

  Add a Go help-text assertion covering `-auth-cache`, and document `.local/auth/microsoft-token.json`, `zeqa.net:19132`, the device-code prompt, cache privacy, and the Rust → local socket → Go → RakNet path.

- [ ] **Step 2: Run the complete repository gate**

  Run: `cargo fmt --all -- --check`, `cargo test --workspace`, `go test ./core/...`, `go vet ./core/...`, and `git diff --check`.

  Expected: all checks pass; no file under `.local/` is tracked.

- [ ] **Step 3: Run the live smoke**

  Start `bedrock-core` with `-upstream zeqa.net:19132 -auth-cache .local/auth/microsoft-token.json`, relay the printed Microsoft URL/code to the user for the one manual approval, then start the current release client on the published socket. Success requires an authenticated upstream connection and the client reaching Zeqa without credential material in logs or Git.

- [ ] **Step 4: Record evidence and commit**

  Record only non-secret connection/result evidence in `plan.md`, keep the phase-wide control-channel work open, and commit: `docs: record authenticated Zeqa smoke`.

