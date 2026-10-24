# Block-state gallery

Start a fresh local world with `-block-gallery -game-mode creative -difficulty peaceful`.
Use a new `-dir` for the fixture. World and player saving are disabled; the normal
local server keeps its usual defaults.

`-block-gallery-filter '*stairs,*glass*'` selects comma-separated identifier globs.
Omitting the filter selects every state in the active server palette, including air
and states without a Dragonfly gameplay implementation. Invalid or empty selections
fail at startup. States are written directly into chunks and sent through the normal
palette and chunk protocol.

Each state occupies one 16-by-16 cell, sorted by identifier and state properties.
Four samples show the state alone, touching itself on three axes, touching stone on
three axes, and connected to itself on all four horizontal sides. The floor is at
Y=63. Time, weather and random ticks are frozen. Floating labels show exact state
properties. Block actors receive empty records; content variations such as banner
patterns and sign text require an additional fixture.

The world directory's `block-gallery.json` lists every state, runtime ID, sample
position and three camera views (eye position, Bedrock yaw and pitch in degrees).
Visit the views in listed order for a deterministic route. Filtering packs the
selected states into a new grid, so use the manifest from that run.

With the client MCP, connect using:

```json
{"local_server":{"args":["-block-gallery","-block-gallery-filter","*stairs","-game-mode","creative","-difficulty","peaceful"]}}
```

The server streams chunks around the player, not the camera, so for each view first
`/tp` the player to its eye position, then set the MCP camera path to the view, wait
for the local chunks to load, and capture. Hide the HUD for captures. This fixture covers
placed states; inventory and held-item presentation have separate fixtures.
