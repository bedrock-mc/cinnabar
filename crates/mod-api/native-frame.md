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
