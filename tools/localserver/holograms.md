# Floating-text workload

Build with `make local-server`. Start a local world with `-holograms`, or pass
`local_server.args: ["-holograms"]` to the client MCP `connect` tool.
Use a headless client and look south (yaw 0) from spawn.

Each connection gets 150 zero-size armor stands in a fixed grid ahead of its
initial position. Their always-shown names have one to ten lines with colours,
bold text and animated obfuscation. Every input tick rewrites 75 labels: some
change their last-line counter, others also rotate colours. The rest retain
their original text. Existing client presentation budgets still apply.

`/holograms` restarts the grid ahead of the player's current position.
`/holograms freeze` restarts at tick zero and stops server text updates;
client-side obfuscation keeps animating. `/holograms clear` removes the fixture.
Each restart removes the old actors first. Reconnect with the same spawn and
camera for repeatable steady-state comparisons.
