# Phase 1 — Core service (Go): productize the boundary

[Roadmap](../../plan.md) · [Acceptance ledger](../evidence/phases-2-5-completion-ledger.md)

This is the retained phase checklist. Checked items describe their recorded scope only;
unchecked items and explicit limitations remain open. Historical verification statements
are not fresh test results. Follow current repository guidance when running checks.


**Goal:** `bedrock-core` becomes a real service the client can ship: session lifecycle, auth,
control channel, conformance harness. Deliverable proof: a headless Go CLI (`corectl`) can
device-code-auth, list Realms/friends, and join any of the three transport targets through
the core — before any more Rust exists.

Scope (detailed plan to be written at phase start):
- Control channel on `control.sock`: protobuf or JSON-RPC; methods — `Status`, `StartAuth` (device-code events streamed), `SignOut`, `ListServers`, `ListRealms`, `ListFriends` (gophertunnel realms package + go-xsapi sessions; the join side of what go-mcxboxbroadcast does), `Connect{target}`, `Disconnect`; events — auth state, connection state, transfer notices, disconnect reasons.
- Session lifecycle: begin by copying Lunar's `lunar/internal/relay/relay.go` and relay tests
  into `core/internal/relay`, adapting only imports/helpers needed to make the package
  standalone. The core uses that relay logic to dial upstream (RakNet / NetherNet via Xbox
  signaling / Realms address), serve the session endpoint, and handle transfers. Lunar remains a
  source donor, never a module dependency.
- Resource-pack negotiation upstream; pack payloads handed to client over the control channel as files in a cache dir (client applies them — Phase 6 renders them).
- Windows transport flavor (named pipe or TCP) behind the same listener interface.
- **Conformance harness (promoted from deferral):** `tools/fixturegen` grows to full packet coverage; CI job round-trips gophertunnel↔valentine bytes both directions on every core and defs bump. This is the automated version of spike task 0.4.
- Consumer-surface work in `platform/Lunar` to expose what the core needs through the facade (measured, minimal, per AGENTS.md ABI rules).

**Early authenticated RakNet smoke (Zeqa):**

- [x] Add an explicit, ignored Microsoft token cache and optional authenticated upstream
  dial mode while preserving the offline BDS path when `-auth-cache` is omitted.
- [x] Document the exact `bedrock-core` and release `cinnabar` commands, device-code
  stdout flow, cache privacy requirements, and Rust → local socket → Go → RakNet boundary.
- [x] Report the core's startup lifecycle synchronously: Go build start, process/auth state,
  published local endpoint, local Rust-client acceptance, and upstream connect/success/failure.
  Commit `46a4e9f` covers ordered, secret-safe logging and fatal-startup tests.
- [x] Live smoke: authenticate `bedrock-core` to `zeqa.net:19132` with
  `.local/auth/microsoft-token.json` and confirm the current release client reaches Zeqa.
- [x] Record non-secret live evidence below; never record the device code, access token,
  refresh token, or token-cache contents.

Live evidence:

- Date/time: 2026-07-11 19:17 PDT
- Authenticated upstream connection observed: yes; the authenticated protocol-1001 entry
  connection returned Zeqa's pre-login transfer to `pvp.inpvp.net:19132`, and the bounded
  core transfer follower completed the regional connection.
- Client reached Zeqa lobby/session: yes; the release client reached position
  `(-117.50, 87.62, 195.50)`, streamed `1105/5376` chunks while the count continued rising,
  and held approximately 100 FPS. A native Windows screenshot was inspected from the user
  temp directory and was not added to the repository.
- Credential hygiene (`git ls-files .local` empty; no credential material in retained logs):
  passed. The token cache remained inside the ignored `.local/` tree, its contents were never
  inspected, and the temporary device-code stdout log was removed after authentication.

This early direct-RakNet smoke does not close the phase-wide control-channel, `corectl`,
Realms, friends, NetherNet, general/post-login transfer, or sign-out work above.

Exit: `corectl join --friend <gamertag>` works from a clean machine; conformance CI green.
