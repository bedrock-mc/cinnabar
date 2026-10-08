# Actor burst fixture

Start the local Dragonfly server with `-actor-burst`. Through the headless client MCP,
use `chat` to send `/actorburst 128 shared`, `/actorburst 128 skins`, or
`/actorburst 128 geometry`. Each command replaces the previous crowd near the player;
`/actorburst clear` removes it. The command reports the supported count limit when an argument is out of range.

`shared` repeats identical generated skins and models, `skins` varies only the texels,
and `geometry` varies both. The fixture uses original generated images and models,
synthetic identities, and no remote accounts. Capture an idle baseline, one cold burst,
a steady crowd, then a clear and repeated burst to distinguish preparation from reuse.

For cold resource-pack publication, start with `-actor-burst-artwork 128`. The server
offers one original generated pack with distinct texture sizes through normal pack
delivery. Capture the join, then send `/actorburst 128 artwork` to show its custom
actors. Reconnect to exercise session publication again; resource packs are immutable
during a connection. An artwork command cannot exceed the count offered at startup.

`/actorburst 128 complex` adds many articulated detail bones and small cubes per player. Models and pixels vary independently across the crowd; use this case to
measure cold source parsing and mesh building, then clear the crowd normally.
