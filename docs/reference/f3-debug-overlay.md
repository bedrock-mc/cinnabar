# F3 debug overlay

This is the user-requested Java-style developer feature. Its visual reference is
the supplied Minecraft Java 19w05a screenshot: left/right corner columns, white
text, translucent gray strips measured per line, and blank gaps between groups.
It is an explicit styling exception for these diagnostics, not evidence of
vanilla Bedrock UI parity. The strip opacity is a provisional visual match.

F3 toggles the overlay, hidden at startup. No launch flag is required;
`--dev-debug-overlay` remains accepted for existing scripts. F2 still captures a
screenshot. The native window title contains only the shared product name.

The JSON-UI resolver, bindings, layout and painter render the overlay during
gameplay. Pause, inventory, chat, loading and other screens hide it without
resetting the F3 toggle. The screen has no input targets. Font size depends only
on viewport height, DPI and GUI scale, reserving space for forty rows regardless
of changing values or target properties. Long strings truncate to one line;
they never shrink the font. Text starts one GUI pixel below its strip's top.
Empty rows produce gaps without backgrounds.

The overlay reads existing client authorities every frame while enabled. Only
FPS and average/max frame timing use a 250 ms aggregation window:

- Client/Bedrock versions, negotiated ID mode, wire/content protocol identities.
- FPS and average/max update interval for the latest refresh window.
- Player feet XYZ, block/sub-chunk coordinates and offsets, heading/yaw/pitch,
  eye light, biome, dimension, world tick and weather.
- Game/movement modes, motion in blocks/s, grounding/sneak/sprint, physics tick,
  session/FIFO identity, authority, sent/pending inputs and dropped ticks.
- Resident/meshed/cave-visible sections, tracked entities, view/publisher radii,
  decode/light/mesh queues, chunk requests/retries and malformed/skipped counts.
- Physical display, DPI/GUI setting, camera perspective/FOV, GPU/backend/driver,
  requested or confirmed present mode, upload queues/totals and UI GPU usage.
- Network queue depth, missing block visuals and peak worker timings.
- Selection-ray block coordinates, identifier, runtime ID, face, distance and
  up to ten registered state properties.

XYZ stays attached to the player in third person. The diagnostic selection ray
inspects up to twenty blocks without changing interaction reach. Missing terrain
or state is reported as unavailable; it does not disconnect the session. Queue
inspection does not drain work. Peak worker and cumulative counters are labeled
explicitly; GPU upload totals are not presented as resident memory or frame GPU
time. F3 does not claim server TPS, ping or Java/JVM memory measurements.
