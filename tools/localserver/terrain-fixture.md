# Synthetic terrain fixture

The local server still uses its default flat generators. `-terrain-fixture`
opts into repeatable hills, cliffs, caves, persistent oak leaves, and water
valleys with clear weather and fixed daylight. This is a renderer workload,
not vanilla terrain generation or a parity reference. It has no persistent
actor or particle workload.

Build and pregenerate outside the repository, before any measured capture:

```sh
cd tools/localserver
GOWORK=off go build -p 4 -o /private/tmp/cinnabar-terrain-server .
/private/tmp/cinnabar-terrain-server -dir /private/tmp/cinnabar-terrain-template -terrain-fixture-generate
```

Generation refuses any existing `db` path, including an empty directory or
symlink. `-terrain-fixture-radius` changes the pregenerated square radius;
its limits and default are defined in `terrain_fixture_setup.go`. The
generated `fixture-manifest.json` records the fixed seed, bounds, workload,
camera position and rotation, and flight route. Keep the template untouched
and copy it to a fresh directory for each capture.

Launch the client headlessly through the developer-control endpoint and use
the MCP `connect` tool with a local server:

```json
{
  "local_server": {
    "binary": "/private/tmp/cinnabar-terrain-server",
    "world_dir": "/private/tmp/cinnabar-terrain-run",
    "args": ["-terrain-fixture", "-game-mode", "creative", "-difficulty", "peaceful"]
  }
}
```

MCP supplies `-dir` and `-addr`; the server retains its `ready`/stdin control
protocol. Read the camera from the manifest. Client resolution, vsync, and
the owner's saved render distance belong to the capture configuration, not
the terrain generator. Keep worlds, binaries, screenshots, and traces out
of git. See [live testing](../../docs/agents/live-testing.md) and
[client MCP](../../docs/agents/client-mcp.md) for capture rules.
