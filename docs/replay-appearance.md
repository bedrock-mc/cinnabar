# Frozen replay appearance

Each fighter's `skinId` identifies its PNG bytes. An independent `appearanceId`
identifies a JSON bundle containing the original native geometry JSON, resource
patch, optional cape PNG, and optional skin animation PNG atlases. Separate
identities preserve geometry changes even when the base PNG is unchanged.

The exporter copies these fields while sampling the fighter, then encodes PNGs
on its existing bounded appearance worker. Replay capture retains these immutable
bytes before the live cache closes. Appearance assets use the same content store,
compression, reference tracking, recovery, and 25 GB quota as arenas and frames.
Only hashes referenced by a recording are served by its appearance endpoint.

Bounds: 384 KiB per bundle, 256 KiB geometry, 16 KiB resource patch, 128 KiB per
cape/animation PNG, 256 px cape dimensions, three animation atlases with dimensions
up to 512 px and at most 256 frames, and 64 appearances per recording. Invalid or
unavailable referenced assets abort recording rather than creating a broken link.

Minecraft playback restores geometry, native model configuration, capes and
animation atlases. The website builds custom geometry through Cinnabar's existing
skin parser and rig builder. Capes use the shared canonical `render::cape` rig,
UV mapping and pose conversion also used by the native client.

Browser animated persona layers consume the native actor animation store's
skin-layer snapshots, including canonical geometry, pose, hidden bones, UV frame
transforms and blinking controller. Original PNG atlas sizes and alpha are retained
in artwork pages; they are not resampled onto the base skin. Layer geometry caches
are bounded at 192 entries and upload queues at 24 images.

Character Creator assets or geometry resource-patch keys which are absent from
the server's native skin data cannot be reconstructed. Browser first-person
persona layers follow the existing native first-person renderer's body/arm behavior;
extra animated texture geometry is currently published only with the visible body.
