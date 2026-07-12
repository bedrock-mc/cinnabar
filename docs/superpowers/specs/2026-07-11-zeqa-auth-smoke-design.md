# Zeqa authenticated join smoke design

## Goal and scope

Add the smallest production-shaped Go-core path needed to join an authenticated
third-party RakNet server such as `zeqa.net:19132` from the current Rust client.
Authentication and upstream encryption remain entirely in Go. The existing
offline BDS path must remain byte-for-byte selectable by omitting the new flag.

This is an early Phase 1 smoke slice, not a substitute for the planned control
channel, `corectl`, sign-out, Realms/friends, NetherNet, transfer handling, or
in-client auth UI.

## Chosen interface

`bedrock-core` gains one optional flag:

```text
-auth-cache <path-to-ignored-json-token>
```

When absent, `proxy.Config.TokenSource` is nil and the current offline identity
dial remains unchanged. When present, startup obtains a Microsoft Live token
source from the explicit cache path and passes it to `minecraft.Dialer`.
Authenticated mode does not copy the local offline display identity into the
upstream login chain; gophertunnel supplies the Xbox identity.

The intended local invocation is:

```text
bedrock-core -socket-dir .local/run-zeqa -upstream zeqa.net:19132 \
  -auth-cache .local/auth/microsoft-token.json
```

The Rust application continues connecting only to the local game socket. It
does not receive, parse, or store Microsoft credentials.

## Token acquisition and storage

The implementation uses the pinned lunar-branch gophertunnel auth APIs and the
same device-code/refresh semantics as its proven `main.go` example:

1. If the explicit cache is absent, request a token with
   `auth.AndroidConfig.RequestLiveTokenContext`, writing only the device URL and
   code to core stdout.
2. If a checked cache is present, decode one `oauth2.Token` and construct
   `auth.AndroidConfig.RefreshTokenSourceWriter`.
3. Validate the source by obtaining a token before accepting a local client. If
   a cached refresh token is no longer usable, start one new cancellable device
   flow and replace it only after success.
4. Persist the resulting token as JSON using a same-directory temporary regular
   file, mode `0600`, flush/close, and atomic rename. Never print token JSON,
   access tokens, or refresh tokens.

An existing cache must be a regular non-link file. A malformed, oversized,
non-regular, or unsafe linked cache fails closed and is not overwritten. The
parent directory is created only for the explicit path and remains under the
already ignored `.local/` tree in the documented workflow. Cache writes are
bounded and serialized so concurrent local sessions cannot publish partial
JSON.

## Core and relay data flow

The command owns token-cache lifecycle and supplies an `oauth2.TokenSource` to
`proxy.Config`. The proxy carries that source unchanged to each upstream dial.
The `minecraft.Dialer` retains downstream `ClientData`, protocol selection,
logging, cancellation, spawn barrier, batching, packet relay, and disconnect
semantics. Its mode is:

- nil token source: current `IdentityData` offline login for local BDS;
- non-nil token source: `TokenSource` authenticated login for Zeqa/other online
  RakNet servers, with no offline identity override.

Authentication failure is returned through the existing session failure path
and closes the downstream session cleanly. The listener remains reusable only
for ordinary peer disconnects; a startup cache/device-flow failure prevents the
service from advertising a usable local endpoint.

## Verification

Tests use injected token request/refresh functions and never contact Microsoft:

- no `-auth-cache` preserves the nil/offline dial configuration;
- missing cache invokes one device flow and atomically publishes mode-0600 JSON;
- valid cache refreshes without device flow and persists rotated tokens;
- expired refresh falls back to one cancellable device flow;
- malformed/oversized/non-regular/linked cache fails closed without overwrite;
- cancellation stops initial device acquisition;
- authenticated dials receive the configured token source and omit offline
  identity data; direct/relay lifecycle tests remain green;
- token/cache contents never appear in logged errors or test snapshots.

The live smoke gate is successful only when the current client reaches Zeqa
through `zeqa.net:19132`, the core log proves authenticated upstream login, and
no credential file is tracked by Git. A Microsoft device-code approval is the
only expected manual step.

