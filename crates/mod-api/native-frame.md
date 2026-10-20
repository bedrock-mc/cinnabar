# Native frames for trusted WASI packet mods

This optional core-module ABI extends packet mods without changing their existing
`start`, `on-batch`, `tick` or `stop` exports. Components retain the existing relay ABI.

Export both `native-init(mask: i32) -> ()` and `frame(ptr: i32, len: i32) -> i32`.
After `_initialize` and before `start`, the host calls `native-init`. Bit 0 means
the host supplies player frames and accepts local camera deltas. It is set only
with both `players` and `camera` grants. Unrecognized bits must be ignored.

`frame` receives UTF-8 JSON matching `native_frame::Frame`, or `null` when gameplay
does not own input. Return the existing canonical `result<list<u8>, string>`:
discriminant at offset 0, list pointer at 4 and list length at 8. Successful bytes
contain JSON `null` or `native_frame::Delta`. An optional `cabi_post_frame(i32)`
releases the result. Input and output each have a 65536-byte bound.

The host owns input capture, snapshots and camera permission. Guest code owns its
feature policy. Calls run on the session's packet worker, serialized with relay
calls. Frames coalesce under load; the renderer never waits for the guest. Output
is consumed once and discarded after input or session ownership changes. Angles
are radians; positive yaw turns left and positive pitch turns up. This API adds no
packets and does not bypass the normal camera owner.

`native-frame-v1.json` is the shared cross-language conformance fixture. Treat its
64-bit IDs as integers, preserving all bits. SDK bounds derive from the existing
player and camera limits.

## General camera effects

A core module may additionally export `camera-frame(ptr: i32, len: i32) -> i32`.
Capability bit 2 (`4`) selects this callback when both `camera` and `movement` are
granted. Its input and canonical result ABI match `frame`. A connected world without
gameplay input carries `suspended: true` and no targets or attack state; the older
callback still receives `null`. Successful output is
`null` or `native_camera::Effects`. It takes precedence over the older `frame`
callback. Without a players grant the host removes player observations before
calling the guest. `cabi_post_camera-frame(i32)` may release the result.

Effects select either a once-consumed rotation or a detached view, plus optional
damage-camera intensity and teleport-view preservation. Detached views keep
native look separate from body heading. Orbit follows the body with host terrain
collision; flying views consume movement only for the camera while the body's
physics, gravity, server corrections and network cadence continue. No effect
changes a teleport destination or its acknowledgment. The shared damage-camera
owner validates intensity. `native-camera-v1.json` is the cross-language output
fixture.

Presentation configuration remains valid until replaced or released; only relative
rotations expire after two client frames and are consumed once. Worker contention
does not reset detached position. Packet updates invalidate unconsumed rotations. A connected suspended frame
retains only passive damage and teleport-view policies; it releases detached
input immediately. Revocation, dimension/session changes, failure and stop
invalidate every effect. Omission
restores the player's current camera settings and perspective.


## World drawing

The independent `render-frame` callback uses native-init bit `8` when both
`world_draw` and `render_through_walls` are explicitly granted. Its canonical ABI,
optional `cabi_post_render-frame`, serialization and 64 KiB bounds match the
other frame callbacks. Its JSON input is `native_render::Frame`: version, session
and dimension only, or `null` outside a current world. Drawing does not require
camera/movement/player observation and continues while gameplay input is suspended.

Successful output is `null` or a complete `native_render::Scene` with the matching
session/dimension. It contains at most 64 oriented boxes and 128 world lines, using
existing retained-drawing colour and coordinate bounds. The renderer reuses unit
geometry and instance handles and splits long lines into bounded retained pieces.
A line exceeding the remaining per-owner instance budget is omitted whole.
Retained geometry and depth permissions are admitted by the same drawing store
used by component extensions. Invalid wire output quarantines its producer;
resource-admission failure clears its scene atomically. World loss, revocation,
callback failure and stop release every drawing. Unchanged scenes preserve their
resource identity. The shared `native-render-v1.json` fixture records the wire ABI.
